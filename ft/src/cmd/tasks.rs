use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use chrono::NaiveDate;
use clap::{Args, Subcommand, ValueEnum};
use ft_core::{
    dates,
    graph::{
        query::{parse_with as parse_query, GraphQuery, Profile},
        NodeKind,
    },
    query::{
        filter::Filter,
        preset,
        sort::{parse_sort_key, sort_by_keys},
        SortKey, SortOrder,
    },
    selector,
    task::{
        ops::{
            self, CompleteError, CompleteOptions, CreateError, CreateInput, CreateOptions,
            MoveSource, MoveTarget, Position,
        },
        Priority, Status, Task,
    },
    vault::Vault,
};

use crate::output::{self, Format, GroupBy};

#[derive(Args)]
pub struct TasksArgs {
    #[command(subcommand)]
    pub command: TasksCommand,
}

#[derive(Subcommand)]
pub enum TasksCommand {
    /// List tasks across the vault, optionally filtered.
    List(ListArgs),
    /// Create a new task.
    Create(CreateArgs),
    /// Mark a task complete (and write the next instance if recurring).
    Complete(CompleteArgs),
    /// Move tasks (and their subtasks) to another file or section.
    Move(MoveArgs),
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum StatusFlag {
    Open,
    Done,
    #[value(name = "in-progress")]
    InProgress,
    Cancelled,
}

impl From<StatusFlag> for Status {
    fn from(s: StatusFlag) -> Self {
        match s {
            StatusFlag::Open => Status::Open,
            StatusFlag::Done => Status::Done,
            StatusFlag::InProgress => Status::InProgress,
            StatusFlag::Cancelled => Status::Cancelled,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum PriorityFlag {
    Highest,
    High,
    Medium,
    Low,
    Lowest,
}

impl From<PriorityFlag> for Priority {
    fn from(p: PriorityFlag) -> Self {
        match p {
            PriorityFlag::Highest => Priority::Highest,
            PriorityFlag::High => Priority::High,
            PriorityFlag::Medium => Priority::Medium,
            PriorityFlag::Low => Priority::Low,
            PriorityFlag::Lowest => Priority::Lowest,
        }
    }
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Preset name (built-in or from config). If no preset of this name
    /// exists, the value is parsed as a query DSL string instead.
    #[arg(value_name = "PRESET_OR_QUERY")]
    pub preset_or_query: Option<String>,

    /// Explicit query DSL (composed with flags and any positional query as
    /// additional `and` clauses). See docs/graph-query-dsl.md for the
    /// grammar; tasks queries run under `Profile::Tasks` so bare
    /// predicates like `priority = high` desugar to
    /// `node where kind = Task and self.priority = high`.
    #[arg(long, value_name = "DSL")]
    pub query: Option<String>,

    /// Filter by status (repeatable).
    #[arg(long, value_enum)]
    pub status: Vec<StatusFlag>,

    /// Filter by priority (repeatable).
    #[arg(long, value_enum)]
    pub priority: Vec<PriorityFlag>,

    /// Filter by tag (repeatable). Leading `#` is optional.
    #[arg(long)]
    pub tag: Vec<String>,

    /// Substring filter on the source file path (repeatable; all must match).
    #[arg(long)]
    pub path: Vec<String>,

    /// Only tasks due strictly before this date (YYYY-MM-DD).
    #[arg(long, value_name = "DATE")]
    pub due_before: Option<NaiveDate>,

    /// Only tasks due strictly after this date (YYYY-MM-DD).
    #[arg(long, value_name = "DATE")]
    pub due_after: Option<NaiveDate>,

    /// Only tasks scheduled strictly before this date (YYYY-MM-DD).
    #[arg(long, value_name = "DATE")]
    pub scheduled_before: Option<NaiveDate>,

    /// Only tasks scheduled strictly after this date (YYYY-MM-DD).
    #[arg(long, value_name = "DATE")]
    pub scheduled_after: Option<NaiveDate>,

    /// Only tasks that have a due date.
    #[arg(long, conflicts_with = "no_due")]
    pub has_due: bool,

    /// Only tasks without a due date.
    #[arg(long)]
    pub no_due: bool,

    /// Sort keys, comma-separated or repeated (e.g. `--sort priority,due` or
    /// `--sort priority --sort due`). Suffix `:reverse` to invert a key
    /// (e.g. `--sort due:reverse`).
    #[arg(long)]
    pub sort: Vec<String>,

    /// Cap the number of rows returned.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,

    /// Group rows in the table output. Has no effect on JSON / NDJSON / markdown.
    #[arg(long, value_enum)]
    pub group_by: Option<GroupBy>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Table)]
    pub format: Format,

    /// Disable colored output (also honored: `NO_COLOR` env var).
    #[arg(long)]
    pub no_color: bool,

    /// Treat an empty result set as a successful run. Default: exit 1 when
    /// nothing matches (useful in scripting).
    #[arg(long)]
    pub allow_empty: bool,
}

pub fn run(args: TasksArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    match args.command {
        TasksCommand::List(list_args) => run_list(list_args, vault_flag),
        TasksCommand::Create(create_args) => run_create(create_args, vault_flag),
        TasksCommand::Complete(complete_args) => run_complete(complete_args, vault_flag),
        TasksCommand::Move(move_args) => run_move(move_args, vault_flag),
    }
}

fn run_list(args: ListArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let scan = vault.scan();

    for err in &scan.errors {
        tracing::warn!("{}", err);
    }

    if args.has_due && args.no_due {
        return Err(anyhow!("--has-due and --no-due are mutually exclusive"));
    }

    let filter = build_filter(&args);
    let today = dates::today();

    // Resolve positional argument: preset (built-in or user) → expand to DSL.
    // Anything else is treated as a DSL string.
    let positional_dsl = args
        .preset_or_query
        .as_deref()
        .map(|name| resolve_preset(name, &vault).unwrap_or_else(|| name.to_string()));

    // Compose all query sources into a single AND of GraphQueries. Empty
    // sources are skipped. Each source is parsed under Profile::Tasks so
    // bare predicates (`priority = high`) desugar to the canonical
    // `node where kind = Task and self.priority = high` form.
    let mut graph_queries: Vec<GraphQuery> = Vec::new();
    for src in [positional_dsl.as_deref(), args.query.as_deref()]
        .into_iter()
        .flatten()
    {
        let q = parse_query(src, Profile::Tasks, today)
            .map_err(|e| anyhow!("invalid query `{src}`: {e}"))?;
        graph_queries.push(q);
    }

    // Build the graph once. The query evaluator runs against the graph and
    // returns task NoteIds; we map those back to `&Task` for sort/render.
    let graph = crate::cmd::common::build_graph(&vault, &scan)?;
    let matched_task_keys: std::collections::HashSet<(PathBuf, usize)> = if graph_queries.is_empty()
    {
        // No query — every task in scan is admissible (filter is the
        // only remaining gate).
        scan.tasks
            .iter()
            .map(|t| (t.source_file.clone(), t.source_line))
            .collect()
    } else {
        // AND-compose: a task must match every graph query. Each query
        // produces a set of NoteIds; we intersect them.
        let mut acc: Option<std::collections::HashSet<(PathBuf, usize)>> = None;
        for q in &graph_queries {
            let ids = q.select(&graph);
            let keys: std::collections::HashSet<(PathBuf, usize)> = ids
                .into_iter()
                .filter_map(|id| match graph.node(id) {
                    NodeKind::Task(td) => Some((td.source_file.clone(), td.source_line)),
                    _ => None,
                })
                .collect();
            acc = Some(match acc {
                None => keys,
                Some(prev) => prev.intersection(&keys).cloned().collect(),
            });
        }
        acc.unwrap_or_default()
    };

    let mut matches: Vec<&Task> = scan
        .tasks
        .iter()
        .filter(|t| filter.matches(t))
        .filter(|t| matched_task_keys.contains(&(t.source_file.clone(), t.source_line)))
        .collect();

    let cli_sort = parse_cli_sort_keys(&args.sort)?;
    sort_by_keys(&mut matches, &cli_sort);

    if let Some(limit) = args.limit {
        matches.truncate(limit);
    }

    let exit = if matches.is_empty() && !args.allow_empty {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    };

    match args.format {
        Format::Table => {
            let use_color = !args.no_color
                && std::env::var_os("NO_COLOR").is_none()
                && is_terminal::IsTerminal::is_terminal(&std::io::stdout());
            let opts = output::table::TableOpts { use_color };
            if let Some(group) = args.group_by {
                let groups = group_tasks(&matches, group);
                let out = output::table::render_grouped(&groups, opts);
                print!("{out}");
            } else {
                let out = output::table::render(&matches, opts);
                println!("{out}");
            }
        }
        Format::Json => output::json::render(&matches)?,
        Format::Ndjson => output::ndjson::render(&matches)?,
        Format::Markdown => print!("{}", output::markdown::render(&matches)),
    }

    Ok(exit)
}

/// Look up a preset by name, preferring the user's config over built-ins.
fn resolve_preset(name: &str, vault: &Vault) -> Option<String> {
    if let Some(user) = vault.config.config.presets.get(name) {
        return Some(user.clone());
    }
    preset::builtin(name).map(|s| s.to_string())
}

fn build_filter(args: &ListArgs) -> Filter {
    let has_due = if args.has_due {
        Some(true)
    } else if args.no_due {
        Some(false)
    } else {
        None
    };

    Filter {
        statuses: args.status.iter().copied().map(Into::into).collect(),
        priorities: args.priority.iter().copied().map(Into::into).collect(),
        tags: args.tag.clone(),
        paths: args.path.clone(),
        due_before: args.due_before,
        due_after: args.due_after,
        scheduled_before: args.scheduled_before,
        scheduled_after: args.scheduled_after,
        has_due,
    }
}

/// Parse `--sort` values: each value can be a comma-separated list of keys,
/// each key optionally suffixed with `:reverse` or `:desc` for descending.
fn parse_cli_sort_keys(values: &[String]) -> Result<Vec<(SortKey, SortOrder)>> {
    let mut out = Vec::new();
    for v in values {
        for part in v.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (name, order) = match part.rsplit_once(':') {
                Some((n, "reverse" | "desc" | "rev")) => (n, SortOrder::Desc),
                Some((n, "asc")) => (n, SortOrder::Asc),
                Some((_, other)) => {
                    return Err(anyhow!(
                        "unknown sort modifier `:{other}` in `--sort {part}` (use `:reverse` or `:asc`)"
                    ));
                }
                None => (part, SortOrder::Asc),
            };
            let key = parse_sort_key(name).map_err(|e| anyhow!("bad sort key: {e}"))?;
            out.push((key, order));
        }
    }
    Ok(out)
}

/// Group tasks by the given key, returning sorted groups.
fn group_tasks<'a>(tasks: &[&'a Task], by: GroupBy) -> Vec<(String, Vec<&'a Task>)> {
    let mut buckets: BTreeMap<String, Vec<&Task>> = BTreeMap::new();
    for t in tasks {
        for label in group_labels(t, by) {
            buckets.entry(label).or_default().push(t);
        }
    }
    buckets.into_iter().collect()
}

/// One task may belong to multiple groups (only `Tag` produces > 1 today).
fn group_labels(t: &Task, by: GroupBy) -> Vec<String> {
    match by {
        GroupBy::Path => vec![t.source_file.display().to_string()],
        GroupBy::Folder => {
            let folder = t
                .source_file
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            vec![if folder.is_empty() {
                ".".into()
            } else {
                folder
            }]
        }
        GroupBy::Due => vec![t
            .due
            .map(|d| d.to_string())
            .unwrap_or_else(|| "(no due date)".into())],
        GroupBy::Priority => vec![match t.priority {
            Some(Priority::Highest) => "highest".into(),
            Some(Priority::High) => "high".into(),
            Some(Priority::Medium) => "medium".into(),
            Some(Priority::Low) => "low".into(),
            Some(Priority::Lowest) => "lowest".into(),
            None => "(no priority)".into(),
        }],
        GroupBy::Tag => {
            if t.tags.is_empty() {
                vec!["(no tags)".into()]
            } else {
                t.tags.iter().map(|s| format!("#{s}")).collect()
            }
        }
    }
}

// ── ft tasks create ──────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// Task description (free text). Tags from `--tag` are appended.
    #[arg(value_name = "DESCRIPTION", required = true)]
    pub description: Vec<String>,

    /// Due date. Accepts ISO (`2026-05-10`), keywords (`today`, `tomorrow`),
    /// relative (`+3d`, `-1w`), or natural language (`next monday`).
    #[arg(long, value_name = "DATE")]
    pub due: Option<String>,

    /// Scheduled date.
    #[arg(long, value_name = "DATE")]
    pub scheduled: Option<String>,

    /// Start date.
    #[arg(long, value_name = "DATE")]
    pub start: Option<String>,

    /// Priority.
    #[arg(long, value_enum)]
    pub priority: Option<PriorityFlag>,

    /// Tag (repeatable). Leading `#` is optional.
    #[arg(long)]
    pub tag: Vec<String>,

    /// Recurrence rule, preserved verbatim (e.g. `"every month on the 18th"`).
    #[arg(long)]
    pub recurrence: Option<String>,

    /// Stable identifier for this task (the 🆔 field).
    #[arg(long)]
    pub id: Option<String>,

    /// Other task IDs this one depends on (repeatable).
    #[arg(long = "depends-on")]
    pub depends_on: Vec<String>,

    /// Target file (relative to vault root). Defaults to today's daily note.
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Insert at the end of the section under this heading; create the
    /// heading at file end if missing.
    #[arg(long, value_name = "HEADING", conflicts_with_all = ["at_line", "append"])]
    pub under_heading: Option<String>,

    /// Insert at this 1-indexed line.
    #[arg(long, value_name = "N", conflicts_with_all = ["under_heading", "append"])]
    pub at_line: Option<usize>,

    /// Append at file end (the default for daily notes; explicit for clarity).
    #[arg(long, conflicts_with_all = ["under_heading", "at_line"])]
    pub append: bool,

    /// After writing, open `$EDITOR` on the new task line.
    #[arg(long)]
    pub edit: bool,

    /// Insert even if a duplicate task (same description + dates) already exists.
    #[arg(long)]
    pub force: bool,
}

fn run_create(args: CreateArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let today = dates::today();

    let target = resolve_target_path(&args, &vault, today)?;

    let parse_date = |s: &str, label: &str| -> Result<NaiveDate> {
        dates::parse(s, today).map_err(|e| anyhow!("--{label}: {e}"))
    };

    let description = args.description.join(" ");
    let input = CreateInput {
        description,
        status: Status::Open,
        priority: args.priority.map(Into::into),
        tags: args.tag,
        created: None,
        start: args
            .start
            .as_deref()
            .map(|s| parse_date(s, "start"))
            .transpose()?,
        scheduled: args
            .scheduled
            .as_deref()
            .map(|s| parse_date(s, "scheduled"))
            .transpose()?,
        due: args
            .due
            .as_deref()
            .map(|s| parse_date(s, "due"))
            .transpose()?,
        recurrence: args.recurrence,
        id: args.id,
        depends_on: args.depends_on,
    };

    let position = if let Some(h) = args.under_heading {
        Position::UnderHeading(h)
    } else if let Some(n) = args.at_line {
        Position::AtLine(n)
    } else {
        Position::Append
    };

    let outcome = ops::create_task(
        &target,
        input,
        CreateOptions {
            position,
            force: args.force,
        },
    )
    .map_err(|e| match e {
        CreateError::Duplicate { path, line } => {
            let rel = vault.relativize(&path);
            anyhow!(
                "duplicate task already exists at {}:{} (use --force to insert anyway)",
                rel.display(),
                line
            )
        }
        other => anyhow!("{other}"),
    })?;

    let display_path = vault.relativize(&target);
    println!(
        "Created task at {}:{}\n  {}",
        display_path.display(),
        outcome.line,
        outcome.serialized
    );

    if args.edit {
        open_editor(&target, outcome.line)?;
    }

    Ok(ExitCode::SUCCESS)
}

/// Resolve `--file` against the vault root, or fall back to today's daily
/// note. Returns an absolute path. Thin wrapper over `Vault::resolve_target`
/// so the CLI error type stays anyhow.
fn resolve_target_path(args: &CreateArgs, vault: &Vault, today: NaiveDate) -> Result<PathBuf> {
    vault
        .resolve_target(today, args.file.as_deref())
        .map_err(|e| anyhow!("{e}"))
}

fn open_editor(file: &std::path::Path, line: usize) -> Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
    let basename = std::path::Path::new(&editor)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let supports_line_flag = matches!(
        basename,
        "vi" | "vim" | "nvim" | "view" | "nano" | "less" | "more"
    );

    let status = if supports_line_flag {
        std::process::Command::new(&editor)
            .arg(format!("+{line}"))
            .arg(file)
            .status()
    } else {
        std::process::Command::new(&editor).arg(file).status()
    }
    .with_context(|| format!("failed to launch editor `{editor}`"))?;

    if !status.success() {
        return Err(anyhow!(
            "editor `{editor}` exited with status {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

// ── ft tasks complete ────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct CompleteArgs {
    /// Selector: task id (`abc123`), `<file>:<line>`, or fuzzy substring.
    /// If omitted, all open tasks are presented in an interactive picker.
    #[arg(value_name = "SELECTOR")]
    pub selector: Option<String>,

    /// Date to record as the done date. Accepts ISO, keywords, relative,
    /// and natural language (same forms as `ft tasks create --due`).
    /// Defaults to today.
    #[arg(long, value_name = "DATE")]
    pub on: Option<String>,

    /// Skip the interactive picker even when there are multiple matches.
    /// With `--yes`, the picker is replaced by an error listing candidates.
    #[arg(long)]
    pub yes: bool,
}

fn run_complete(args: CompleteArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;

    let today = dates::today();
    let on = match args.on.as_deref() {
        Some(s) => dates::parse(s, today).map_err(|e| anyhow!("--on: {e}"))?,
        None => today,
    };

    let scan = vault.scan();
    for err in &scan.errors {
        tracing::warn!("{}", err);
    }

    let chosen = pick_task(&args, &scan.tasks)?;

    let absolute_path = vault.path.join(&chosen.source_file);
    let outcome = ops::complete_task(&absolute_path, chosen.source_line, CompleteOptions { on })
        .map_err(|e| translate_complete_error(e, &vault.path))?;

    let rel = vault.relativize(&absolute_path);
    println!(
        "Completed {}:{}\n  {}",
        rel.display(),
        outcome.completed_line,
        outcome.completed_serialized
    );
    if let Some(next) = outcome.next_instance {
        println!(
            "Recurring: next instance at {}:{}\n  {}",
            rel.display(),
            next.line,
            next.serialized
        );
    }

    Ok(ExitCode::SUCCESS)
}

/// Resolve the selector argument into exactly one task. The selector can be
/// missing (use the interactive picker over open tasks), produce zero matches
/// (error), one match (use it directly), or many (interactive picker, or error
/// under `--yes` / non-TTY).
fn pick_task<'a>(args: &CompleteArgs, tasks: &'a [Task]) -> Result<&'a Task> {
    let candidates: Vec<&Task> = match args.selector.as_deref() {
        None => tasks
            .iter()
            .filter(|t| !matches!(t.status, Status::Done))
            .collect(),
        Some(s) => {
            // Try the structured form first. If a bare-id-shaped selector
            // matches no task by id, fall through to fuzzy matching so users
            // can type a single word and have it match a description.
            let sel = selector::parse(s);
            let mut matches = selector::resolve(tasks, &sel);
            if matches.is_empty() && matches!(sel, ft_core::selector::Selector::Id(_)) {
                let fuzzy = ft_core::selector::Selector::Fuzzy(s.to_string());
                matches = selector::resolve(tasks, &fuzzy);
            }
            if matches.is_empty() {
                return Err(anyhow!("no tasks match selector `{s}`"));
            }
            matches
        }
    };

    if candidates.len() == 1 {
        return Ok(candidates[0]);
    }

    if candidates.is_empty() {
        return Err(anyhow!("no open tasks in vault"));
    }

    let stdin_is_tty = is_terminal::IsTerminal::is_terminal(&std::io::stdin());
    if args.yes || !stdin_is_tty {
        let preview: Vec<String> = candidates
            .iter()
            .take(5)
            .map(|t| {
                format!(
                    "  {}:{}  {}",
                    t.source_file.display(),
                    t.source_line,
                    t.description
                )
            })
            .collect();
        let extra = if candidates.len() > 5 {
            format!("\n  … and {} more", candidates.len() - 5)
        } else {
            String::new()
        };
        return Err(anyhow!(
            "{} candidates match — be more specific:\n{}{extra}",
            candidates.len(),
            preview.join("\n")
        ));
    }

    let labels: Vec<String> = candidates
        .iter()
        .map(|t| {
            format!(
                "{}:{}  {}",
                t.source_file.display(),
                t.source_line,
                t.description
            )
        })
        .collect();
    let chosen = dialoguer::FuzzySelect::new()
        .with_prompt("complete which task?")
        .items(&labels)
        .default(0)
        .interact_opt()
        .map_err(|e| anyhow!("picker failed: {e}"))?
        .ok_or_else(|| anyhow!("no task selected"))?;
    Ok(candidates[chosen])
}

fn translate_complete_error(e: CompleteError, vault_root: &std::path::Path) -> anyhow::Error {
    use CompleteError::*;
    match e {
        Read { path, source } => {
            let rel = path.strip_prefix(vault_root).unwrap_or(&path);
            anyhow!("could not read {}: {source}", rel.display())
        }
        LineMissing {
            path,
            line,
            file_lines,
        } => {
            let rel = path.strip_prefix(vault_root).unwrap_or(&path);
            anyhow!(
                "line {line} not found in {} ({file_lines} lines)",
                rel.display()
            )
        }
        NotATask { path, line } => {
            let rel = path.strip_prefix(vault_root).unwrap_or(&path);
            anyhow!("line {line} in {} is not a task", rel.display())
        }
        AlreadyDone { path, line, done } => {
            let rel = path.strip_prefix(vault_root).unwrap_or(&path);
            anyhow!(
                "task at {}:{} is already done (on {done})",
                rel.display(),
                line
            )
        }
        other => anyhow!("{other}"),
    }
}

// ── ft tasks move ────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct MoveArgs {
    /// Selector for a single task (id, `<file>:<line>`, or fuzzy substring).
    /// Mutually exclusive with `--query`.
    #[arg(value_name = "SELECTOR", conflicts_with = "query")]
    pub selector: Option<String>,

    /// Bulk move: select tasks by query DSL. Mutually exclusive with the
    /// positional selector.
    #[arg(long, value_name = "DSL")]
    pub query: Option<String>,

    /// Target: a file path relative to the vault root, optionally suffixed
    /// with `#Heading` to land under that section.
    #[arg(long, value_name = "FILE[#HEADING]", required = true)]
    pub to: String,

    /// Print a unified diff of every affected file without writing anything.
    #[arg(long)]
    pub dry_run: bool,

    /// Skip the confirmation prompt for bulk moves.
    #[arg(long)]
    pub yes: bool,
}

fn run_move(args: MoveArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let today = dates::today();

    if args.selector.is_none() && args.query.is_none() {
        return Err(anyhow!("provide either a selector or --query"));
    }

    let scan = vault.scan();
    for err in &scan.errors {
        tracing::warn!("{}", err);
    }

    let target = parse_move_target(&args.to, &vault.path);

    let chosen: Vec<&Task> = if let Some(q) = args.query.as_deref() {
        let parsed = parse_query(q, Profile::Tasks, today)
            .map_err(|e| anyhow!("invalid query `{q}`: {e}"))?;
        let graph = crate::cmd::common::build_graph(&vault, &scan)?;
        let ids = parsed.select(&graph);
        let keys: std::collections::HashSet<(PathBuf, usize)> = ids
            .into_iter()
            .filter_map(|id| match graph.node(id) {
                NodeKind::Task(td) => Some((td.source_file.clone(), td.source_line)),
                _ => None,
            })
            .collect();
        scan.tasks
            .iter()
            .filter(|t| keys.contains(&(t.source_file.clone(), t.source_line)))
            .collect()
    } else {
        let s = args.selector.as_deref().unwrap();
        let sel = selector::parse(s);
        let mut matches = selector::resolve(&scan.tasks, &sel);
        if matches.is_empty() && matches!(sel, ft_core::selector::Selector::Id(_)) {
            let fuzzy = ft_core::selector::Selector::Fuzzy(s.to_string());
            matches = selector::resolve(&scan.tasks, &fuzzy);
        }
        if matches.is_empty() {
            return Err(anyhow!("no tasks match selector `{s}`"));
        }
        matches
    };

    if chosen.is_empty() {
        return Err(anyhow!("no tasks matched"));
    }

    // Confirm bulk operations interactively.
    let bulk = chosen.len() > 1;
    if bulk && !args.yes && !args.dry_run {
        let stdin_is_tty = is_terminal::IsTerminal::is_terminal(&std::io::stdin());
        if !stdin_is_tty {
            return Err(anyhow!(
                "{} tasks would be moved — pass --yes to confirm or --dry-run to preview",
                chosen.len()
            ));
        }
        let preview: Vec<String> = chosen
            .iter()
            .take(5)
            .map(|t| {
                format!(
                    "  {}:{}  {}",
                    t.source_file.display(),
                    t.source_line,
                    t.description
                )
            })
            .collect();
        let extra = if chosen.len() > 5 {
            format!("\n  … and {} more", chosen.len() - 5)
        } else {
            String::new()
        };
        let prompt = format!(
            "Move {} task(s) to {}?\n{}{extra}",
            chosen.len(),
            args.to,
            preview.join("\n")
        );
        let confirmed = dialoguer::Confirm::new()
            .with_prompt(prompt)
            .default(false)
            .interact_opt()
            .map_err(|e| anyhow!("confirmation failed: {e}"))?
            .unwrap_or(false);
        if !confirmed {
            return Err(anyhow!("aborted"));
        }
    }

    let sources: Vec<MoveSource> = chosen
        .iter()
        .map(|t| MoveSource {
            path: vault.path.join(&t.source_file),
            line: t.source_line,
        })
        .collect();

    let plan = ops::plan_move(&sources, &target).map_err(|e| anyhow!("{e}"))?;

    if args.dry_run {
        for edit in &plan.edits {
            if edit.original == edit.new {
                continue;
            }
            let rel = vault.relativize(&edit.path);
            print_diff(rel, &edit.original, &edit.new);
        }
        return Ok(ExitCode::SUCCESS);
    }

    ops::apply_move_plan(&plan).map_err(|e| anyhow!("{e}"))?;

    let target_rel = vault.relativize(target.path());
    println!(
        "Moved {} task(s) → {}",
        plan.blocks.len(),
        target_rel.display()
    );
    Ok(ExitCode::SUCCESS)
}

/// Parse `path[#heading]` into a [`MoveTarget`]. The path is resolved against
/// the vault root if relative.
fn parse_move_target(spec: &str, vault_root: &std::path::Path) -> MoveTarget {
    let (file_part, heading_part) = match spec.split_once('#') {
        Some((f, h)) => (f, Some(h.to_string())),
        None => (spec, None),
    };
    let raw = std::path::Path::new(file_part);
    let abs = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        vault_root.join(raw)
    };
    match heading_part {
        Some(h) => MoveTarget::UnderHeading(abs, h),
        None => MoveTarget::Append(abs),
    }
}

fn print_diff(path: &std::path::Path, original: &str, new: &str) {
    use similar::{ChangeTag, TextDiff};
    println!("--- {} (before)", path.display());
    println!("+++ {} (after)", path.display());
    let diff = TextDiff::from_lines(original, new);
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        print!("{sign}{change}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_sort_parses_compound() {
        let v = vec!["priority,due:reverse".to_string()];
        let parsed = parse_cli_sort_keys(&v).unwrap();
        assert_eq!(
            parsed,
            vec![
                (SortKey::Priority, SortOrder::Asc),
                (SortKey::Due, SortOrder::Desc)
            ]
        );
    }

    #[test]
    fn cli_sort_parses_repeated() {
        let v = vec!["priority".into(), "due:reverse".into()];
        let parsed = parse_cli_sort_keys(&v).unwrap();
        assert_eq!(
            parsed,
            vec![
                (SortKey::Priority, SortOrder::Asc),
                (SortKey::Due, SortOrder::Desc)
            ]
        );
    }

    #[test]
    fn cli_sort_rejects_unknown_key() {
        let v = vec!["nonsense".to_string()];
        assert!(parse_cli_sort_keys(&v).is_err());
    }

    #[test]
    fn cli_sort_rejects_unknown_modifier() {
        let v = vec!["due:sideways".to_string()];
        assert!(parse_cli_sort_keys(&v).is_err());
    }
}

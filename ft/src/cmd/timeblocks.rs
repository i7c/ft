//! `ft timeblocks` — list/add/edit/delete day-planner timeblocks under
//! the configured daily-note heading.
//!
//! Thin shell over [`ft_core::timeblock`]: this module owns clap parsing,
//! vault discovery, output formatting, and the `--dry-run` diff renderer.
//! All on-disk mutations go through [`ft_core::timeblock::ops`], which in
//! turn writes atomically via `ft_core::fs::write_atomic`.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, Result};
use chrono::{NaiveDate, NaiveTime};
use clap::{Args, Subcommand, ValueEnum};
use ft_core::{
    dates,
    timeblock::{
        self,
        doc::Document,
        ops::{self, AddOptions, EditMutation, Selector, TimeChange},
        ParseError, Tag, Timeblock,
    },
    vault::Vault,
};

#[derive(Args)]
pub struct TimeblocksArgs {
    #[command(subcommand)]
    pub command: TimeblocksCommand,
}

#[derive(Subcommand)]
pub enum TimeblocksCommand {
    /// List timeblocks for a day.
    List(ListArgs),
    /// Add a new timeblock.
    Add(AddArgs),
    /// Edit an existing timeblock.
    Edit(EditArgs),
    /// Delete a timeblock.
    Delete(DeleteArgs),
    /// Report time spent per tag over a date range.
    Spent(SpentArgs),
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum OutputFormat {
    Table,
    Json,
    Ndjson,
    Markdown,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum SpentFormat {
    Text,
    Json,
}

pub fn run(args: TimeblocksArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    match args.command {
        TimeblocksCommand::List(a) => run_list(a, vault_flag),
        TimeblocksCommand::Add(a) => run_add(a, vault_flag),
        TimeblocksCommand::Edit(a) => run_edit(a, vault_flag),
        TimeblocksCommand::Delete(a) => run_delete(a, vault_flag),
        TimeblocksCommand::Spent(a) => run_spent(a, vault_flag),
    }
}

// ── ft timeblocks list ───────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Date to read. Accepts ISO (`2026-05-16`), keywords (`today`,
    /// `tomorrow`, `yesterday`), or relative shifts (`+3d`, `-1w`).
    /// Defaults to today.
    #[arg(long, value_name = "DATE")]
    pub date: Option<String>,

    /// Filter by tag prefix (repeatable; multiple compose as OR). Leading
    /// `@` optional. `--tag work` matches `@work` and `@work/meeting`.
    #[arg(long)]
    pub tag: Vec<String>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,

    /// Explicit file (relative to vault root or absolute). Overrides the
    /// daily-note resolution from `--date`.
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Treat an empty result set as a successful run. Default: exit 1
    /// when nothing matches (useful for scripting).
    #[arg(long)]
    pub allow_empty: bool,
}

fn run_list(args: ListArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let today = dates::today();
    let date = parse_date_arg(args.date.as_deref(), today)?;
    let path = resolve_path(&vault, date, args.file.as_deref())?;
    let heading = vault.config.config.timeblocks_heading().to_string();

    let doc = Document::read(&path, &heading)?;
    let filtered = filter_by_tags(&doc.blocks, &args.tag)?;

    render_list(&filtered, args.format)?;

    let exit = if filtered.is_empty() && !args.allow_empty {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    };
    Ok(exit)
}

// ── ft timeblocks add ────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct AddArgs {
    /// Full blockstring: `HH:MM - HH:MM <desc> [@tag…]` or the short
    /// form `HH:MM <desc>` (end derived as `start + 30m`). Mutually
    /// exclusive with the `--start/--end/--desc/--tag` flag form.
    #[arg(value_name = "BLOCKSTRING", conflicts_with_all = ["start", "end", "desc", "tag"])]
    pub blockstring: Option<String>,

    /// Start time (`HH:MM`). Required when not using the positional form.
    #[arg(long, value_name = "HH:MM")]
    pub start: Option<String>,

    /// End time (`HH:MM`). When omitted, `start + 30m`.
    #[arg(long, value_name = "HH:MM")]
    pub end: Option<String>,

    /// Description.
    #[arg(long)]
    pub desc: Option<String>,

    /// Tag to attach (repeatable). Leading `@` optional. Validated as a
    /// strict tag (max 3 levels, `[A-Za-z0-9_-]+` per level).
    #[arg(long)]
    pub tag: Vec<String>,

    #[arg(long, value_name = "DATE")]
    pub date: Option<String>,

    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Insert even when an exact duplicate (same start, end, desc) exists.
    #[arg(long)]
    pub force: bool,

    /// Show the diff without writing.
    #[arg(long)]
    pub dry_run: bool,
}

fn run_add(args: AddArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let today = dates::today();
    let date = parse_date_arg(args.date.as_deref(), today)?;
    let path = resolve_path(&vault, date, args.file.as_deref())?;
    let heading = vault.config.config.timeblocks_heading().to_string();

    let block = build_block_from_args(&args)?;
    let opts = AddOptions { force: args.force };

    if args.dry_run {
        let mut doc = Document::read(&path, &heading)?;
        // Replicate the duplicate check in dry-run so users see the same
        // error without writing.
        if !args.force
            && doc
                .blocks
                .iter()
                .any(|b| b.start == block.start && b.end == block.end && b.desc == block.desc)
        {
            return Err(anyhow!(
                "duplicate block at {} - {} {}: use --force to insert anyway",
                fmt_hhmm(block.start),
                fmt_hhmm(block.end),
                block.desc
            ));
        }
        doc.blocks.push(block);
        print_diff(vault.relativize(&path), &doc.source_content, &doc.render());
        return Ok(ExitCode::SUCCESS);
    }

    // Ensure the target exists before writing: a missing default daily note
    // is rendered from its template so the file matches what `ft notes today`
    // would produce, rather than a bare `## Time Blocks`-only file. Explicit
    // `--file` paths are left to `add_block` to create.
    let (today_n, now_n) = dates::now_pair();
    let path = vault
        .ensure_target(date, args.file.as_deref(), today_n, now_n)
        .map_err(|e| {
            anyhow!(
                "{e}\nhint: add `[periodic_notes.daily]` to your config or pass `--file <PATH>`"
            )
        })?;

    let block_summary = format!(
        "+ {} - {} {}",
        fmt_hhmm(block.start),
        fmt_hhmm(block.end),
        block.desc.trim()
    );
    ops::add_block(&path, &heading, block, opts)?;
    println!("{}\n  {}", vault.relativize(&path).display(), block_summary);
    Ok(ExitCode::SUCCESS)
}

// ── ft timeblocks edit ───────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct EditArgs {
    /// Selector: `<N>` (1-indexed line in the section), `<HH:MM>` (exact
    /// start match), or any other string (case-insensitive substring
    /// match on description; ambiguous matches error with a list).
    #[arg(value_name = "SELECTOR")]
    pub selector: String,

    /// New start time. Absolute (`HH:MM`) or relative (`+5m`, `-15m`).
    #[arg(long, value_name = "TIME_OR_DELTA", allow_hyphen_values = true)]
    pub start: Option<String>,

    /// New end time. Absolute (`HH:MM`) or relative (`+5m`, `-15m`).
    #[arg(long, value_name = "TIME_OR_DELTA", allow_hyphen_values = true)]
    pub end: Option<String>,

    /// Replace the description.
    #[arg(long)]
    pub desc: Option<String>,

    /// Add a tag (repeatable). Strictly validated.
    #[arg(long = "add-tag", value_name = "TAG")]
    pub add_tag: Vec<String>,

    /// Remove a tag (repeatable). Strictly validated.
    #[arg(long = "remove-tag", value_name = "TAG")]
    pub remove_tag: Vec<String>,

    #[arg(long, value_name = "DATE")]
    pub date: Option<String>,

    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Show the diff without writing.
    #[arg(long)]
    pub dry_run: bool,
}

fn run_edit(args: EditArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let today = dates::today();
    let date = parse_date_arg(args.date.as_deref(), today)?;
    let path = resolve_path(&vault, date, args.file.as_deref())?;
    let heading = vault.config.config.timeblocks_heading().to_string();

    let selector = parse_selector(&args.selector);
    let mutation = build_edit_mutation(&args)?;

    if args.dry_run {
        let mut doc = Document::read(&path, &heading)?;
        // Use the library's selector + mutation logic by cloning so we
        // can render without writing.
        let idx = match selector.resolve(&doc.blocks) {
            ops::SelectorResult::Found(i) => i,
            ops::SelectorResult::None => {
                return Err(anyhow!("no block matched selector `{}`", args.selector));
            }
            ops::SelectorResult::Ambiguous(candidates) => {
                return Err(ambiguous_error(&args.selector, &candidates, &doc.blocks));
            }
        };
        apply_mutation_for_dry_run(&mut doc.blocks[idx], mutation)?;
        // Re-sort to mirror what ops would write.
        doc.blocks.sort_by_key(|b| b.start);
        for (i, b) in doc.blocks.iter_mut().enumerate() {
            b.source_line = i + 1;
        }
        print_diff(vault.relativize(&path), &doc.source_content, &doc.render());
        return Ok(ExitCode::SUCCESS);
    }

    let doc = ops::edit_block(&path, &heading, &selector, mutation)?;
    // Report the line that was edited — selector::resolve may have moved
    // it after re-sort, so we find it by closest match in the new state.
    println!("Edited {}", vault.relativize(&path).display());
    if let Some(b) = doc.blocks.iter().find(|b| b.source_line == 1) {
        // Print the first block as a basic confirmation; specific block
        // identity isn't tracked through the sort.
        println!(
            "  {}: {} - {} {}",
            b.source_line,
            fmt_hhmm(b.start),
            fmt_hhmm(b.end),
            b.desc
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// `apply_mutation` lives inside `ft_core::timeblock::ops` and is
/// crate-private there. We replicate just enough of it here to drive the
/// dry-run preview — keep this in sync with [`ops::edit_block`].
fn apply_mutation_for_dry_run(b: &mut Timeblock, m: EditMutation) -> Result<()> {
    if let Some(change) = m.start {
        b.start = apply_change(b.start, change);
    }
    if let Some(change) = m.end {
        b.end = apply_change(b.end, change);
        b.end_explicit = true;
    }
    if b.end <= b.start {
        return Err(anyhow!(
            "end {} must be after start {}",
            fmt_hhmm(b.end),
            fmt_hhmm(b.start)
        ));
    }
    if let Some(desc) = m.desc {
        b.desc = desc;
        b.tags = timeblock::parse_tags(&b.desc);
    }
    for tag in m.add_tags {
        let token = tag.to_string_form();
        if !b.desc.split_whitespace().any(|w| w == token) {
            if !b.desc.is_empty() {
                b.desc.push(' ');
            }
            b.desc.push_str(&token);
        }
    }
    b.tags = timeblock::parse_tags(&b.desc);
    for tag in m.remove_tags {
        let token = tag.to_string_form();
        b.desc = strip_token(&b.desc, &token);
    }
    b.tags = timeblock::parse_tags(&b.desc);
    Ok(())
}

fn apply_change(t: NaiveTime, change: TimeChange) -> NaiveTime {
    use chrono::Timelike;
    match change {
        TimeChange::Absolute(t) => t,
        TimeChange::ShiftMinutes(m) => {
            let cur = (t.hour() as i32) * 60 + (t.minute() as i32);
            let new = (cur + m).clamp(0, 23 * 60 + 59);
            NaiveTime::from_hms_opt((new / 60) as u32, (new % 60) as u32, 0).unwrap()
        }
    }
}

fn strip_token(desc: &str, token: &str) -> String {
    let mut out = String::with_capacity(desc.len());
    let mut chars = desc.char_indices().peekable();
    while let Some((i, _c)) = chars.next() {
        if desc[i..].starts_with(token) {
            let next = i + token.len();
            let ends_clean = desc[next..]
                .chars()
                .next()
                .map(|c| c.is_whitespace())
                .unwrap_or(true);
            if ends_clean {
                if out.ends_with(' ') {
                    out.pop();
                }
                while chars.peek().map(|(j, _)| *j < next).unwrap_or(false) {
                    chars.next();
                }
                continue;
            }
        }
        out.push(desc[i..].chars().next().unwrap());
    }
    out
}

// ── ft timeblocks delete ─────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// Selector: same grammar as `ft timeblocks edit`.
    #[arg(value_name = "SELECTOR")]
    pub selector: String,

    #[arg(long, value_name = "DATE")]
    pub date: Option<String>,

    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Skip the interactive confirmation prompt. Required when stdin is
    /// not a TTY.
    #[arg(long)]
    pub yes: bool,

    #[arg(long)]
    pub dry_run: bool,
}

fn run_delete(args: DeleteArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let today = dates::today();
    let date = parse_date_arg(args.date.as_deref(), today)?;
    let path = resolve_path(&vault, date, args.file.as_deref())?;
    let heading = vault.config.config.timeblocks_heading().to_string();

    let selector = parse_selector(&args.selector);

    // Preview the block we're about to remove so the confirmation prompt
    // and the success line have meaningful content.
    let doc = Document::read(&path, &heading)?;
    let idx = match selector.resolve(&doc.blocks) {
        ops::SelectorResult::Found(i) => i,
        ops::SelectorResult::None => {
            return Err(anyhow!("no block matched selector `{}`", args.selector));
        }
        ops::SelectorResult::Ambiguous(candidates) => {
            return Err(ambiguous_error(&args.selector, &candidates, &doc.blocks));
        }
    };
    let target = doc.blocks[idx].clone();
    let summary = format!(
        "- {} - {} {}",
        fmt_hhmm(target.start),
        fmt_hhmm(target.end),
        target.desc
    );

    if args.dry_run {
        let mut new_doc = doc.clone();
        new_doc.blocks.remove(idx);
        print_diff(
            vault.relativize(&path),
            &doc.source_content,
            &new_doc.render(),
        );
        return Ok(ExitCode::SUCCESS);
    }

    if !args.yes {
        let stdin_is_tty = std::io::stdin().is_terminal();
        if !stdin_is_tty {
            return Err(anyhow!(
                "would delete `{summary}` — pass --yes to confirm or --dry-run to preview"
            ));
        }
        let confirmed = dialoguer::Confirm::new()
            .with_prompt(format!("Delete `{summary}`?"))
            .default(false)
            .interact_opt()
            .map_err(|e| anyhow!("confirmation failed: {e}"))?
            .unwrap_or(false);
        if !confirmed {
            return Err(anyhow!("aborted"));
        }
    }

    ops::delete_block(&path, &heading, &selector)?;
    println!(
        "Deleted from {}\n  {summary}",
        vault.relativize(&path).display()
    );
    Ok(ExitCode::SUCCESS)
}

// ── ft timeblocks spent ──────────────────────────────────────────────────────

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpentPeriod {
    Today,
    #[value(name = "this-week")]
    ThisWeek,
    #[value(name = "this-month")]
    ThisMonth,
    #[value(name = "this-year")]
    ThisYear,
    #[value(name = "last-week")]
    LastWeek,
}

#[derive(Args, Debug)]
pub struct SpentArgs {
    /// Preset period. Mutually exclusive with `--from`/`--to`.
    #[arg(value_enum, value_name = "PERIOD", default_value_t = SpentPeriod::Today, conflicts_with_all = ["from", "to"])]
    pub period: SpentPeriod,

    /// Range start (YYYY-MM-DD), inclusive. Requires `--to`.
    #[arg(long, value_name = "DATE")]
    pub from: Option<String>,

    /// Range end (YYYY-MM-DD), inclusive. Requires `--from`.
    #[arg(long, value_name = "DATE")]
    pub to: Option<String>,

    /// Filter blocks by tag prefix (repeatable; multiple compose as OR).
    /// `--tag work` matches `@work` and `@work/meeting`.
    #[arg(long)]
    pub tag: Vec<String>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = SpentFormat::Text)]
    pub format: SpentFormat,

    /// Treat an empty result as a successful run.
    #[arg(long)]
    pub allow_empty: bool,
}

fn run_spent(args: SpentArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    use ft_core::timeblock::report;

    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let today = dates::today();
    let (from, to) = resolve_period(&args, today)?;
    if from > to {
        return Err(anyhow!("--from {} must not be after --to {}", from, to));
    }

    let daily_cfg = vault
        .config
        .config
        .periodic_notes
        .daily
        .as_ref()
        .ok_or_else(|| {
            anyhow!(
                "no `[periodic_notes.daily]` configured — add it to your config to use `ft timeblocks spent`"
            )
        })?;
    let heading = vault.config.config.timeblocks_heading().to_string();

    // Walk every date in [from, to], resolve its daily-note path, and
    // read any blocks that exist. Missing files are silently skipped.
    let mut all_blocks: Vec<Timeblock> = Vec::new();
    let mut cur = from;
    while cur <= to {
        let path = ft_core::periodic::resolve_periodic_path(&vault.path, daily_cfg, cur)?;
        if path.exists() {
            let doc = Document::read(&path, &heading)?;
            all_blocks.extend(doc.blocks);
        }
        cur = cur.succ_opt().ok_or_else(|| anyhow!("date overflow"))?;
    }

    // Apply tag prefix filter (same OR semantics as `list`).
    let filtered: Vec<Timeblock> = if args.tag.is_empty() {
        all_blocks
    } else {
        let needles: Result<Vec<_>> = args
            .tag
            .iter()
            .map(|s| timeblock::parse_tag_string(s).map_err(parse_error_to_anyhow))
            .collect();
        let needles = needles?;
        all_blocks
            .into_iter()
            .filter(|b| {
                b.tags
                    .iter()
                    .any(|t| needles.iter().any(|n| is_prefix(n, t)))
            })
            .collect()
    };

    let total = report::total_minutes(&filtered);
    let tags = report::time_per_tag(&filtered);

    match args.format {
        SpentFormat::Text => render_spent_text(from, to, total, &tags),
        SpentFormat::Json => render_spent_json(from, to, total, &tags)?,
    }

    let exit = if filtered.is_empty() && !args.allow_empty {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    };
    Ok(exit)
}

fn resolve_period(args: &SpentArgs, today: NaiveDate) -> Result<(NaiveDate, NaiveDate)> {
    use ft_core::timeblock::report::{
        last_week_bounds, month_bounds, today_bounds, week_bounds, year_bounds,
    };

    if args.from.is_some() || args.to.is_some() {
        let from = args
            .from
            .as_deref()
            .ok_or_else(|| anyhow!("--to requires --from"))?;
        let to = args
            .to
            .as_deref()
            .ok_or_else(|| anyhow!("--from requires --to"))?;
        let from = NaiveDate::parse_from_str(from, "%Y-%m-%d")
            .map_err(|_| anyhow!("--from must be YYYY-MM-DD, got `{from}`"))?;
        let to = NaiveDate::parse_from_str(to, "%Y-%m-%d")
            .map_err(|_| anyhow!("--to must be YYYY-MM-DD, got `{to}`"))?;
        return Ok((from, to));
    }
    Ok(match args.period {
        SpentPeriod::Today => today_bounds(today),
        SpentPeriod::ThisWeek => week_bounds(today),
        SpentPeriod::ThisMonth => month_bounds(today),
        SpentPeriod::ThisYear => year_bounds(today),
        SpentPeriod::LastWeek => last_week_bounds(today),
    })
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn parse_date_arg(s: Option<&str>, today: NaiveDate) -> Result<NaiveDate> {
    match s {
        Some(s) => dates::parse(s, today).map_err(|e| anyhow!("--date: {e}")),
        None => Ok(today),
    }
}

fn resolve_path(vault: &Vault, date: NaiveDate, file_override: Option<&Path>) -> Result<PathBuf> {
    vault.resolve_target(date, file_override).map_err(|e| {
        anyhow!("{e}\nhint: add `[periodic_notes.daily]` to your config or pass `--file <PATH>`")
    })
}

fn fmt_hhmm(t: NaiveTime) -> String {
    use chrono::Timelike;
    format!("{:02}:{:02}", t.hour(), t.minute())
}

fn parse_hhmm(s: &str) -> Result<NaiveTime> {
    if s.len() == 5 && s.as_bytes()[2] == b':' {
        let h: u32 = s[..2].parse().map_err(|_| anyhow!("bad time `{s}`"))?;
        let m: u32 = s[3..].parse().map_err(|_| anyhow!("bad time `{s}`"))?;
        if h < 24 && m < 60 {
            return Ok(NaiveTime::from_hms_opt(h, m, 0).unwrap());
        }
    }
    Err(anyhow!("expected `HH:MM`, got `{s}`"))
}

/// Parse a time-or-delta argument: `HH:MM` (absolute) or `±N[m]`
/// (relative, minutes). The trailing `m` is optional.
fn parse_time_change(s: &str) -> Result<TimeChange> {
    if let Some(rest) = s.strip_prefix('+') {
        let n = parse_minutes(rest)?;
        Ok(TimeChange::ShiftMinutes(n))
    } else if let Some(rest) = s.strip_prefix('-') {
        let n = parse_minutes(rest)?;
        Ok(TimeChange::ShiftMinutes(-n))
    } else {
        Ok(TimeChange::Absolute(parse_hhmm(s)?))
    }
}

fn parse_minutes(s: &str) -> Result<i32> {
    let s = s.strip_suffix('m').unwrap_or(s);
    s.parse::<i32>()
        .map_err(|_| anyhow!("expected `±N` or `±Nm`, got `{s}`"))
}

fn parse_selector(s: &str) -> Selector {
    if let Ok(n) = s.parse::<usize>() {
        if n > 0 {
            return Selector::Line(n);
        }
    }
    if let Ok(t) = parse_hhmm(s) {
        return Selector::Time(t);
    }
    Selector::Fuzzy(s.to_string())
}

fn ambiguous_error(needle: &str, candidates: &[usize], blocks: &[Timeblock]) -> anyhow::Error {
    let mut lines: Vec<String> = candidates
        .iter()
        .take(5)
        .map(|i| {
            let b = &blocks[*i];
            format!(
                "  {}: {} - {} {}",
                b.source_line,
                fmt_hhmm(b.start),
                fmt_hhmm(b.end),
                b.desc
            )
        })
        .collect();
    if candidates.len() > 5 {
        lines.push(format!("  … and {} more", candidates.len() - 5));
    }
    anyhow!(
        "ambiguous selector `{needle}` — {} blocks matched:\n{}",
        candidates.len(),
        lines.join("\n")
    )
}

fn build_block_from_args(args: &AddArgs) -> Result<Timeblock> {
    if let Some(s) = &args.blockstring {
        return timeblock::parse_line(s).map_err(parse_error_to_anyhow);
    }
    let start = args
        .start
        .as_deref()
        .ok_or_else(|| anyhow!("--start is required (or pass a positional blockstring)"))?;
    let start = parse_hhmm(start)?;
    let end = match args.end.as_deref() {
        Some(s) => parse_hhmm(s)?,
        None => start + chrono::Duration::minutes(30),
    };
    if end <= start {
        return Err(anyhow!(
            "--end {} must be after --start {}",
            fmt_hhmm(end),
            fmt_hhmm(start)
        ));
    }
    let mut desc = args.desc.clone().unwrap_or_default();
    for raw in &args.tag {
        let tag = timeblock::parse_tag_string(raw).map_err(parse_error_to_anyhow)?;
        let token = tag.to_string_form();
        if !desc.is_empty() {
            desc.push(' ');
        }
        desc.push_str(&token);
    }
    let tags = timeblock::parse_tags(&desc);
    Ok(Timeblock {
        start,
        end,
        end_explicit: true,
        desc,
        tags,
        source_line: 0,
    })
}

fn build_edit_mutation(args: &EditArgs) -> Result<EditMutation> {
    let start = args.start.as_deref().map(parse_time_change).transpose()?;
    let end = args.end.as_deref().map(parse_time_change).transpose()?;
    let mut add_tags = Vec::new();
    for raw in &args.add_tag {
        add_tags.push(timeblock::parse_tag_string(raw).map_err(parse_error_to_anyhow)?);
    }
    let mut remove_tags = Vec::new();
    for raw in &args.remove_tag {
        remove_tags.push(timeblock::parse_tag_string(raw).map_err(parse_error_to_anyhow)?);
    }
    Ok(EditMutation {
        start,
        end,
        desc: args.desc.clone(),
        add_tags,
        remove_tags,
    })
}

fn parse_error_to_anyhow(e: ParseError) -> anyhow::Error {
    anyhow!("{e}")
}

fn filter_by_tags<'a>(blocks: &'a [Timeblock], tags: &[String]) -> Result<Vec<&'a Timeblock>> {
    if tags.is_empty() {
        return Ok(blocks.iter().collect());
    }
    let mut needles: Vec<Tag> = Vec::new();
    for raw in tags {
        needles.push(timeblock::parse_tag_string(raw).map_err(parse_error_to_anyhow)?);
    }
    Ok(blocks
        .iter()
        .filter(|b| {
            b.tags
                .iter()
                .any(|t| needles.iter().any(|n| is_prefix(n, t)))
        })
        .collect())
}

/// `prefix` is a prefix of `tag` when its level segments match the
/// leading segments of `tag` one-for-one.
fn is_prefix(prefix: &Tag, tag: &Tag) -> bool {
    if prefix.levels.len() > tag.levels.len() {
        return false;
    }
    prefix
        .levels
        .iter()
        .zip(tag.levels.iter())
        .all(|(a, b)| a == b)
}

// ── output renderers ─────────────────────────────────────────────────────────

fn render_list(blocks: &[&Timeblock], format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Table => render_table(blocks),
        OutputFormat::Json => render_json(blocks)?,
        OutputFormat::Ndjson => render_ndjson(blocks)?,
        OutputFormat::Markdown => render_markdown(blocks),
    }
    Ok(())
}

fn render_table(blocks: &[&Timeblock]) {
    use comfy_table::{ContentArrangement, Table};
    let mut table = Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Line", "Start", "End", "Min", "Desc"]);
    for b in blocks {
        table.add_row(vec![
            b.source_line.to_string(),
            fmt_hhmm(b.start),
            fmt_hhmm(b.end),
            duration_minutes(b).to_string(),
            b.desc.clone(),
        ]);
    }
    println!("{table}");
}

fn render_json(blocks: &[&Timeblock]) -> Result<()> {
    let arr: Vec<serde_json::Value> = blocks.iter().map(|b| block_to_json(b)).collect();
    println!("{}", serde_json::to_string_pretty(&arr)?);
    Ok(())
}

fn render_ndjson(blocks: &[&Timeblock]) -> Result<()> {
    for b in blocks {
        println!("{}", serde_json::to_string(&block_to_json(b))?);
    }
    Ok(())
}

fn render_markdown(blocks: &[&Timeblock]) {
    for b in blocks {
        println!("- {}", timeblock::serialize_line(b));
    }
}

fn block_to_json(b: &Timeblock) -> serde_json::Value {
    let tags: Vec<Vec<String>> = b.tags.iter().map(|t| t.levels.clone()).collect();
    serde_json::json!({
        "line": b.source_line,
        "start": fmt_hhmm(b.start),
        "end": fmt_hhmm(b.end),
        "minutes": duration_minutes(b),
        "desc": b.desc,
        "tags": tags,
    })
}

fn duration_minutes(b: &Timeblock) -> u32 {
    use chrono::Timelike;
    let s = b.start.hour() * 60 + b.start.minute();
    let e = b.end.hour() * 60 + b.end.minute();
    e.saturating_sub(s)
}

fn render_spent_text(
    from: NaiveDate,
    to: NaiveDate,
    total: u32,
    tags: &[ft_core::timeblock::report::TagTime],
) {
    use comfy_table::{presets, ContentArrangement, Table};
    use ft_core::timeblock::report::minutes_to_hours_minutes;

    let summable: u32 = tags
        .iter()
        .filter(|t| t.tag != "break")
        .map(|t| t.minutes)
        .sum();

    let mut table = Table::new();
    table
        .load_preset(presets::UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Tag", "..", "..", "Time", "%"]);

    add_tag_rows(&mut table, tags, 0, summable);

    if from == to {
        println!("Time spent on {from}");
    } else {
        println!("Time spent {from} → {to}");
    }
    println!("{table}");
    let (h, m) = minutes_to_hours_minutes(total);
    println!("{h:02}:{m:02}  total (excluding @break)");
}

fn add_tag_rows(
    table: &mut comfy_table::Table,
    tags: &[ft_core::timeblock::report::TagTime],
    level: usize,
    total_for_pct: u32,
) {
    use ft_core::timeblock::report::minutes_to_hours_minutes;

    for tt in tags {
        let mut row: Vec<String> = Vec::with_capacity(5);
        for _ in 0..level {
            row.push(String::new());
        }
        row.push(tt.tag.clone());
        for _ in level..2 {
            row.push(String::new());
        }
        let (h, m) = minutes_to_hours_minutes(tt.minutes);
        row.push(format!("{h:02}:{m:02}"));
        // Percentages are computed against the non-break total so they
        // remain comparable across reports that include vs exclude breaks.
        let pct = if total_for_pct > 0 && tt.tag != "break" {
            format!("{:3}%", tt.minutes * 100 / total_for_pct)
        } else {
            String::new()
        };
        row.push(pct);
        table.add_row(row);
        add_tag_rows(table, &tt.children, level + 1, total_for_pct);
    }
}

fn render_spent_json(
    from: NaiveDate,
    to: NaiveDate,
    total: u32,
    tags: &[ft_core::timeblock::report::TagTime],
) -> Result<()> {
    let body = serde_json::json!({
        "from": from.to_string(),
        "to": to.to_string(),
        "total_minutes": total,
        "tags": tags.iter().map(tag_time_to_json).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&body)?);
    Ok(())
}

fn tag_time_to_json(tt: &ft_core::timeblock::report::TagTime) -> serde_json::Value {
    serde_json::json!({
        "tag": tt.tag,
        "minutes": tt.minutes,
        "children": tt.children.iter().map(tag_time_to_json).collect::<Vec<_>>(),
    })
}

fn print_diff(path: &Path, original: &str, new: &str) {
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

// ── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_selector_classifies_input() {
        assert!(matches!(parse_selector("3"), Selector::Line(3)));
        assert!(matches!(parse_selector("0"), Selector::Fuzzy(_))); // 0 isn't a valid line
        assert!(matches!(parse_selector("09:00"), Selector::Time(_)));
        assert!(matches!(parse_selector("standup"), Selector::Fuzzy(_)));
        assert!(matches!(parse_selector("09:00 chat"), Selector::Fuzzy(_)));
    }

    #[test]
    fn parse_time_change_absolute_and_relative() {
        assert!(matches!(
            parse_time_change("09:00").unwrap(),
            TimeChange::Absolute(_)
        ));
        assert_eq!(
            parse_time_change("+5m").unwrap(),
            TimeChange::ShiftMinutes(5)
        );
        assert_eq!(
            parse_time_change("-15").unwrap(),
            TimeChange::ShiftMinutes(-15)
        );
    }

    #[test]
    fn tag_prefix_matching() {
        let work = timeblock::parse_tag_string("work").unwrap();
        let work_meeting = timeblock::parse_tag_string("work/meeting").unwrap();
        let other = timeblock::parse_tag_string("personal").unwrap();
        assert!(is_prefix(&work, &work_meeting));
        assert!(is_prefix(&work, &work));
        assert!(!is_prefix(&work_meeting, &work));
        assert!(!is_prefix(&work, &other));
    }
}

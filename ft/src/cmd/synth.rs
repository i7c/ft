//! `ft synth` — scaffold protected sections into a synth note from
//! the multi-source journal, plus a `verify` sub-subcommand for
//! checking on-disk synth notes against their pinned sources.
//!
//! Scaffold flow (`ft synth <target.md> --link "[[Foo]]" ...`):
//! 1. Resolve each `--link` to a graph target (note or ghost).
//! 2. Build the multi-source journal for those targets.
//! 3. Apply optional in-window filter when `--in-window` + a window
//!    flag are present.
//! 4. Optionally extend the entry set with `--from <path>:<line>`
//!    paragraphs picked directly.
//! 5. `plan_synth_scaffold` → `apply_synth_scaffold` → editor handoff
//!    (unless `--no-edit`).
//!
//! Verify flow (`ft synth verify [<note.md> | --all]`): walks the
//! requested notes through [`ft_core::synth::verify::verify_synth_note`]
//! / [`verify_all`] and prints per-section status. Exit code is 0
//! when every section is `Ok`, else 1.

use std::collections::HashSet;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcCommand, ExitCode};

use anyhow::{anyhow, Context, Result};
use chrono::NaiveDate;
use clap::{Args, Subcommand};
use ft_core::blame_cache::{paragraph_date, BlameCache};
use ft_core::graph::{Graph, NodeKind, NoteId};
use ft_core::journal::{build_journal, JournalEntry};
use ft_core::link_review::{compute_link_review, WindowRange};
use ft_core::synth::scaffold::{apply_synth_scaffold, plan_synth_scaffold};
use ft_core::synth::verify::{verify_all, verify_synth_note, SectionStatus, VerificationResult};
use ft_core::vault::{Scan, Vault};

#[derive(Args, Debug)]
pub struct SynthArgs {
    #[command(subcommand)]
    pub command: SynthCommand,
}

#[derive(Subcommand, Debug)]
pub enum SynthCommand {
    /// Scaffold protected sections into a target synth note (creating
    /// it with `ft-synth: true` frontmatter if needed). Default action.
    #[command(name = "scaffold")]
    Scaffold(ScaffoldArgs),
    /// Verify on-disk synth notes against their pinned sources.
    Verify(VerifyArgs),
}

pub fn run(args: SynthArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    match args.command {
        SynthCommand::Scaffold(a) => run_scaffold(a, vault_flag),
        SynthCommand::Verify(a) => run_verify(a, vault_flag),
    }
}

// ── scaffold ─────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct ScaffoldArgs {
    /// Target synth note (vault-relative). Created if missing, appended
    /// to otherwise. `.md` extension is added when missing.
    #[arg(value_name = "TARGET.md")]
    pub target: PathBuf,

    /// A `[[wikilink]]` to source paragraphs from. Repeatable.
    /// At least one of `--link` or `--from` is required.
    #[arg(long, value_name = "LINK")]
    pub link: Vec<String>,

    /// Explicit source paragraph: `<vault-relative-path>:<line>`.
    /// Repeatable. Identifies the paragraph whose `line_start` equals
    /// `<line>` in the named file. Use with or instead of `--link`.
    #[arg(long, value_name = "PATH:LINE")]
    pub from: Vec<String>,

    /// Duration window for `--link` sourcing: `7d`, `24h`, `2w`, `1m`.
    /// Mutually exclusive with `--range`. Only takes effect when
    /// combined with `--in-window`; otherwise all-time mentions are
    /// included.
    #[arg(long, value_name = "DURATION", conflicts_with = "range")]
    pub since: Option<String>,

    /// Commit range `X..Y` (two git refs). Mutually exclusive with
    /// `--since`. Same semantics as `--since`.
    #[arg(long, value_name = "X..Y", conflicts_with = "since")]
    pub range: Option<String>,

    /// Restrict `--link`-sourced entries to paragraphs touched by the
    /// window. Requires `--since` or `--range`.
    #[arg(long)]
    pub in_window: bool,

    /// Skip launching `$EDITOR` after writing.
    #[arg(long)]
    pub no_edit: bool,
}

fn run_scaffold(args: ScaffoldArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    if args.link.is_empty() && args.from.is_empty() {
        return Err(anyhow!(
            "one of --link or --from is required (no entries to scaffold)"
        ));
    }
    if args.in_window && args.since.is_none() && args.range.is_none() {
        return Err(anyhow!("--in-window requires --since or --range"));
    }

    let vault = Vault::discover(vault_flag).context("could not locate an Obsidian vault")?;
    ft_core::git::discover_repo(&vault.path).ok_or_else(|| {
        anyhow!("vault is not inside a git repository — `ft synth` needs git history")
    })?;
    let graph = Graph::build(&vault, &Scan::default()).context("building note graph")?;
    let target = normalize_md_target(&args.target);

    let mut entries: Vec<JournalEntry> = Vec::new();

    // ── --link sourcing via multi-target journal ─────────────────────
    if !args.link.is_empty() {
        let targets: Vec<NoteId> = args
            .link
            .iter()
            .filter_map(|s| resolve_link_to_id(&graph, s))
            .collect();
        if targets.is_empty() {
            return Err(anyhow!(
                "none of the --link values resolved to a note or ghost in the vault"
            ));
        }
        let mut cache = BlameCache::load(&vault.path).context("loading blame cache")?;
        let report = build_journal(&graph, &targets, &vault, &vault.path, &mut cache)
            .context("building multi-source journal")?;
        let _ = cache.save(&vault.path);

        let filtered = if args.in_window {
            let window = resolve_window(&args.since, &args.range)?
                .expect("validated above: in_window implies since/range");
            let cfg = vault.config.config.synth.clone();
            let review = compute_link_review(&graph, &vault, &vault.path, &window, &cfg)
                .context("computing in-window filter")?;
            report
                .entries
                .into_iter()
                .filter(|e| entry_overlaps_window(e, &review.added_lines))
                .collect()
        } else {
            report.entries
        };
        entries.extend(filtered);
    }

    // ── --from sourcing (explicit paragraph picks) ───────────────────
    for spec in &args.from {
        let (path, line) = parse_from_spec(spec)?;
        let entry = pick_paragraph(&graph, &vault, &path, line)?;
        entries.push(entry);
    }

    if entries.is_empty() {
        return Err(anyhow!(
            "no entries to scaffold (multi-source journal was empty and no --from picks supplied)"
        ));
    }

    // Dedup by (source_path, line_start) — same paragraph picked by
    // multiple --link targets shouldn't double up in the scaffold.
    entries = dedup_entries(entries);

    let plan = plan_synth_scaffold(&vault, &vault.path, &target, &entries)
        .context("planning synth scaffold")?;
    let written = apply_synth_scaffold(&vault, &plan).context("writing synth scaffold")?;

    let rel = written
        .strip_prefix(&vault.path)
        .unwrap_or(&written)
        .display()
        .to_string();
    if plan.create {
        println!("created {} with {} section(s)", rel, plan.sections.len());
    } else {
        println!("appended {} section(s) to {}", plan.sections.len(), rel);
    }

    if !args.no_edit {
        open_editor(&written)?;
    }

    Ok(ExitCode::SUCCESS)
}

/// Append `.md` to a target if missing.
fn normalize_md_target(p: &Path) -> PathBuf {
    if p.extension().and_then(|s| s.to_str()) == Some("md") {
        p.to_path_buf()
    } else {
        let mut s = p.as_os_str().to_owned();
        s.push(".md");
        PathBuf::from(s)
    }
}

/// Resolve a CLI link argument (`"[[Foo]]"`, `"Foo"`, or even a path
/// stem) to a graph `NoteId` (or ghost). Returns `None` when nothing
/// matches; the caller decides whether to error.
fn resolve_link_to_id(graph: &Graph, raw: &str) -> Option<NoteId> {
    let trimmed = raw
        .trim()
        .trim_start_matches("[[")
        .trim_end_matches("]]")
        .trim();
    if trimmed.is_empty() {
        return None;
    }
    // 1. Existing note by title (case-insensitive title index).
    for (id, node) in graph.nodes() {
        if let NodeKind::Note(n) = node {
            if n.title.eq_ignore_ascii_case(trimmed) {
                return Some(id);
            }
        }
    }
    // 2. Existing ghost by raw.
    if let Some(id) = graph.ghost_by_raw(trimmed) {
        return Some(id);
    }
    None
}

fn resolve_window(since: &Option<String>, range: &Option<String>) -> Result<Option<WindowRange>> {
    if let Some(s) = since {
        let dur = WindowRange::parse_since(s)
            .ok_or_else(|| anyhow!("invalid --since value `{s}` (try e.g. 7d, 24h, 2w, 1m)"))?;
        return Ok(Some(WindowRange::Since(dur)));
    }
    if let Some(r) = range {
        let (from, to) = r
            .split_once("..")
            .ok_or_else(|| anyhow!("invalid --range value `{r}` (expected `X..Y`)"))?;
        if from.is_empty() || to.is_empty() {
            return Err(anyhow!(
                "invalid --range value `{r}` (both X and Y required)"
            ));
        }
        return Ok(Some(WindowRange::Range {
            from: from.to_string(),
            to: to.to_string(),
        }));
    }
    Ok(None)
}

fn entry_overlaps_window(
    entry: &JournalEntry,
    added_lines: &std::collections::HashMap<PathBuf, std::collections::BTreeSet<u32>>,
) -> bool {
    let Some(lines) = added_lines.get(&entry.source_path) else {
        return false;
    };
    (entry.line_start..=entry.line_end).any(|ln| lines.contains(&ln))
}

/// Parse `<path>:<line>` into its parts. Rejects ambiguous forms (e.g.
/// no colon, non-numeric tail).
fn parse_from_spec(spec: &str) -> Result<(PathBuf, u32)> {
    let (path, line) = spec
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("invalid --from `{spec}` (expected `<path>:<line>`)"))?;
    let line: u32 = line
        .parse()
        .map_err(|_| anyhow!("invalid --from `{spec}` (line must be a positive integer)"))?;
    Ok((PathBuf::from(path), line))
}

/// Build a [`JournalEntry`] for the paragraph at `(path, line_start)`.
fn pick_paragraph(
    graph: &Graph,
    vault: &Vault,
    path: &Path,
    line_start: u32,
) -> Result<JournalEntry> {
    let p_id = graph
        .paragraph_by_loc(path, line_start)
        .ok_or_else(|| anyhow!("no paragraph found at {}:{}", path.display(), line_start))?;
    let NodeKind::Paragraph(p) = graph.node(p_id) else {
        return Err(anyhow!(
            "node at {}:{} is not a paragraph",
            path.display(),
            line_start
        ));
    };
    let source_title = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Resolve date via blame, best-effort.
    let mut cache = BlameCache::load(&vault.path).unwrap_or_default();
    let head = ft_core::git::head_hash(&vault.path).unwrap_or_default();
    let date = if cache.get(&path.to_string_lossy(), &head).is_some() {
        cache
            .get(&path.to_string_lossy(), &head)
            .and_then(|blame| paragraph_date(blame, p.line_start, p.line_end))
            .unwrap_or_else(today_naive)
    } else if let Ok(blame) = ft_core::git::blame_file(&vault.path, path) {
        cache.insert(path.to_string_lossy().into_owned(), head.clone(), blame);
        cache
            .get(&path.to_string_lossy(), &head)
            .and_then(|blame| paragraph_date(blame, p.line_start, p.line_end))
            .unwrap_or_else(today_naive)
    } else {
        today_naive()
    };
    let _ = cache.save(&vault.path);
    Ok(JournalEntry {
        source_title,
        source_path: p.source_file.clone(),
        line_start: p.line_start,
        line_end: p.line_end,
        section_text: p.text.clone(),
        date,
        matched: vec![],
    })
}

fn today_naive() -> NaiveDate {
    ft_core::dates::today()
}

/// Dedup journal entries by `(source_path, line_start)`. Sorts by date
/// desc afterward to preserve "newest first" scaffold order.
fn dedup_entries(mut entries: Vec<JournalEntry>) -> Vec<JournalEntry> {
    let mut seen: HashSet<(PathBuf, u32)> = HashSet::new();
    entries.retain(|e| seen.insert((e.source_path.clone(), e.line_start)));
    entries.sort_by(|a, b| {
        b.date
            .cmp(&a.date)
            .then_with(|| a.source_title.cmp(&b.source_title))
    });
    entries
}

fn open_editor(path: &Path) -> Result<()> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".into());
    let status = ProcCommand::new(&editor)
        .arg(path)
        .status()
        .with_context(|| format!("failed to spawn editor `{editor}`"))?;
    if !status.success() {
        return Err(anyhow!("editor exited with non-zero status"));
    }
    Ok(())
}

// ── verify ───────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct VerifyArgs {
    /// Verify a single synth note (vault-relative path).
    #[arg(value_name = "NOTE.md", conflicts_with = "all")]
    pub note: Option<PathBuf>,

    /// Verify every `.md` marked `ft-synth: true` in the vault.
    #[arg(long, conflicts_with = "note")]
    pub all: bool,

    /// JSON output.
    #[arg(long)]
    pub json: bool,

    /// Disable colored output.
    #[arg(long)]
    pub no_color: bool,
}

fn run_verify(args: VerifyArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    if args.note.is_none() && !args.all {
        return Err(anyhow!("provide a NOTE.md path or pass --all"));
    }
    let vault = Vault::discover(vault_flag).context("could not locate an Obsidian vault")?;
    ft_core::git::discover_repo(&vault.path).ok_or_else(|| {
        anyhow!("vault is not inside a git repository — `ft synth verify` needs git history")
    })?;

    let groups: Vec<(PathBuf, Vec<VerificationResult>)> = if let Some(note) = args.note {
        let results = verify_synth_note(&vault.path, &vault, &note)
            .with_context(|| format!("verifying {}", note.display()))?;
        vec![(note, results)]
    } else {
        verify_all(&vault, &vault.path).context("walking synth notes")?
    };

    let any_fail = groups
        .iter()
        .any(|(_, rs)| rs.iter().any(|r| r.status != SectionStatus::Ok));

    if args.json {
        render_verify_json(&groups)?;
    } else {
        let use_color =
            !args.no_color && std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal();
        render_verify_text(&groups, use_color);
    }
    Ok(if any_fail {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn render_verify_text(groups: &[(PathBuf, Vec<VerificationResult>)], use_color: bool) {
    use owo_colors::OwoColorize;
    if groups.is_empty() {
        println!("no synth notes found");
        return;
    }
    let mut first = true;
    for (note_path, results) in groups {
        if !first {
            println!();
        }
        first = false;
        let header = note_path.display().to_string();
        if use_color {
            println!("{}", header.bold());
        } else {
            println!("{header}");
        }
        if results.is_empty() {
            println!("  (no [!ft-source] callouts)");
            continue;
        }
        for r in results {
            let tag = match r.status {
                SectionStatus::Ok => "ok",
                SectionStatus::Drifted => "drifted",
                SectionStatus::SourceMissing => "source-missing",
                SectionStatus::Malformed => "malformed",
            };
            let line = format!(
                "  {tag:14} | {}:{} → {} L{}-{} @{}",
                note_path.display(),
                r.header_line,
                r.source_path.display(),
                r.line_start,
                r.line_end,
                r.commit_sha
            );
            if !use_color || matches!(r.status, SectionStatus::Ok) {
                println!("{line}");
            } else {
                println!("{}", line.red());
            }
            if !r.detail.is_empty() && r.status != SectionStatus::Ok {
                println!("      {}", r.detail);
            }
        }
    }
}

fn render_verify_json(groups: &[(PathBuf, Vec<VerificationResult>)]) -> Result<()> {
    #[derive(serde::Serialize)]
    struct Row<'a> {
        note: String,
        header_line: u32,
        source_path: String,
        line_start: u32,
        line_end: u32,
        commit_sha: &'a str,
        status: &'static str,
        detail: &'a str,
    }
    let mut rows: Vec<Row> = Vec::new();
    for (note, results) in groups {
        for r in results {
            let status = match r.status {
                SectionStatus::Ok => "ok",
                SectionStatus::Drifted => "drifted",
                SectionStatus::SourceMissing => "source-missing",
                SectionStatus::Malformed => "malformed",
            };
            rows.push(Row {
                note: note.display().to_string(),
                header_line: r.header_line,
                source_path: r.source_path.display().to_string(),
                line_start: r.line_start,
                line_end: r.line_end,
                commit_sha: &r.commit_sha,
                status,
                detail: &r.detail,
            });
        }
    }
    let s = serde_json::to_string_pretty(&rows).context("serialize verify json")?;
    println!("{s}");
    Ok(())
}

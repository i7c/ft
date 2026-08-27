//! `ft synth` — scaffold protected sections into a synth note from the
//! paragraph search index, plus `verify`/`repair`/`reslice` for
//! checking on-disk synth notes against their pinned sources.
//!
//! Scaffold flow (`ft synth <target.md> --search "<query>" ...`):
//! 1. Parse the query and search the scan-derived index (substring
//!    default, `=word`, `~fuzzy`, `"phrase"`, `[[link]]`, `-exclude`;
//!    AND by default, `--any` for OR).
//! 2. Sort by relevance or blame date (`--sort date`).
//! 3. Optionally extend the set with `--from <path>:<line>` paragraphs
//!    picked directly.
//! 4. `plan_synth_scaffold` → `apply_synth_scaffold` → editor handoff
//!    (unless `--no-edit`).
//!
//! The `--link` form is a deprecated transitional alias: it lowers to
//! an any-mode search for the given links (no Related-alias
//! resolution). Verify flow (`ft synth verify [<note.md> | --all]`):
//! walks the requested notes through
//! [`ft_core::synth::verify::verify_synth_note`] / [`verify_all`] and
//! prints per-section status. Exit code is 0 when every section is
//! `Ok`, else 1.

use std::collections::HashSet;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcCommand, ExitCode};

use crate::cmd::search::SortArg;
use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use ft_core::blame_cache::BlameCache;
use ft_core::graph::{Graph, NodeKind};
use ft_core::search::{parse_query, search, search_with_dates, SearchIndex};
use ft_core::synth::repair::{
    apply_synth_repair, plan_repair_all, plan_synth_repair, RepairAction, SynthRepairPlan,
};
use ft_core::synth::reslice::{apply_reslice, plan_reslice, NewRange};
use ft_core::synth::scaffold::{apply_synth_scaffold, plan_synth_scaffold};
use ft_core::synth::verify::{verify_all, verify_synth_note, SectionStatus, VerificationResult};

#[derive(Subcommand, Debug)]
pub enum SynthCommand {
    /// Scaffold protected sections into a target synth note (creating
    /// it with `ft.synth.enabled: true` frontmatter if needed). Default action.
    #[command(name = "scaffold")]
    Scaffold(ScaffoldArgs),
    /// Grow or shrink a protected section's line range, re-pinned at its
    /// existing commit.
    Reslice(ResliceArgs),
    /// Verify on-disk synth notes against their pinned sources.
    Verify(VerifyArgs),
    /// Repair broken [!ft-source] pins: rehash valid-but-mislabeled
    /// sections and re-pin stranded ones to HEAD by locating the quoted
    /// body in the current source.
    Repair(RepairArgs),
}

pub fn run_command(command: SynthCommand, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    match command {
        SynthCommand::Scaffold(a) => run_scaffold(a, vault_flag),
        SynthCommand::Reslice(a) => run_reslice(a, vault_flag),
        SynthCommand::Verify(a) => run_verify(a, vault_flag),
        SynthCommand::Repair(a) => run_repair(a, vault_flag),
    }
}

// ── scaffold ─────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct ScaffoldArgs {
    /// Target synth note (vault-relative). Created if missing, appended
    /// to otherwise. `.md` extension is added when missing.
    #[arg(value_name = "TARGET.md")]
    pub target: PathBuf,

    /// Search query sourcing every matching paragraph (see
    /// `ft notes search`: substring, `=word`, `~fuzzy`, `"phrase"`,
    /// `[[link]]`, `-exclude`; AND by default).
    #[arg(long, value_name = "QUERY")]
    pub search: Option<String>,

    /// A `[[wikilink]]` to source paragraphs from. Repeatable.
    /// DEPRECATED transitional form: lowers to an any-mode search for
    /// the given links; prefer `--search`.
    #[arg(long, value_name = "LINK")]
    pub link: Vec<String>,

    /// Any-mode: a paragraph matching ANY clause qualifies (default:
    /// every clause must match). Implied for `--link`.
    #[arg(long)]
    pub any: bool,

    /// Sort order for search-sourced sections: relevance (default) or
    /// date (newest edit first).
    #[arg(long, value_enum, default_value = "relevance")]
    pub sort: SortArg,

    /// Explicit source paragraph: `<vault-relative-path>:<line>`.
    /// Repeatable. Identifies the paragraph whose `line_start` equals
    /// `<line>` in the named file. Use with or instead of `--search`.
    #[arg(long, value_name = "PATH:LINE")]
    pub from: Vec<String>,

    /// Skip launching `$EDITOR` after writing.
    #[arg(long)]
    pub no_edit: bool,
}

fn run_scaffold(args: ScaffoldArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    if args.search.is_none() && args.link.is_empty() && args.from.is_empty() {
        return Err(anyhow!(
            "one of --search, --link, or --from is required (no entries to scaffold)"
        ));
    }

    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    ft_core::git::discover_repo(&vault.path).ok_or_else(|| {
        anyhow!("vault is not inside a git repository — `ft synth` needs git history")
    })?;
    let scan = vault.scan();
    let target = normalize_md_target(&args.target);

    let mut sources: Vec<ft_core::synth::source::SynthSource> = Vec::new();

    // ── --search / --link sourcing via the search index ─────────────
    // `--link` lowers to an any-mode search over the given links
    // (any-mode: a paragraph mentioning any link qualifies).
    let query_text = if let Some(q) = &args.search {
        Some(q.clone())
    } else if !args.link.is_empty() {
        Some(
            args.link
                .iter()
                .map(|l| to_link_clause(l))
                .collect::<Vec<_>>()
                .join(" "),
        )
    } else {
        None
    };

    if let Some(q) = query_text {
        let any = args.any || !args.link.is_empty();
        let query = parse_query(&q, any);
        let exclude = vault.config.config.synth.exclude_prefixes.clone();
        let index = SearchIndex::build(&scan, &exclude);
        let results = match args.sort {
            SortArg::Relevance => search(&index, &query),
            SortArg::Date => {
                let mut cache = BlameCache::load(&vault.path).context("loading blame cache")?;
                let results = search_with_dates(&index, &query, &vault, &mut cache)
                    .context("searching with dates")?;
                let _ = cache.save(&vault.path);
                results
            }
        };
        for r in results {
            sources.push(ft_core::synth::source::SynthSource {
                source_path: r.path,
                line_start: r.line_start,
                line_end: r.line_end,
                body: r.body,
            });
        }
    }

    // ── --from sourcing (explicit paragraph picks) ───────────────────
    if !args.from.is_empty() {
        let graph = crate::cmd::common::build_graph(&scan)?;
        for spec in &args.from {
            let (path, line) = parse_from_spec(spec)?;
            sources.push(pick_paragraph(&graph, &path, line)?);
        }
    }

    if sources.is_empty() {
        return Err(anyhow!(
            "no entries to scaffold (search returned nothing and no --from picks supplied)"
        ));
    }

    // Dedup by (source_path, line_start) — the same paragraph picked by
    // several clauses or by both search and --from shouldn't double up.
    sources = dedup_sources(sources);

    let plan = plan_synth_scaffold(&vault, &target, &sources).context("planning synth scaffold")?;
    let written = apply_synth_scaffold(&vault, &plan).context("writing synth scaffold")?;

    let rel = vault.relativize(&written).display().to_string();
    if plan.create {
        println!("created {} with {} section(s)", rel, plan.sections.len());
    } else if plan.dedup_skipped > 0 {
        println!(
            "appended {} section(s) to {} ({} already pinned, skipped)",
            plan.sections.len(),
            rel,
            plan.dedup_skipped
        );
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

/// Lower a deprecated `--link` value (`"Foo"` or `"[[Foo]]"`) to a
/// `[[…]]` search clause.
fn to_link_clause(raw: &str) -> String {
    let t = raw.trim();
    if t.starts_with("[[") && t.ends_with("]]") {
        t.to_string()
    } else {
        format!("[[{t}]]")
    }
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

/// Build a [`SynthSource`] for the paragraph at `(path, line_start)`.
fn pick_paragraph(
    graph: &Graph,
    path: &Path,
    line_start: u32,
) -> Result<ft_core::synth::source::SynthSource> {
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
    Ok(ft_core::synth::source::SynthSource {
        source_path: p.source_file.clone(),
        line_start: p.line_start,
        line_end: p.line_end,
        body: p.text.clone(),
    })
}

/// Dedup sources by `(source_path, line_start)`. Order is preserved:
/// search results arrive pre-sorted (relevance or date), and `--from`
/// picks append after them.
fn dedup_sources(
    mut sources: Vec<ft_core::synth::source::SynthSource>,
) -> Vec<ft_core::synth::source::SynthSource> {
    let mut seen: HashSet<(PathBuf, u32)> = HashSet::new();
    sources.retain(|s| seen.insert((s.source_path.clone(), s.line_start)));
    sources
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

// ── reslice ────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct ResliceArgs {
    /// Synth note holding the section (vault-relative path).
    #[arg(value_name = "NOTE.md")]
    pub note: PathBuf,

    /// Header line of the `[!ft-source]` section to reslice (the line
    /// number `ft synth verify` prints). Optional when the note has a
    /// single section.
    #[arg(long, value_name = "LINE")]
    pub at: Option<u32>,

    /// Absolute new range `A-B` (1-indexed inclusive). Mutually
    /// exclusive with `--up` / `--down`.
    #[arg(long, value_name = "A-B", conflicts_with_all = ["up", "down"])]
    pub lines: Option<String>,

    /// Lines of context to add above the start (negative shrinks).
    #[arg(long, value_name = "N", allow_hyphen_values = true)]
    pub up: Option<i32>,

    /// Lines of context to add below the end (negative shrinks).
    #[arg(long, value_name = "N", allow_hyphen_values = true)]
    pub down: Option<i32>,
}

fn run_reslice(args: ResliceArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let range = parse_reslice_range(&args)?;

    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    ft_core::git::discover_repo(&vault.path).ok_or_else(|| {
        anyhow!("vault is not inside a git repository — `ft synth reslice` needs git history")
    })?;

    let note = normalize_md_target(&args.note);
    let plan = plan_reslice(&vault, &note, args.at, range).context("planning reslice")?;
    let written = apply_reslice(&vault, &plan).context("writing reslice")?;

    let rel = vault.relativize(&written).display().to_string();
    let n = &plan.new;
    println!(
        "resliced {} → {} L{}-{} @{}",
        rel,
        n.source_path.display(),
        n.line_start,
        n.line_end,
        n.commit_sha
    );
    if plan.healed_drift {
        println!("note: section had drifted; body reset to the pinned source");
    }

    // Re-verify the touched section so the user sees it landed `ok`.
    if let Ok(results) = verify_synth_note(&vault, &note) {
        if let Some(r) = results.iter().find(|r| r.line_start == n.line_start) {
            let tag = match r.status {
                SectionStatus::Ok => "ok",
                SectionStatus::Drifted => "drifted",
                SectionStatus::SourceMissing => "source-missing",
                SectionStatus::Malformed => "malformed",
            };
            println!("verify: {tag}");
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// Turn the `--lines` / `--up` / `--down` flags into a [`NewRange`],
/// rejecting the empty case.
fn parse_reslice_range(args: &ResliceArgs) -> Result<NewRange> {
    if let Some(spec) = &args.lines {
        let (a, b) = spec
            .split_once('-')
            .ok_or_else(|| anyhow!("invalid --lines `{spec}` (expected `A-B`)"))?;
        let start: u32 = a
            .trim()
            .parse()
            .map_err(|_| anyhow!("invalid --lines `{spec}` (A must be a positive integer)"))?;
        let end: u32 = b
            .trim()
            .parse()
            .map_err(|_| anyhow!("invalid --lines `{spec}` (B must be a positive integer)"))?;
        return Ok(NewRange::Absolute { start, end });
    }
    if args.up.is_none() && args.down.is_none() {
        return Err(anyhow!(
            "provide --lines A-B or at least one of --up / --down"
        ));
    }
    Ok(NewRange::Delta {
        up: args.up.unwrap_or(0),
        down: args.down.unwrap_or(0),
    })
}

// ── verify ───────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct VerifyArgs {
    /// Verify a single synth note (vault-relative path).
    #[arg(value_name = "NOTE.md", conflicts_with = "all")]
    pub note: Option<PathBuf>,

    /// Verify every `.md` marked `ft.synth.enabled: true` in the vault.
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
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    ft_core::git::discover_repo(&vault.path).ok_or_else(|| {
        anyhow!("vault is not inside a git repository — `ft synth verify` needs git history")
    })?;

    let groups: Vec<(PathBuf, Vec<VerificationResult>)> = if let Some(note) = args.note {
        let results = verify_synth_note(&vault, &note)
            .with_context(|| format!("verifying {}", note.display()))?;
        vec![(note, results)]
    } else {
        verify_all(&vault).context("walking synth notes")?
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

// ── repair ───────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct RepairArgs {
    /// Repair a single synth note (vault-relative path).
    #[arg(value_name = "NOTE.md", conflicts_with = "all")]
    pub note: Option<PathBuf>,

    /// Repair every `.md` marked `ft.synth.enabled: true` in the vault.
    #[arg(long, conflicts_with = "note")]
    pub all: bool,

    /// Show what would change without writing anything.
    #[arg(long)]
    pub dry_run: bool,

    /// JSON output.
    #[arg(long)]
    pub json: bool,

    /// Disable colored output.
    #[arg(long)]
    pub no_color: bool,
}

fn run_repair(args: RepairArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    if args.note.is_none() && !args.all {
        return Err(anyhow!("provide a NOTE.md path or pass --all"));
    }
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    ft_core::git::discover_repo(&vault.path).ok_or_else(|| {
        anyhow!("vault is not inside a git repository — `ft synth repair` needs git history")
    })?;

    let plans: Vec<SynthRepairPlan> = if let Some(note) = args.note {
        let plan = plan_synth_repair(&vault, &note)
            .with_context(|| format!("planning repair of {}", note.display()))?;
        vec![plan]
    } else {
        plan_repair_all(&vault).context("walking synth notes")?
    };

    if !args.dry_run {
        for plan in &plans {
            apply_synth_repair(&vault, plan)
                .with_context(|| format!("repairing {}", plan.note.display()))?;
        }
    }

    let any_unrecoverable = plans.iter().any(|p| p.unrecoverable().next().is_some());

    if args.json {
        render_repair_json(&plans, args.dry_run)?;
    } else {
        let use_color =
            !args.no_color && std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal();
        render_repair_text(&plans, args.dry_run, use_color);
    }
    // Mirror `verify`: broken provenance that remains broken is a
    // failing exit so scripts can gate on it.
    Ok(if any_unrecoverable {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn repair_action_tag(action: &RepairAction) -> &'static str {
    match action {
        RepairAction::AlreadyOk => "ok",
        RepairAction::Rehashed => "rehashed",
        RepairAction::Repinned { .. } => "repinned",
        RepairAction::Unrecoverable { .. } => "unrecoverable",
    }
}

fn render_repair_text(plans: &[SynthRepairPlan], dry_run: bool, use_color: bool) {
    use owo_colors::OwoColorize;
    if plans.is_empty() {
        println!("no synth notes found");
        return;
    }
    let verb = if dry_run { "would repair" } else { "repaired" };
    let mut first = true;
    for plan in plans {
        if !first {
            println!();
        }
        first = false;
        let header = plan.note.display().to_string();
        if use_color {
            println!("{}", header.bold());
        } else {
            println!("{header}");
        }
        if plan.sections.is_empty() {
            println!("  (no [!ft-source] callouts)");
            continue;
        }
        for s in &plan.sections {
            let tag = repair_action_tag(&s.action);
            let mut line = format!(
                "  {tag:14} | {}:{} → {} L{}-{} @{}",
                plan.note.display(),
                s.header_line,
                s.old.source_path.display(),
                s.old.line_start,
                s.old.line_end,
                s.old.commit_sha
            );
            if let Some(new) = &s.new {
                line.push_str(&format!(
                    " ⇒ L{}-{} @{} #{}",
                    new.line_start, new.line_end, new.commit_sha, new.content_hash
                ));
            }
            match &s.action {
                RepairAction::AlreadyOk => println!("{line}"),
                RepairAction::Unrecoverable { reason } => {
                    if use_color {
                        println!("{}", line.red());
                    } else {
                        println!("{line}");
                    }
                    println!("      {reason}");
                }
                RepairAction::Repinned { matches } => {
                    if use_color {
                        println!("{}", line.green());
                    } else {
                        println!("{line}");
                    }
                    if *matches > 1 {
                        println!(
                            "      {matches} candidate locations; nearest to the old range chosen"
                        );
                    }
                }
                RepairAction::Rehashed => {
                    if use_color {
                        println!("{}", line.green());
                    } else {
                        println!("{line}");
                    }
                }
            }
        }
        let changed = plan.changed().count();
        let broken = plan.unrecoverable().count();
        let mut summary = format!("  {verb} {changed} section(s)");
        if broken > 0 {
            summary.push_str(&format!(", {broken} unrecoverable"));
        }
        println!("{summary}");
    }
}

fn render_repair_json(plans: &[SynthRepairPlan], dry_run: bool) -> Result<()> {
    #[derive(serde::Serialize)]
    struct Row {
        note: String,
        header_line: u32,
        source_path: String,
        action: &'static str,
        old_range: [u32; 2],
        old_sha: String,
        new_range: Option<[u32; 2]>,
        new_sha: Option<String>,
        detail: String,
        applied: bool,
    }
    let mut rows: Vec<Row> = Vec::new();
    for plan in plans {
        for s in &plan.sections {
            let detail = match &s.action {
                RepairAction::Unrecoverable { reason } => reason.clone(),
                RepairAction::Repinned { matches } if *matches > 1 => {
                    format!("{matches} candidate locations; nearest chosen")
                }
                _ => String::new(),
            };
            rows.push(Row {
                note: plan.note.display().to_string(),
                header_line: s.header_line,
                source_path: s.old.source_path.display().to_string(),
                action: repair_action_tag(&s.action),
                old_range: [s.old.line_start, s.old.line_end],
                old_sha: s.old.commit_sha.clone(),
                new_range: s.new.as_ref().map(|n| [n.line_start, n.line_end]),
                new_sha: s.new.as_ref().map(|n| n.commit_sha.clone()),
                detail,
                applied: !dry_run && s.new.is_some(),
            });
        }
    }
    let s = serde_json::to_string_pretty(&rows).context("serialize repair json")?;
    println!("{s}");
    Ok(())
}

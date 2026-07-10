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
use ft_core::gather::{build_gather, GatherEntry};
use ft_core::graph::{Graph, NodeKind, NoteId};
use ft_core::pulse::{compute_pulse, WindowRange};
use ft_core::synth::repair::{
    apply_synth_repair, plan_repair_all, plan_synth_repair, RepairAction, SynthRepairPlan,
};
use ft_core::synth::reslice::{apply_reslice, plan_reslice, NewRange};
use ft_core::synth::scaffold::{apply_synth_scaffold, plan_synth_scaffold};
use ft_core::synth::verify::{verify_all, verify_synth_note, SectionStatus, VerificationResult};
use ft_core::vault::Vault;

#[derive(Subcommand, Debug)]
pub enum SynthCommand {
    /// Scaffold protected sections into a target synth note (creating
    /// it with `ft.synth.enabled: true` frontmatter if needed). Default action.
    #[command(name = "scaffold")]
    Scaffold(ScaffoldArgs),
    /// Accrete missing journal entries into an existing synth note,
    /// optionally scoped to entries newer than the note's last synth
    /// watermark. See `ft notes synth grow --help`.
    #[command(name = "grow")]
    Grow(GrowArgs),
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
        SynthCommand::Grow(a) => run_grow(a, vault_flag),
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

#[derive(Args, Debug)]
pub struct GrowArgs {
    /// Target synth note (vault-relative). MUST already exist — use
    /// `ft synth scaffold` to create one.
    #[arg(value_name = "TARGET.md")]
    pub target: PathBuf,

    /// A `[[wikilink]]` to source paragraphs from. Repeatable. When
    /// omitted (along with `--from`), targets are read from the note's
    /// `ft.synth.targets` frontmatter.
    #[arg(long, value_name = "LINK")]
    pub link: Vec<String>,

    /// Explicit source paragraph: `<vault-relative-path>:<line>`.
    /// Repeatable.
    #[arg(long, value_name = "PATH:LINE")]
    pub from: Vec<String>,

    /// Scope to entries whose `date` is newer than the note's last-synth
    /// watermark (the newest pinned commit SHA among its callouts).
    /// Falls back to "all missing" with a warning when the watermark
    /// is unavailable (no callouts or all SHAs unreachable).
    #[arg(long)]
    pub new_only: bool,

    /// Duration window: `7d`, `24h`, `2w`, `1m`. Mutually exclusive
    /// with `--range`. Only takes effect when combined with `--in-window`.
    #[arg(long, value_name = "DURATION", conflicts_with = "range")]
    pub since: Option<String>,

    /// Commit range `X..Y`. Mutually exclusive with `--since`. Same
    /// semantics as `--since`.
    #[arg(long, value_name = "X..Y", conflicts_with = "since")]
    pub range: Option<String>,

    /// Restrict `--link`-sourced entries to paragraphs touched by the
    /// window. Requires `--since` or `--range`.
    #[arg(long)]
    pub in_window: bool,

    /// Cap the number of appended sections to the newest `N` (after
    /// dedup and, if active, the new-only filter).
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,

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

    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    ft_core::git::discover_repo(&vault.path).ok_or_else(|| {
        anyhow!("vault is not inside a git repository — `ft synth` needs git history")
    })?;
    let graph = crate::cmd::common::build_graph(&vault, &vault.scan())?;
    let target = normalize_md_target(&args.target);

    let mut entries: Vec<GatherEntry> = Vec::new();

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
        let report = build_gather(&graph, &targets, &vault, &mut cache)
            .context("building multi-source journal")?;
        let _ = cache.save(&vault.path);

        let filtered = if args.in_window {
            let window = resolve_window(&args.since, &args.range)?
                .expect("validated above: in_window implies since/range");
            let cfg = vault.config.config.synth.clone();
            let review = compute_pulse(&graph, &vault, &window, &cfg)
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

    let plan = plan_synth_scaffold(&vault, &target, &entries).context("planning synth scaffold")?;
    let written = apply_synth_scaffold(&vault, &plan).context("writing synth scaffold")?;

    // When --link was supplied, persist the targets into frontmatter
    // (write on create; upsert when the key is absent on append). This
    // makes the note self-describing so future `ft synth grow` runs need
    // no --link.
    if !args.link.is_empty() {
        ensure_synth_targets(&written, &args.link)?;
    }

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

/// Idempotently ensure the synth note at `absolute_path` carries an
/// `ft.synth.targets` frontmatter key matching `links`. Writes only when
/// the on-disk key differs (create, or absent on an existing note).
/// Existing keys are replaced so a re-run with different `--link` values
/// updates the record. Best-effort: a write failure is surfaced as an
/// error but does not undo the scaffold write that already landed.
fn ensure_synth_targets(absolute_path: &Path, links: &[String]) -> Result<()> {
    let content = std::fs::read_to_string(absolute_path)
        .with_context(|| format!("reading {} for target upsert", absolute_path.display()))?;
    let new_content = ft_core::synth::callout::upsert_synth_frontmatter(&content, Some(links));
    if new_content != content {
        ft_core::fs::write_atomic(absolute_path, &new_content)
            .with_context(|| format!("writing targets to {}", absolute_path.display()))?;
    }
    Ok(())
}

/// `ft synth grow` — accrete missing journal entries into an existing
/// synth note. Mirrors `run_scaffold`'s sourcing (multi-source journal +
/// `--from` picks + `--in-window` filter) but adds two selection steps
/// on top of the planner's dedup-on-append:
///
/// - `--new-only`: compute the note's last-synth watermark and keep only
///   entries whose `date` is strictly newer. Falls back to "all missing"
///   with a warning when the watermark is unavailable.
/// - `--limit N`: cap the appended sections to the newest N.
///
/// Targets come from `--link`/`--from`, or — when both are absent — from
/// the note's `ft.synth.targets` frontmatter.
fn run_grow(args: GrowArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    if args.in_window && args.since.is_none() && args.range.is_none() {
        return Err(anyhow!("--in-window requires --since or --range"));
    }

    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    ft_core::git::discover_repo(&vault.path).ok_or_else(|| {
        anyhow!("vault is not inside a git repository — `ft synth grow` needs git history")
    })?;
    let target = normalize_md_target(&args.target);
    let target_abs = vault.path.join(&target);
    if !target_abs.exists() {
        return Err(anyhow!(
            "target note does not exist: {} — use `ft synth scaffold` to create it",
            target.display()
        ));
    }
    let graph = crate::cmd::common::build_graph(&vault, &vault.scan())?;

    // Resolve targets: explicit --link / --from, else frontmatter.
    let explicit_links = !args.link.is_empty();
    let explicit_from = !args.from.is_empty();
    if !explicit_links && !explicit_from {
        // Read targets from the note's frontmatter.
        let content = std::fs::read_to_string(&target_abs)
            .with_context(|| format!("reading {} for targets", target.display()))?;
        let fm_targets = ft_core::synth::callout::parse_synth_targets(&content).unwrap_or_default();
        if fm_targets.is_empty() {
            return Err(anyhow!(
                "no targets: pass --link or add `ft.synth.targets` frontmatter to {}",
                target.display()
            ));
        }
        // Re-resolve frontmatter targets via the same path as --link.
        return run_grow_with_targets(args, vault, graph, target, fm_targets, false);
    }

    let links: Vec<String> = if explicit_links {
        args.link.clone()
    } else {
        Vec::new()
    };
    run_grow_with_targets(args, vault, graph, target, links, explicit_links)
}

/// Shared grow body: build the journal for `links` (if any), add `--from`
/// picks, apply `--new-only` watermark + `--limit`, plan (dedup happens
/// in the planner), apply, upsert frontmatter targets when `--link` was
/// explicit, and hand off to the editor.
#[allow(clippy::too_many_arguments)]
fn run_grow_with_targets(
    args: GrowArgs,
    vault: Vault,
    graph: Graph,
    target: PathBuf,
    links: Vec<String>,
    links_were_explicit: bool,
) -> Result<ExitCode> {
    let mut entries: Vec<GatherEntry> = Vec::new();

    if !links.is_empty() {
        let targets: Vec<NoteId> = links
            .iter()
            .filter_map(|s| resolve_link_to_id(&graph, s))
            .collect();
        if targets.is_empty() {
            return Err(anyhow!(
                "none of the link values resolved to a note or ghost in the vault"
            ));
        }
        let mut cache = BlameCache::load(&vault.path).context("loading blame cache")?;
        let report = build_gather(&graph, &targets, &vault, &mut cache)
            .context("building multi-source journal")?;
        let _ = cache.save(&vault.path);

        let filtered = if args.in_window {
            let window = resolve_window(&args.since, &args.range)?
                .expect("validated above: in_window implies since/range");
            let cfg = vault.config.config.synth.clone();
            let review = compute_pulse(&graph, &vault, &window, &cfg)
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

    for spec in &args.from {
        let (path, line) = parse_from_spec(spec)?;
        let entry = pick_paragraph(&graph, &vault, &path, line)?;
        entries.push(entry);
    }

    if entries.is_empty() {
        return Err(anyhow!(
            "no entries to grow (multi-source journal was empty and no --from picks supplied)"
        ));
    }

    // Within-run dedup by (source_path, line_start).
    entries = dedup_entries(entries);

    // --new-only: scope to entries newer than the note's last-synth
    // watermark. The watermark is derived from the note's existing
    // callouts; None degrades to "all missing" with a warning.
    if args.new_only {
        let target_abs = vault.path.join(&target);
        let content = std::fs::read_to_string(&target_abs)
            .with_context(|| format!("reading {} for watermark", target.display()))?;
        let callouts = ft_core::synth::callout::parse(&content);
        let repo = ft_core::git::RepoMap::discover(&vault.path)?;
        match ft_core::synth::accrete::last_synth_watermark(repo.root(), &callouts)? {
            Some((_sha, watermark_date)) => {
                let before = entries.len();
                entries.retain(|e| e.date > watermark_date);
                let dropped = before.saturating_sub(entries.len());
                if dropped > 0 {
                    eprintln!(
                        "--new-only: dropped {dropped} entr{} at or before the last-synth watermark ({watermark_date})",
                        if dropped == 1 { "y" } else { "ies" }
                    );
                }
            }
            None => {
                eprintln!(
                    "warning: --new-only could not determine a last-synth watermark \
                     (no callouts or all pinned SHAs unreachable) — falling back to all missing"
                );
            }
        }
    }

    // --limit: cap to the newest N (entries are already date-desc from
    // dedup_entries' sort).
    if let Some(limit) = args.limit {
        if entries.len() > limit {
            entries.truncate(limit);
        }
    }

    if entries.is_empty() {
        eprintln!("nothing to grow: all entries already pinned or filtered out");
        return Ok(ExitCode::SUCCESS);
    }

    let plan = plan_synth_scaffold(&vault, &target, &entries).context("planning synth grow")?;
    let written = apply_synth_scaffold(&vault, &plan).context("writing synth grow")?;

    // Persist explicit --link targets into frontmatter (upsert when the
    // key is absent). Frontmatter-derived targets are left untouched.
    if links_were_explicit && !links.is_empty() {
        ensure_synth_targets(&written, &links)?;
    }

    let rel = vault.relativize(&written).display().to_string();
    if plan.dedup_skipped > 0 {
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
    entry: &GatherEntry,
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

/// Build a [`GatherEntry`] for the paragraph at `(path, line_start)`.
fn pick_paragraph(
    graph: &Graph,
    vault: &Vault,
    path: &Path,
    line_start: u32,
) -> Result<GatherEntry> {
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
    Ok(GatherEntry {
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
fn dedup_entries(mut entries: Vec<GatherEntry>) -> Vec<GatherEntry> {
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

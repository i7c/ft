//! Notes History — a whole-vault, windowed, recency-ordered feed of the
//! paragraphs that were edited within a git window.
//!
//! Where [`crate::gather`] selects paragraphs by *link target* ("what
//! mentions `[[X]]`?"), history selects them by *recency of edit* ("what
//! did I change anywhere in the vault lately?"). It is the untargeted,
//! time-shaped sibling of the journal and shares its entry shape, blame
//! dates, and sort.
//!
//! Selection reuses the link-review engine's `added_lines` map for the
//! window: a paragraph is included iff its line range overlaps a line
//! added within the window. That map's keys are the only files touched in
//! the window, so it doubles as the perf prefilter — only those files are
//! blamed. Dates come from `git blame` via [`crate::blame_cache`], exactly
//! as in the journal.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use chrono::NaiveDate;

use crate::blame_cache::{paragraph_date, BlameCache};
use crate::config::Synth as SynthCfg;
use crate::error::Result;
use crate::git;
use crate::graph::{Graph, NodeKind};
use crate::pulse::{compute_pulse, WindowRange};
use crate::synth::callout::is_synth_note;
use crate::vault::Vault;

/// One row of the history feed. Mirrors [`crate::gather::GatherEntry`]
/// minus the `matched` field — history has no link target to attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentEntry {
    /// Filename stem of the note the paragraph lives in (no `.md`).
    pub source_title: String,
    /// Vault-relative path of the source note.
    pub source_path: PathBuf,
    /// 1-indexed line number of the paragraph's first line.
    pub line_start: u32,
    /// 1-indexed line number of the paragraph's last line.
    pub line_end: u32,
    /// The paragraph text itself.
    pub section_text: String,
    /// Date of the most recent commit touching any line in the paragraph.
    pub date: NaiveDate,
}

/// Result of [`build_recent`]: the feed plus per-file blame diagnostics,
/// matching [`crate::gather::GatherReport`] so callers can warn instead
/// of silently dropping entries.
#[derive(Debug, Default, Clone)]
pub struct RecentReport {
    /// The feed, already sorted reverse-chronologically.
    pub entries: Vec<RecentEntry>,
    /// Vault-relative paths whose paragraphs were dropped because
    /// `git blame` failed (untracked or outside the repo).
    pub skipped_blame: Vec<PathBuf>,
}

/// Behavioral knobs for [`build_recent`].
#[derive(Debug, Clone, Default)]
pub struct RecentOptions {
    /// Include paragraphs from synth notes (`ft.synth.enabled: true`). Off by
    /// default so the synth flow does not feed itself.
    pub include_synth: bool,
}

/// Build the windowed, recency-ordered history feed.
///
/// 1. Resolve `added_lines` for `window` via [`compute_pulse`] — a
///    per-file map of lines added in the window. Its keys are the files
///    touched in the window and the only files that get blamed.
/// 2. For every `Paragraph` node whose `source_file` is in that map and
///    whose line range overlaps an added line, emit an entry. Synth notes
///    are skipped unless `opts.include_synth`; periodic notes are kept.
/// 3. Dates come from `git blame` (lazy-populated `cache`), and the feed
///    is sorted date-descending, then source title ascending, then
///    `line_start` ascending — identical to the journal's sort.
pub fn build_recent(
    graph: &Graph,
    vault: &Vault,
    window: &WindowRange,
    cfg: &SynthCfg,
    opts: &RecentOptions,
    cache: &mut BlameCache,
) -> Result<RecentReport> {
    let repo = git::RepoMap::discover(&vault.path)?;

    // The added-lines map is both the edit filter and the file prefilter:
    // only files touched in the window appear as keys.
    let review = compute_pulse(graph, vault, window, cfg)?;
    if review.added_lines.is_empty() {
        return Ok(RecentReport::default());
    }

    let head = git::head_hash(repo.root())?;
    let mut entries: Vec<RecentEntry> = Vec::new();
    let mut skipped_blame: Vec<PathBuf> = Vec::new();
    let mut skipped_seen: HashSet<PathBuf> = HashSet::new();
    // Per-file synth-marker memo so each touched file is read at most once.
    let mut synth_memo: HashMap<PathBuf, bool> = HashMap::new();

    for (_id, node) in graph.nodes() {
        let NodeKind::Paragraph(p) = node else {
            continue;
        };
        let Some(added) = review.added_lines.get(&p.source_file) else {
            continue; // file not touched in the window
        };
        if !(p.line_start..=p.line_end).any(|ln| added.contains(&ln)) {
            continue; // paragraph's own lines unchanged in the window
        }

        if !opts.include_synth {
            let is_synth = *synth_memo.entry(p.source_file.clone()).or_insert_with(|| {
                std::fs::read_to_string(vault.path.join(&p.source_file))
                    .map(|c| is_synth_note(&c))
                    .unwrap_or(false)
            });
            if is_synth {
                continue;
            }
        }

        let path_str = p.source_file.to_string_lossy().into_owned();
        if cache.get(&path_str, &head).is_none() {
            match git::blame_file(repo.root(), &repo.to_repo(&p.source_file)) {
                Ok(blame) => cache.insert(path_str.clone(), head.clone(), blame),
                Err(_) => {
                    if skipped_seen.insert(p.source_file.clone()) {
                        skipped_blame.push(p.source_file.clone());
                    }
                    continue;
                }
            }
        }
        let Some(blame) = cache.get(&path_str, &head) else {
            continue;
        };
        let Some(date) = paragraph_date(blame, p.line_start, p.line_end) else {
            continue;
        };
        let source_title = p
            .source_file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        entries.push(RecentEntry {
            source_title,
            source_path: p.source_file.clone(),
            line_start: p.line_start,
            line_end: p.line_end,
            section_text: p.text.clone(),
            date,
        });
    }

    // Reverse-chronological; title ascending tiebreak; `line_start`
    // ascending so co-located same-date paragraphs read top-to-bottom.
    entries.sort_by(|a, b| {
        b.date
            .cmp(&a.date)
            .then_with(|| a.source_title.cmp(&b.source_title))
            .then_with(|| a.line_start.cmp(&b.line_start))
    });
    skipped_blame.sort();
    Ok(RecentReport {
        entries,
        skipped_blame,
    })
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn run_git(repo: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}");
    }

    fn init_git_repo(repo: &Path) {
        run_git(repo, &["init", "-b", "main"]);
        run_git(repo, &["config", "user.name", "T"]);
        run_git(repo, &["config", "user.email", "t@e.com"]);
        run_git(repo, &["config", "commit.gpgsign", "false"]);
    }

    fn commit_all_dated(repo: &Path, msg: &str, date: &str) {
        run_git(repo, &["add", "."]);
        let out = Command::new("git")
            .current_dir(repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .args(["commit", "-m", msg])
            .output()
            .expect("git commit");
        assert!(out.status.success());
    }

    fn range(from: &str, to: &str) -> WindowRange {
        WindowRange::Range {
            from: from.to_string(),
            to: to.to_string(),
        }
    }

    fn default_cfg() -> SynthCfg {
        SynthCfg::default()
    }

    /// A paragraph edited in the window is included; a paragraph committed
    /// before the window (unchanged since) is excluded.
    #[test]
    fn includes_window_edit_excludes_older() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        // c1: an old paragraph.
        tmp.child("Note.md")
            .write_str("# Note\n\nOld para.\n")
            .unwrap();
        init_git_repo(tmp.path());
        commit_all_dated(tmp.path(), "c1", "2025-01-01T00:00:00");
        // c2 (HEAD): append a new paragraph.
        tmp.child("Note.md")
            .write_str("# Note\n\nOld para.\n\nNew para.\n")
            .unwrap();
        commit_all_dated(tmp.path(), "c2", "2025-02-01T00:00:00");

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let mut cache = BlameCache::default();
        let report = build_recent(
            &graph,
            &vault,
            &range("HEAD~1", "HEAD"),
            &default_cfg(),
            &RecentOptions::default(),
            &mut cache,
        )
        .unwrap();
        assert!(
            report.skipped_blame.is_empty(),
            "{:?}",
            report.skipped_blame
        );
        let bodies: Vec<&str> = report
            .entries
            .iter()
            .map(|e| e.section_text.as_str())
            .collect();
        assert!(bodies.iter().any(|t| t.contains("New para")), "{bodies:?}");
        assert!(!bodies.iter().any(|t| t.contains("Old para")), "{bodies:?}");
    }

    /// Synth notes are excluded by default and surfaced with include_synth.
    #[test]
    fn synth_notes_excluded_by_default() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("Plain.md")
            .write_str("# Plain\n\nPlain para.\n")
            .unwrap();
        tmp.child("Synth.md")
            .write_str("---\nft:\n  synth:\n    enabled: true\n---\n\nSynth para.\n")
            .unwrap();
        init_git_repo(tmp.path());
        // Base empty commit so HEAD~1 exists and the window covers c2.
        commit_all_dated(tmp.path(), "base", "2025-01-01T00:00:00");
        tmp.child("Plain.md")
            .write_str("# Plain\n\nPlain para.\n\nMore plain.\n")
            .unwrap();
        tmp.child("Synth.md")
            .write_str("---\nft:\n  synth:\n    enabled: true\n---\n\nSynth para.\n\nMore synth.\n")
            .unwrap();
        commit_all_dated(tmp.path(), "c2", "2025-02-01T00:00:00");

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();

        let mut cache = BlameCache::default();
        let default_report = build_recent(
            &graph,
            &vault,
            &range("HEAD~1", "HEAD"),
            &default_cfg(),
            &RecentOptions::default(),
            &mut cache,
        )
        .unwrap();
        let titles: HashSet<&str> = default_report
            .entries
            .iter()
            .map(|e| e.source_title.as_str())
            .collect();
        assert!(titles.contains("Plain"), "{titles:?}");
        assert!(
            !titles.contains("Synth"),
            "synth excluded by default: {titles:?}"
        );

        let mut cache2 = BlameCache::default();
        let incl_report = build_recent(
            &graph,
            &vault,
            &range("HEAD~1", "HEAD"),
            &default_cfg(),
            &RecentOptions {
                include_synth: true,
            },
            &mut cache2,
        )
        .unwrap();
        let titles2: HashSet<&str> = incl_report
            .entries
            .iter()
            .map(|e| e.source_title.as_str())
            .collect();
        assert!(
            titles2.contains("Synth"),
            "include_synth surfaces synth: {titles2:?}"
        );
    }

    /// Entries are ordered most-recent-edit first, then title, then line.
    #[test]
    fn ordered_reverse_chronological() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("Seed.md").write_str("# Seed\n").unwrap();
        init_git_repo(tmp.path());
        commit_all_dated(tmp.path(), "base", "2025-01-01T00:00:00");
        // c2: older edit (A).
        tmp.child("A.md").write_str("# A\n\nAlpha para.\n").unwrap();
        commit_all_dated(tmp.path(), "c2", "2025-03-01T00:00:00");
        // c3: newer edit (B).
        tmp.child("B.md").write_str("# B\n\nBeta para.\n").unwrap();
        commit_all_dated(tmp.path(), "c3", "2025-06-01T00:00:00");

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let mut cache = BlameCache::default();
        let report = build_recent(
            &graph,
            &vault,
            &range("HEAD~2", "HEAD"),
            &default_cfg(),
            &RecentOptions::default(),
            &mut cache,
        )
        .unwrap();
        let order: Vec<&str> = report
            .entries
            .iter()
            .map(|e| e.source_title.as_str())
            .collect();
        let a = order.iter().position(|t| *t == "A");
        let b = order.iter().position(|t| *t == "B");
        assert!(a.is_some() && b.is_some(), "both present: {order:?}");
        assert!(b < a, "newer B before older A: {order:?}");
    }

    /// Files untouched in the window are neither blamed nor surfaced.
    #[test]
    fn untouched_files_excluded() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("Old.md")
            .write_str("# Old\n\nOld body.\n")
            .unwrap();
        init_git_repo(tmp.path());
        commit_all_dated(tmp.path(), "c1", "2025-01-01T00:00:00");
        tmp.child("New.md")
            .write_str("# New\n\nNew body.\n")
            .unwrap();
        commit_all_dated(tmp.path(), "c2", "2025-02-01T00:00:00");

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let mut cache = BlameCache::default();
        let report = build_recent(
            &graph,
            &vault,
            &range("HEAD~1", "HEAD"),
            &default_cfg(),
            &RecentOptions::default(),
            &mut cache,
        )
        .unwrap();
        let titles: HashSet<&str> = report
            .entries
            .iter()
            .map(|e| e.source_title.as_str())
            .collect();
        assert!(titles.contains("New"), "{titles:?}");
        assert!(
            !titles.contains("Old"),
            "untouched file excluded: {titles:?}"
        );
    }
}

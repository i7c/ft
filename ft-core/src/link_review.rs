//! Engine 2 — link review over a commit window.
//!
//! Scans the unified diff between two commits, identifies added
//! `[[wikilink]]` mentions, maps each back to the paragraph it currently
//! lives in (HEAD paragraph index), and aggregates a frequency-ranked
//! list of `(target, count)` rows. Paragraph-level dedup: the same link
//! repeated within one paragraph counts once; the same link in two
//! paragraphs counts twice.
//!
//! Powers the `ft review` CLI and the `Review` TUI tab. The `added_lines`
//! map is also consumed by the multi-source journal's `--in-window`
//! filter (it asks "did any line of this paragraph get touched in the
//! window?").
//!
//! For v1 the "to" side of the window is always HEAD: the graph's
//! paragraph index is HEAD-relative, and synthesis happens against the
//! current vault state. `WindowRange::Range { to, .. }` values other
//! than HEAD-equivalent refs are still accepted but content for
//! callout-exclusion is read from the to-ref via `git show`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{Duration, NaiveDate};

use crate::config::Synth as SynthCfg;
use crate::error::Result;
use crate::git;
use crate::graph::{EdgeKind, Graph, NodeKind, NoteId};
use crate::synth::callout::{
    is_synth_note, line_is_inside_callout, parse as parse_callouts, path_excluded,
};
use crate::vault::Vault;

/// The window the link review operates over.
#[derive(Debug, Clone)]
pub enum WindowRange {
    /// A relative duration (e.g. `7d`) back from today.
    Since(Duration),
    /// An explicit commit-ref range (`from..to`). When `to` resolves to
    /// HEAD, paragraph lookup uses the graph directly; otherwise file
    /// content at `to` is read via `git show`.
    Range { from: String, to: String },
}

impl WindowRange {
    /// Parse a `--since` token like `7d`, `24h`, `2w`. Returns
    /// `Err`-friendly `None` on unrecognized format; callers convert to
    /// a clap-friendly error string.
    pub fn parse_since(s: &str) -> Option<Duration> {
        let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit())?);
        let n: i64 = num.parse().ok()?;
        match unit {
            "d" | "D" => Some(Duration::days(n)),
            "h" | "H" => Some(Duration::hours(n)),
            "w" | "W" => Some(Duration::weeks(n)),
            "m" | "M" => Some(Duration::days(n * 30)),
            _ => None,
        }
    }
}

/// One row of the link review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkReviewRow {
    /// Number of distinct paragraphs (or synthetic keys) containing
    /// added mentions of this target in the window.
    pub count: usize,
    /// Display name for the target. For notes: the filename stem. For
    /// ghosts: the verbatim `[[raw]]` text.
    pub target: String,
    /// `true` when the target does not resolve to a `Note` in the
    /// current graph (rendered with a `?` suffix in CLI/TUI output).
    pub is_ghost: bool,
    /// Vault-relative paths of the notes whose paragraphs contributed
    /// to this row. Sorted, deduped.
    pub source_paths: Vec<PathBuf>,
}

/// Full output of [`compute_link_review`]. Rows are sorted by count
/// desc, target asc. `added_lines` is keyed on vault-relative path and
/// stores the set of post-`to` line numbers added in the window — used
/// by the journal's `--in-window` filter.
#[derive(Debug, Clone, Default)]
pub struct LinkReview {
    pub rows: Vec<LinkReviewRow>,
    pub added_lines: HashMap<PathBuf, BTreeSet<u32>>,
}

/// Compute the link review for `window`.
///
/// `repo` is the git repo path (typically `vault.path` per the existing
/// blame convention). `cfg` provides `exclude_prefixes` and is read but
/// not modified.
pub fn compute_link_review(
    graph: &Graph,
    vault: &Vault,
    repo: &Path,
    window: &WindowRange,
    cfg: &SynthCfg,
) -> Result<LinkReview> {
    let (from_sha, to_sha) = resolve_window(repo, window)?;
    let head_sha = git::head_hash(repo)?;
    let to_is_head = to_sha == head_sha;

    let changed = git::diff_changed_paths(repo, &from_sha, &to_sha)?;

    // Aggregator: dedup (target_id, paragraph_key) tuples and track
    // contributing source paths per target.
    let mut counted: HashSet<(NoteId, ParagraphKey)> = HashSet::new();
    let mut source_paths_per_target: HashMap<NoteId, BTreeSet<PathBuf>> = HashMap::new();
    let mut added_lines: HashMap<PathBuf, BTreeSet<u32>> = HashMap::new();

    for path in changed {
        if path_excluded(&path, &cfg.exclude_prefixes) {
            continue;
        }
        // Only markdown files contribute wikilinks.
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }

        let added = match git::diff_added_lines(repo, &from_sha, &to_sha, &path) {
            Ok(v) => v,
            Err(_) => continue, // file added in a way diff can't render (binary, etc.)
        };
        if added.is_empty() {
            continue;
        }

        let set: BTreeSet<u32> = added.iter().copied().collect();
        added_lines.insert(path.clone(), set.clone());

        // Identify the note's id in the graph (HEAD state). If the
        // file no longer exists at HEAD, skip — we can't map to a
        // current paragraph.
        let Some(note_id) = graph.note_by_path(&path) else {
            continue;
        };

        // If the note is a synth note, parse its callouts at the `to`
        // state and use them to skip wikilinks that come from quoted
        // material.
        let to_content_for_callouts = if to_is_head {
            std::fs::read_to_string(vault.path.join(&path)).ok()
        } else {
            git::show_file_at(repo, &to_sha, &path).ok()
        };
        let callouts_to_skip = to_content_for_callouts
            .as_deref()
            .filter(|c| is_synth_note(c))
            .map(parse_callouts)
            .unwrap_or_default();

        // For each paragraph the note owns, see if any of its lines
        // overlap an added line.
        for (paragraph_id, edge) in graph.outgoing(note_id) {
            if !matches!(edge, EdgeKind::OwnsParagraph) {
                continue;
            }
            let NodeKind::Paragraph(p) = graph.node(paragraph_id) else {
                continue;
            };
            // Overlap test: any added line ∈ [line_start, line_end]?
            let overlaps = (p.line_start..=p.line_end).any(|ln| set.contains(&ln));
            if !overlaps {
                continue;
            }
            // If any of the overlapping lines fall inside a synth-source
            // callout, skip the paragraph entirely (the entire callout
            // body is quoted material).
            if !callouts_to_skip.is_empty()
                && (p.line_start..=p.line_end)
                    .any(|ln| line_is_inside_callout(ln, &callouts_to_skip))
            {
                continue;
            }

            // Walk paragraph→target edges.
            for (target_id, edge) in graph.outgoing(paragraph_id) {
                if !matches!(edge, EdgeKind::ParagraphLink) {
                    continue;
                }
                let key = (target_id, ParagraphKey::Paragraph(paragraph_id));
                if counted.insert(key) {
                    source_paths_per_target
                        .entry(target_id)
                        .or_default()
                        .insert(p.source_file.clone());
                }
            }
        }
    }

    // Materialize rows.
    let mut rows: Vec<LinkReviewRow> = source_paths_per_target
        .iter()
        .map(|(target_id, paths)| {
            let (target, is_ghost) = match graph.node(*target_id) {
                NodeKind::Note(n) => (n.title.clone(), false),
                NodeKind::Ghost(g) => (g.raw.clone(), true),
                _ => (String::new(), false),
            };
            let count = counted.iter().filter(|(t, _)| t == target_id).count();
            LinkReviewRow {
                count,
                target,
                is_ghost,
                source_paths: paths.iter().cloned().collect(),
            }
        })
        .filter(|r| !r.target.is_empty())
        .collect();

    // Sort by count desc, target asc.
    rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.target.cmp(&b.target)));

    let _ = BTreeMap::<(), ()>::new(); // keep BTreeMap import live for future use
    Ok(LinkReview { rows, added_lines })
}

/// Internal dedup key. For v1 the only variant is `Paragraph(id)`;
/// `Synthetic(path, line)` is reserved for the precision-loss fallback
/// described in the design but not yet implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ParagraphKey {
    Paragraph(NoteId),
    #[allow(dead_code)]
    Synthetic(SyntheticKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
struct SyntheticKey {
    path_hash: u64,
    line: u32,
}

/// Resolve a [`WindowRange`] to a `(from_sha, to_sha)` pair.
fn resolve_window(repo: &Path, window: &WindowRange) -> Result<(String, String)> {
    match window {
        WindowRange::Since(dur) => {
            let today = today_naive();
            // Subtract; clamp at the unix epoch in absurd cases.
            let cutoff = today
                .checked_sub_signed(*dur)
                .unwrap_or(NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());
            let iso = cutoff.format("%Y-%m-%d").to_string();
            let from = git::commit_before(repo, &iso)?;
            let to = git::head_hash(repo)?;
            Ok((from, to))
        }
        WindowRange::Range { from, to } => {
            let from_sha = git::rev_parse(repo, from)?;
            let to_sha = git::rev_parse(repo, to)?;
            Ok((from_sha, to_sha))
        }
    }
}

fn today_naive() -> NaiveDate {
    crate::dates::today()
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use std::process::Command;

    fn parse_since_basic() -> Option<Duration> {
        WindowRange::parse_since("7d")
    }

    #[test]
    fn parse_since_days() {
        assert_eq!(parse_since_basic(), Some(Duration::days(7)));
        assert_eq!(WindowRange::parse_since("24h"), Some(Duration::hours(24)));
        assert_eq!(WindowRange::parse_since("2w"), Some(Duration::weeks(2)));
        assert_eq!(WindowRange::parse_since("not-a-duration"), None);
    }

    /// Mini-fixture builder: an `assert_fs::TempDir` initialized as a git
    /// repo with two commits — one creating files, one adding more.
    /// Returns the second commit's SHA so callers can range against it.
    fn make_two_commit_repo(
        tmp: &assert_fs::TempDir,
    ) -> (Vault, Graph, std::path::PathBuf, String) {
        tmp.child(".obsidian").create_dir_all().unwrap();

        // Commit 1: baseline file, no wikilinks.
        tmp.child("baseline.md")
            .write_str("# Baseline\n\nSome prose.\n")
            .unwrap();

        let repo = tmp.path().to_path_buf();
        let run_git = |args: &[&str]| {
            let out = Command::new("git")
                .current_dir(&repo)
                .env("GIT_TERMINAL_PROMPT", "0")
                .args(args)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}");
        };
        run_git(&["init", "-b", "main"]);
        run_git(&["config", "user.name", "T"]);
        run_git(&["config", "user.email", "t@e.com"]);
        run_git(&["config", "commit.gpgsign", "false"]);
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "c1"]);

        let first_sha_out = Command::new("git")
            .current_dir(&repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("rev-parse");
        let first_sha = String::from_utf8_lossy(&first_sha_out.stdout)
            .trim()
            .to_string();

        // Commit 2: notes with wikilinks.
        tmp.child("note-a.md")
            .write_str("First paragraph mentions [[Foo]] and [[Bar]].\n\nSecond paragraph mentions [[Foo]] only.\n")
            .unwrap();
        tmp.child("note-b.md")
            .write_str("Just [[Bar]] here.\n")
            .unwrap();
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "c2"]);

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &crate::vault::Scan::default()).unwrap();
        (vault, graph, repo, first_sha)
    }

    fn default_cfg() -> SynthCfg {
        SynthCfg {
            folder: "Synthesis/".into(),
            exclude_prefixes: vec![],
        }
    }

    #[test]
    fn counts_paragraph_level_dedup() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let (vault, graph, repo, c1) = make_two_commit_repo(&tmp);
        let head = git::head_hash(&repo).unwrap();
        let window = WindowRange::Range { from: c1, to: head };
        let review = compute_link_review(&graph, &vault, &repo, &window, &default_cfg()).unwrap();

        // Expected: [[Foo]] count = 2 (two paragraphs in note-a, only first
        // also mentions [[Bar]]). Wait — the second paragraph also has Foo.
        // note-a paragraph 1: [Foo, Bar]
        // note-a paragraph 2: [Foo]
        // note-b paragraph 1: [Bar]
        // So Foo = 2 (two distinct paragraphs in note-a), Bar = 2 (one in note-a, one in note-b).
        let foo = review.rows.iter().find(|r| r.target == "Foo").unwrap();
        let bar = review.rows.iter().find(|r| r.target == "Bar").unwrap();
        assert_eq!(foo.count, 2, "Foo across two distinct paragraphs");
        assert_eq!(bar.count, 2, "Bar across two distinct paragraphs");
        assert!(foo.is_ghost, "Foo is a ghost (no Foo.md exists)");
        assert!(bar.is_ghost, "Bar is a ghost");
    }

    #[test]
    fn same_link_twice_in_one_paragraph_counts_once() {
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        let repo = tmp.path().to_path_buf();
        let run_git = |args: &[&str]| {
            let out = Command::new("git")
                .current_dir(&repo)
                .env("GIT_TERMINAL_PROMPT", "0")
                .args(args)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}");
        };
        tmp.child("baseline.md").write_str("baseline\n").unwrap();
        run_git(&["init", "-b", "main"]);
        run_git(&["config", "user.name", "T"]);
        run_git(&["config", "user.email", "t@e.com"]);
        run_git(&["config", "commit.gpgsign", "false"]);
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "c1"]);
        let c1 = String::from_utf8_lossy(
            &Command::new("git")
                .current_dir(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        // One paragraph, three [[Foo]] mentions.
        tmp.child("note.md")
            .write_str("[[Foo]] then [[Foo]] then [[Foo]] in one paragraph.\n")
            .unwrap();
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "c2"]);

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &crate::vault::Scan::default()).unwrap();
        let head = git::head_hash(&repo).unwrap();
        let window = WindowRange::Range { from: c1, to: head };
        let review = compute_link_review(&graph, &vault, &repo, &window, &default_cfg()).unwrap();
        let foo = review.rows.iter().find(|r| r.target == "Foo").unwrap();
        assert_eq!(foo.count, 1, "paragraph-level dedup");
    }

    #[test]
    fn sort_by_count_desc_then_target_asc() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let (vault, graph, repo, c1) = make_two_commit_repo(&tmp);
        let head = git::head_hash(&repo).unwrap();
        let window = WindowRange::Range { from: c1, to: head };
        let review = compute_link_review(&graph, &vault, &repo, &window, &default_cfg()).unwrap();
        // Both counts are 2; alphabetical tiebreak → Bar before Foo.
        assert_eq!(review.rows[0].target, "Bar");
        assert_eq!(review.rows[1].target, "Foo");
    }

    #[test]
    fn excluded_prefix_dropped() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let (vault, graph, repo, c1) = make_two_commit_repo(&tmp);
        let mut cfg = default_cfg();
        cfg.exclude_prefixes = vec!["note-a".into()]; // drop note-a contributions
        let head = git::head_hash(&repo).unwrap();
        let window = WindowRange::Range { from: c1, to: head };
        let review = compute_link_review(&graph, &vault, &repo, &window, &cfg).unwrap();
        // note-a contributed Foo (×2) and Bar (×1). Excluding note-a leaves
        // only note-b's Bar.
        let bar = review.rows.iter().find(|r| r.target == "Bar").unwrap();
        assert_eq!(bar.count, 1);
        assert!(
            review.rows.iter().all(|r| r.target != "Foo"),
            "Foo should be entirely absent"
        );
    }

    #[test]
    fn empty_window_returns_empty_rows() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let (vault, graph, repo, _c1) = make_two_commit_repo(&tmp);
        let head = git::head_hash(&repo).unwrap();
        let window = WindowRange::Range {
            from: head.clone(),
            to: head,
        };
        let review = compute_link_review(&graph, &vault, &repo, &window, &default_cfg()).unwrap();
        assert!(review.rows.is_empty());
    }

    #[test]
    fn ghost_marking_and_resolved_note() {
        // Add a Foo.md to the same fixture so [[Foo]] resolves; [[Bar]] stays ghost.
        let tmp = assert_fs::TempDir::new().unwrap();
        let (_vault, _graph, repo, c1) = make_two_commit_repo(&tmp);
        let run_git = |args: &[&str]| {
            let out = Command::new("git")
                .current_dir(&repo)
                .env("GIT_TERMINAL_PROMPT", "0")
                .args(args)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}");
        };
        tmp.child("Foo.md").write_str("# Foo\n").unwrap();
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "c3"]);

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &crate::vault::Scan::default()).unwrap();
        let head = git::head_hash(&repo).unwrap();
        let window = WindowRange::Range { from: c1, to: head };
        let review = compute_link_review(&graph, &vault, &repo, &window, &default_cfg()).unwrap();
        let foo = review.rows.iter().find(|r| r.target == "Foo").unwrap();
        let bar = review.rows.iter().find(|r| r.target == "Bar").unwrap();
        assert!(!foo.is_ghost, "Foo.md exists → not a ghost");
        assert!(bar.is_ghost, "Bar.md missing → still a ghost");
    }

    #[test]
    fn fenced_code_block_links_ignored() {
        // Wikilinks inside fenced code blocks must not contribute to
        // counts. Relies on the graph's paragraph/link extractors both
        // honoring LineSkipState.
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        let repo = tmp.path().to_path_buf();
        let run_git = |args: &[&str]| {
            let out = Command::new("git")
                .current_dir(&repo)
                .env("GIT_TERMINAL_PROMPT", "0")
                .args(args)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}");
        };
        tmp.child("baseline.md").write_str("baseline\n").unwrap();
        run_git(&["init", "-b", "main"]);
        run_git(&["config", "user.name", "T"]);
        run_git(&["config", "user.email", "t@e.com"]);
        run_git(&["config", "commit.gpgsign", "false"]);
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "c1"]);
        let c1 = String::from_utf8_lossy(
            &Command::new("git")
                .current_dir(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        // Note with [[Foo]] only inside a fenced code block, [[Bar]] in
        // real prose.
        tmp.child("note.md")
            .write_str("Real mention of [[Bar]] here.\n\n```\n[[Foo]] in code block\n```\n")
            .unwrap();
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "c2"]);

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &crate::vault::Scan::default()).unwrap();
        let head = git::head_hash(&repo).unwrap();
        let window = WindowRange::Range { from: c1, to: head };
        let review = compute_link_review(&graph, &vault, &repo, &window, &default_cfg()).unwrap();

        assert!(review.rows.iter().any(|r| r.target == "Bar"));
        assert!(
            review.rows.iter().all(|r| r.target != "Foo"),
            "wikilink inside fenced code block must not be counted"
        );
    }

    #[test]
    fn removed_only_link_not_counted() {
        // Wikilink existed at `from`, removed in window. Should NOT
        // appear (the file's HEAD content has no link, so the paragraph
        // has no ParagraphLink edge to it).
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        let repo = tmp.path().to_path_buf();
        let run_git = |args: &[&str]| {
            let out = Command::new("git")
                .current_dir(&repo)
                .env("GIT_TERMINAL_PROMPT", "0")
                .args(args)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}");
        };
        tmp.child("note.md")
            .write_str("Mentions [[Removed]] originally.\n")
            .unwrap();
        run_git(&["init", "-b", "main"]);
        run_git(&["config", "user.name", "T"]);
        run_git(&["config", "user.email", "t@e.com"]);
        run_git(&["config", "commit.gpgsign", "false"]);
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "c1"]);
        let c1 = String::from_utf8_lossy(
            &Command::new("git")
                .current_dir(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        // Commit 2 removes [[Removed]] and replaces with [[Replacement]].
        tmp.child("note.md")
            .write_str("Mentions [[Replacement]] now.\n")
            .unwrap();
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "c2"]);

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &crate::vault::Scan::default()).unwrap();
        let head = git::head_hash(&repo).unwrap();
        let window = WindowRange::Range { from: c1, to: head };
        let review = compute_link_review(&graph, &vault, &repo, &window, &default_cfg()).unwrap();

        assert!(
            review.rows.iter().any(|r| r.target == "Replacement"),
            "added link counted"
        );
        assert!(
            review.rows.iter().all(|r| r.target != "Removed"),
            "removed-only link must not appear"
        );
    }

    #[test]
    fn synth_callout_links_skipped() {
        // A synth note whose body has a [!ft-source] callout containing
        // [[Foo]] (quoted material) and prose containing [[Bar]] (user's
        // own link). Only Bar should count.
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        let repo = tmp.path().to_path_buf();
        let run_git = |args: &[&str]| {
            let out = Command::new("git")
                .current_dir(&repo)
                .env("GIT_TERMINAL_PROMPT", "0")
                .args(args)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}");
        };
        tmp.child("baseline.md").write_str("baseline\n").unwrap();
        run_git(&["init", "-b", "main"]);
        run_git(&["config", "user.name", "T"]);
        run_git(&["config", "user.email", "t@e.com"]);
        run_git(&["config", "commit.gpgsign", "false"]);
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "c1"]);
        let c1 = String::from_utf8_lossy(
            &Command::new("git")
                .current_dir(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        // Synth note: frontmatter marker, one ft-source callout quoting
        // [[Foo]], and a prose paragraph mentioning [[Bar]].
        let synth_body = "\
---
ft-synth: true
---

> [!ft-source] \"notes/x.md\" L1-1 @aaaaaaa #aaaaaa
> Original line mentions [[Foo]] verbatim.

Then I add a thought about [[Bar]].
";
        tmp.child("Synthesis/topic.md")
            .write_str(synth_body)
            .unwrap();
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "c2"]);

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &crate::vault::Scan::default()).unwrap();
        let head = git::head_hash(&repo).unwrap();
        let window = WindowRange::Range { from: c1, to: head };
        let review = compute_link_review(&graph, &vault, &repo, &window, &default_cfg()).unwrap();

        assert!(
            review.rows.iter().all(|r| r.target != "Foo"),
            "[[Foo]] inside ft-source callout must be excluded"
        );
        assert!(
            review.rows.iter().any(|r| r.target == "Bar"),
            "[[Bar]] in synth-note prose must be counted"
        );
    }
}

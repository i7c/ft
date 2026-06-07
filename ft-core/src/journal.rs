//! Related Notes Journal — reverse-chronological feed of paragraph
//! sections from across the vault that mention a given note (or any of
//! its Related-section aliases).
//!
//! The feed is structurally derived from the graph: paragraph nodes
//! with `ParagraphLink` edges pointing at the target note (or any
//! alias) are the journal entries. Dates come from `git blame` via the
//! [`crate::blame_cache::BlameCache`] — populated lazily, one file at a
//! time, on first journal query.
//!
//! Aliases are resolved at query time, not at graph-build time: the
//! note's `## Related` heading's line range is computed from
//! [`crate::markdown::extract_headings`], and outgoing `Link` edges
//! within that range identify the alias `NoteId`s. This keeps the
//! graph free of a special "related alias" edge kind.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use crate::blame_cache::{paragraph_date, BlameCache};
use crate::error::Result;
use crate::git;
use crate::graph::{EdgeKind, Graph, NodeKind, NoteId};
use crate::markdown::extract_headings;
use crate::vault::Vault;

/// One row of the journal feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    /// Filename stem of the note the section lives in (no `.md`).
    pub source_title: String,
    /// Vault-relative path of the source note.
    pub source_path: PathBuf,
    /// 1-indexed line number of the paragraph's first line inside
    /// `source_path`. Lets consumers (e.g. the TUI Journal tab) open
    /// `$EDITOR` at the exact paragraph rather than the top of the file.
    pub line_start: u32,
    /// The paragraph text itself.
    pub section_text: String,
    /// Date of the most recent commit touching any line in the
    /// paragraph (UTC).
    pub date: NaiveDate,
}

/// Result of [`build_journal`]: the feed plus per-file diagnostics so
/// the CLI/TUI can warn instead of silently dropping entries.
#[derive(Debug, Default, Clone)]
pub struct JournalReport {
    /// The feed itself, already sorted reverse-chronologically.
    pub entries: Vec<JournalEntry>,
    /// Vault-relative paths whose paragraphs were dropped because
    /// `git blame` failed — typically untracked files or files outside
    /// the git repo. Useful as a warning signal: if this is non-empty
    /// the user is probably looking at a configuration problem (e.g.
    /// the vault sits below the repo root and paths weren't rewritten),
    /// not a genuinely empty feed.
    pub skipped_blame: Vec<PathBuf>,
}

/// Build the reverse-chronological journal feed for `note_id`.
///
/// Accepts both `Note` and `Ghost` targets — a ghost is a not-yet-
/// created note whose incoming `ParagraphLink` edges are still
/// meaningful "references to a concept". For ghosts, alias resolution
/// (which reads the target's `## Related` section) and the "exclude N's
/// own paragraphs" step are no-ops, since there is no backing file.
///
/// `Directory`/`Task`/`Paragraph` targets return an empty feed — the
/// journal isn't defined for them.
///
/// Steps:
/// 1. Resolve aliases by scanning `note_id`'s `## Related` heading
///    range for outgoing `Link` edges (notes only).
/// 2. Collect every `Paragraph` node with a `ParagraphLink` edge into
///    `note_id` or any alias.
/// 3. For notes only: exclude paragraphs whose `source_file` is
///    `note_id`'s own path.
/// 4. For each remaining paragraph, look up its date via `cache`,
///    populating with a `git blame` call on cache miss.
/// 5. Sort by date descending, then by source title ascending.
pub fn build_journal(
    graph: &Graph,
    note_id: NoteId,
    vault: &Vault,
    repo: &Path,
    cache: &mut BlameCache,
) -> Result<JournalReport> {
    let note_path: Option<PathBuf> = match graph.node(note_id) {
        NodeKind::Note(n) => Some(n.path.clone()),
        NodeKind::Ghost(_) => None,
        // Directories, tasks, and paragraphs have no journal semantics.
        _ => return Ok(JournalReport::default()),
    };

    let alias_ids = match &note_path {
        Some(p) => resolve_related_aliases(graph, note_id, vault, p)?,
        None => Vec::new(),
    };
    let mut target_set: HashSet<NoteId> = HashSet::new();
    target_set.insert(note_id);
    target_set.extend(alias_ids.iter().copied());

    // Collect candidate paragraph nodes (dedup via HashSet).
    let mut paragraph_ids: Vec<NoteId> = Vec::new();
    let mut seen_paragraph: HashSet<NoteId> = HashSet::new();
    for target in &target_set {
        for (src, edge) in graph.incoming(*target) {
            if !matches!(edge, EdgeKind::ParagraphLink) {
                continue;
            }
            if !matches!(graph.node(src), NodeKind::Paragraph(_)) {
                continue;
            }
            if seen_paragraph.insert(src) {
                paragraph_ids.push(src);
            }
        }
    }

    // Resolve dates per paragraph, fetching blame lazily on demand.
    let head = git::head_hash(repo)?;
    let mut entries: Vec<JournalEntry> = Vec::new();
    let mut skipped_blame: Vec<PathBuf> = Vec::new();
    let mut skipped_seen: HashSet<PathBuf> = HashSet::new();
    for p_id in paragraph_ids {
        let NodeKind::Paragraph(p) = graph.node(p_id) else {
            continue;
        };
        if note_path.as_ref().is_some_and(|np| p.source_file == *np) {
            continue; // exclude N's own paragraphs (notes only; ghosts own nothing)
        }
        let path_str = p.source_file.to_string_lossy().into_owned();
        if cache.get(&path_str, &head).is_none() {
            // Try to populate. On blame failure (untracked / outside
            // repo / path-relativity bug) record it once per file so
            // callers can surface a warning instead of silently
            // returning an empty feed.
            match git::blame_file(repo, &p.source_file) {
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
        entries.push(JournalEntry {
            source_title,
            source_path: p.source_file.clone(),
            line_start: p.line_start,
            section_text: p.text.clone(),
            date,
        });
    }

    // Reverse-chronological; stable tiebreak on title ascending.
    entries.sort_by(|a, b| {
        b.date
            .cmp(&a.date)
            .then_with(|| a.source_title.cmp(&b.source_title))
    });
    skipped_blame.sort();
    Ok(JournalReport {
        entries,
        skipped_blame,
    })
}

/// Resolve the alias `NoteId`s declared in `note_id`'s `## Related`
/// section. Aliases are the targets of outgoing `Link` edges that fall
/// within the Related section's line range (inclusive of the heading
/// line, exclusive of the next equal-or-higher heading).
///
/// Returns an empty vec when the note has no Related heading (case
/// insensitive on the heading text).
pub fn resolve_related_aliases(
    graph: &Graph,
    note_id: NoteId,
    vault: &Vault,
    note_path: &Path,
) -> Result<Vec<NoteId>> {
    let abs = vault.path.join(note_path);
    let content = match std::fs::read_to_string(&abs) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };
    let headings = extract_headings(&content);
    let related_range = match find_related_range(&headings, &content) {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let mut alias_ids: Vec<NoteId> = Vec::new();
    let mut seen: HashSet<NoteId> = HashSet::new();
    for (dst, edge) in graph.outgoing(note_id) {
        let link = match edge {
            EdgeKind::Link(l) => l,
            _ => continue,
        };
        let line = link.line as u32;
        if line < related_range.0 || line > related_range.1 {
            continue;
        }
        if seen.insert(dst) {
            alias_ids.push(dst);
        }
    }
    Ok(alias_ids)
}

/// Return the inclusive 1-indexed `(start_line, end_line)` of the
/// `## Related` heading and its body — up to the next heading of equal
/// or higher level, or end of file. Heading text match is
/// case-insensitive; comparison ignores trailing whitespace and `#`s.
fn find_related_range(headings: &[crate::markdown::Heading], content: &str) -> Option<(u32, u32)> {
    let total_lines = content.lines().count() as u32;
    for (i, h) in headings.iter().enumerate() {
        if h.text.eq_ignore_ascii_case("Related") {
            let start = h.line as u32;
            // Find the next heading of equal-or-higher level (lower
            // or equal numeric level).
            let end = headings
                .iter()
                .skip(i + 1)
                .find(|next| next.level <= h.level)
                .map(|next| (next.line as u32) - 1)
                .unwrap_or(total_lines);
            return Some((start, end));
        }
    }
    None
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_related_range_no_related_heading() {
        let headings = vec![crate::markdown::Heading {
            text: "Other".into(),
            level: 2,
            line: 1,
        }];
        assert!(find_related_range(&headings, "## Other\n").is_none());
    }

    #[test]
    fn find_related_range_to_eof() {
        let content = "# Top\n\n## Related\n- [[Bar]]\n";
        let headings = extract_headings(content);
        let r = find_related_range(&headings, content).unwrap();
        assert_eq!(r, (3, 4));
    }

    #[test]
    fn find_related_range_bounded_by_next_heading() {
        let content = "## Related\n- [[Bar]]\n\n## Next\nbody\n";
        let headings = extract_headings(content);
        let r = find_related_range(&headings, content).unwrap();
        assert_eq!(r, (1, 3));
    }

    #[test]
    fn find_related_range_case_insensitive_match() {
        let content = "## related\n- [[Bar]]\n";
        let headings = extract_headings(content);
        let r = find_related_range(&headings, content).unwrap();
        assert_eq!(r.0, 1);
    }

    // ── build_journal integration ─────────────────────────────────────

    /// Build a vault under `tmp` with two commits so blame has real
    /// per-line dates. Returns (Vault, Graph, repo_path).
    fn make_vault_with_history(tmp: &assert_fs::TempDir) -> (Vault, Graph, std::path::PathBuf) {
        use assert_fs::prelude::*;
        use std::process::Command;

        tmp.child(".obsidian").create_dir_all().unwrap();

        // Commit 1: target note + one journal note linking to it.
        tmp.child("Target.md")
            .write_str("# Target\n\n## Related\n- [[Bar]]\n")
            .unwrap();
        tmp.child("Daily-A.md")
            .write_str("Note about [[Target]] today.\n")
            .unwrap();
        tmp.child("Bar.md").write_str("# Bar\n").unwrap();

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

        // Commit 2: a Bar-mentioning note added later (newer date).
        std::thread::sleep(std::time::Duration::from_millis(1100));
        tmp.child("Daily-B.md")
            .write_str("Followup about [[Bar]].\n")
            .unwrap();
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "c2"]);

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &crate::vault::Scan::default()).unwrap();
        (vault, graph, repo)
    }

    #[test]
    fn journal_includes_target_mentions_and_related_aliases() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let (vault, graph, repo) = make_vault_with_history(&tmp);
        let target = graph.note_by_path(Path::new("Target.md")).unwrap();
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, target, &vault, &repo, &mut cache).unwrap();
        assert!(report.skipped_blame.is_empty());

        // Daily-A mentions [[Target]]; Daily-B mentions [[Bar]] which
        // is a Related alias of Target. Both should appear.
        let titles: Vec<&str> = report
            .entries
            .iter()
            .map(|e| e.source_title.as_str())
            .collect();
        assert!(titles.contains(&"Daily-A"));
        assert!(titles.contains(&"Daily-B"));
        // Target.md itself should NOT appear (it links to its own
        // Bar alias through the Related list, but we exclude paragraphs
        // whose source_file is the queried note).
        assert!(!titles.contains(&"Target"));
    }

    #[test]
    fn journal_orders_entries_with_stable_title_tiebreak() {
        // Two entries committed seconds apart land on the same calendar
        // day → reverse-chrono sort can't distinguish them. The
        // deterministic title-ascending tiebreak should pick Daily-A
        // before Daily-B.
        let tmp = assert_fs::TempDir::new().unwrap();
        let (vault, graph, repo) = make_vault_with_history(&tmp);
        let target = graph.note_by_path(Path::new("Target.md")).unwrap();
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, target, &vault, &repo, &mut cache).unwrap();
        assert_eq!(report.entries.len(), 2);
        assert_eq!(
            report.entries[0].date, report.entries[1].date,
            "same-day commits"
        );
        assert_eq!(report.entries[0].source_title, "Daily-A");
        assert_eq!(report.entries[1].source_title, "Daily-B");
    }

    /// Journal works on a Ghost target — an unresolved-link concept
    /// with no backing file. The incoming `ParagraphLink` edges from
    /// notes that wrote `[[Phantom]]` still carry the references we
    /// want to surface.
    #[test]
    fn journal_includes_ghost_target_mentions() {
        use assert_fs::prelude::*;
        use std::process::Command;

        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        // Two notes that both reference [[Phantom]]; no Phantom.md
        // exists, so the wiki link target stays a Ghost in the graph.
        tmp.child("Notes-A.md")
            .write_str("Thinking about [[Phantom]] today.\n")
            .unwrap();
        tmp.child("Notes-B.md")
            .write_str("More on [[Phantom]] later.\n")
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

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &crate::vault::Scan::default()).unwrap();

        let phantom = graph
            .ghost_by_raw("Phantom")
            .expect("Phantom should be materialized as a Ghost");
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, phantom, &vault, &repo, &mut cache).unwrap();
        assert!(report.skipped_blame.is_empty(), "blame should succeed");
        let mut titles: Vec<&str> = report
            .entries
            .iter()
            .map(|e| e.source_title.as_str())
            .collect();
        titles.sort();
        assert_eq!(
            titles,
            vec!["Notes-A", "Notes-B"],
            "both ghost-mentioning notes must appear"
        );
    }

    /// Regression test for the subdirectory-vault bug: when the vault
    /// lives below the git repo root, we still want callers to be able
    /// to pass the vault path as the `repo` argument and have blame
    /// succeed against vault-relative paths (`git -C <vault>` resolves
    /// them inside the enclosing repo).
    #[test]
    fn journal_works_when_vault_is_repo_subdir() {
        use assert_fs::prelude::*;
        use std::process::Command;
        let tmp = assert_fs::TempDir::new().unwrap();
        let repo_root = tmp.path().to_path_buf();
        let vault_dir = repo_root.join("brain");
        std::fs::create_dir_all(&vault_dir).unwrap();
        // Initialize git at the parent, not the vault.
        let run_git = |dir: &Path, args: &[&str]| {
            let out = Command::new("git")
                .current_dir(dir)
                .env("GIT_TERMINAL_PROMPT", "0")
                .args(args)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}");
        };
        run_git(&repo_root, &["init", "-b", "main"]);
        run_git(&repo_root, &["config", "user.name", "T"]);
        run_git(&repo_root, &["config", "user.email", "t@e.com"]);
        run_git(&repo_root, &["config", "commit.gpgsign", "false"]);

        // Vault contents.
        tmp.child("brain/.obsidian").create_dir_all().unwrap();
        tmp.child("brain/Target.md")
            .write_str("# Target\n")
            .unwrap();
        tmp.child("brain/Daily.md")
            .write_str("Mention [[Target]] here.\n")
            .unwrap();
        run_git(&repo_root, &["add", "."]);
        run_git(&repo_root, &["commit", "-m", "c1"]);

        let vault = Vault::discover(Some(vault_dir.clone())).unwrap();
        let graph = Graph::build(&vault, &crate::vault::Scan::default()).unwrap();
        let target = graph.note_by_path(Path::new("Target.md")).unwrap();
        let mut cache = BlameCache::default();
        // Pass vault.path (not the repo root) as the `repo` argument —
        // git -C still finds the enclosing repo and vault-relative
        // paths resolve correctly.
        let report = build_journal(&graph, target, &vault, &vault.path, &mut cache).unwrap();
        assert!(
            report.skipped_blame.is_empty(),
            "expected no blame skips, got {:?}",
            report.skipped_blame
        );
        let titles: Vec<&str> = report
            .entries
            .iter()
            .map(|e| e.source_title.as_str())
            .collect();
        assert!(titles.contains(&"Daily"), "got titles: {titles:?}");
    }
}

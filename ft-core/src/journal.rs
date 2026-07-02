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
    /// 1-indexed line number of the paragraph's last line.
    pub line_end: u32,
    /// The paragraph text itself.
    pub section_text: String,
    /// Date of the most recent commit touching any line in the
    /// paragraph (UTC).
    pub date: NaiveDate,
    /// The subset of the caller's `targets` slice that this paragraph
    /// has a `ParagraphLink` edge to. In single-target mode this is
    /// always `vec![targets[0]]`; renderers can ignore the field. In
    /// multi-target mode it identifies which selected links a paragraph
    /// matched — used by the TUI to render a `matched: X, Y` badge when
    /// `matched.len() > 1`.
    pub matched: Vec<NoteId>,
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

/// Build the reverse-chronological journal feed for the given `targets`.
///
/// **Single-target mode** (`targets.len() == 1`): preserves the original
/// per-note journal semantics:
/// 1. Resolve aliases by scanning the target's `## Related` heading
///    range for outgoing `Link` edges (notes only; skipped for ghosts).
/// 2. Collect every `Paragraph` node with a `ParagraphLink` edge into
///    the target or any alias.
/// 3. For notes only: exclude paragraphs whose `source_file` is the
///    target's own path.
///
/// Every returned entry's `matched` field is `vec![targets[0]]`.
///
/// **Multi-target mode** (`targets.len() > 1`): the user has explicitly
/// selected a set of links; alias resolution and self-exclusion are
/// SKIPPED. A paragraph is included if it has a `ParagraphLink` edge to
/// any of the targets. Each entry's `matched` field carries the subset
/// of `targets` that the paragraph linked to (preserving the order in
/// which they appear in `targets`).
///
/// Both modes: dates come from `git blame` via `cache` (lazy-populated),
/// sort is date descending with source title ascending, then
/// `line_start` ascending as a final document-order tiebreak (never
/// overrides a date or title difference — paragraph recentness stays
/// dominant).
///
/// ## Heading-section expansion
///
/// A paragraph is also included when a heading in its owning chain has
/// a `HeadingLink` edge to a target (a link written *inside* heading
/// text, e.g. `## Thoughts about [[Foo]]`). The whole section under
/// that heading — its `OwnsParagraph` children plus those of its
/// `OwnsHeading`-descendant sub-headings, via `Graph::note_paragraphs`
/// — is added, one entry per paragraph. Per-paragraph dates are
/// preserved (each entry's date is its own blame range, not a shared
/// section date). A paragraph reachable both via a direct
/// `ParagraphLink` and via expansion appears once; its `matched` is
/// derived from its own direct edge when present (direct wins), else
/// inherited from the linking heading chain. Anchored links *targeting*
/// a heading from elsewhere (`[[Foo#Bar]]` in a body) do not trigger
/// expansion — only `HeadingLink` *from* a heading does.
///
/// Targets that resolve to `Directory`/`Task`/`Paragraph` nodes are
/// silently ignored — those node kinds have no journal semantics.
///
/// Passing an empty slice returns an empty report.
pub fn build_journal(
    graph: &Graph,
    targets: &[NoteId],
    vault: &Vault,
    cache: &mut BlameCache,
) -> Result<JournalReport> {
    if targets.is_empty() {
        return Ok(JournalReport::default());
    }

    // All git lookups run against the repo root with repo-root-relative
    // paths; node paths are vault-relative, so translate at the boundary.
    let repo = git::RepoMap::discover(&vault.path)?;

    let single_mode = targets.len() == 1;

    // In single-target mode, resolve aliases and capture the target's
    // path for self-exclusion. In multi-target mode, both are skipped.
    let (self_path, alias_ids): (Option<PathBuf>, Vec<NoteId>) = if single_mode {
        let primary = targets[0];
        let note_path: Option<PathBuf> = match graph.node(primary) {
            NodeKind::Note(n) => Some(n.path.clone()),
            NodeKind::Ghost(_) => None,
            _ => return Ok(JournalReport::default()),
        };
        let aliases = match &note_path {
            Some(p) => resolve_related_aliases(graph, primary, vault, p)?,
            None => Vec::new(),
        };
        (note_path, aliases)
    } else {
        (None, Vec::new())
    };

    // Build the set of NoteIds whose mentions count. Use mentions_of
    // (note + its headings) so anchored links targeting a heading of
    // the note still count as a mention of the note.
    let mut target_set: HashSet<NoteId> = targets.iter().copied().collect();
    target_set.extend(alias_ids.iter().copied());

    // Collect candidate paragraph nodes (dedup via HashSet). Two passes:
    //
    // (1) Direct match: a mention's source is the paragraph node for
    //     ParagraphLink edges (and the note for NoteLink, which we skip
    //     — journal entries are paragraph-scoped).
    // (2) Heading-section expansion: a Heading source (a heading whose
    //     HeadingLink targets a target) contributes every paragraph
    //     transitively owned by it via `Graph::note_paragraphs(H)` —
    //     the section up to the next same-or-higher heading, including
    //     nested sub-sections. Sibling paragraphs need not repeat the
    //     link; having it in the heading is enough.
    //
    // A paragraph reachable both ways is deduped via `seen_paragraph`;
    // the direct-match pass runs first so direct attribution wins.
    let mut paragraph_ids: Vec<NoteId> = Vec::new();
    let mut seen_paragraph: HashSet<NoteId> = HashSet::new();
    let mut expansion_headings: HashSet<NoteId> = HashSet::new();
    for target in &target_set {
        for (src, _edge) in graph.mentions_of(*target) {
            match graph.node(src) {
                NodeKind::Paragraph(_) if seen_paragraph.insert(src) => {
                    paragraph_ids.push(src);
                }
                NodeKind::Heading(_) => {
                    // Defer expansion until after the direct pass so
                    // direct-matched paragraphs keep their attribution.
                    expansion_headings.insert(src);
                }
                _ => {} // NoteLink sources (the note) have no paragraph entry.
            }
        }
    }
    for h_id in expansion_headings {
        for p_id in graph.note_paragraphs(h_id) {
            if seen_paragraph.insert(p_id) {
                paragraph_ids.push(p_id);
            }
        }
    }

    // Resolve dates per paragraph, fetching blame lazily on demand.
    let head = git::head_hash(repo.root())?;
    let mut entries: Vec<JournalEntry> = Vec::new();
    let mut skipped_blame: Vec<PathBuf> = Vec::new();
    let mut skipped_seen: HashSet<PathBuf> = HashSet::new();
    for p_id in paragraph_ids {
        let NodeKind::Paragraph(p) = graph.node(p_id) else {
            continue;
        };
        if self_path.as_ref().is_some_and(|np| p.source_file == *np) {
            continue; // single-target self-exclusion
        }
        let path_str = p.source_file.to_string_lossy().into_owned();
        if cache.get(&path_str, &head).is_none() {
            // Try to populate. On blame failure (untracked / outside
            // repo / path-relativity bug) record it once per file so
            // callers can surface a warning instead of silently
            // returning an empty feed.
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

        // Compute the matched subset. Single-target: always vec![targets[0]].
        // Multi-target: the subset of `targets` (preserving caller order) that
        // this paragraph has a ParagraphLink edge to. A ParagraphLink may
        // target a heading node (anchored link); map each target back to
        // its note-level identity via link_target_note so anchored links
        // still match the owning note. Direct matches win: an expansion-only
        // paragraph (no direct ParagraphLink to any target) falls back to
        // the targets its owning-chain headings link to, so it is attributed
        // to the heading that pulled it in rather than reported as empty.
        let matched: Vec<NoteId> = if single_mode {
            vec![targets[0]]
        } else {
            let direct: HashSet<NoteId> = graph
                .outgoing(p_id)
                .filter_map(|(dst, edge)| matches!(edge, EdgeKind::ParagraphLink(_)).then_some(dst))
                .filter_map(|dst| graph.link_target_note(dst))
                .filter(|t| target_set.contains(t))
                .collect();
            let inherited = if direct.is_empty() {
                heading_chain_targets(graph, p_id, &target_set)
            } else {
                HashSet::new()
            };
            targets
                .iter()
                .copied()
                .filter(|t| direct.contains(t) || inherited.contains(t))
                .collect()
        };

        entries.push(JournalEntry {
            source_title,
            source_path: p.source_file.clone(),
            line_start: p.line_start,
            line_end: p.line_end,
            section_text: p.text.clone(),
            date,
            matched,
        });
    }

    // Reverse-chronological; title ascending tiebreak; final `line_start`
    // ascending so co-located same-date paragraphs read top-to-bottom.
    // The tiebreak never overrides a date or title difference.
    entries.sort_by(|a, b| {
        b.date
            .cmp(&a.date)
            .then_with(|| a.source_title.cmp(&b.source_title))
            .then_with(|| a.line_start.cmp(&b.line_start))
    });
    skipped_blame.sort();
    Ok(JournalReport {
        entries,
        skipped_blame,
    })
}

/// Targets reachable from a paragraph's owning heading chain via
/// `HeadingLink` edges — the set used to attribute `matched` for an
/// expansion-only paragraph (one with no direct `ParagraphLink` to any
/// target).
///
/// Walks from the paragraph's nearest `OwnsParagraph` container; if it
/// is a `Heading`, climbs `OwnsHeading` ancestors to the note, collecting
/// every `HeadingLink` destination (mapped to note identity through
/// [`Graph::link_target_note`]) that is in `target_set`. Returns the empty
/// set when the container is the note or no heading links a target.
///
/// Only `HeadingLink` edges count — a link written *inside* heading text
/// is the expansion trigger. Anchored links *targeting* a heading from
/// elsewhere are handled by the direct-match pass and do not contribute
/// here.
fn heading_chain_targets(
    graph: &Graph,
    paragraph_id: NoteId,
    target_set: &HashSet<NoteId>,
) -> HashSet<NoteId> {
    // Find the nearest OwnsParagraph container of the paragraph.
    let owner = graph
        .incoming(paragraph_id)
        .find(|(_, e)| matches!(e, EdgeKind::OwnsParagraph))
        .map(|(src, _)| src);
    let Some(owner) = owner else {
        return HashSet::new();
    };
    if !matches!(graph.node(owner), NodeKind::Heading(_)) {
        return HashSet::new(); // note-owned paragraph: no heading chain.
    }

    // Climb OwnsHeading from the owning heading to the note, collecting
    // HeadingLink destinations at each heading along the way.
    let mut out = HashSet::new();
    let mut cur = Some(owner);
    while let Some(h) = cur {
        for (dst, edge) in graph.outgoing(h) {
            if !matches!(edge, EdgeKind::HeadingLink(_)) {
                continue;
            }
            if let Some(note_dst) = graph.link_target_note(dst) {
                if target_set.contains(&note_dst) {
                    out.insert(note_dst);
                }
            }
        }
        cur = graph
            .incoming(h)
            .find(|(_, e)| matches!(e, EdgeKind::OwnsHeading))
            .map(|(src, _)| src);
    }
    out
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
            EdgeKind::NoteLink(l) => l,
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
    use std::process::Command;

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
    fn make_vault_with_history(tmp: &assert_fs::TempDir) -> (Vault, Graph) {
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
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        (vault, graph)
    }

    #[test]
    fn journal_includes_target_mentions_and_related_aliases() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let (vault, graph) = make_vault_with_history(&tmp);
        let target = graph.note_by_path(Path::new("Target.md")).unwrap();
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, &[target], &vault, &mut cache).unwrap();
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

        // Single-target mode: every entry's `matched` is exactly the
        // one passed target (renderers can ignore the field).
        for entry in &report.entries {
            assert_eq!(entry.matched, vec![target]);
        }
    }

    #[test]
    fn journal_orders_entries_with_stable_title_tiebreak() {
        // Two entries committed seconds apart land on the same calendar
        // day → reverse-chrono sort can't distinguish them. The
        // deterministic title-ascending tiebreak should pick Daily-A
        // before Daily-B.
        let tmp = assert_fs::TempDir::new().unwrap();
        let (vault, graph) = make_vault_with_history(&tmp);
        let target = graph.note_by_path(Path::new("Target.md")).unwrap();
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, &[target], &vault, &mut cache).unwrap();
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
        let graph = Graph::build(&vault, &vault.scan()).unwrap();

        let phantom = graph
            .ghost_by_raw("Phantom")
            .expect("Phantom should be materialized as a Ghost");
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, &[phantom], &vault, &mut cache).unwrap();
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
    /// lives below the git repo root, `build_journal` discovers the
    /// enclosing repo and prefixes vault-relative node paths so blame
    /// resolves against the repo-root-relative path.
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
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let target = graph.note_by_path(Path::new("Target.md")).unwrap();
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, &[target], &vault, &mut cache).unwrap();
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

    // ── heading-section expansion ────────────────────────────────────
    //
    // The fixture style below uses `git commit` per paragraph-day so
    // blame yields distinguishable per-paragraph dates. `run_git` and
    // `commit_all` are local to each test that needs history; the
    // heading-expansion cases that don't need date distinctions reuse
    // a single commit.

    fn init_git_repo(repo: &Path) {
        let run_git = |args: &[&str]| {
            let out = Command::new("git")
                .current_dir(repo)
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
    }

    fn commit_all(repo: &Path, msg: &str) {
        let out = Command::new("git")
            .current_dir(repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["add", "."])
            .output()
            .expect("git add");
        assert!(out.status.success());
        let out = Command::new("git")
            .current_dir(repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["commit", "-m", msg])
            .output()
            .expect("git commit");
        assert!(out.status.success());
    }

    /// Heading link expands to all sibling paragraphs in the section.
    /// `## Thoughts about [[Foo]]` followed by A/B/C under it and a
    /// `## Next section` paragraph D: A, B, C included; D excluded.
    #[test]
    fn heading_link_expands_section_paragraphs() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("Foo.md").write_str("# Foo\n").unwrap();
        // Heading line begins paragraph A (Fork A2); blank-line-separated
        // B and C follow under the same heading; D starts a new section.
        tmp.child("Daily.md")
            .write_str(
                "# Day\n\n## Thoughts about [[Foo]]\nPara A.\n\nPara B.\n\nPara C.\n\n## Next section\n\nPara D.\n",
            )
            .unwrap();
        let repo = tmp.path().to_path_buf();
        init_git_repo(&repo);
        commit_all(&repo, "c1");

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let foo = graph.note_by_path(Path::new("Foo.md")).unwrap();
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, &[foo], &vault, &mut cache).unwrap();
        assert!(report.skipped_blame.is_empty());

        // The three section paragraphs (A includes the heading line text
        // per Fork A2) are present; the next-section paragraph is not.
        let bodies: Vec<String> = report
            .entries
            .iter()
            .map(|e| e.section_text.clone())
            .collect();
        assert_eq!(bodies.len(), 3, "got {bodies:?}");
        assert!(bodies.iter().any(|t| t.contains("Para A")), "{bodies:?}");
        assert!(bodies.iter().any(|t| t.contains("Para B")), "{bodies:?}");
        assert!(bodies.iter().any(|t| t.contains("Para C")), "{bodies:?}");
        assert!(!bodies.iter().any(|t| t.contains("Para D")), "{bodies:?}");
        // Each is a separate entry (one paragraph per entry).
        assert_eq!(report.entries.len(), 3);
    }

    /// Expansion includes paragraphs under nested sub-headings: A under
    /// `## Thoughts about [[Foo]]`, B under `### Sub-point`, C under the
    /// next `## Next section`. Only A and B are in the section.
    #[test]
    fn expansion_includes_nested_subheading_paragraphs() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("Foo.md").write_str("# Foo\n").unwrap();
        tmp.child("Daily.md")
            .write_str(
                "# Day\n\n## Thoughts about [[Foo]]\n\nPara A.\n\n### Sub-point\n\nPara B.\n\n## Next section\n\nPara C.\n",
            )
            .unwrap();
        let repo = tmp.path().to_path_buf();
        init_git_repo(&repo);
        commit_all(&repo, "c1");

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let foo = graph.note_by_path(Path::new("Foo.md")).unwrap();
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, &[foo], &vault, &mut cache).unwrap();
        let bodies: Vec<String> = report
            .entries
            .iter()
            .map(|e| e.section_text.clone())
            .collect();
        assert!(bodies.iter().any(|t| t.contains("Para A")), "{bodies:?}");
        assert!(bodies.iter().any(|t| t.contains("Para B")), "{bodies:?}");
        assert!(!bodies.iter().any(|t| t.contains("Para C")), "{bodies:?}");
    }

    /// Each expanded paragraph keeps its own per-paragraph blame date,
    /// not a shared section date. Commits A, B, C on different days.
    #[test]
    fn expanded_paragraphs_keep_own_dates() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("Foo.md").write_str("# Foo\n").unwrap();
        // Day 1: heading + Para A (no blank line → Fork A2 merges them
        // into one paragraph that carries the heading link).
        tmp.child("Daily.md")
            .write_str("# Day\n\n## Thoughts about [[Foo]]\nPara A.\n")
            .unwrap();
        let repo = tmp.path().to_path_buf();
        init_git_repo(&repo);
        // Backdate commit 1 to a known older date.
        let out = Command::new("git")
            .current_dir(&repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_DATE", "2025-01-01T00:00:00")
            .env("GIT_COMMITTER_DATE", "2025-01-01T00:00:00")
            .args(["add", "."])
            .output()
            .expect("git add");
        assert!(out.status.success());
        let out = Command::new("git")
            .current_dir(&repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_DATE", "2025-01-01T00:00:00")
            .env("GIT_COMMITTER_DATE", "2025-01-01T00:00:00")
            .args(["commit", "-m", "c1"])
            .output()
            .expect("git commit");
        assert!(out.status.success());
        // Day 2: append Para B.
        tmp.child("Daily.md")
            .write_str("# Day\n\n## Thoughts about [[Foo]]\nPara A.\n\nPara B.\n")
            .unwrap();
        let out = Command::new("git")
            .current_dir(&repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_DATE", "2025-02-01T00:00:00")
            .env("GIT_COMMITTER_DATE", "2025-02-01T00:00:00")
            .args(["add", "."])
            .output()
            .expect("git add");
        assert!(out.status.success());
        let out = Command::new("git")
            .current_dir(&repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_DATE", "2025-02-01T00:00:00")
            .env("GIT_COMMITTER_DATE", "2025-02-01T00:00:00")
            .args(["commit", "-m", "c2"])
            .output()
            .expect("git commit");
        assert!(out.status.success());

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let foo = graph.note_by_path(Path::new("Foo.md")).unwrap();
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, &[foo], &vault, &mut cache).unwrap();
        assert_eq!(report.entries.len(), 2, "got {:?}", report.entries);
        // Map text → date, then assert each paragraph's date is its own.
        let by_text: std::collections::HashMap<&str, chrono::NaiveDate> = report
            .entries
            .iter()
            .map(|e| {
                let key = if e.section_text.contains("Para A") {
                    "A"
                } else {
                    "B"
                };
                (key, e.date)
            })
            .collect();
        assert_eq!(
            by_text["A"],
            chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()
        );
        assert_eq!(
            by_text["B"],
            chrono::NaiveDate::from_ymd_opt(2025, 2, 1).unwrap()
        );
    }

    /// Multi-target: an expansion-only paragraph (no direct ParagraphLink)
    /// under `## About [[Foo]]` gets matched == [Foo] (inherited from the
    /// linking heading).
    #[test]
    fn expansion_matched_inherited_from_heading() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("Foo.md").write_str("# Foo\n").unwrap();
        tmp.child("Bar.md").write_str("# Bar\n").unwrap();
        // Heading links Foo; Para B has no direct link.
        tmp.child("Daily.md")
            .write_str("# Day\n\n## About [[Foo]]\n\nPara A.\n\nPara B.\n")
            .unwrap();
        let repo = tmp.path().to_path_buf();
        init_git_repo(&repo);
        commit_all(&repo, "c1");

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let foo = graph.note_by_path(Path::new("Foo.md")).unwrap();
        let bar = graph.note_by_path(Path::new("Bar.md")).unwrap();
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, &[foo, bar], &vault, &mut cache).unwrap();
        // Para B: expansion-only, no direct link → matched should be [Foo].
        let para_b = report
            .entries
            .iter()
            .find(|e| e.section_text.contains("Para B"))
            .expect("Para B present");
        assert_eq!(para_b.matched, vec![foo], "got {:?}", para_b.matched);
        // Para A: also expansion-only here (the heading link is the only
        // link; Para A's body has no [[Foo]]) → also [Foo].
        let para_a = report
            .entries
            .iter()
            .find(|e| e.section_text.contains("Para A"))
            .expect("Para A present");
        assert_eq!(para_a.matched, vec![foo], "got {:?}", para_a.matched);
    }

    /// Direct- and expansion-matched paragraph appears once, with matched
    /// derived from its own direct ParagraphLink (direct wins).
    #[test]
    fn direct_and_expansion_matched_appears_once_direct_wins() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("Foo.md").write_str("# Foo\n").unwrap();
        tmp.child("Bar.md").write_str("# Bar\n").unwrap();
        // Heading links Foo; Para A ALSO directly links Bar in its body.
        tmp.child("Daily.md")
            .write_str("# Day\n\n## About [[Foo]]\n\nPara A about [[Bar]].\n\nPara B.\n")
            .unwrap();
        let repo = tmp.path().to_path_buf();
        init_git_repo(&repo);
        commit_all(&repo, "c1");

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let foo = graph.note_by_path(Path::new("Foo.md")).unwrap();
        let bar = graph.note_by_path(Path::new("Bar.md")).unwrap();
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, &[foo, bar], &vault, &mut cache).unwrap();
        // Para A appears exactly once.
        let para_a: Vec<_> = report
            .entries
            .iter()
            .filter(|e| e.section_text.contains("Para A"))
            .collect();
        assert_eq!(para_a.len(), 1, "got {:?}", para_a);
        // Direct wins: matched is [Bar] (its own ParagraphLink), not [Foo]
        // (the inherited heading-link target) — both are targets, direct
        // is the only attribution source used.
        assert_eq!(para_a[0].matched, vec![bar], "got {:?}", para_a[0].matched);
    }

    /// Single-target self-exclusion drops the target note's own paragraphs
    /// even when reached via heading expansion.
    #[test]
    fn single_target_self_exclusion_drops_expanded_own_paragraphs() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        // Foo links itself from a heading. Foo's own paragraphs must be
        // excluded (self-exclusion), so an empty journal results.
        tmp.child("Foo.md")
            .write_str("# Foo\n\n## Notes about [[Foo]]\n\nSelf para A.\n\nSelf para B.\n")
            .unwrap();
        let repo = tmp.path().to_path_buf();
        init_git_repo(&repo);
        commit_all(&repo, "c1");

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let foo = graph.note_by_path(Path::new("Foo.md")).unwrap();
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, &[foo], &vault, &mut cache).unwrap();
        assert!(
            report.entries.is_empty(),
            "self-exclusion should drop Foo's own paragraphs, got {:?}",
            report.entries
        );
    }

    /// Heading link to a ghost target expands its section.
    #[test]
    fn heading_link_to_ghost_expands_section() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        // No Phantom.md exists — the heading links a ghost.
        tmp.child("Daily.md")
            .write_str("# Day\n\n## About [[Phantom]]\n\nPara A.\n\nPara B.\n")
            .unwrap();
        let repo = tmp.path().to_path_buf();
        init_git_repo(&repo);
        commit_all(&repo, "c1");

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let phantom = graph
            .ghost_by_raw("Phantom")
            .expect("Phantom should be a ghost");
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, &[phantom], &vault, &mut cache).unwrap();
        assert!(report.skipped_blame.is_empty());
        let bodies: Vec<String> = report
            .entries
            .iter()
            .map(|e| e.section_text.clone())
            .collect();
        assert!(bodies.iter().any(|t| t.contains("Para A")), "{bodies:?}");
        assert!(bodies.iter().any(|t| t.contains("Para B")), "{bodies:?}");
    }

    /// Same-date same-title paragraphs sort by line_start ascending
    /// (document order). All committed in one commit → same date.
    #[test]
    fn same_date_paragraphs_ordered_by_line_start() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("Foo.md").write_str("# Foo\n").unwrap();
        // Three sibling paragraphs under one heading, one commit → same date.
        // No blank line after the heading so it merges with the first body
        // (Fork A2); then two blank-line-separated paragraphs follow.
        tmp.child("Daily.md")
            .write_str(
                "# Day\n\n## Thoughts about [[Foo]]\nFirst para.\n\nSecond para.\n\nThird para.\n",
            )
            .unwrap();
        let repo = tmp.path().to_path_buf();
        init_git_repo(&repo);
        commit_all(&repo, "c1");

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let foo = graph.note_by_path(Path::new("Foo.md")).unwrap();
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, &[foo], &vault, &mut cache).unwrap();
        assert_eq!(report.entries.len(), 3);
        // Reverse-chrono by date (all equal) → title (all equal) → line_start asc.
        let texts: Vec<&str> = report
            .entries
            .iter()
            .map(|e| e.section_text.as_str())
            .collect();
        assert!(texts[0].contains("First para"), "got {texts:?}");
        assert!(texts[1].contains("Second para"), "got {texts:?}");
        assert!(texts[2].contains("Third para"), "got {texts:?}");
        // line_start strictly ascending.
        let starts: Vec<u32> = report.entries.iter().map(|e| e.line_start).collect();
        assert!(
            starts[0] < starts[1] && starts[1] < starts[2],
            "got {starts:?}"
        );
    }

    /// Anchored link targeting a heading (`[[Foo#Bar]]` in a body) includes
    /// the body paragraph (direct match) but does NOT expand Foo's `## Bar`
    /// section — only a HeadingLink *from* a heading triggers expansion.
    #[test]
    fn anchored_link_targeting_heading_does_not_expand() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        // Foo has a `## Bar` heading with body paragraphs that do NOT link
        // anything — they should NOT be pulled in.
        tmp.child("Foo.md")
            .write_str("# Foo\n\n## Bar\n\nFoo Bar body one.\n\nFoo Bar body two.\n")
            .unwrap();
        // Daily has a body paragraph with an anchored link to Foo#Bar.
        tmp.child("Daily.md")
            .write_str("# Day\n\nSee [[Foo#Bar]] for context.\n")
            .unwrap();
        let repo = tmp.path().to_path_buf();
        init_git_repo(&repo);
        commit_all(&repo, "c1");

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let foo = graph.note_by_path(Path::new("Foo.md")).unwrap();
        let mut cache = BlameCache::default();
        let report = build_journal(&graph, &[foo], &vault, &mut cache).unwrap();
        // The Daily body paragraph is a direct match → included.
        let daily_present = report
            .entries
            .iter()
            .any(|e| e.section_text.contains("See [[Foo#Bar]]"));
        assert!(
            daily_present,
            "direct anchored-link paragraph must be included"
        );
        // Foo's `## Bar` section paragraphs must NOT be expanded in.
        let foo_bodies: Vec<_> = report
            .entries
            .iter()
            .filter(|e| e.source_title == "Foo")
            .collect();
        assert!(
            foo_bodies.is_empty(),
            "anchored link must not expand Foo#Bar section, got {foo_bodies:?}"
        );
    }
}

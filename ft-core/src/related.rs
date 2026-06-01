//! Related Section Updater — co-occurrence scoring + plan/apply
//! write-back for the `## Related` section of a note.
//!
//! Scoring is driven entirely by the graph: paragraph nodes with
//! `ParagraphLink` edges identify which concepts co-occur with the
//! target note (or any of its current Related aliases). Same-paragraph
//! co-occurrence is the strongest signal (+3); same-file
//! cross-paragraph co-occurrence is weaker (+1).
//!
//! The `plan_related_update` / `apply_related_update` pair follows the
//! library's plan/apply pattern: planning is pure (string in, plan
//! out); apply touches the filesystem via `write_atomic`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::fs::write_atomic;
use crate::graph::{EdgeKind, Graph, NodeKind, NoteId};
use crate::journal::resolve_related_aliases;
use crate::markdown::extract_headings;
use crate::vault::Vault;

/// One row of the Related-updater suggestion list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedScore {
    /// The concept's graph node (a Note or a Ghost).
    pub note_id: NoteId,
    /// Display string: note's filename stem, or ghost's raw target.
    pub title: String,
    /// Aggregate score: 3 × same-paragraph co-occurrences + 1 ×
    /// same-file cross-paragraph co-occurrences (distinct files).
    pub score: u32,
    /// True when this concept is already a `[[wiki link]]` in N's
    /// `## Related` section.
    pub already_in_related: bool,
}

/// Compute Related-section scoring for `note_id`. N is excluded from
/// results; concepts that score 0 are omitted. Aliases of N (concepts
/// already in the Related section) appear in the result with
/// `already_in_related = true`.
pub fn score_related(graph: &Graph, note_id: NoteId, vault: &Vault) -> Result<Vec<RelatedScore>> {
    let note_path = match graph.node(note_id) {
        NodeKind::Note(n) => n.path.clone(),
        _ => return Ok(Vec::new()),
    };

    let alias_ids = resolve_related_aliases(graph, note_id, vault, &note_path)?;
    let alias_set: HashSet<NoteId> = alias_ids.iter().copied().collect();
    let mut target_set: HashSet<NoteId> = HashSet::new();
    target_set.insert(note_id);
    target_set.extend(alias_set.iter().copied());

    // Same-paragraph co-occurrence: walk paragraphs that link to any
    // target; collect their other ParagraphLink targets.
    let mut same_paragraph: HashMap<NoteId, u32> = HashMap::new();
    let mut matched_paragraphs: HashSet<NoteId> = HashSet::new();
    for target in &target_set {
        for (src, edge) in graph.incoming(*target) {
            if !matches!(edge, EdgeKind::ParagraphLink) {
                continue;
            }
            if !matches!(graph.node(src), NodeKind::Paragraph(_)) {
                continue;
            }
            matched_paragraphs.insert(src);
        }
    }
    for p_id in &matched_paragraphs {
        let mut per_paragraph: HashSet<NoteId> = HashSet::new();
        for (dst, edge) in graph.outgoing(*p_id) {
            if !matches!(edge, EdgeKind::ParagraphLink) {
                continue;
            }
            if dst == note_id {
                continue;
            }
            // De-dup per paragraph: a paragraph counts +3 for C once
            // even if it links to C multiple times.
            per_paragraph.insert(dst);
        }
        for c in per_paragraph {
            *same_paragraph.entry(c).or_insert(0) += 3;
        }
    }

    // Same-file cross-paragraph co-occurrence: for each file that
    // contains a matched paragraph, find non-matched paragraphs and
    // collect their ParagraphLink targets. Each file contributes at
    // most +1 per concept.
    let mut files_with_matches: HashSet<PathBuf> = HashSet::new();
    for p_id in &matched_paragraphs {
        if let NodeKind::Paragraph(p) = graph.node(*p_id) {
            files_with_matches.insert(p.source_file.clone());
        }
    }
    let mut same_file: HashMap<NoteId, u32> = HashMap::new();
    for file_path in &files_with_matches {
        let Some(owner) = graph.note_by_path(file_path) else {
            continue;
        };
        // Collect distinct C's from non-matched paragraphs in this file.
        let mut file_concepts: HashSet<NoteId> = HashSet::new();
        for (p_id, edge) in graph.outgoing(owner) {
            if !matches!(edge, EdgeKind::OwnsParagraph) {
                continue;
            }
            if matched_paragraphs.contains(&p_id) {
                continue;
            }
            for (dst, e) in graph.outgoing(p_id) {
                if !matches!(e, EdgeKind::ParagraphLink) {
                    continue;
                }
                if dst == note_id {
                    continue;
                }
                file_concepts.insert(dst);
            }
        }
        for c in file_concepts {
            *same_file.entry(c).or_insert(0) += 1;
        }
    }

    // Combine + filter zero-score; build display rows.
    let mut all_concepts: HashSet<NoteId> = HashSet::new();
    all_concepts.extend(same_paragraph.keys().copied());
    all_concepts.extend(same_file.keys().copied());

    let mut rows: Vec<RelatedScore> = Vec::new();
    for c in all_concepts {
        let score =
            same_paragraph.get(&c).copied().unwrap_or(0) + same_file.get(&c).copied().unwrap_or(0);
        if score == 0 {
            continue;
        }
        let title = match graph.node(c) {
            NodeKind::Note(n) => n.title.clone(),
            NodeKind::Ghost(g) => g.raw.clone(),
            _ => continue,
        };
        rows.push(RelatedScore {
            note_id: c,
            title,
            score,
            already_in_related: alias_set.contains(&c),
        });
    }
    rows.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));
    Ok(rows)
}

// ── Plan / apply ────────────────────────────────────────────────────────

/// Pure description of an update to a note's `## Related` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedUpdatePlan {
    /// The full new contents of the note after applying selected
    /// concepts. Empty `new_concepts` produces a no-op plan with
    /// `new_content == original`.
    pub new_content: String,
    /// `true` when the original content had no `## Related` heading
    /// and one was inserted; informational for the caller.
    pub created_section: bool,
    /// Concepts that were actually appended (after dedup against the
    /// existing Related section's link targets).
    pub appended: Vec<String>,
}

impl RelatedUpdatePlan {
    pub fn is_noop(&self, original: &str) -> bool {
        self.new_content == original
    }
}

/// Build a `RelatedUpdatePlan` that appends each entry in
/// `new_concepts` as a bare `- [[Concept]]` line at the end of the
/// `## Related` section. If the section doesn't exist, append
/// `## Related` at the end of the file before the entries.
///
/// Entries that already appear as wiki links inside the existing
/// Related section are silently dropped (no duplicates).
pub fn plan_related_update(content: &str, new_concepts: &[String]) -> RelatedUpdatePlan {
    if new_concepts.is_empty() {
        return RelatedUpdatePlan {
            new_content: content.to_string(),
            created_section: false,
            appended: Vec::new(),
        };
    }

    let headings = extract_headings(content);
    let related = headings
        .iter()
        .enumerate()
        .find(|(_, h)| h.text.eq_ignore_ascii_case("Related"));

    let (related_start_line, related_end_line, has_section): (usize, usize, bool) = match related {
        Some((idx, h)) => {
            // Inclusive end-line of the related section's body.
            let total_lines = content.lines().count();
            let end = headings
                .iter()
                .skip(idx + 1)
                .find(|next| next.level <= h.level)
                .map(|next| next.line.saturating_sub(1))
                .unwrap_or(total_lines);
            (h.line, end, true)
        }
        None => (0, 0, false),
    };

    // Dedup against existing wiki link targets in the section (when
    // present). Targets are matched case-insensitively, matching the
    // graph's resolution behavior.
    let existing_targets: HashSet<String> = if has_section {
        let lines: Vec<&str> = content.lines().collect();
        let mut s = HashSet::new();
        for line in lines.iter().take(related_end_line).skip(related_start_line) {
            for target in extract_wiki_targets(line) {
                s.insert(target.to_lowercase());
            }
        }
        s
    } else {
        HashSet::new()
    };

    let mut appended: Vec<String> = Vec::new();
    let mut seen_new: HashSet<String> = HashSet::new();
    for concept in new_concepts {
        let key = concept.to_lowercase();
        if existing_targets.contains(&key) {
            continue;
        }
        if !seen_new.insert(key) {
            continue;
        }
        appended.push(concept.clone());
    }

    if appended.is_empty() {
        return RelatedUpdatePlan {
            new_content: content.to_string(),
            created_section: false,
            appended,
        };
    }

    let mut new_content = String::new();
    if has_section {
        let lines: Vec<&str> = content.lines().collect();
        // Re-emit lines 1..=related_end_line, then append, then
        // re-emit the rest. Preserve the trailing newline shape of
        // the original.
        for (i, line) in lines.iter().enumerate() {
            new_content.push_str(line);
            new_content.push('\n');
            if i + 1 == related_end_line {
                for c in &appended {
                    new_content.push_str("- [[");
                    new_content.push_str(c);
                    new_content.push_str("]]\n");
                }
            }
        }
        // Edge case: the file ends inside the Related section (no
        // following content after related_end_line). The loop above
        // still hits `i + 1 == related_end_line` on the last
        // iteration, so appended entries land correctly.
    } else {
        new_content.push_str(content);
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        // Blank line before the new heading if the file has content
        // and doesn't already end with one.
        if !new_content.is_empty() && !new_content.ends_with("\n\n") {
            new_content.push('\n');
        }
        new_content.push_str("## Related\n");
        for c in &appended {
            new_content.push_str("- [[");
            new_content.push_str(c);
            new_content.push_str("]]\n");
        }
    }

    RelatedUpdatePlan {
        new_content,
        created_section: !has_section,
        appended,
    }
}

/// Write the planned new content to `path` via `write_atomic`. A
/// no-op plan (no concepts appended) skips the write entirely.
pub fn apply_related_update(plan: &RelatedUpdatePlan, path: &Path) -> Result<()> {
    if plan.appended.is_empty() {
        return Ok(());
    }
    write_atomic(path, &plan.new_content)
}

/// Extract `[[Target]]` and `[[Target|alias]]` wikilink targets from
/// a single line. Sufficient for the dedup check inside an existing
/// Related section — no need to invoke the full link parser here.
fn extract_wiki_targets(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let start = i + 2;
            let mut end = start;
            while end + 1 < bytes.len() && !(bytes[end] == b']' && bytes[end + 1] == b']') {
                end += 1;
            }
            if end + 1 < bytes.len() {
                let inside = &line[start..end];
                let target = inside.split('|').next().unwrap_or("");
                let target = target.split('#').next().unwrap_or("");
                let target = target.trim();
                if !target.is_empty() {
                    out.push(target.to_string());
                }
                i = end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;

    fn build_vault_and_graph(
        setup: impl FnOnce(&assert_fs::TempDir),
    ) -> (assert_fs::TempDir, Vault, Graph) {
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        setup(&tmp);
        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &crate::vault::Scan::default()).unwrap();
        (tmp, v, g)
    }

    fn title(g: &Graph, id: NoteId) -> String {
        match g.node(id) {
            NodeKind::Note(n) => n.title.clone(),
            NodeKind::Ghost(gh) => gh.raw.clone(),
            _ => String::new(),
        }
    }

    // ── score_related ────────────────────────────────────────────────

    #[test]
    fn same_paragraph_co_occurrence_scores_3() {
        let (_tmp, v, g) = build_vault_and_graph(|tmp| {
            tmp.child("N.md").write_str("# N\n").unwrap();
            tmp.child("C.md").write_str("# C\n").unwrap();
            tmp.child("Note.md")
                .write_str("Mentions [[N]] and [[C]] together.\n")
                .unwrap();
        });
        let n = g.note_by_path(Path::new("N.md")).unwrap();
        let rows = score_related(&g, n, &v).unwrap();
        let c = rows.iter().find(|r| r.title == "C").expect("C scored");
        assert_eq!(c.score, 3);
        assert!(!c.already_in_related);
    }

    #[test]
    fn same_file_cross_paragraph_scores_1() {
        let (_tmp, v, g) = build_vault_and_graph(|tmp| {
            tmp.child("N.md").write_str("# N\n").unwrap();
            tmp.child("C.md").write_str("# C\n").unwrap();
            tmp.child("Note.md")
                .write_str("Mentions [[N]].\n\nDifferent paragraph about [[C]].\n")
                .unwrap();
        });
        let n = g.note_by_path(Path::new("N.md")).unwrap();
        let rows = score_related(&g, n, &v).unwrap();
        let c = rows.iter().find(|r| r.title == "C").expect("C scored");
        assert_eq!(c.score, 1);
    }

    #[test]
    fn paragraph_with_duplicate_links_to_c_scores_three_not_six() {
        let (_tmp, v, g) = build_vault_and_graph(|tmp| {
            tmp.child("N.md").write_str("# N\n").unwrap();
            tmp.child("C.md").write_str("# C\n").unwrap();
            tmp.child("Note.md")
                .write_str("[[N]] and [[C]] and [[C]] again\n")
                .unwrap();
        });
        let n = g.note_by_path(Path::new("N.md")).unwrap();
        let rows = score_related(&g, n, &v).unwrap();
        let c = rows.iter().find(|r| r.title == "C").expect("C scored");
        assert_eq!(c.score, 3);
    }

    #[test]
    fn n_itself_excluded_from_results() {
        let (_tmp, v, g) = build_vault_and_graph(|tmp| {
            tmp.child("N.md").write_str("# N\n").unwrap();
            tmp.child("Note.md")
                .write_str("Mentions [[N]] only.\n")
                .unwrap();
        });
        let n = g.note_by_path(Path::new("N.md")).unwrap();
        let rows = score_related(&g, n, &v).unwrap();
        assert!(rows.iter().all(|r| title(&g, r.note_id) != "N"));
    }

    #[test]
    fn zero_score_concept_omitted() {
        let (_tmp, v, g) = build_vault_and_graph(|tmp| {
            tmp.child("N.md").write_str("# N\n").unwrap();
            tmp.child("Lone.md")
                .write_str("[[N]] standalone.\n\nseparate paragraph.\n")
                .unwrap();
            // Other.md exists in the vault but never co-occurs with N
            // — it should not appear in the results.
            tmp.child("Other.md").write_str("# Other\n").unwrap();
        });
        let n = g.note_by_path(Path::new("N.md")).unwrap();
        let rows = score_related(&g, n, &v).unwrap();
        assert!(rows.iter().all(|r| r.title != "Other"));
    }

    #[test]
    fn alias_appears_with_already_in_related_flag() {
        let (_tmp, v, g) = build_vault_and_graph(|tmp| {
            tmp.child("N.md")
                .write_str("# N\n\n## Related\n- [[Alias]]\n")
                .unwrap();
            tmp.child("Alias.md").write_str("# Alias\n").unwrap();
            tmp.child("Note.md")
                .write_str("[[N]] and [[Alias]] together\n")
                .unwrap();
        });
        let n = g.note_by_path(Path::new("N.md")).unwrap();
        let rows = score_related(&g, n, &v).unwrap();
        let alias = rows
            .iter()
            .find(|r| r.title == "Alias")
            .expect("Alias appears in results");
        assert!(alias.already_in_related);
        assert!(alias.score > 0);
    }

    // ── plan_related_update / apply_related_update ────────────────────

    #[test]
    fn plan_append_to_existing_related_section() {
        let original = "# N\n\n## Related\n- [[Foo]]\n";
        let plan = plan_related_update(original, &["Bar".to_string()]);
        assert_eq!(plan.appended, vec!["Bar".to_string()]);
        assert!(!plan.created_section);
        assert!(plan.new_content.contains("- [[Foo]]\n- [[Bar]]\n"));
    }

    #[test]
    fn plan_create_section_when_absent() {
        let original = "# N\n\nbody\n";
        let plan = plan_related_update(original, &["Bar".to_string()]);
        assert!(plan.created_section);
        assert!(plan.new_content.ends_with("## Related\n- [[Bar]]\n"));
    }

    #[test]
    fn plan_empty_selection_is_noop() {
        let original = "# N\n";
        let plan = plan_related_update(original, &[]);
        assert!(plan.is_noop(original));
        assert!(plan.appended.is_empty());
    }

    #[test]
    fn plan_dedups_against_existing_related_targets() {
        let original = "# N\n\n## Related\n- [[Foo]]\n";
        let plan = plan_related_update(
            original,
            &["Foo".to_string(), "Bar".to_string(), "foo".to_string()],
        );
        // Foo / foo both deduped; only Bar appended.
        assert_eq!(plan.appended, vec!["Bar".to_string()]);
    }

    #[test]
    fn plan_dedups_within_new_concepts() {
        let original = "# N\n";
        let plan = plan_related_update(
            original,
            &["Bar".to_string(), "bar".to_string(), "Baz".to_string()],
        );
        assert_eq!(plan.appended, vec!["Bar".to_string(), "Baz".to_string()]);
    }

    #[test]
    fn apply_writes_via_write_atomic() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let f = tmp.child("note.md");
        f.write_str("# N\n").unwrap();
        let plan = plan_related_update(
            &std::fs::read_to_string(f.path()).unwrap(),
            &["Bar".to_string()],
        );
        apply_related_update(&plan, f.path()).unwrap();
        let after = std::fs::read_to_string(f.path()).unwrap();
        assert!(after.contains("## Related\n- [[Bar]]\n"));
    }

    #[test]
    fn apply_skips_write_when_no_concepts() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let f = tmp.child("note.md");
        f.write_str("# N\n").unwrap();
        let original = std::fs::read_to_string(f.path()).unwrap();
        let plan = plan_related_update(&original, &[]);
        apply_related_update(&plan, f.path()).unwrap();
        let after = std::fs::read_to_string(f.path()).unwrap();
        assert_eq!(after, original);
    }
}

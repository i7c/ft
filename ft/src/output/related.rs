//! Output formatter for related-concept rows (`ft notes related`).
//!
//! Same shape across all four formats (`table` / `json` / `ndjson` /
//! `markdown`) so a single [`RelatedRow`] type drives them all. The flat
//! shape keeps the JSON / NDJSON wire format stable for scripting
//! consumers and mirrors `output::links`.

use comfy_table::presets::UTF8_FULL;
use comfy_table::{ContentArrangement, Table};
use ft_core::graph::{Graph, NodeKind};
use serde::Serialize;

pub use super::links::LinkRowTarget;

/// One row in a `ft notes related` result.
///
/// Carries the scored concept plus its vault-relative identity. The
/// `target` field reuses [`LinkRowTarget`] so the JSON path field
/// matches `ft notes links` exactly (`{kind:"resolved", path}` for
/// notes, `{kind:"unresolved", raw}` for ghosts).
#[derive(Debug, Clone, Serialize)]
pub struct RelatedRow {
    /// Concept's filename stem (note) or raw wikilink target (ghost).
    pub title: String,
    /// Aggregate co-occurrence score (3 × same-paragraph + 1 ×
    /// same-file cross-paragraph).
    pub score: u32,
    /// `true` when the concept is already a `[[wiki link]]` in the
    /// target note's `## Related` section. Always `false` for
    /// ghost-target results (a ghost has no Related section).
    pub already_in_related: bool,
    /// Vault-relative path (resolved note) or raw target (unresolved
    /// ghost).
    pub target: LinkRowTarget,
}

impl RelatedRow {
    /// Build a row from a scored concept, resolving its path/title
    /// from the graph (notes → Resolved{path}; ghosts → Unresolved{raw}).
    pub fn from_score(graph: &Graph, s: &ft_core::related::RelatedScore) -> Self {
        let target = match graph.node(s.note_id) {
            NodeKind::Note(n) => LinkRowTarget::Resolved {
                path: n.path.clone(),
            },
            NodeKind::Ghost(g) => LinkRowTarget::Unresolved { raw: g.raw.clone() },
            // Non-note/non-ghost concepts are filtered out by
            // score_related before reaching here; fall back to a
            // resolved-empty path rather than panic.
            _ => LinkRowTarget::Resolved {
                path: std::path::PathBuf::new(),
            },
        };
        Self {
            title: s.title.clone(),
            score: s.score,
            already_in_related: s.already_in_related,
            target,
        }
    }
}

pub struct TableOpts {
    pub use_color: bool,
}

/// Table form. Columns: `Status`, `Concept`, `Score`, `Path`. The
/// `Status` column marks already-in-related rows with `✓` (resolved
/// concepts that the target note already declares in its `## Related`
/// section); candidates are blank. Sorting mirrors `score_related`
/// (already-in-related first, then descending by score), so the marked
/// rows naturally cluster at the top.
pub fn render_table(rows: &[RelatedRow], opts: TableOpts) -> String {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    let _ = opts.use_color; // reserved for future per-row coloring
    table.set_header(vec!["Status", "Concept", "Score", "Path"]);
    for r in rows {
        let status = if r.already_in_related { "✓" } else { "" };
        let path_label = match &r.target {
            LinkRowTarget::Resolved { path } => path.display().to_string(),
            LinkRowTarget::Unresolved { raw } => format!("? {raw}"),
        };
        table.add_row(vec![
            status.to_string(),
            format!("[[{}]]", r.title),
            r.score.to_string(),
            path_label,
        ]);
    }
    table.to_string()
}

pub fn render_json(rows: &[RelatedRow]) -> anyhow::Result<()> {
    let stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(stdout, rows)?;
    println!();
    Ok(())
}

pub fn render_ndjson(rows: &[RelatedRow]) -> anyhow::Result<()> {
    use std::io::Write as _;
    let mut out = std::io::stdout().lock();
    for r in rows {
        serde_json::to_writer(&mut out, r)?;
        writeln!(out)?;
    }
    Ok(())
}

/// Markdown bullet per row. Already-in-related rows are prefixed with
/// `✓` so the marker survives in plain-text form. Shape:
/// `- ✓ [[Concept]] (3) path.md` / `- [[Concept]] (1) path.md`.
pub fn render_markdown(rows: &[RelatedRow]) -> String {
    let mut out = String::new();
    for r in rows {
        let mark = if r.already_in_related { "✓ " } else { "" };
        let path_label = match &r.target {
            LinkRowTarget::Resolved { path } => path.display().to_string(),
            LinkRowTarget::Unresolved { raw } => format!("? {raw}"),
        };
        out.push_str(&format!(
            "- {mark}[[{}]] ({}) {}\n",
            r.title, r.score, path_label
        ));
    }
    out
}

/// Resolve the NoteIds that scored concepts refer to into display rows.
/// Kept here so call sites stay one-liners; mirrors the journal's
/// `resolve_titles` closure pattern.
pub fn rows_from_scores(
    graph: &Graph,
    scores: &[ft_core::related::RelatedScore],
) -> Vec<RelatedRow> {
    scores
        .iter()
        .map(|s| RelatedRow::from_score(graph, s))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(title: &str, score: u32, already: bool, path: &str) -> RelatedRow {
        RelatedRow {
            title: title.to_string(),
            score,
            already_in_related: already,
            target: LinkRowTarget::Resolved {
                path: std::path::PathBuf::from(path),
            },
        }
    }

    #[test]
    fn table_marks_already_in_related_rows_with_check() {
        let rows = vec![
            row("Alias", 3, true, "Alias.md"),
            row("C", 3, false, "C.md"),
        ];
        let out = render_table(&rows, TableOpts { use_color: false });
        let lines: Vec<&str> = out.lines().collect();
        // Header is line 1; find the Alias and C data rows.
        let alias = lines
            .iter()
            .find(|l| l.contains("Alias"))
            .expect("alias row present");
        let c = lines
            .iter()
            .find(|l| l.contains("[[C]]"))
            .expect("C row present");
        assert!(alias.contains('✓'), "already-in-related row marked:\n{out}");
        assert!(!c.contains('✓'), "candidate row unmarked:\n{out}");
    }

    #[test]
    fn markdown_prefixes_already_in_related_with_check() {
        let rows = vec![
            row("Alias", 3, true, "Alias.md"),
            row("C", 1, false, "C.md"),
        ];
        let out = render_markdown(&rows);
        assert!(out.contains("- ✓ [[Alias]] (3) Alias.md"));
        assert!(out.contains("- [[C]] (1) C.md"));
    }

    #[test]
    fn json_serializes_target_as_tagged_enum() {
        let rows = vec![row("C", 3, false, "C.md")];
        let mut buf = Vec::new();
        serde_json::to_writer_pretty(&mut buf, &rows).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("\"kind\": \"resolved\""), "{s}");
        assert!(s.contains("\"already_in_related\": false"), "{s}");
    }
}

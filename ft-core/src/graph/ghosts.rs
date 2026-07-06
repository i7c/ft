//! Vault-wide ghost ranking: which unresolved `[[targets]]` have
//! earned a note of their own.
//!
//! A ghost's weight is the number of **distinct paragraphs** that
//! mention it — the same dedup rule as `ft review` (three mentions
//! inside one paragraph count once), read straight from
//! [`EdgeKind::ParagraphLink`] edges. Pure graph: no git history
//! involved.

use std::collections::HashSet;

use crate::graph::{EdgeKind, Graph, NodeKind, NoteId};

/// One ranked ghost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhostRank {
    /// Graph node id of the ghost.
    pub id: NoteId,
    /// The verbatim unresolved target string (`GhostData::raw`).
    pub raw: String,
    /// Distinct paragraphs mentioning the ghost.
    pub mentions: usize,
}

/// Rank every ghost in the graph by distinct-paragraph mentions,
/// descending; ties break alphabetically by `raw`.
pub fn rank_ghosts(graph: &Graph) -> Vec<GhostRank> {
    let mut out: Vec<GhostRank> = graph
        .nodes()
        .filter_map(|(id, node)| {
            let NodeKind::Ghost(g) = node else {
                return None;
            };
            let paragraphs: HashSet<NoteId> = graph
                .incoming(id)
                .filter(|(src, edge)| {
                    matches!(edge, EdgeKind::ParagraphLink(_))
                        && matches!(graph.node(*src), NodeKind::Paragraph(_))
                })
                .map(|(src, _)| src)
                .collect();
            Some(GhostRank {
                id,
                raw: g.raw.clone(),
                mentions: paragraphs.len(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.mentions.cmp(&a.mentions).then_with(|| a.raw.cmp(&b.raw)));
    out
}

/// Mention count for one ghost id, on the same counting rule.
/// Returns 0 for non-ghost ids.
pub fn mention_count(graph: &Graph, id: NoteId) -> usize {
    if !matches!(graph.node(id), NodeKind::Ghost(_)) {
        return 0;
    }
    let paragraphs: HashSet<NoteId> = graph
        .incoming(id)
        .filter(|(src, edge)| {
            matches!(edge, EdgeKind::ParagraphLink(_))
                && matches!(graph.node(*src), NodeKind::Paragraph(_))
        })
        .map(|(src, _)| src)
        .collect();
    paragraphs.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::Vault;
    use assert_fs::prelude::*;

    fn graph_of(files: &[(&str, &str)]) -> (assert_fs::TempDir, Graph) {
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        for (name, content) in files {
            tmp.child(name).write_str(content).unwrap();
        }
        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &v.scan()).unwrap();
        (tmp, g)
    }

    #[test]
    fn multi_mention_paragraph_counts_once() {
        let (_tmp, g) = graph_of(&[(
            "a.md",
            "[[foo]] and [[foo]] and [[foo]] in one paragraph.\n\nAnd [[foo]] in another.\n",
        )]);
        let ranks = rank_ghosts(&g);
        assert_eq!(ranks.len(), 1);
        assert_eq!(ranks[0].raw, "foo");
        assert_eq!(ranks[0].mentions, 2, "3+1 mentions in 2 paragraphs");
        assert_eq!(mention_count(&g, ranks[0].id), 2);
    }

    #[test]
    fn sorted_desc_then_alphabetical() {
        let (_tmp, g) = graph_of(&[(
            "a.md",
            "[[busy]] here.\n\n[[busy]] again.\n\n[[beta]] once.\n\n[[alpha]] once.\n",
        )]);
        let ranks = rank_ghosts(&g);
        let shape: Vec<(&str, usize)> =
            ranks.iter().map(|r| (r.raw.as_str(), r.mentions)).collect();
        assert_eq!(shape, vec![("busy", 2), ("alpha", 1), ("beta", 1)]);
    }

    #[test]
    fn no_ghosts_is_empty() {
        let (_tmp, g) = graph_of(&[("a.md", "links to [[b]]\n"), ("b.md", "target\n")]);
        assert!(rank_ghosts(&g).is_empty());
    }

    #[test]
    fn ghosts_preset_walk_is_ranked() {
        // Spec "Preset becomes the ranked view": the builtin `ghosts`
        // preset walk lists ghosts mentions-desc, name-asc.
        use crate::graph::preset::builtin;
        use crate::graph::query::{parse_with, Profile, WalkOptions};

        let (_tmp, g) = graph_of(&[(
            "a.md",
            "[[one]] here.\n\n[[four]] a.\n\n[[four]] b.\n\n[[four]] c.\n\n[[four]] d.\n\n\
             [[two]] a.\n\n[[two]] b.\n",
        )]);
        let dsl = builtin("ghosts").expect("builtin ghosts preset");
        let query = parse_with(
            dsl,
            Profile::Default,
            chrono::NaiveDate::from_ymd_opt(2026, 7, 6).unwrap(),
        )
        .unwrap();
        let walked = query.walk(&g, &WalkOptions::default());
        let order: Vec<String> = walked
            .iter()
            .map(|n| match g.node(n.id) {
                NodeKind::Ghost(gh) => gh.raw.clone(),
                _ => panic!("ghosts preset selected a non-ghost"),
            })
            .collect();
        assert_eq!(order, vec!["four", "two", "one"]);
    }

    #[test]
    fn resolved_links_do_not_rank() {
        // One resolved link and one ghost: only the ghost ranks, and
        // note-level edges to the ghost don't inflate the paragraph
        // count.
        let (_tmp, g) = graph_of(&[
            ("a.md", "see [[real]] and [[phantom]]\n"),
            ("real.md", "exists\n"),
        ]);
        let ranks = rank_ghosts(&g);
        assert_eq!(ranks.len(), 1);
        assert_eq!(ranks[0].raw, "phantom");
        assert_eq!(ranks[0].mentions, 1);
    }
}

//! Graph tab — infinite-tree viewer for the note-link graph.
//!
//! Session 1: TreeState data structure (pure logic, no TUI rendering).
//! Session 2+ will wire this into the TUI tab; dead_code warnings are
//! suppressed until then.

#![allow(dead_code)]

use std::collections::HashMap;

use ft_core::graph::query::GraphQuery;
use ft_core::graph::{Graph, NodeKind, NoteId};

/// One visible row in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub depth: usize,
    pub note_id: NoteId,
    pub display: String,
    pub kind_char: char,
    pub expanded: bool,
    pub expandable: bool,
}

/// The flat-list tree with expansion cache. Manipulated imperatively:
/// expanding inserts children after the parent row; collapsing removes
/// all descendant rows.
#[derive(Debug, Default)]
pub struct TreeState {
    rows: Vec<TreeRow>,
    expansion_cache: HashMap<NoteId, Option<Vec<NoteId>>>,
}

impl TreeState {
    /// Build the tree from root nodes. Clears any prior state and cache.
    /// Returns the indices of the root rows (always `0..roots.len()`).
    pub fn build_from(&mut self, roots: &[NoteId], graph: &Graph) {
        self.rows.clear();
        self.expansion_cache.clear();
        for id in roots {
            self.rows.push(Self::make_row(*id, 0, graph));
        }
    }

    /// Toggle expansion at the given row index. If the row is already
    /// expanded, collapses it. Otherwise, attempts to expand. Returns
    /// `true` if the tree was modified.
    pub fn expand_at(&mut self, index: usize, graph: &Graph, query: &GraphQuery) -> bool {
        if index >= self.rows.len() {
            return false;
        }

        if self.rows[index].expanded {
            self.collapse_at(index);
            return true;
        }

        if !self.rows[index].expandable {
            return false;
        }

        let id = self.rows[index].note_id;

        // Compute or fetch cached children
        let children = self
            .expansion_cache
            .entry(id)
            .or_insert_with(|| query.expand(graph, id));

        let child_ids: &[NoteId] = match children {
            Some(v) => v.as_slice(),
            None => {
                self.rows[index].expandable = false;
                return false;
            }
        };

        let child_depth = self.rows[index].depth + 1;
        let insert_pos = index + 1;
        for child_id in child_ids.iter().rev() {
            self.rows
                .insert(insert_pos, Self::make_row(*child_id, child_depth, graph));
        }

        self.rows[index].expanded = true;
        self.rows[index].expandable = !child_ids.is_empty();
        true
    }

    /// Collapse the row at `index` by removing all descendant rows
    /// (rows at greater depth between this row and the next row at
    /// the same or lower depth).
    pub fn collapse_at(&mut self, index: usize) {
        if index >= self.rows.len() || !self.rows[index].expanded {
            return;
        }

        let bound_depth = self.rows[index].depth;
        let mut end = index + 1;
        while end < self.rows.len() && self.rows[end].depth > bound_depth {
            end += 1;
        }

        self.rows.drain(index + 1..end);
        self.rows[index].expanded = false;
    }

    /// Move selection up, wrapping to the last row. Returns new index.
    pub fn move_selection_up(&self, current: usize) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        if current == 0 {
            self.rows.len() - 1
        } else {
            current - 1
        }
    }

    /// Move selection down, wrapping to the first row. Returns new index.
    pub fn move_selection_down(&self, current: usize) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        if current + 1 >= self.rows.len() {
            0
        } else {
            current + 1
        }
    }

    pub fn rows(&self) -> &[TreeRow] {
        &self.rows
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    // ── helpers ──────────────────────────────────────────────────────

    fn make_row(id: NoteId, depth: usize, graph: &Graph) -> TreeRow {
        let (display, kind_char) = match graph.node(id) {
            NodeKind::Note(n) => (
                n.path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| n.path.to_string_lossy().into_owned()),
                'N',
            ),
            NodeKind::Directory(d) => {
                if d.path.as_os_str().is_empty() {
                    ("/".to_string(), 'D')
                } else {
                    (format!("{}/", d.name), 'D')
                }
            }
            NodeKind::Ghost(g) => (g.raw.clone(), 'G'),
        };
        TreeRow {
            depth,
            note_id: id,
            display,
            kind_char,
            expanded: false,
            expandable: true, // optimistic; set during expand_at
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tree_tests {
    use std::path::PathBuf;

    use ft_core::graph::query::parse as parse_query;
    use ft_core::graph::Graph;
    use ft_core::vault::Vault;

    use super::*;

    fn dirs_graph() -> Graph {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dirs");
        let v = Vault::discover(Some(path)).expect("dirs fixture vault must exist");
        Graph::build(&v).unwrap()
    }

    fn dirs_query() -> GraphQuery {
        parse_query(
            "node n with n.kind = Directory without (edge e(_, n) with e.kind = directory-contains); expand over e(n, m) with n.kind = Directory with e.kind = directory-contains with m.kind in {Note, Directory};",
        )
        .unwrap()
    }

    #[test]
    fn build_from_roots_creates_flat_rows() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g);
        // Only root directory is a top-level directory (no parent)
        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0].depth, 0);
        assert_eq!(state.rows[0].kind_char, 'D');
    }

    #[test]
    fn expand_inserts_children_at_correct_position() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g);
        assert_eq!(state.rows.len(), 1); // root dir

        // Expand root directory
        let changed = state.expand_at(0, &g, &q);
        assert!(changed);
        // Root has 3 children: root.md, Areas, Projects
        assert_eq!(state.rows.len(), 4);
        assert!(state.rows[0].expanded);
        assert_eq!(state.rows[0].depth, 0);
        assert_eq!(state.rows[1].depth, 1);
        assert_eq!(state.rows[2].depth, 1);
        assert_eq!(state.rows[3].depth, 1);
    }

    #[test]
    fn collapse_removes_descendants() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g);
        state.expand_at(0, &g, &q);
        assert_eq!(state.rows.len(), 4);

        state.collapse_at(0);
        assert_eq!(state.rows.len(), 1);
        assert!(!state.rows[0].expanded);
    }

    #[test]
    fn expand_toggle_collapses_when_already_expanded() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g);

        // First expand
        state.expand_at(0, &g, &q);
        assert_eq!(state.rows.len(), 4);
        assert!(state.rows[0].expanded);

        // Second expand toggles to collapsed
        let changed = state.expand_at(0, &g, &q);
        assert!(changed);
        assert_eq!(state.rows.len(), 1);
        assert!(!state.rows[0].expanded);
    }

    #[test]
    fn expand_then_expand_child() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g);

        // Expand root → children: root.md(N), Areas/(D), Projects/(D)
        state.expand_at(0, &g, &q);
        assert_eq!(state.rows.len(), 4);

        // Find Areas — it's the directory child (kind_char 'D', not root.md which is 'N')
        let areas_idx = state
            .rows
            .iter()
            .position(|r| r.kind_char == 'D' && r.display == "Areas/")
            .unwrap();

        // Expand Areas → children: finance.md(N), operations/(D)
        state.expand_at(areas_idx, &g, &q);
        // Should have: root (0) + root.md (1) + Areas (2) + Areas/finance.md (3) + Areas/operations/ (4) + Projects (5)
        // Actually projects was at index 3 before Areas expanded, now it shifted
        assert_eq!(state.rows.len(), 6);

        // Verify Areas/operations/ is present
        let ops = state
            .rows
            .iter()
            .find(|r| r.display == "operations/")
            .unwrap();
        assert_eq!(ops.depth, 2);
    }

    #[test]
    fn expand_unexpandable_node_returns_false() {
        let g = dirs_graph();
        // Query with NO expansion rule
        let q = parse_query("node n with n.kind = Note;").unwrap();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g);

        // All rows are initially expandable=true (optimistic)
        // First expand on a Note node with no expansion rule in query
        let changed = state.expand_at(0, &g, &q);
        assert!(!changed); // None returned → expandable set to false
        assert!(!state.rows[0].expandable);
    }

    #[test]
    fn move_selection_wraps_at_bounds() {
        let g = dirs_graph();
        let roots: Vec<_> = g
            .nodes()
            .filter(|(_, k)| matches!(k, NodeKind::Note(_)))
            .map(|(id, _)| id)
            .take(3)
            .collect();

        let mut state = TreeState::default();
        state.build_from(&roots, &g);
        assert_eq!(state.rows.len(), 3);

        // Up from 0 wraps to last
        assert_eq!(state.move_selection_up(0), 2);
        // Down from last wraps to 0
        assert_eq!(state.move_selection_down(2), 0);
        // Normal movement
        assert_eq!(state.move_selection_down(0), 1);
        assert_eq!(state.move_selection_up(1), 0);
    }

    #[test]
    fn empty_tree_selection_is_zero() {
        let state = TreeState::default();
        assert_eq!(state.move_selection_up(0), 0);
        assert_eq!(state.move_selection_down(0), 0);
    }

    #[test]
    fn cache_is_used_on_repeat_expand() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g);

        // Expand, collapse, expand again — should use cache
        state.expand_at(0, &g, &q);
        let first_len = state.rows.len();
        state.collapse_at(0);
        state.expand_at(0, &g, &q);
        // Same result from cache
        assert_eq!(state.rows.len(), first_len);
        // Cache has entry
        assert!(state.expansion_cache.contains_key(&state.rows[0].note_id));
    }

    #[test]
    fn expand_empty_children_marks_expandable_false() {
        // Empty vault: only root directory, no children
        let tmp = assert_fs::TempDir::new().unwrap();
        use assert_fs::prelude::*;
        tmp.child(".obsidian").create_dir_all().unwrap();

        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v).unwrap();

        let q = parse_query(
            "expand over e(n, m) with n.kind = Directory with e.kind = directory-contains with m.kind = Note;",
        ).unwrap();

        let root_id = g.node_by_path(std::path::Path::new("")).unwrap();

        let mut state = TreeState::default();
        state.build_from(&[root_id], &g);

        // Expanding root with no children
        state.expand_at(0, &g, &q);
        // Row is expanded but has no children
        assert!(state.rows[0].expanded);
        assert!(!state.rows[0].expandable);
        assert_eq!(state.rows.len(), 1);
    }
}

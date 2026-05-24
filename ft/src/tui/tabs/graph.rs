//! Graph tab — infinite-tree viewer for the note-link graph.
//!
//! Session 1: TreeState data structure (pure logic, no TUI rendering).
//! Session 2: GraphTab skeleton + input bar + query parsing.

#![allow(dead_code)]

use std::collections::HashMap;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use ft_core::graph::query::{parse as parse_query, GraphQuery};
use ft_core::graph::{Graph, NodeKind, NoteId};

use crate::tui::{
    event::Event,
    tab::{EventOutcome, Tab, TabCtx},
};

// ── GraphTab ──────────────────────────────────────────────────────────

pub struct GraphTab {
    graph: Option<Graph>,
    query: Option<GraphQuery>,
    query_text: String,
    parse_error: Option<String>,
    input_cursor: usize,
    input_mode: bool,
    tree: TreeState,
    selected: usize,
}

impl GraphTab {
    pub fn new() -> Self {
        Self {
            graph: None,
            query: None,
            query_text: String::new(),
            parse_error: None,
            input_cursor: 0,
            input_mode: false,
            tree: TreeState::default(),
            selected: 0,
        }
    }

    fn apply_query(&mut self) {
        self.parse_error = None;
        if self.query_text.trim().is_empty() {
            self.query = None;
            self.tree = TreeState::default();
            self.selected = 0;
            return;
        }

        match parse_query(&self.query_text) {
            Ok(q) => {
                self.query = Some(q);
                if let Some(ref g) = self.graph {
                    let roots = self.query.as_ref().unwrap().select(g);
                    self.tree.build_from(&roots, g);
                    self.selected = 0;
                }
                self.parse_error = None;
            }
            Err(e) => {
                self.parse_error = Some(e.to_string());
            }
        }
    }

    fn handle_input_event(&mut self, ev: &crossterm::event::KeyEvent) -> Result<EventOutcome> {
        match (ev.code, ev.modifiers) {
            (KeyCode::Enter, _) => {
                self.apply_query();
                self.input_mode = false;
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Esc, _) => {
                self.input_mode = false;
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                self.query_text.insert(self.input_cursor, c);
                self.input_cursor += c.len_utf8();
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Backspace, _) => {
                if self.input_cursor > 0 {
                    let prev = self
                        .query_text
                        .char_indices()
                        .rev()
                        .find(|(i, _)| *i < self.input_cursor)
                        .map(|(i, c)| (i, c.len_utf8()));
                    if let Some((_, len)) = prev {
                        let start = self.input_cursor - len;
                        self.query_text.replace_range(start..self.input_cursor, "");
                        self.input_cursor = start;
                    }
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Delete, _) => {
                if self.input_cursor < self.query_text.len() {
                    let ch = self.query_text[self.input_cursor..].chars().next().unwrap();
                    let end = self.input_cursor + ch.len_utf8();
                    self.query_text.replace_range(self.input_cursor..end, "");
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Left, _) => {
                if self.input_cursor > 0 {
                    let prev = self
                        .query_text
                        .char_indices()
                        .rev()
                        .find(|(i, _)| *i < self.input_cursor)
                        .map(|(i, _)| i);
                    if let Some(i) = prev {
                        self.input_cursor = i;
                    }
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Right, _) => {
                if self.input_cursor < self.query_text.len() {
                    let next = self.query_text[self.input_cursor..]
                        .chars()
                        .next()
                        .map(|c| self.input_cursor + c.len_utf8());
                    if let Some(i) = next {
                        self.input_cursor = i;
                    }
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Home, _) => {
                self.input_cursor = 0;
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::End, _) => {
                self.input_cursor = self.query_text.len();
                Ok(EventOutcome::Consumed)
            }
            _ => Ok(EventOutcome::NotHandled),
        }
    }
}

impl Tab for GraphTab {
    fn title(&self) -> &str {
        "Graph"
    }

    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()> {
        if self.graph.is_none() {
            self.graph = Some(Graph::build(ctx.vault)?);
            // Re-apply current query if one is set
            if self.query.is_some() {
                let roots = self
                    .query
                    .as_ref()
                    .unwrap()
                    .select(self.graph.as_ref().unwrap());
                self.tree.build_from(&roots, self.graph.as_ref().unwrap());
                self.selected = 0;
            }
        }
        Ok(())
    }

    fn handle_event(&mut self, ev: Event, _ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };

        // Tab switching keys pass through
        if matches!(k.code, KeyCode::Tab | KeyCode::BackTab)
            || (matches!(k.code, KeyCode::Char(c) if c.is_ascii_digit()))
        {
            return Ok(EventOutcome::NotHandled);
        }

        if self.input_mode {
            self.handle_input_event(&k)
        } else {
            match (k.code, k.modifiers) {
                (KeyCode::Char('/'), KeyModifiers::NONE) => {
                    self.input_mode = true;
                    Ok(EventOutcome::Consumed)
                }
                (KeyCode::Esc, _) => Ok(EventOutcome::NotHandled),
                _ => Ok(EventOutcome::NotHandled),
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        // Placeholder: show input bar at bottom, empty tree area above
        let [tree_area, input_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

        // Input bar
        let prompt = format!("> {}", self.query_text);
        let styled = Span::styled(prompt, Style::default().fg(Color::White));
        let widget = Paragraph::new(Line::from(styled));
        frame.render_widget(widget, input_area);

        // Placeholder tree area
        let status = if let Some(ref err) = self.parse_error {
            Span::styled(err.as_str(), Style::default().fg(Color::Red))
        } else if self.query.is_some() {
            Span::styled(
                format!("query ok — {} root(s)", self.tree.len()),
                Style::default().fg(Color::Green),
            )
        } else if self.graph.is_some() {
            Span::styled("type a query", Style::default().fg(Color::DarkGray))
        } else {
            Span::styled("building graph...", Style::default().fg(Color::Yellow))
        };
        frame.render_widget(Paragraph::new(Line::from(status)), tree_area);
    }

    fn refresh(&mut self, ctx: &mut TabCtx) -> Result<()> {
        self.graph = Some(Graph::build(ctx.vault)?);
        if let Some(ref q) = self.query {
            let roots = q.select(self.graph.as_ref().unwrap());
            self.tree.build_from(&roots, self.graph.as_ref().unwrap());
            self.selected = 0;
        }
        Ok(())
    }
}

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

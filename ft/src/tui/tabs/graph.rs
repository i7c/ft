//! Graph tab — infinite-tree viewer for the note-link graph.

#![allow(dead_code)]

use std::collections::HashMap;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
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
    scroll_offset: usize,
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
            scroll_offset: 0,
        }
    }

    fn apply_query(&mut self) {
        self.parse_error = None;
        if self.query_text.trim().is_empty() {
            self.query = None;
            self.tree = TreeState::default();
            self.selected = 0;
            self.scroll_offset = 0;
            return;
        }

        match parse_query(&self.query_text) {
            Ok(q) => {
                self.query = Some(q);
                if let Some(ref g) = self.graph {
                    let roots = self.query.as_ref().unwrap().select(g);
                    self.tree.build_from(&roots, g);
                    self.selected = 0;
                    self.scroll_offset = 0;
                }
                self.parse_error = None;
            }
            Err(e) => {
                self.parse_error = Some(e.to_string());
            }
        }
    }

    fn scroll_to_selection(&mut self, visible_rows: usize) {
        if visible_rows == 0 || self.tree.is_empty() {
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.selected.saturating_sub(visible_rows - 1);
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

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
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
            return self.handle_input_event(&k);
        }

        if self.graph.is_none() || self.tree.is_empty() {
            if let (KeyCode::Char('/'), KeyModifiers::NONE) = (k.code, k.modifiers) {
                self.input_mode = true;
                return Ok(EventOutcome::Consumed);
            }
            return Ok(EventOutcome::NotHandled);
        }

        let vis = 20; // approximation; scroll correction in render handles exact
        match (k.code, k.modifiers) {
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                self.input_mode = true;
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => {
                self.selected = self.tree.move_selection_down(self.selected);
                self.scroll_to_selection(vis);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => {
                self.selected = self.tree.move_selection_up(self.selected);
                self.scroll_to_selection(vis);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Enter, _) | (KeyCode::Char('l'), _) => {
                if let Some(ref g) = self.graph {
                    if let Some(ref q) = self.query {
                        self.tree.expand_at(self.selected, g, q);
                        self.scroll_to_selection(vis);
                    }
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('h'), _) => {
                let expanded = self
                    .tree
                    .rows()
                    .get(self.selected)
                    .is_some_and(|r| r.expanded);
                if expanded {
                    self.tree.collapse_at(self.selected);
                    self.scroll_to_selection(vis);
                } else {
                    let depth = self.tree.rows().get(self.selected).map_or(0, |r| r.depth);
                    if depth > 0 {
                        let target = depth.saturating_sub(1);
                        let mut pos = self.selected;
                        while pos > 0 {
                            pos -= 1;
                            if self.tree.rows()[pos].depth == target {
                                self.selected = pos;
                                self.scroll_to_selection(vis);
                                break;
                            }
                        }
                    }
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('g'), _) => {
                self.selected = 0;
                self.scroll_offset = 0;
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('G'), _) => {
                self.selected = self.tree.len().saturating_sub(1);
                self.scroll_to_selection(vis);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                let rows = vis.max(1);
                self.selected = (self.selected + rows / 2).min(self.tree.len().saturating_sub(1));
                self.scroll_offset =
                    (self.scroll_offset + rows / 2).min(self.tree.len().saturating_sub(1));
                self.scroll_to_selection(vis);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                let rows = vis.max(1);
                self.selected = self.selected.saturating_sub(rows / 2);
                self.scroll_offset = self.scroll_offset.saturating_sub(rows / 2);
                self.scroll_to_selection(vis);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('r'), _) => {
                self.graph = Some(Graph::build(ctx.vault)?);
                if let Some(ref q) = self.query {
                    let roots = q.select(self.graph.as_ref().unwrap());
                    self.tree.build_from(&roots, self.graph.as_ref().unwrap());
                    self.selected = self.selected.min(self.tree.len().saturating_sub(1));
                    self.scroll_offset = 0;
                }
                Ok(EventOutcome::Consumed)
            }
            _ => Ok(EventOutcome::NotHandled),
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        let [tree_area, input_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

        let visible = tree_area.height.saturating_sub(1).max(1) as usize;

        let items: Vec<ListItem> = self
            .tree
            .rows()
            .iter()
            .enumerate()
            .skip(self.scroll_offset)
            .take(visible)
            .map(|(i, row)| {
                let indent = "  ".repeat(row.depth);
                let indicator = if row.expanded {
                    '▼'
                } else if row.expandable {
                    '▶'
                } else {
                    ' '
                };
                let line = format!(
                    "{indent}{indicator} {kind} {display}",
                    kind = row.kind_char,
                    display = row.display,
                );
                let style = if i == self.selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(Span::styled(line, style)))
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, tree_area);

        // Input bar
        let input_text = format!("> {}", self.query_text);
        let input_style = if self.input_mode {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(input_text, input_style))),
            input_area,
        );

        // Error line overlays bottom of tree area
        if let Some(ref err) = self.parse_error {
            if tree_area.height > 0 {
                let err_rect = Rect {
                    y: tree_area.y + tree_area.height.saturating_sub(1),
                    height: 1,
                    ..tree_area
                };
                let err_span = Span::styled(err.as_str(), Style::default().fg(Color::Red));
                frame.render_widget(Paragraph::new(Line::from(err_span)), err_rect);
            }
        }
    }

    fn refresh(&mut self, ctx: &mut TabCtx) -> Result<()> {
        self.graph = Some(Graph::build(ctx.vault)?);
        if let Some(ref q) = self.query {
            let roots = q.select(self.graph.as_ref().unwrap());
            self.tree.build_from(&roots, self.graph.as_ref().unwrap());
            self.selected = self.selected.min(self.tree.len().saturating_sub(1));
            self.scroll_offset = 0;
        }
        Ok(())
    }
}

// ── TreeState ─────────────────────────────────────────────────────────

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
    pub fn build_from(&mut self, roots: &[NoteId], graph: &Graph) {
        self.rows.clear();
        self.expansion_cache.clear();
        for id in roots {
            self.rows.push(Self::make_row(*id, 0, graph));
        }
    }

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
            expandable: true,
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
            "node where kind = Directory without incoming(kind = directory-contains); expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};",
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
        assert_eq!(state.rows.len(), 1);

        let changed = state.expand_at(0, &g, &q);
        assert!(changed);
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

        state.expand_at(0, &g, &q);
        assert_eq!(state.rows.len(), 4);
        assert!(state.rows[0].expanded);

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

        state.expand_at(0, &g, &q);
        assert_eq!(state.rows.len(), 4);

        let areas_idx = state
            .rows
            .iter()
            .position(|r| r.kind_char == 'D' && r.display == "Areas/")
            .unwrap();

        state.expand_at(areas_idx, &g, &q);
        assert_eq!(state.rows.len(), 6);

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
        let q = parse_query("node where kind = Note;").unwrap();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g);

        let changed = state.expand_at(0, &g, &q);
        assert!(!changed);
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

        assert_eq!(state.move_selection_up(0), 2);
        assert_eq!(state.move_selection_down(2), 0);
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

        state.expand_at(0, &g, &q);
        let first_len = state.rows.len();
        state.collapse_at(0);
        state.expand_at(0, &g, &q);
        assert_eq!(state.rows.len(), first_len);
        assert!(state.expansion_cache.contains_key(&state.rows[0].note_id));
    }

    #[test]
    fn expand_empty_children_marks_expandable_false() {
        let tmp = assert_fs::TempDir::new().unwrap();
        use assert_fs::prelude::*;
        tmp.child(".obsidian").create_dir_all().unwrap();

        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v).unwrap();

        let q = parse_query(
            "node where indegree = 0; expand where from.kind = Directory and edge.kind = directory-contains and to.kind = Note;",
        ).unwrap();

        let root_id = g.node_by_path(std::path::Path::new("")).unwrap();

        let mut state = TreeState::default();
        state.build_from(&[root_id], &g);

        state.expand_at(0, &g, &q);
        assert!(state.rows[0].expanded);
        assert!(!state.rows[0].expandable);
        assert_eq!(state.rows.len(), 1);
    }
}

//! Per-view state: `ExpandedView` (query + tree + selection) and the
//! `TreeState`/`TreeRow` flat-tree model, plus row rendering helpers.

use super::*;

/// Per-view state. A graph tab owns a `Vec<ExpandedView>` and renders the
/// active one. The view holds both *spec* fields (`query_buf`,
/// `expanded_paths`, `selected_path`) and *derived* fields (`tree`,
/// `selected`, `scroll_offset`); spec fields survive a graph rebuild and
/// drive the rebuild of derived fields via [`Self::restore_expansion`].
#[derive(Debug, Default)]
pub struct ExpandedView {
    /// Editable query text + cursor. Single source of truth for the
    /// query bar; the `QueryBar` modal forwards every key event into
    /// this buffer.
    pub(crate) query_buf: EditBuffer,
    pub(crate) parse_error: Option<String>,
    pub(crate) query: Option<GraphQuery>,
    /// Root-anchored paths the user has expanded. Each path is the
    /// sequence of [`NodeKey`]s from a root (inclusive) down to the
    /// expanded node (inclusive). Closed under prefixes by
    /// construction — expanding a child always implies its parents are
    /// also expanded. `NodeKey` (path-based) is used instead of
    /// `NoteId` so the set survives `Graph::build`.
    pub(crate) expanded_paths: HashSet<Vec<NodeKey>>,
    /// Path of the currently-selected row (root-to-leaf, inclusive).
    /// Used to restore selection across graph rebuilds; on a missing
    /// leaf we shed the tail and re-try until we hit an ancestor that
    /// still exists.
    pub(crate) selected_path: Option<Vec<NodeKey>>,
    /// Space-toggled multi-selection. When non-empty, `r` triggers Flow
    /// A (move to directory) instead of Flow B (rename in place).
    /// Stored as `NodeKey` so the marker set survives a graph rebuild
    /// (e.g. a sibling file is deleted while the marks stand).
    pub(crate) multi_selected: HashSet<NodeKey>,
    pub(crate) tree: TreeState,
    pub(crate) selected: usize,
    pub(crate) scroll_offset: usize,
}

impl ExpandedView {
    /// Replace the query text with `s` and place the cursor at the end.
    /// Used by seeding paths (preset apply, default query, `z`
    /// rewrites) — never on a user keystroke.
    pub(crate) fn set_query_text(&mut self, s: impl AsRef<str>) {
        self.query_buf = EditBuffer::from(s.as_ref());
    }

    /// Parse the editable query text, swap in the parsed query, and
    /// rebuild the tree against the current graph. Clears expansion
    /// state — a new query starts fresh.
    pub(crate) fn apply_query(&mut self, graph: Option<&Graph>, today: chrono::NaiveDate) {
        self.parse_error = None;
        if self.query_buf.text.trim().is_empty() {
            self.query = None;
            self.expanded_paths.clear();
            self.selected_path = None;
            self.tree = TreeState::default();
            self.selected = 0;
            self.scroll_offset = 0;
            return;
        }
        match parse_query(&self.query_buf.text) {
            Ok(q) => {
                self.query = Some(q);
                self.expanded_paths.clear();
                self.selected_path = None;
                self.selected = 0;
                self.scroll_offset = 0;
                if let Some(g) = graph {
                    let q = self.query.as_ref().unwrap();
                    let roots = q.select(g);
                    self.tree.build_from(&roots, g, q, today);
                    self.refresh_selected_path(g);
                }
            }
            Err(e) => self.parse_error = Some(e.to_string()),
        }
    }

    /// Re-derive the flat tree from the saved expansion paths against
    /// the given graph. Paths whose nodes no longer exist are dropped;
    /// selection falls back to the nearest restored ancestor (then
    /// row 0). `scroll_offset` is preserved — the next render's
    /// `scroll_to_selection(visible)` only moves the view if the new
    /// `selected` actually ended up off-screen.
    pub(crate) fn restore_expansion(&mut self, graph: &Graph, today: chrono::NaiveDate) {
        if self.query.is_none() {
            // No parsed query (empty text, or a parse error): nothing
            // to materialize.
            self.tree = TreeState::default();
            self.selected = 0;
            self.scroll_offset = 0;
            return;
        }

        // Clone the GraphQuery once so we can mutably borrow `self.tree`
        // alongside; query is a cheap-ish AST tree.
        let query = self.query.clone().unwrap();
        let roots = query.select(graph);
        self.tree.build_from(&roots, graph, &query, today);

        // Replay expansions shortest-path-first so parents are expanded
        // before their children.
        let mut sorted: Vec<Vec<NodeKey>> = std::mem::take(&mut self.expanded_paths)
            .into_iter()
            .collect();
        sorted.sort_by_key(|p| p.len());
        let mut restored: HashSet<Vec<NodeKey>> = HashSet::new();
        for path in sorted {
            if let Some(idx) = self.find_row_for_path(&path, graph) {
                let already = self.tree.rows()[idx].expanded;
                if already || self.tree.expand_at(idx, graph, &query, today) {
                    restored.insert(path);
                }
            }
            // else: path disappeared — drop it.
        }
        self.expanded_paths = restored;

        // Restore selection: walk the saved selected_path, shedding the
        // suffix until we find a matching row; fall back to row 0.
        self.selected = 0;
        if let Some(path) = self.selected_path.clone() {
            let mut len = path.len();
            while len > 0 {
                if let Some(idx) = self.find_row_for_path(&path[..len], graph) {
                    self.selected = idx;
                    break;
                }
                len -= 1;
            }
        }
        // Preserve `scroll_offset` deliberately — only the next render
        // (which knows the real visible budget) is allowed to scroll,
        // and only when `selected` is off-screen. This keeps the
        // viewport pinned across editor close / rename / delete.
        self.refresh_selected_path(graph);
    }

    /// Locate the row corresponding to a root-anchored path, walking
    /// only through currently-visible children of each step. `path` is
    /// expressed in build-stable [`NodeKey`]s; each step is converted
    /// to a current-build `NoteId` once via `graph.id_for_key`. Returns
    /// `None` if any step doesn't resolve or isn't in the visible tree.
    pub(crate) fn find_row_for_path(&self, path: &[NodeKey], graph: &Graph) -> Option<usize> {
        if path.is_empty() {
            return None;
        }
        // Resolve every key once; if any step is missing in the new
        // graph, the path is dead.
        let ids: Vec<NoteId> = path
            .iter()
            .map(|k| graph.id_for_key(k))
            .collect::<Option<Vec<_>>>()?;
        let rows = self.tree.rows();
        let mut idx = rows
            .iter()
            .position(|r| r.depth == 0 && r.note_id == ids[0])?;
        for &next in &ids[1..] {
            let parent_depth = rows[idx].depth;
            let mut found = None;
            for (i, r) in rows.iter().enumerate().skip(idx + 1) {
                if r.depth <= parent_depth {
                    break;
                }
                if r.depth == parent_depth + 1 && r.note_id == next {
                    found = Some(i);
                    break;
                }
            }
            idx = found?;
        }
        Some(idx)
    }

    /// Walk the visible tree backward from `index` to assemble its
    /// root-to-leaf path, expressed in build-stable [`NodeKey`]s.
    /// Returns an empty vec for out-of-bounds.
    pub(crate) fn path_to(&self, index: usize, graph: &Graph) -> Vec<NodeKey> {
        let rows = self.tree.rows();
        if index >= rows.len() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut next_depth = rows[index].depth + 1;
        for i in (0..=index).rev() {
            if rows[i].depth + 1 == next_depth {
                out.push(graph.stable_key(rows[i].note_id));
                next_depth = rows[i].depth;
                if next_depth == 0 {
                    break;
                }
            }
        }
        out.reverse();
        out
    }

    /// Record an expansion. Also adds every ancestor prefix (defensive
    /// — by construction the user's prior expansions should already
    /// have those, but enforcing the invariant locally keeps
    /// `restore_expansion` simple).
    pub(crate) fn add_expansion_path(&mut self, path: Vec<NodeKey>) {
        for i in 1..=path.len() {
            self.expanded_paths.insert(path[..i].to_vec());
        }
    }

    /// Drop a collapse target plus every path that extends it. Mirrors
    /// `TreeState::collapse_at`, which removes all descendant rows.
    pub(crate) fn forget_expansion_subtree(&mut self, path: &[NodeKey]) {
        self.expanded_paths.retain(|p| !starts_with(p, path));
    }

    pub(crate) fn refresh_selected_path(&mut self, graph: &Graph) {
        if self.tree.is_empty() {
            self.selected_path = None;
        } else {
            self.selected_path = Some(self.path_to(self.selected, graph));
        }
    }

    pub(crate) fn scroll_to_selection(&mut self, visible_rows: usize) {
        if visible_rows == 0 || self.tree.is_empty() {
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.selected.saturating_sub(visible_rows - 1);
        }
    }

    /// Width-limited query snippet for the tab strip label.
    pub(crate) fn query_snippet(&self) -> String {
        let s = self.query_buf.text.trim();
        if s.is_empty() {
            return "(empty)".to_string();
        }
        if s.chars().count() <= VIEW_LABEL_QUERY_WIDTH {
            return s.to_string();
        }
        let mut buf: String = s
            .chars()
            .take(VIEW_LABEL_QUERY_WIDTH.saturating_sub(1))
            .collect();
        buf.push('…');
        buf
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
    pub(crate) rows: Vec<TreeRow>,
    pub(crate) expansion_cache: HashMap<NoteId, Option<Vec<NoteId>>>,
}

impl TreeState {
    pub fn build_from(
        &mut self,
        roots: &[NoteId],
        graph: &Graph,
        query: &GraphQuery,
        today: chrono::NaiveDate,
    ) {
        self.rows.clear();
        self.expansion_cache.clear();
        for id in roots {
            self.rows.push(Self::make_row(*id, 0, graph, query, today));
        }
    }

    pub fn expand_at(
        &mut self,
        index: usize,
        graph: &Graph,
        query: &GraphQuery,
        today: chrono::NaiveDate,
    ) -> bool {
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
            self.rows.insert(
                insert_pos,
                Self::make_row(*child_id, child_depth, graph, query, today),
            );
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

    pub(crate) fn make_row(
        id: NoteId,
        depth: usize,
        graph: &Graph,
        query: &GraphQuery,
        today: chrono::NaiveDate,
    ) -> TreeRow {
        let (display, kind_char) = leaf_display(graph, id, today);
        // Compute expandability up-front by asking the policy how many
        // children this node has. None = no expand block at all (still
        // not expandable). Some(empty) = policy says zero children.
        // This avoids the misleading ▶ arrow on leaves that disappears
        // only after the user tries to expand.
        let expandable = matches!(query.expand(graph, id), Some(ref v) if !v.is_empty());
        TreeRow {
            depth,
            note_id: id,
            display,
            kind_char,
            expanded: false,
            expandable,
        }
    }
}

/// Foreground color for a node kind, used to visually differentiate types
/// in the tree view. Palette inspired by the Monokai theme.
pub(super) fn node_kind_color(kind: &NodeKind) -> Color {
    match kind {
        NodeKind::Note(_) => Color::Rgb(166, 210, 50), // warm green
        NodeKind::Directory(_) => Color::Rgb(80, 190, 200), // warm cyan
        NodeKind::Ghost(_) => palette::DIM,            // warm gray
        NodeKind::Task(_) => palette::PRIMARY,         // orange
        NodeKind::Paragraph(_) => Color::Rgb(210, 150, 100), // warm tan/purple
        NodeKind::Heading(_) => Color::Rgb(190, 130, 200), // warm magenta
    }
}

/// Leaf row text + kind char for a node. Single source of truth shared by
/// `TreeState::make_row` (tree rendering) and `collect_search_candidates`
/// (jump-to-node picker), so search labels always match what's visible in
/// the tree.
pub(super) fn leaf_display(graph: &Graph, id: NoteId, today: chrono::NaiveDate) -> (String, char) {
    use crate::tui::tabs::tasks::edit_popup::relative_date;
    match graph.node(id) {
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
        NodeKind::Task(t) => {
            let marker = match t.status.as_str() {
                "Open" => "[ ]",
                "Done" => "[x]",
                "InProgress" => "[/]",
                "Cancelled" => "[-]",
                _ => "[ ]",
            };
            let mut s = format!("{marker} {}", t.description);
            if let Some(due) = t.due.as_deref().and_then(parse_task_date) {
                s.push_str(&format!("  📅 {}", relative_date(due, today)));
            }
            if let Some(sched) = t.scheduled.as_deref().and_then(parse_task_date) {
                s.push_str(&format!("  ⏳ {}", relative_date(sched, today)));
            }
            if let Some(p) = t.priority.as_deref() {
                let mark = match p {
                    "Highest" => "⏫",
                    "High" => "⏫",
                    "Medium" => "🔼",
                    "Low" => "🔽",
                    "Lowest" => "🔽",
                    _ => "",
                };
                if !mark.is_empty() {
                    s.push_str(&format!("  {mark}"));
                }
            }
            (s, 'T')
        }
        NodeKind::Paragraph(p) => {
            let snippet: String = p.text.chars().take(60).collect();
            let trunc = if p.text.chars().count() > 60 {
                format!("{snippet}…")
            } else {
                snippet
            };
            if p.line_start == p.line_end {
                (
                    format!("{}:{}  {trunc}", p.source_file.display(), p.line_start),
                    'P',
                )
            } else {
                (
                    format!(
                        "{}:{}-{}  {trunc}",
                        p.source_file.display(),
                        p.line_start,
                        p.line_end
                    ),
                    'P',
                )
            }
        }
        NodeKind::Heading(h) => {
            let hashes = "#".repeat(h.level as usize);
            (
                format!("{} {} {}", hashes, h.text, h.source_file.display()),
                'H',
            )
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

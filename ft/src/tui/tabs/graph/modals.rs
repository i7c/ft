//! Modal state machines hosted by the Graph tab (rename, related,
//! task edit/create, multi-move) and its picker sources — kept next
//! to the tab because they reach graph-internal types.

use super::*;

pub struct PresetPickerSource {
    pub(crate) items: Vec<(String, String)>,
    matcher: nucleo_matcher::Matcher,
    buf: Vec<char>,
}

impl PresetPickerSource {
    pub(crate) fn new(vault: &ft_core::vault::Vault) -> Self {
        let mut items: Vec<(String, String)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (name, dsl) in &vault.config.config.graph.presets {
            if seen.insert(name.clone()) {
                items.push((name.clone(), dsl.clone()));
            }
        }
        for name in preset::builtin_names() {
            if seen.insert(name.to_string()) {
                items.push((name.to_string(), preset::builtin(name).unwrap().to_string()));
            }
        }
        Self {
            items,
            matcher: nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT),
            buf: Vec::new(),
        }
    }
}

impl crate::tui::widgets::PickerSource for PresetPickerSource {
    type Item = String;

    fn query(&mut self, q: &str, limit: usize) -> Vec<crate::tui::widgets::PickerItem<String>> {
        let pat = nucleo_matcher::pattern::Pattern::parse(
            q,
            nucleo_matcher::pattern::CaseMatching::Smart,
            nucleo_matcher::pattern::Normalization::Smart,
        );
        let mut ranked: Vec<(u32, usize, Vec<u32>)> = Vec::new();
        for (i, (name, _)) in self.items.iter().enumerate() {
            self.buf.clear();
            let haystack = nucleo_matcher::Utf32Str::new(name, &mut self.buf);
            let mut indices = Vec::new();
            if let Some(score) = pat.indices(haystack, &mut self.matcher, &mut indices) {
                ranked.push((score, i, indices));
            }
        }
        ranked.sort_by_key(|b| std::cmp::Reverse(b.0));
        ranked
            .into_iter()
            .take(limit)
            .map(|(_, i, match_indices)| {
                let (name, _) = &self.items[i];
                crate::tui::widgets::PickerItem {
                    label: name.clone(),
                    match_indices,
                    data: name.clone(),
                }
            })
            .collect()
    }

    fn initial_items(&mut self, limit: usize) -> Vec<crate::tui::widgets::PickerItem<String>> {
        self.items
            .iter()
            .take(limit)
            .map(|(name, _)| crate::tui::widgets::PickerItem {
                label: name.clone(),
                match_indices: Vec::new(),
                data: name.clone(),
            })
            .collect()
    }
}

// ── Search-in-tree picker ─────────────────────────────────────────────

/// One reachable node in the active view's policy-induced subgraph,
/// pre-computed at picker-open time. `path` is the shortest BFS path from
/// some root to `id` (inclusive of both endpoints); `leaf` is the same
/// string `TreeState::make_row` puts in `TreeRow.display`; `breadcrumb`
/// is the ancestor leafs joined with `/`.
#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub(crate) path: Vec<NoteId>,
    pub(crate) leaf: String,
    pub(crate) breadcrumb: String,
    pub(crate) kind_char: char,
}

/// Render `path[..len-1]` as a path-like breadcrumb. Directory leafs end
/// with `/` and the vault root's leaf is `/`; naïve `join("/")` produces
/// doubled separators. This walker trims trailing slashes from each leaf
/// and prepends a single `/` when the ancestor chain starts at the root,
/// so `[root, Areas, operations]` renders `/Areas/operations` (not
/// `//Areas//operations/`).
fn format_breadcrumb(graph: &Graph, path: &[NoteId], today: chrono::NaiveDate) -> String {
    if path.len() <= 1 {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::with_capacity(path.len() - 1);
    for &aid in &path[..path.len() - 1] {
        let (s, _) = leaf_display(graph, aid, today);
        parts.push(s.trim_end_matches('/').to_string());
    }
    let rooted = parts.first().map(|s| s.is_empty()).unwrap_or(false);
    if rooted {
        format!("/{}", parts[1..].join("/"))
    } else {
        parts.join("/")
    }
}

/// BFS from `query.select(graph)` following `query.expand(graph, id)` as
/// the successor function. Cycles are handled by a visited set. Each
/// node is emitted at most once, at its shortest distance from a root;
/// ties resolved by BFS visit order (which itself depends on `query`'s
/// root ordering and the sorted child order in `query.expand`).
pub(crate) fn collect_search_candidates(
    graph: &Graph,
    query: &GraphQuery,
    today: chrono::NaiveDate,
) -> Vec<Candidate> {
    use std::collections::VecDeque;

    let roots = query.select(graph);
    let mut visited: HashSet<NoteId> = HashSet::with_capacity(roots.len());
    let mut queue: VecDeque<(NoteId, Vec<NoteId>)> = VecDeque::new();
    for r in &roots {
        if visited.insert(*r) {
            queue.push_back((*r, vec![*r]));
        }
    }

    let mut out: Vec<Candidate> = Vec::new();
    while let Some((id, path)) = queue.pop_front() {
        let (leaf, kind_char) = leaf_display(graph, id, today);
        let breadcrumb = format_breadcrumb(graph, &path, today);
        out.push(Candidate {
            path: path.clone(),
            leaf,
            breadcrumb,
            kind_char,
        });
        if let Some(children) = query.expand(graph, id) {
            for child in children {
                if visited.insert(child) {
                    let mut child_path = path.clone();
                    child_path.push(child);
                    queue.push_back((child, child_path));
                }
            }
        }
    }
    out
}

pub struct GraphSearchPickerSource {
    pub(crate) candidates: Vec<Candidate>,
    pub(crate) matcher: nucleo_matcher::Matcher,
    pub(crate) buf: Vec<char>,
}

impl GraphSearchPickerSource {
    pub(crate) fn new(graph: &Graph, query: &GraphQuery, today: chrono::NaiveDate) -> Self {
        Self {
            candidates: collect_search_candidates(graph, query, today),
            matcher: nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT),
            buf: Vec::new(),
        }
    }

    /// Build the rendered label string for a candidate: leaf, separator,
    /// breadcrumb. Pure so the test for label-format invariants doesn't
    /// have to construct a picker.
    fn format_label(c: &Candidate) -> String {
        if c.breadcrumb.is_empty() {
            c.leaf.clone()
        } else {
            format!("{}  ·  {}", c.leaf, c.breadcrumb)
        }
    }
}

impl crate::tui::widgets::PickerSource for GraphSearchPickerSource {
    type Item = Vec<NoteId>;

    fn query(
        &mut self,
        q: &str,
        limit: usize,
    ) -> Vec<crate::tui::widgets::PickerItem<Vec<NoteId>>> {
        let pat = nucleo_matcher::pattern::Pattern::parse(
            q,
            nucleo_matcher::pattern::CaseMatching::Smart,
            nucleo_matcher::pattern::Normalization::Smart,
        );
        let mut ranked: Vec<(u32, usize, Vec<u32>)> = Vec::new();
        for (i, c) in self.candidates.iter().enumerate() {
            let haystack_str = if c.breadcrumb.is_empty() {
                c.leaf.clone()
            } else {
                format!("{} {}", c.leaf, c.breadcrumb)
            };
            self.buf.clear();
            let haystack = nucleo_matcher::Utf32Str::new(&haystack_str, &mut self.buf);
            let mut indices = Vec::new();
            if let Some(score) = pat.indices(haystack, &mut self.matcher, &mut indices) {
                ranked.push((score, i, indices));
            }
        }
        ranked.sort_by_key(|b| std::cmp::Reverse(b.0));
        ranked
            .into_iter()
            .take(limit)
            .map(|(_, i, raw_indices)| {
                let c = &self.candidates[i];
                let leaf_chars = c.leaf.chars().count() as u32;
                // Highlight indices in the haystack `"{leaf} {breadcrumb}"`
                // line up with `format_label`'s `"{leaf}  ·  {breadcrumb}"`
                // only inside the leaf portion (positions < leaf_chars).
                // Drop matches that land in the breadcrumb to avoid
                // misaligned highlights — the separator widths differ.
                let match_indices: Vec<u32> = raw_indices
                    .into_iter()
                    .filter(|idx| *idx < leaf_chars)
                    .collect();
                crate::tui::widgets::PickerItem {
                    label: GraphSearchPickerSource::format_label(c),
                    match_indices,
                    data: c.path.clone(),
                }
            })
            .collect()
    }

    fn initial_items(&mut self, limit: usize) -> Vec<crate::tui::widgets::PickerItem<Vec<NoteId>>> {
        self.candidates
            .iter()
            .take(limit)
            .map(|c| crate::tui::widgets::PickerItem {
                label: GraphSearchPickerSource::format_label(c),
                match_indices: Vec::new(),
                data: c.path.clone(),
            })
            .collect()
    }
}

// ── CapturePickerModal ────────────────────────────────────────────────

/// Modal wrapper around the quick-capture preset picker
/// (extract-modal-driver §4). Carries an optional `target_note_override`
/// so the modal can pass the selected note (if any) into
/// [`capture::try_execute_preset`] without reaching back into the host
/// tab's selection state.
///
/// On `Enter`:
/// - `Executed` → return `Closed`. The preset committed via the
///   `AppRequest::OpenInEditor` it queued.
/// - `NeedsVars(state)` → return `OpenSibling(ActiveModal::CaptureVar(state))`.
///   First real use of `OpenSibling`; the modal driver swaps the slot
///   in one event-loop iteration so the user goes straight from the
///   picker selection to the first var prompt.
/// - `Err(msg)` → queue an error toast and return `Closed`.
pub struct CapturePickerModal {
    inner: FuzzyPicker<CapturePresetPickerSource>,
    target_note_override: Option<PathBuf>,
}

impl CapturePickerModal {
    pub(crate) fn new(
        source: CapturePresetPickerSource,
        target_note_override: Option<PathBuf>,
    ) -> Self {
        Self {
            inner: FuzzyPicker::new(source),
            target_note_override,
        }
    }
}

impl Modal for CapturePickerModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match self.inner.handle_key(k) {
            PickerOutcome::Selected(name) => {
                match capture::try_execute_preset(ctx, &name, self.target_note_override.clone()) {
                    Ok(capture::CaptureResult::Executed) => ModalOutcome::Closed,
                    Ok(capture::CaptureResult::NeedsVars(vs)) => {
                        ModalOutcome::OpenSibling(Box::new(ActiveModal::CaptureVar(vs)))
                    }
                    Err(e) => {
                        queue_toast(ctx, &e, ToastStyle::Error);
                        ModalOutcome::Closed
                    }
                }
            }
            PickerOutcome::Cancelled => ModalOutcome::Closed,
            PickerOutcome::StillOpen => ModalOutcome::Consumed,
            PickerOutcome::NotHandled => ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, _ctx: &TabCtx) {
        notes_view::render_picker_popup(
            frame,
            area,
            " quick capture · preset ",
            &mut self.inner,
            &[("Enter", "run"), ("Esc", "cancel")],
            None,
        );
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Quick capture",
            &[
                ("Type", "filter"),
                ("↑ / ↓", "navigate"),
                ("Enter", "run preset"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "capture-picker"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::CAPTURE_PICKER_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::CAPTURE_PICKER_KEYMAP
    }
}

// ── PresetPickerModal ─────────────────────────────────────────────────

/// Modal wrapper around the preset picker (extract-modal-driver §4).
/// Two open paths: `Ctrl+N` opens with `for_active_view = false`
/// (caller pre-pushed a blank view); `Ctrl+P` opens with
/// `for_active_view = true` (applies to existing active view).
///
/// On `Enter`: resolve the picked preset name to its DSL string, post
/// `AppRequest::GraphApplyPreset(dsl)` and return `Closed`.
///
/// On `Esc` with `for_active_view = false`: the pre-pushed blank view
/// drops into edit mode via `AppRequest::GraphFocusQueryBar`. With
/// `for_active_view = true`: no action — just close.
pub struct PresetPickerModal {
    inner: FuzzyPicker<PresetPickerSource>,
    for_active_view: bool,
}

impl PresetPickerModal {
    pub(crate) fn new(source: PresetPickerSource, for_active_view: bool) -> Self {
        Self {
            inner: FuzzyPicker::new(source),
            for_active_view,
        }
    }
}

impl Modal for PresetPickerModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match self.inner.handle_key(k) {
            PickerOutcome::Selected(name) => {
                let dsl = ctx
                    .vault
                    .config
                    .config
                    .graph
                    .presets
                    .get(&name)
                    .cloned()
                    .or_else(|| preset::builtin(&name).map(|s| s.to_string()));
                if let Some(dsl) = dsl {
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphApplyPreset(dsl));
                }
                ModalOutcome::Closed
            }
            PickerOutcome::Cancelled => {
                if !self.for_active_view {
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphFocusQueryBar);
                }
                ModalOutcome::Closed
            }
            PickerOutcome::StillOpen => ModalOutcome::Consumed,
            PickerOutcome::NotHandled => ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, _ctx: &TabCtx) {
        let popup_area = centered_rect(60, 60, area);
        frame.render_widget(Clear, popup_area);
        self.inner.render(frame, popup_area);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Preset picker",
            &[
                ("Type", "filter"),
                ("↑ / ↓", "navigate"),
                ("Enter", "apply preset"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "preset-picker"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::PRESET_PICKER_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::PRESET_PICKER_KEYMAP
    }
}

// ── SearchPickerModal ─────────────────────────────────────────────────

/// Modal wrapper around the in-tree fuzzy search picker
/// (extract-modal-driver §4). Owns the [`FuzzyPicker`] for the
/// duration of the modal's lifetime; on `Enter` posts
/// [`AppRequest::GraphJumpToNodes`] back to the Graph tab so the
/// cursor jumps to the chosen node, auto-expanding ancestors.
pub struct SearchPickerModal {
    inner: FuzzyPicker<GraphSearchPickerSource>,
}

impl SearchPickerModal {
    pub(crate) fn new(source: GraphSearchPickerSource) -> Self {
        Self {
            inner: FuzzyPicker::new(source),
        }
    }
}

impl Modal for SearchPickerModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match self.inner.handle_key(k) {
            PickerOutcome::Selected(path) => {
                *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphJumpToNodes(path));
                ModalOutcome::Closed
            }
            PickerOutcome::Cancelled => ModalOutcome::Closed,
            PickerOutcome::StillOpen => ModalOutcome::Consumed,
            PickerOutcome::NotHandled => ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, _ctx: &TabCtx) {
        let popup_area = centered_rect(60, 60, area);
        frame.render_widget(Clear, popup_area);
        let [picker_area, footer_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(popup_area);
        self.inner.render(frame, picker_area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Enter: jump · Esc: cancel",
                Style::default().fg(palette::DIM),
            ))),
            footer_area,
        );
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Graph search",
            &[
                ("Type", "filter"),
                ("↑ / ↓", "navigate"),
                ("Enter", "jump to node"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "search"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::SEARCH_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::SEARCH_KEYMAP
    }
}

// ── GraphTab ──────────────────────────────────────────────────────────

/// Inline rename-in-place state — the modal owns its edit buffer and
/// the node identity. Migrated through `ActiveModal` in
/// extract-modal-driver §4; commits via `AppRequest::GraphCommitRename`
/// so the host can plan/apply/refresh against the in-memory graph.
#[derive(Debug)]
pub struct GraphRenameState {
    note_id: NoteId,
    is_directory: bool,
    buffer: EditBuffer,
    source_rel: PathBuf,
}

impl GraphRenameState {
    pub fn for_note(note_id: NoteId, title: &str, source_rel: PathBuf) -> Self {
        Self {
            note_id,
            is_directory: false,
            buffer: EditBuffer::from(title),
            source_rel,
        }
    }

    pub fn for_directory(note_id: NoteId, name: &str, source_rel: PathBuf) -> Self {
        Self {
            note_id,
            is_directory: true,
            buffer: EditBuffer::from(name),
            source_rel,
        }
    }
}

impl Modal for GraphRenameState {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match k.code {
            KeyCode::Esc => ModalOutcome::Closed,
            KeyCode::Enter => {
                let new_name = self.buffer.text.trim().to_string();
                if new_name.is_empty() {
                    queue_toast(ctx, "name cannot be empty", ToastStyle::Error);
                    return ModalOutcome::Consumed;
                }
                if new_name.contains('/') {
                    queue_toast(
                        ctx,
                        "name cannot contain / — use move (Space-select + r) to change directories",
                        ToastStyle::Error,
                    );
                    return ModalOutcome::Consumed;
                }
                *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphCommitRename {
                    note_id: self.note_id,
                    is_directory: self.is_directory,
                    source_rel: self.source_rel.clone(),
                    new_name,
                });
                // The modal closes here; if the host's commit hits a
                // recoverable error (target exists, plan failure, etc.)
                // it re-opens the modal via OpenModal so the user can
                // edit the name and retry — preserving the
                // pre-migration UX (`reopen_on_error` in `commit_rename`).
                ModalOutcome::Closed
            }
            // All edits + cursor moves + readline chords (Ctrl+A/E,
            // Alt+B/F/D, etc.) go through the buffer's EDIT_KEYMAP.
            // Unrecognised chords are still Consumed so they don't
            // leak through to tab- or global-level bindings.
            _ => {
                let _ = self.buffer.handle_event(k);
                ModalOutcome::Consumed
            }
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, _ctx: &TabCtx) {
        let popup_area = centered_rect(60, 30, area);
        frame.render_widget(Clear, popup_area);
        let [title_area, buf_area, footer_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(popup_area);
        let title = if self.is_directory {
            "Rename directory"
        } else {
            "Rename note"
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                title,
                Style::default()
                    .fg(palette::BLACK)
                    .bg(palette::WHITE)
                    .add_modifier(Modifier::BOLD),
            ))),
            title_area,
        );
        let buf_text = &self.buffer.text;
        let buf_display = if buf_text.is_empty() {
            " ".to_string()
        } else {
            buf_text.clone()
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                buf_display,
                Style::default().fg(palette::SECONDARY),
            ))),
            buf_area,
        );
        let footer = "Enter: commit · Esc: discard";
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                footer,
                Style::default().fg(palette::DIM),
            ))),
            footer_area,
        );
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Rename",
            &[
                ("Type", "edit name"),
                ("Enter", "commit"),
                ("Esc", "discard"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "rename"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::RENAME_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::RENAME_KEYMAP
    }
}

// ── TaskEdit modal (graph-task-edit-modal §2) ──────────────────────────

/// Full-form task edit popup hosted on the Graph tab. Wraps the shared
/// [`EditPopup`] in edit mode plus the task's `(path, line)` identity so
/// the commit can post `AppRequest::GraphTaskEdit`. Render + validation
/// reuse the Tasks-tab helpers lifted into `edit_popup`.
pub struct TaskEditState {
    pub popup: crate::tui::tabs::tasks::edit_popup::EditPopup,
    pub path: PathBuf,
    pub line: usize,
}

impl Modal for TaskEditState {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        // Ctrl+S submits regardless of focused field; Enter submits too.
        let submit = (k.code == KeyCode::Char('s') && k.modifiers.contains(KeyModifiers::CONTROL))
            || k.code == KeyCode::Enter;
        if submit {
            return self.commit(ctx);
        }
        match k.code {
            KeyCode::Esc => ModalOutcome::Closed,
            KeyCode::Tab => {
                self.popup.focus = self.popup.next_field();
                ModalOutcome::Consumed
            }
            KeyCode::BackTab => {
                self.popup.focus = self.popup.prev_field();
                ModalOutcome::Consumed
            }
            KeyCode::Down => {
                self.popup.focus = self.popup.next_field();
                ModalOutcome::Consumed
            }
            KeyCode::Up => {
                self.popup.focus = self.popup.prev_field();
                ModalOutcome::Consumed
            }
            _ => {
                let _ = self.popup.focused_buffer_mut().handle_event(k);
                ModalOutcome::Consumed
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        crate::tui::tabs::tasks::edit_popup::render_edit_popup(frame, area, &mut self.popup);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Edit task",
            &[
                ("Tab / Shift+Tab", "next / prev field"),
                ("↑ / ↓", "navigate fields"),
                ("Ctrl+S / Enter", "save"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "task-edit"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::TASK_EDIT_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::TASK_EDIT_KEYMAP
    }

    fn dispatch_command(&mut self, _cmd: &Command, _ctx: &TabCtx) -> CommandOutcome {
        CommandOutcome::NotHandled
    }
}

impl TaskEditState {
    /// Validate the popup fields and post `GraphTaskEdit`. Mirrors the
    /// Tasks-tab `submit_popup` validation, minus the target/move field
    /// (edits don't move the task).
    fn commit(&mut self, ctx: &TabCtx) -> ModalOutcome {
        use crate::tui::tabs::tasks::edit_popup::{
            merge_tags_into_description, parse_optional_date, parse_priority, parse_tags_field,
            EditField,
        };
        let due = match parse_optional_date(&self.popup.due.text, ctx.today) {
            Ok(v) => v,
            Err(e) => {
                self.popup.error = Some(format!("due: {e}"));
                self.popup.focus = EditField::Due;
                return ModalOutcome::Consumed;
            }
        };
        let scheduled = match parse_optional_date(&self.popup.scheduled.text, ctx.today) {
            Ok(v) => v,
            Err(e) => {
                self.popup.error = Some(format!("scheduled: {e}"));
                self.popup.focus = EditField::Scheduled;
                return ModalOutcome::Consumed;
            }
        };
        let priority = match parse_priority(&self.popup.priority.text) {
            Ok(v) => v,
            Err(e) => {
                self.popup.error = Some(e);
                self.popup.focus = EditField::Priority;
                return ModalOutcome::Consumed;
            }
        };
        let recurrence = self.popup.recurrence.text.trim();
        let recurrence = (!recurrence.is_empty()).then(|| recurrence.to_string());
        let raw_description = self.popup.description.text.trim().to_string();
        let tags = parse_tags_field(&self.popup.tags.text);
        let description = merge_tags_into_description(&raw_description, &tags);
        if description.is_empty() {
            self.popup.error = Some("description is empty".into());
            self.popup.focus = EditField::Description;
            return ModalOutcome::Consumed;
        }
        *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphTaskEdit {
            path: self.path.clone(),
            line: self.line,
            fields: (description, due, scheduled, priority, tags, recurrence),
        });
        ModalOutcome::Closed
    }
}

// ── TaskLeader modal (graph-task-edit-modal §4) ───────────────────────

/// Two-key leader (`a` then `c`/`s`) for creating tasks from the Graph
/// tab. Seeded at open time with the focused row's note path and (if a
/// Task is focused) its `(file, line)` so `c`/`s` can open the create
/// popup with the right target/parent. Mirrors `PeriodicLeader`: any
/// other key closes it.
pub struct TaskLeader {
    /// Note path to seed the new task's `target` field with (the focused
    /// Note, or a focused Task's source note). `None` falls back to the
    /// daily note at commit time.
    pub seed_note: Option<PathBuf>,
    /// The focused Task's `(source_file, source_line)`, used as the parent
    /// when creating a subtask. `None` → `s` toasts "select a task first".
    pub focused_task: Option<(PathBuf, usize)>,
}

impl TaskLeader {
    /// Build the seeded create popup the leader hands off to. Top-level
    /// seeds the `target` field from `seed_note`; subtask leaves it blank
    /// (the parent's file wins on commit).
    fn create_modal(&self, subtask_parent: Option<(PathBuf, usize)>) -> ActiveModal {
        let mut popup = EditPopup::new_blank();
        if subtask_parent.is_none() {
            if let Some(p) = &self.seed_note {
                popup.target = EditBuffer::from(&p.display().to_string());
            }
        }
        ActiveModal::TaskCreate(Box::new(TaskCreateState {
            popup,
            subtask_parent,
        }))
    }
}

impl Modal for TaskLeader {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match k.code {
            KeyCode::Char('c') => ModalOutcome::OpenSibling(Box::new(self.create_modal(None))),
            KeyCode::Char('s') => match self.focused_task.clone() {
                Some(parent) => {
                    ModalOutcome::OpenSibling(Box::new(self.create_modal(Some(parent))))
                }
                None => {
                    queue_toast(ctx, "select a task first", ToastStyle::Error);
                    ModalOutcome::Closed
                }
            },
            _ => ModalOutcome::Closed,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        use ratatui::widgets::Clear;
        let area = crate::tui::tabs::tasks::edit_popup::centered_rect(40, 12, area);
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" task: c=create · s=subtask · Esc=cancel ")
            .style(Style::default().bg(palette::BLACK));
        frame.render_widget(block, area);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Task create",
            &[
                ("c", "new top-level task"),
                ("s", "new subtask"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "task-leader"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::TASK_LEADER_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::TASK_LEADER_KEYMAP
    }
}

// ── TaskCreate modal (graph-task-edit-modal §4) ───────────────────────

/// Full-form task *create* popup hosted on the Graph tab. Wraps the
/// shared [`EditPopup`] in New mode plus an optional subtask parent.
/// Render + validation reuse the Tasks-tab helpers; on `Ctrl+S` it posts
/// `AppRequest::GraphTaskCommitCreate`, which the Graph tab services via
/// `ops::create_task`. `Enter` on the `target` field opens the file
/// picker (matching the Tasks-tab create flow), so only `Ctrl+S` submits.
pub struct TaskCreateState {
    pub popup: EditPopup,
    pub subtask_parent: Option<(PathBuf, usize)>,
}

impl Modal for TaskCreateState {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        use crate::tui::tabs::tasks::edit_popup::{
            handle_target_picker_key, open_target_picker, EditField,
        };
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };

        // While the target picker is open every key routes to it.
        if self.popup.target_picker.is_some() {
            handle_target_picker_key(&mut self.popup, k);
            return ModalOutcome::Consumed;
        }

        // Ctrl+S submits regardless of focused field.
        if k.code == KeyCode::Char('s') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return self.commit(ctx);
        }

        // On the target field, Enter or a printable char opens the file
        // picker (seeded with that keystroke) — never inserts inline.
        if self.popup.focus == EditField::Target {
            match (k.code, k.modifiers) {
                (KeyCode::Enter, _) => {
                    open_target_picker(&mut self.popup, ctx, None);
                    return ModalOutcome::Consumed;
                }
                (KeyCode::Char(c), m)
                    if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
                {
                    open_target_picker(&mut self.popup, ctx, Some(c));
                    return ModalOutcome::Consumed;
                }
                _ => {}
            }
        }

        match k.code {
            KeyCode::Esc => ModalOutcome::Closed,
            KeyCode::Tab | KeyCode::Down => {
                self.popup.focus = self.popup.next_field();
                ModalOutcome::Consumed
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.popup.focus = self.popup.prev_field();
                ModalOutcome::Consumed
            }
            _ => {
                let _ = self.popup.focused_buffer_mut().handle_event(k);
                ModalOutcome::Consumed
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        crate::tui::tabs::tasks::edit_popup::render_edit_popup(frame, area, &mut self.popup);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Create task",
            &[
                ("Tab / Shift+Tab", "next / prev field"),
                ("Enter", "pick target file (on target field)"),
                ("Ctrl+S", "create"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "task-create"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::TASK_CREATE_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::TASK_CREATE_KEYMAP
    }

    fn dispatch_command(&mut self, _cmd: &Command, _ctx: &TabCtx) -> CommandOutcome {
        CommandOutcome::NotHandled
    }
}

impl TaskCreateState {
    /// Validate the popup fields and post `GraphTaskCommitCreate`. Mirrors
    /// the Tasks-tab `submit_popup` validation; disk resolution (target /
    /// duplicate) happens in the Graph-tab servicing hook and surfaces as
    /// a toast on error.
    fn commit(&mut self, ctx: &TabCtx) -> ModalOutcome {
        use crate::tui::tabs::tasks::edit_popup::{
            merge_tags_into_description, parse_optional_date, parse_priority, parse_tags_field,
            EditField,
        };
        let due = match parse_optional_date(&self.popup.due.text, ctx.today) {
            Ok(v) => v,
            Err(e) => {
                self.popup.error = Some(format!("due: {e}"));
                self.popup.focus = EditField::Due;
                return ModalOutcome::Consumed;
            }
        };
        let scheduled = match parse_optional_date(&self.popup.scheduled.text, ctx.today) {
            Ok(v) => v,
            Err(e) => {
                self.popup.error = Some(format!("scheduled: {e}"));
                self.popup.focus = EditField::Scheduled;
                return ModalOutcome::Consumed;
            }
        };
        let priority = match parse_priority(&self.popup.priority.text) {
            Ok(v) => v,
            Err(e) => {
                self.popup.error = Some(e);
                self.popup.focus = EditField::Priority;
                return ModalOutcome::Consumed;
            }
        };
        let recurrence = self.popup.recurrence.text.trim();
        let recurrence = (!recurrence.is_empty()).then(|| recurrence.to_string());
        let raw_description = self.popup.description.text.trim().to_string();
        let tags = parse_tags_field(&self.popup.tags.text);
        let description = merge_tags_into_description(&raw_description, &tags);
        if description.is_empty() {
            self.popup.error = Some("description is empty".into());
            self.popup.focus = EditField::Description;
            return ModalOutcome::Consumed;
        }
        *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphTaskCommitCreate {
            fields: (description, due, scheduled, priority, tags, recurrence),
            target: self.popup.target.text.trim().to_string(),
            subtask_parent: self.subtask_parent.clone(),
        });
        ModalOutcome::Closed
    }
}

/// Related panel modal state. Built on `R` keypress against a Note
/// row (or via `ft notes update-related`). A unified read + write
/// surface: it shows the scored concepts (`ft notes related` prints
/// the same data) and optionally commits checked concepts to the
/// note's `## Related` section. Splits scored concepts into two
/// visual groups: entries already in N's Related section
/// (non-interactive, marked) followed by suggested candidates the
/// user toggles with Space. Note-only — ghost rows toast (a ghost
/// has no file to write; reading-for-ghosts is via `ft notes related`).
#[derive(Debug)]
pub struct RelatedModal {
    /// The note whose Related panel is open.
    pub(crate) target_path: PathBuf,
    pub(crate) target_title: String,
    /// Concepts already in the Related section (alias links inside
    /// the section's body). Rendered as non-interactive "✓" rows.
    pub(crate) already: Vec<ft_core::related::RelatedScore>,
    /// Candidates not yet in the Related section. The cursor moves
    /// through this slice; Space toggles `checked` membership.
    pub(crate) candidates: Vec<ft_core::related::RelatedScore>,
    /// Titles the user has checked for inclusion. Keyed by title
    /// (graph NoteIds aren't durable across rebuilds, but titles
    /// are good enough for this short-lived UI state).
    pub(crate) checked: HashSet<String>,
    pub(crate) cursor: usize,
}

impl RelatedModal {
    /// Move cursor through the candidate list. No-op when there are
    /// no candidates (already-in-related rows are non-interactive).
    fn move_cursor(&mut self, delta: isize) {
        if self.candidates.is_empty() {
            return;
        }
        let len = self.candidates.len() as isize;
        let new = (self.cursor as isize + delta).clamp(0, len - 1);
        self.cursor = new as usize;
    }

    fn toggle_current(&mut self) {
        let Some(c) = self.candidates.get(self.cursor) else {
            return;
        };
        let key = c.title.clone();
        if !self.checked.remove(&key) {
            self.checked.insert(key);
        }
    }

    /// Collected concept titles in the same order they appear in
    /// `candidates` (deterministic).
    fn selected_titles(&self) -> Vec<String> {
        self.candidates
            .iter()
            .filter(|c| self.checked.contains(&c.title))
            .map(|c| c.title.clone())
            .collect()
    }
}

impl Modal for RelatedModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match (k.code, k.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('q'), KeyModifiers::NONE) => ModalOutcome::Closed,
            (KeyCode::Enter, _) => {
                let titles = self.selected_titles();
                if !titles.is_empty() {
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphConfirmRelated {
                        target_path: self.target_path.clone(),
                        selected_titles: titles,
                    });
                }
                ModalOutcome::Closed
            }
            (KeyCode::Char(' '), _) => {
                self.toggle_current();
                ModalOutcome::Consumed
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.move_cursor(-1);
                ModalOutcome::Consumed
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.move_cursor(1);
                ModalOutcome::Consumed
            }
            _ => ModalOutcome::Consumed,
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, _ctx: &TabCtx) {
        render_related_modal(frame, area, self);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Related",
            &[
                ("↑/↓ · j/k", "move cursor"),
                ("Space", "toggle"),
                ("Enter", "commit"),
                ("Esc / q", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "related"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::RELATED_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::RELATED_KEYMAP
    }
}

/// Graph-tab outer wrapper around the shared section-move flow.
///
/// The shared module's [`SectionMoveState`] assumes both source and
/// target are picked via fuzzy pickers. The Graph tab inserts two
/// tree-driven phases — `SourceFromTree` before the headings step and
/// `TargetFromTree` after it — and falls back to the shared picker
/// flow via `t`.
pub enum GraphMoveOuter {
    /// `m` pressed once: awaiting `m` again (confirm selected node as
    /// source), `t` (open fuzzy source picker), or Esc (cancel).
    SourceFromTree,
    /// `t` was pressed during phase 1: fuzzy picker open. `Esc` returns
    /// to `SourceFromTree`; selecting a file transitions to
    /// `Inner(HeadingMultiSelect)`.
    SourcePicker {
        picker: FuzzyPicker<VaultFilePickerSource>,
    },
    /// In a shared `SectionMoveState` step (headings multi-select or
    /// composing). The Graph tab intercepts the headings → target
    /// transition and swaps to `TargetFromTree` rather than letting the
    /// shared `TargetPicking` (fuzzy) own the screen.
    Inner(SectionMoveState),
    /// Phase 2: target via tree. `m` confirms the selected node, `t`
    /// falls back to picker, `/` enters input mode for query
    /// refinement, `Esc` returns to the headings step rebuilt from the
    /// carry.
    TargetFromTree { carry: MoveCarry },
    /// Phase 2 fallback: fuzzy target picker open. `Esc` returns to
    /// `TargetFromTree`; selecting a target transitions to
    /// `Inner(Composing)`.
    TargetPicker {
        picker: FuzzyPicker<VaultFilePickerSource>,
        carry: MoveCarry,
    },
    /// Flow A phase 2: selecting target directory for moved notes.
    /// `Enter`/`m` confirms selected Directory row; `t` opens picker;
    /// Esc cancels.
    MoveTargetFromTree { selected: HashSet<NoteId> },
    /// Flow A fallback: fuzzy directory picker for target.
    MoveTargetPicker {
        picker: FuzzyPicker<VaultFilePickerSource>,
        selected: HashSet<NoteId>,
    },
}

/// Build the fuzzy file/directory picker used by the move flow's
/// `t`-fallback. Pulled out of [`GraphTab`] so [`GraphMoveOuter`]'s
/// `Modal` impl can spawn pickers without borrowing the tab.
pub(super) fn open_move_file_picker(ctx: &TabCtx) -> FuzzyPicker<VaultFilePickerSource> {
    FuzzyPicker::new(VaultFilePickerSource::new(
        Arc::clone(ctx.vault),
        Arc::clone(ctx.recents),
    ))
}

impl Modal for GraphMoveOuter {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        // Take the variant by value so each branch can move owned
        // fields into its handler. `*self` is restored on
        // `Consumed`/`NotHandled` paths; `Closed`/`OpenSibling` paths
        // discard whatever's left in `*self` (the App swaps the slot).
        let prev = std::mem::replace(self, GraphMoveOuter::SourceFromTree);
        match prev {
            GraphMoveOuter::SourceFromTree => self.handle_source_from_tree(k, ctx),
            GraphMoveOuter::SourcePicker { picker } => self.handle_source_picker(picker, k, ctx),
            GraphMoveOuter::Inner(sms) => self.handle_inner(sms, k, ctx),
            GraphMoveOuter::TargetFromTree { carry } => self.handle_target_from_tree(carry, k, ctx),
            GraphMoveOuter::TargetPicker { picker, carry } => {
                self.handle_target_picker(picker, carry, k, ctx)
            }
            GraphMoveOuter::MoveTargetFromTree { selected } => {
                self.handle_move_target_from_tree(selected, k, ctx)
            }
            GraphMoveOuter::MoveTargetPicker { picker, selected } => {
                self.handle_move_target_picker(picker, selected, k, ctx)
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        // Reconstruct the tab's strip area (top 1 row of the body) so
        // banners overwrite the view-tab strip exactly as the
        // pre-migration render arm did.
        let strip_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        match self {
            GraphMoveOuter::SourceFromTree => {
                render_move_banner(
                    frame,
                    strip_area,
                    "MOVE source · m: use selected · t: pick from list · Esc: cancel",
                );
            }
            GraphMoveOuter::SourcePicker { picker } => {
                // Forward through a throwaway `SectionMoveState::SourcePicking`
                // so the shared move overlay handles the picker chrome.
                // `mem::replace` lets us hand the picker over by value
                // without taking ownership of the variant.
                let mut wrap = SectionMoveState::SourcePicking {
                    picker: std::mem::replace(picker, open_move_file_picker(ctx)),
                };
                notes_view::render_move_overlay(frame, area, &mut wrap);
                if let SectionMoveState::SourcePicking { picker: orig } = wrap {
                    *picker = orig;
                }
            }
            GraphMoveOuter::Inner(sms) => {
                notes_view::render_move_overlay(frame, area, sms);
            }
            GraphMoveOuter::TargetFromTree { .. } => {
                render_move_banner(
                    frame,
                    strip_area,
                    "MOVE target · m: use selected · t: pick from list · Esc: back",
                );
            }
            GraphMoveOuter::TargetPicker { picker, carry } => {
                let mut wrap = SectionMoveState::TargetPicking {
                    source_rel: carry.source_rel.clone(),
                    source_abs: carry.source_abs.clone(),
                    source_content: carry.source_content.clone(),
                    headings: carry.headings.clone(),
                    selected: carry.selected.clone(),
                    focus: carry.focus,
                    clipboard: carry.clipboard.clone(),
                    picker: std::mem::replace(picker, open_move_file_picker(ctx)),
                    error: None,
                };
                notes_view::render_move_overlay(frame, area, &mut wrap);
                if let SectionMoveState::TargetPicking { picker: orig, .. } = wrap {
                    *picker = orig;
                }
            }
            GraphMoveOuter::MoveTargetFromTree { selected } => {
                let n = selected.len();
                let text = format!(
                    "Move {n} selection(s): navigate to target directory, Enter/m to confirm, t for picker, Esc to cancel"
                );
                render_move_banner(frame, strip_area, &text);
            }
            GraphMoveOuter::MoveTargetPicker { picker, .. } => {
                picker.render(frame, area);
            }
        }
    }

    fn keymap_help(&self) -> HelpSection {
        match self {
            GraphMoveOuter::SourceFromTree => HelpSection::new(
                "Move section · source",
                &[
                    ("m", "use selected as source"),
                    ("t", "pick source from list"),
                    ("Esc", "cancel"),
                ],
            ),
            GraphMoveOuter::SourcePicker { .. } => HelpSection::new(
                "Move section · source picker",
                &[
                    ("Type", "filter"),
                    ("↑ / ↓", "navigate"),
                    ("Enter", "pick source"),
                    ("Esc", "back to tree"),
                ],
            ),
            GraphMoveOuter::Inner(_) => HelpSection::new(
                "Move section",
                &[
                    ("Space", "toggle"),
                    ("↑ / ↓", "navigate"),
                    ("Enter", "confirm step"),
                    ("Esc", "cancel / back"),
                ],
            ),
            GraphMoveOuter::TargetFromTree { .. } => HelpSection::new(
                "Move section · target",
                &[
                    ("m", "use selected as target"),
                    ("t", "pick target from list"),
                    ("/", "refine tree"),
                    ("Esc", "back to headings"),
                ],
            ),
            GraphMoveOuter::TargetPicker { .. } => HelpSection::new(
                "Move section · target picker",
                &[
                    ("Type", "filter"),
                    ("↑ / ↓", "navigate"),
                    ("Enter", "pick target"),
                    ("Esc", "back to tree"),
                ],
            ),
            GraphMoveOuter::MoveTargetFromTree { .. } => HelpSection::new(
                "Move · target directory",
                &[
                    ("Enter / m", "confirm directory"),
                    ("t", "pick directory from list"),
                    ("Esc", "cancel"),
                ],
            ),
            GraphMoveOuter::MoveTargetPicker { .. } => HelpSection::new(
                "Move · directory picker",
                &[
                    ("Type", "filter"),
                    ("↑ / ↓", "navigate"),
                    ("Enter", "confirm directory"),
                    ("Esc", "back to tree"),
                ],
            ),
        }
    }

    fn name(&self) -> &'static str {
        "move"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::MOVE_OUTER_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::MOVE_OUTER_KEYMAP
    }
}

impl GraphMoveOuter {
    fn handle_source_from_tree(&mut self, k: KeyEvent, ctx: &TabCtx) -> ModalOutcome {
        match (k.code, k.modifiers) {
            (KeyCode::Char('m'), KeyModifiers::NONE) => {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::GraphMoveConfirmSourceFromTree);
                // The host hook re-opens SourceFromTree on a toast
                // path or advances to `Inner(...)` on success.
                ModalOutcome::Closed
            }
            (KeyCode::Char('t'), KeyModifiers::NONE) => ModalOutcome::OpenSibling(Box::new(
                ActiveModal::MoveOuter(GraphMoveOuter::SourcePicker {
                    picker: open_move_file_picker(ctx),
                }),
            )),
            (KeyCode::Esc, _) => ModalOutcome::Closed,
            _ => {
                *self = GraphMoveOuter::SourceFromTree;
                ModalOutcome::NotHandled
            }
        }
    }

    fn handle_source_picker(
        &mut self,
        mut picker: FuzzyPicker<VaultFilePickerSource>,
        k: KeyEvent,
        ctx: &TabCtx,
    ) -> ModalOutcome {
        match picker.handle_key(k) {
            PickerOutcome::Selected(hit) => match advance_to_multiselect(ctx, hit) {
                MoveStep::Transition(inner) => ModalOutcome::OpenSibling(Box::new(
                    ActiveModal::MoveOuter(GraphMoveOuter::Inner(inner)),
                )),
                // Toast was queued by advance_to_multiselect; drop back
                // to the tree-driven source phase so the user can
                // pick a different note.
                MoveStep::Finished => ModalOutcome::OpenSibling(Box::new(ActiveModal::MoveOuter(
                    GraphMoveOuter::SourceFromTree,
                ))),
                MoveStep::Stay | MoveStep::NotHandled => {
                    *self = GraphMoveOuter::SourcePicker { picker };
                    ModalOutcome::Consumed
                }
            },
            PickerOutcome::Cancelled => ModalOutcome::OpenSibling(Box::new(
                ActiveModal::MoveOuter(GraphMoveOuter::SourceFromTree),
            )),
            PickerOutcome::StillOpen => {
                *self = GraphMoveOuter::SourcePicker { picker };
                ModalOutcome::Consumed
            }
            PickerOutcome::NotHandled => {
                *self = GraphMoveOuter::SourcePicker { picker };
                ModalOutcome::NotHandled
            }
        }
    }

    fn handle_inner(
        &mut self,
        mut sms: SectionMoveState,
        k: KeyEvent,
        ctx: &TabCtx,
    ) -> ModalOutcome {
        // Drive the shared section-move state machine directly so we
        // can inspect the returned `MoveStep` and intercept the
        // `HeadingMultiSelect → TargetPicking` transition (we replace
        // the shared `TargetPicking` with our tree-driven
        // `TargetFromTree { carry }` instead).
        let step = section_move::handle_key(&mut sms, k, ctx);
        match step {
            MoveStep::Stay => {
                *self = GraphMoveOuter::Inner(sms);
                ModalOutcome::Consumed
            }
            MoveStep::NotHandled => {
                *self = GraphMoveOuter::Inner(sms);
                ModalOutcome::NotHandled
            }
            MoveStep::Finished => ModalOutcome::Closed,
            MoveStep::Transition(SectionMoveState::TargetPicking {
                source_rel,
                source_abs,
                source_content,
                headings,
                selected,
                focus,
                clipboard,
                picker: _,
                error: _,
            }) => {
                let carry = MoveCarry {
                    source_rel,
                    source_abs,
                    source_content,
                    headings,
                    selected,
                    focus,
                    clipboard,
                };
                ModalOutcome::OpenSibling(Box::new(ActiveModal::MoveOuter(
                    GraphMoveOuter::TargetFromTree { carry },
                )))
            }
            MoveStep::Transition(next) => {
                *self = GraphMoveOuter::Inner(next);
                ModalOutcome::Consumed
            }
        }
    }

    fn handle_target_from_tree(
        &mut self,
        carry: MoveCarry,
        k: KeyEvent,
        ctx: &TabCtx,
    ) -> ModalOutcome {
        match (k.code, k.modifiers) {
            (KeyCode::Char('m'), KeyModifiers::NONE) => {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::GraphMoveConfirmTargetFromTree {
                        carry: Box::new(carry),
                    });
                ModalOutcome::Closed
            }
            (KeyCode::Char('t'), KeyModifiers::NONE) => ModalOutcome::OpenSibling(Box::new(
                ActiveModal::MoveOuter(GraphMoveOuter::TargetPicker {
                    picker: open_move_file_picker(ctx),
                    carry,
                }),
            )),
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                // `/` cancels the move flow and opens the host's
                // query bar on the active view. With the modal
                // driver there can only be one active modal at a
                // time, so the pre-migration UX (carry preserved
                // across the bar's lifetime) is no longer
                // expressible — the carry is dropped here. The host
                // hook picks the correct `view_id`.
                let _ = carry;
                *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphFocusQueryBar);
                ModalOutcome::Closed
            }
            (KeyCode::Esc, _) => {
                // Cancel back to the heading-multi-select with the
                // same carry data so the user can re-pick headings.
                ModalOutcome::OpenSibling(Box::new(ActiveModal::MoveOuter(GraphMoveOuter::Inner(
                    SectionMoveState::HeadingMultiSelect {
                        source_rel: carry.source_rel,
                        source_abs: carry.source_abs,
                        source_content: carry.source_content,
                        headings: carry.headings,
                        selected: carry.selected,
                        focus: carry.focus,
                    },
                ))))
            }
            _ => {
                // Pass tree-navigation keys through; keep self alive.
                *self = GraphMoveOuter::TargetFromTree { carry };
                ModalOutcome::NotHandled
            }
        }
    }

    fn handle_target_picker(
        &mut self,
        mut picker: FuzzyPicker<VaultFilePickerSource>,
        carry: MoveCarry,
        k: KeyEvent,
        ctx: &TabCtx,
    ) -> ModalOutcome {
        match picker.handle_key(k) {
            PickerOutcome::Selected(hit) => {
                if hit.path == carry.source_rel {
                    // Same-file: reopen picker with same instance.
                    queue_toast(
                        ctx,
                        "same-file move is out of scope — pick a different target",
                        ToastStyle::Error,
                    );
                    *self = GraphMoveOuter::TargetPicker { picker, carry };
                    return ModalOutcome::Consumed;
                }
                let target_abs = ctx.vault.path.join(&hit.path);
                let target_content = match std::fs::read_to_string(&target_abs) {
                    Ok(s) => s,
                    Err(e) => {
                        queue_toast(
                            ctx,
                            &format!("could not read target: {e}"),
                            ToastStyle::Error,
                        );
                        return ModalOutcome::OpenSibling(Box::new(ActiveModal::MoveOuter(
                            GraphMoveOuter::TargetFromTree { carry },
                        )));
                    }
                };
                match compose_with_existing_target(carry, hit.path, target_abs, target_content) {
                    MoveStep::Transition(inner) => ModalOutcome::OpenSibling(Box::new(
                        ActiveModal::MoveOuter(GraphMoveOuter::Inner(inner)),
                    )),
                    MoveStep::Finished => ModalOutcome::Closed,
                    MoveStep::Stay | MoveStep::NotHandled => ModalOutcome::Closed,
                }
            }
            PickerOutcome::Cancelled => ModalOutcome::OpenSibling(Box::new(
                ActiveModal::MoveOuter(GraphMoveOuter::TargetFromTree { carry }),
            )),
            PickerOutcome::StillOpen => {
                *self = GraphMoveOuter::TargetPicker { picker, carry };
                ModalOutcome::Consumed
            }
            PickerOutcome::NotHandled => {
                *self = GraphMoveOuter::TargetPicker { picker, carry };
                ModalOutcome::NotHandled
            }
        }
    }

    fn handle_move_target_from_tree(
        &mut self,
        selected: HashSet<NoteId>,
        k: KeyEvent,
        ctx: &TabCtx,
    ) -> ModalOutcome {
        match (k.code, k.modifiers) {
            (KeyCode::Enter, _) | (KeyCode::Char('m'), KeyModifiers::NONE) => {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::GraphMoveConfirmMoveTarget { selected });
                ModalOutcome::Closed
            }
            (KeyCode::Char('t'), KeyModifiers::NONE) => ModalOutcome::OpenSibling(Box::new(
                ActiveModal::MoveOuter(GraphMoveOuter::MoveTargetPicker {
                    picker: open_move_file_picker(ctx),
                    selected,
                }),
            )),
            (KeyCode::Esc, _) => {
                // Cancel: multi-selection was already taken by the
                // tab's `r` arm; just drop the modal.
                ModalOutcome::Closed
            }
            _ => {
                // Tree navigation keys pass through to the tab.
                *self = GraphMoveOuter::MoveTargetFromTree { selected };
                ModalOutcome::NotHandled
            }
        }
    }

    fn handle_move_target_picker(
        &mut self,
        mut picker: FuzzyPicker<VaultFilePickerSource>,
        selected: HashSet<NoteId>,
        k: KeyEvent,
        ctx: &TabCtx,
    ) -> ModalOutcome {
        match picker.handle_key(k) {
            PickerOutcome::Selected(hit) => {
                // Hand off to the host so it can plan + apply the
                // multi-rename to the chosen directory.
                *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphMoveExecuteMultiMove {
                    selected,
                    dir_path: hit.path,
                });
                ModalOutcome::Closed
            }
            PickerOutcome::Cancelled => ModalOutcome::OpenSibling(Box::new(
                ActiveModal::MoveOuter(GraphMoveOuter::MoveTargetFromTree { selected }),
            )),
            PickerOutcome::StillOpen => {
                *self = GraphMoveOuter::MoveTargetPicker { picker, selected };
                ModalOutcome::Consumed
            }
            PickerOutcome::NotHandled => {
                *self = GraphMoveOuter::MoveTargetPicker { picker, selected };
                ModalOutcome::NotHandled
            }
        }
    }
}

/// One-line status banner overlaid on the view-strip row while a
/// tree-driven move phase is active (Source/Target). Replaces the
/// strip's view labels so the user can see which keys fire what right
/// now.
pub(super) fn render_related_modal(frame: &mut Frame, area: Rect, modal: &RelatedModal) {
    let popup_area = centered_rect(70, 70, area);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Related: {} ", modal.target_title))
        .style(Style::default());
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // The `already`-in-Related rows are non-interactive, so they live in
    // a fixed header band above the scrolling candidate list (capped so a
    // long `already` list can't starve the candidates of rows). Only the
    // candidates scroll, with the cursor kept in view.
    let already_rows = (modal.already.len() as u16).min(inner.height / 3);
    let [summary_area, already_area, list_area, footer_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(already_rows),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let header_text = if modal.candidates.is_empty() && modal.already.is_empty() {
        "no co-occurring concepts found".to_string()
    } else {
        format!(
            "{} already in Related · {} candidate(s)",
            modal.already.len(),
            modal.candidates.len()
        )
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            header_text,
            Style::default().fg(palette::DIM),
        ))),
        summary_area,
    );

    if already_rows > 0 {
        let already_lines: Vec<Line> = modal
            .already
            .iter()
            .map(|s| {
                Line::from(vec![
                    Span::styled("  ✓  ", Style::default().fg(palette::SUCCESS)),
                    Span::styled(
                        format!("[[{}]]", s.title),
                        Style::default().fg(palette::DIM),
                    ),
                    Span::styled(
                        format!("  ({})", s.score),
                        Style::default().fg(palette::DIM),
                    ),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(already_lines), already_area);
    }

    let items: Vec<ListItem> = modal
        .candidates
        .iter()
        .map(|s| {
            let checked = modal.checked.contains(&s.title);
            let marker = if checked { "[x]" } else { "[ ]" };
            ListItem::new(Line::from(vec![
                Span::raw(format!("{marker} ")),
                Span::raw(format!("[[{}]]", s.title)),
                Span::styled(
                    format!("  ({})", s.score),
                    Style::default().fg(palette::DIM),
                ),
            ]))
        })
        .collect();
    let selected = (!modal.candidates.is_empty()).then_some(modal.cursor);
    render_scroll_list(
        frame,
        list_area,
        items,
        selected,
        ScrollListOpts {
            highlight_symbol: "▶ ",
            highlight_style: Style::default().add_modifier(Modifier::REVERSED),
            scrollbar: true,
        },
    );

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Space: toggle · Enter: confirm · Esc/q: cancel",
            Style::default().fg(palette::DIM),
        ))),
        footer_area,
    );
}

pub(super) fn render_move_banner(frame: &mut Frame, area: Rect, text: &str) {
    let span = Span::styled(
        text,
        Style::default()
            .fg(palette::BLACK)
            .bg(palette::PRIMARY)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(Paragraph::new(Line::from(span)), area);
}

use anyhow::Result;
use chrono::{Duration, Local, NaiveDate};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ft_core::{
    graph::{
        query::{parse_with as parse_query, GraphQuery, Profile},
        NodeKind,
    },
    query::sort::{sort_by_keys, SortKey, SortOrder},
    task::{
        ops::{self, CompleteOptions, CreateInput},
        Priority, Status, Task,
    },
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use std::sync::LazyLock;

use crate::tui::{
    command::{Command, CommandOutcome},
    event::Event,
    keymap::{KeyChord, KeyMap},
    palette,
    tab::{AppRequest, EventOutcome, TabCtx, ToastStyle},
    tabs::tasks::{
        edit_popup::{
            handle_target_picker_key, merge_tags_into_description, open_target_picker,
            parse_optional_date, parse_priority, parse_tags_field, relative_date,
            render_edit_popup, EditField, EditPopup, PopupFields, PopupMode,
        },
        quickline::parse_quickline,
        view::View,
    },
    widgets::{render_inline_input, CursorMode, EditBuffer, InlineInput},
};

/// Idle-state keymap for the SearchView (the only view under TasksTab
/// today). Sub-modes (popup, quickline, edit_state, target picker)
/// capture keys at the top of `handle_event` and bypass this map.
/// Command names live in `tabs::tasks::TASKS_COMMANDS` (see
/// `tabs/tasks/mod.rs`).
pub(super) static SEARCH_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        // Navigation
        .bind("/", "tasks.edit-query")
        .bind("Up", "tasks.cursor-up")
        .bind("k", "tasks.cursor-up")
        .bind("Down", "tasks.cursor-down")
        .bind("j", "tasks.cursor-down")
        .bind("R", "tasks.reload")
        .bind("Enter", "tasks.open-in-editor")
        // Subtask tree: expand / collapse the selected task.
        .bind("l", "tasks.expand")
        .bind("Right", "tasks.expand")
        .bind("h", "tasks.collapse")
        .bind("Left", "tasks.collapse")
        // Mutations — special-char bindings (normalization strips SHIFT
        // for non-alpha chars).
        .bind("]", "tasks.due-next-day")
        .bind("[", "tasks.due-prev-day")
        .bind("}", "tasks.scheduled-next-day")
        .bind("{", "tasks.scheduled-prev-day")
        .bind("p", "tasks.priority-next")
        .bind("P", "tasks.priority-prev")
        .bind("x", "tasks.complete")
        .bind("X", "tasks.cancel")
        .bind("t", "tasks.due-today")
        .bind("e", "tasks.edit-popup")
        // Create / edit
        .bind("c", "tasks.quickline")
        .bind("C", "tasks.new-blank-form")
        .bind("s", "tasks.new-subtask")
});

/// Search view: lazy task scan, editable DSL query bar, and a paginated list
/// split into "overdue" and "upcoming" buckets. Quick mutations and editor
/// handoff land in sessions 4–5 — this session lays the foundation.
pub struct SearchView {
    /// Tasks cloned from the last-adopted snapshot. Cloning keeps the
    /// many `self.tasks[idx]` sites simple; the copy is refreshed only
    /// when a new generation is adopted.
    tasks: Vec<Task>,
    /// The App-owned snapshot this view last derived from (openspec:
    /// shared-graph-snapshot). Supplies the graph for DSL evaluation;
    /// its generation gates re-derivation. Never built here.
    snapshot: Option<std::sync::Arc<crate::tui::snapshot::GraphSnapshot>>,
    /// Cursor targets to try (in order) after the next adoption —
    /// e.g. `[new task, prior cursor]` after a create. First hit wins.
    pending_anchors: Vec<(std::path::PathBuf, usize)>,
    /// Whether `tasks` reflects an adopted snapshot (vs. initial empty).
    loaded: bool,
    /// Indices into `tasks` (sorted) that match the active query and pass
    /// the today-cutoff. Recomputed on load, on query apply, and on `R`.
    matches: Vec<usize>,
    /// Number of leading entries in `matches` that are overdue (due < today).
    /// The remainder are upcoming.
    overdue_count: usize,
    /// Index into `matches` for the highlighted row. Saturates at boundaries
    /// when wrapping is disabled, otherwise wraps via `↑` past 0 / `↓` past N.
    selected: usize,
    /// Top-of-viewport row offset within the visible row sequence (including
    /// dividers). Updated to keep `selected` on screen.
    scroll: u16,

    /// Currently active query string (the one driving `matches`).
    query_text: String,
    /// Most recent parse outcome for `query_text`. `Ok(None)` = empty query
    /// (matches all). `Err(msg)` shows the message in place of the list.
    parse_state: ParseState,

    /// Whether the query bar is focused for editing. While editing, all key
    /// events go to the buffer (not the list).
    edit_state: Option<EditBuffer>,

    /// Open edit-popup state, if any. Set by `e`; cleared by Esc / Ctrl+S.
    /// While the popup is open, all keys go to it.
    popup: Option<EditPopup>,

    /// Open quickline state, if any. Set by `c`; cleared by Esc / Enter
    /// (on a successful write). While the quickline is open, all keys
    /// go to its input buffer.
    quickline: Option<Quickline>,

    /// Keys (`source_file`, `source_line`) of tasks the user has expanded.
    /// Keyed by identity (not index) so expansion survives a reload.
    expanded: std::collections::HashSet<(std::path::PathBuf, usize)>,
    /// Flattened, depth-annotated rows actually rendered: every match plus
    /// the visible (expanded) subtree beneath it. `selected` indexes this.
    /// Rebuilt by `rebuild_display` after matches or expansion change.
    display: Vec<DisplayRow>,
    /// Number of leading `display` rows belonging to the overdue bucket
    /// (a top-level overdue match and its expanded subtree). The rest are
    /// upcoming. Drives the section divider placement.
    overdue_display_count: usize,

    /// When a create modal was opened via `tasks.new-subtask`, the
    /// `(source_file, source_line)` of the task the new entry should nest
    /// under. The quickline / form are otherwise identical; only the write
    /// position changes. `None` for ordinary top-level creates. Set fresh by
    /// every create-open command so it can't go stale.
    subtask_parent: Option<(std::path::PathBuf, usize)>,
}

/// One rendered list row: the task plus the tree-state needed to draw it.
#[derive(Debug, Clone, Copy)]
struct DisplayRow {
    /// Index into `SearchView::tasks`.
    task_idx: usize,
    /// Nesting depth: 0 for a top-level match, +1 per subtask level.
    depth: usize,
    /// Whether this task has any subtasks (shows a ▸/▾ affordance).
    has_children: bool,
    /// Whether this task is currently expanded.
    expanded: bool,
}

/// "New task" quickline state — a single edit buffer plus a slot for
/// post-submit errors (duplicate detection, IO failures). The parsed form
/// is re-derived on every render from `input.text`; parsing is cheap
/// enough that caching adds complexity without buying us anything.
#[derive(Debug, Clone, Default)]
struct Quickline {
    input: EditBuffer,
    error: Option<String>,
}

/// Result of compiling the active `query_text` against the current `today`.
#[derive(Debug, Clone)]
enum ParseState {
    Ok(Option<GraphQuery>),
    Err(String),
}

// EditBuffer now lives in crate::tui::widgets — see import at the top.

impl SearchView {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            snapshot: None,
            pending_anchors: Vec::new(),
            loaded: false,
            matches: Vec::new(),
            overdue_count: 0,
            selected: 0,
            scroll: 0,
            query_text: String::new(),
            parse_state: ParseState::Ok(None),
            edit_state: None,
            popup: None,
            quickline: None,
            expanded: std::collections::HashSet::new(),
            display: Vec::new(),
            overdue_display_count: 0,
            subtask_parent: None,
        }
    }

    /// Default DSL: tasks that are still actionable, due before `today + 8`.
    /// The literal date keeps the bar copy-pastable and round-trippable
    /// through the parser. Sorting is applied by the view independently;
    /// the unified DSL does not carry sort clauses.
    fn default_query(today: NaiveDate) -> String {
        let upper = today + Duration::days(8);
        format!(
            "status in {{Open, InProgress}} and due < {}",
            upper.format("%Y-%m-%d")
        )
    }

    /// Adopt `ctx.snapshot` when it is newer than the one this view
    /// last derived from (or on first load), then resolve any pending
    /// cursor anchors. No-op while no snapshot is installed yet or the
    /// generation is unchanged.
    fn adopt_snapshot(&mut self, ctx: &mut TabCtx) -> Result<()> {
        let ctx_gen = ctx.snapshot.as_ref().map(|s| s.generation);
        let seen = self.snapshot.as_ref().map(|s| s.generation);
        if ctx_gen.is_none() || (self.loaded && ctx_gen == seen) {
            return Ok(());
        }
        self.reload(ctx)?;
        let anchors = std::mem::take(&mut self.pending_anchors);
        for a in &anchors {
            if self.select_display_row(a) {
                return Ok(());
            }
        }
        self.clamp_selection();
        Ok(())
    }

    /// Re-derive tasks + matches from the installed snapshot. The
    /// graph and task list come from the same scan pass, so DSL node
    /// results map back to `tasks` by `(path, line)`.
    fn reload(&mut self, ctx: &mut TabCtx) -> Result<()> {
        let Some(snap) = ctx.snapshot.as_ref() else {
            // Nothing installed yet — stay unloaded so the next focus
            // or snapshot arrival retries.
            return Ok(());
        };
        self.snapshot = Some(std::sync::Arc::clone(snap));
        self.tasks = snap.scan.tasks.clone();
        self.loaded = true;
        if self.query_text.is_empty() {
            self.query_text = Self::default_query(ctx.today);
        }
        self.recompile(ctx.today);
        self.recompute_matches(ctx.today);
        ctx.last_refresh.set(Some(Local::now()));
        Ok(())
    }

    fn recompile(&mut self, today: NaiveDate) {
        let trimmed = self.query_text.trim();
        if trimmed.is_empty() {
            self.parse_state = ParseState::Ok(None);
            return;
        }
        match parse_query(trimmed, Profile::Tasks, today) {
            Ok(q) => self.parse_state = ParseState::Ok(Some(q)),
            Err(e) => self.parse_state = ParseState::Err(e.to_string()),
        }
    }

    fn recompute_matches(&mut self, today: NaiveDate) {
        self.matches.clear();
        self.overdue_count = 0;
        self.selected = 0;
        self.scroll = 0;

        let query = match &self.parse_state {
            ParseState::Ok(q) => q.clone(),
            ParseState::Err(_) => return,
        };

        // Membership: tasks the parsed query allows. With no graph (first
        // frame before reload) or no query, every task is admissible.
        let allowed: Option<std::collections::HashSet<(std::path::PathBuf, usize)>> =
            match (query.as_ref(), self.snapshot.as_ref().map(|s| &s.graph)) {
                (Some(q), Some(g)) => {
                    let ids = q.select(g);
                    Some(
                        ids.into_iter()
                            .filter_map(|id| match g.node(id) {
                                NodeKind::Task(td) => {
                                    Some((td.source_file.clone(), td.source_line))
                                }
                                _ => None,
                            })
                            .collect(),
                    )
                }
                _ => None,
            };

        // Build the filtered, sorted slice. Sort always uses the view's
        // default (due asc, priority desc) since the DSL no longer carries
        // sort clauses.
        let mut keep: Vec<&Task> = self
            .tasks
            .iter()
            .filter(|t| {
                allowed
                    .as_ref()
                    .is_none_or(|s| s.contains(&(t.source_file.clone(), t.source_line)))
            })
            .collect();

        let sort_keys: Vec<(SortKey, SortOrder)> = Vec::new();
        sort_by_keys(&mut keep, &sort_keys);

        // Reverse-map back to indices into self.tasks. Tasks are uniquely
        // identified by (path, line); we look each one up.
        for t in &keep {
            if let Some(idx) = self
                .tasks
                .iter()
                .position(|s| s.source_file == t.source_file && s.source_line == t.source_line)
            {
                self.matches.push(idx);
            }
        }

        // Bucket: count leading overdue entries. After sort by due asc, all
        // overdue rows precede upcoming ones.
        self.overdue_count = self
            .matches
            .iter()
            .take_while(|&&i| self.tasks[i].due.map(|d| d < today).unwrap_or(false))
            .count();

        self.rebuild_display();
    }

    /// Flatten `matches` into `display`, splicing in the visible subtree
    /// beneath every expanded task. Keeps the overdue/upcoming bucket
    /// boundary (`overdue_display_count`) in display-row space.
    fn rebuild_display(&mut self) {
        // Deduplicated forest via the shared `expand_forest_visible`
        // (graph-task-interaction §D7): a matched subtask whose parent is
        // also matched appears once, nested — never also as a depth-0 root.
        // The overdue/upcoming split is preserved by building each bucket's
        // matched-key slice separately and emitting them back-to-back.
        let split = self.overdue_count.min(self.matches.len());
        let keys_for = |slice: &[usize]| -> Vec<ft_core::task::TaskKey> {
            slice
                .iter()
                .map(|&i| (self.tasks[i].source_file.clone(), self.tasks[i].source_line))
                .collect()
        };
        let overdue_keys = keys_for(&self.matches[..split]);
        let upcoming_keys = keys_for(&self.matches[split..]);

        let mut rows: Vec<DisplayRow> = Vec::with_capacity(self.matches.len());
        let push = |vrows: Vec<ft_core::task::hierarchy::VisibleRow>,
                    rows: &mut Vec<DisplayRow>| {
            for r in vrows {
                rows.push(DisplayRow {
                    task_idx: r.idx,
                    depth: r.depth,
                    has_children: r.has_children,
                    expanded: r.expanded,
                });
            }
        };
        push(
            ft_core::task::hierarchy::expand_forest_visible(
                &self.tasks,
                &overdue_keys,
                &self.expanded,
            ),
            &mut rows,
        );
        self.overdue_display_count = rows.len();
        push(
            ft_core::task::hierarchy::expand_forest_visible(
                &self.tasks,
                &upcoming_keys,
                &self.expanded,
            ),
            &mut rows,
        );
        self.display = rows;
        if self.selected >= self.display.len() {
            self.selected = self.display.len().saturating_sub(1);
        }
    }

    /// Task index under the cursor, if any. The single accessor mutation and
    /// editor paths go through, so they don't care that `selected` indexes
    /// `display` (which includes spliced-in subtasks), not `matches`.
    fn selected_task_idx(&self) -> Option<usize> {
        self.display.get(self.selected).map(|r| r.task_idx)
    }

    // --- selection ---------------------------------------------------------

    fn select_prev(&mut self) {
        if self.display.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.display.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    fn select_next(&mut self) {
        if self.display.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.display.len();
    }

    /// Expand the selected task one level. If it's a leaf, no-op. If it's
    /// already expanded, move the cursor onto its first subtask (file-explorer
    /// `→` idiom).
    fn expand_selected(&mut self) {
        let Some(row) = self.display.get(self.selected).copied() else {
            return;
        };
        if !row.has_children {
            return;
        }
        if row.expanded {
            // Already open — step into the first child (the next display row).
            if self.selected + 1 < self.display.len() {
                self.selected += 1;
            }
            return;
        }
        let key = (
            self.tasks[row.task_idx].source_file.clone(),
            self.tasks[row.task_idx].source_line,
        );
        self.expanded.insert(key);
        self.rebuild_display();
    }

    /// Collapse the selected task. If it's an expanded parent, close it. If
    /// it's a nested row, move the cursor up to its parent instead (so a
    /// second `←` then closes that parent).
    fn collapse_selected(&mut self) {
        let Some(row) = self.display.get(self.selected).copied() else {
            return;
        };
        if row.has_children && row.expanded {
            let key = (
                self.tasks[row.task_idx].source_file.clone(),
                self.tasks[row.task_idx].source_line,
            );
            self.expanded.remove(&key);
            self.rebuild_display();
        } else if row.depth > 0 {
            // Walk up to the nearest shallower row — that's the parent.
            if let Some(p) = (0..self.selected)
                .rev()
                .find(|&i| self.display[i].depth < row.depth)
            {
                self.selected = p;
            }
        }
    }

    // --- query editing -----------------------------------------------------

    fn enter_edit_mode(&mut self) {
        self.edit_state = Some(EditBuffer::from(&self.query_text));
    }

    fn cancel_edit(&mut self) {
        self.edit_state = None;
    }

    fn apply_edit(&mut self, ctx: &mut TabCtx) {
        if let Some(buf) = self.edit_state.take() {
            self.query_text = buf.text;
            self.recompile(ctx.today);
            self.recompute_matches(ctx.today);
        }
    }

    // --- rendering helpers -------------------------------------------------

    fn render_query_bar(&self, frame: &mut Frame, area: Rect) {
        let editing = self.edit_state.is_some();
        let title = if editing {
            " query (editing) "
        } else {
            " query "
        };
        let border_style = if editing {
            Style::default().fg(palette::PRIMARY)
        } else {
            Style::default().fg(palette::DIM)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // While editing, scroll horizontally so the caret stays visible —
        // long queries would otherwise drop it off the right edge.
        if let Some(buf) = &self.edit_state {
            let caret = Style::default()
                .fg(palette::PRIMARY)
                .add_modifier(Modifier::BOLD);
            render_inline_input(frame, inner, InlineInput::new(buf, CursorMode::Bar(caret)));
        } else {
            let display = if self.query_text.is_empty() {
                "(no filter — press / to edit)".to_string()
            } else {
                self.query_text.clone()
            };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    display,
                    Style::default().fg(palette::WHITE),
                ))),
                inner,
            );
        }
    }

    /// Render the new-task quickline panel. The caller picks a 4-row
    /// `area` (3 for the bordered input, 1 for the preview underneath).
    fn render_quickline(&self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        let Some(ql) = self.quickline.as_ref() else {
            return;
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(1)])
            .split(area);

        // ── input row ───────────────────────────────────────────────
        let chars: Vec<char> = ql.input.text.chars().collect();
        let cursor = ql.input.cursor.min(chars.len());
        let line = if chars.is_empty() {
            Line::from(Span::styled(
                "type a task — e.g. \"email Sarah due:tomorrow pri:high #work\"",
                Style::default()
                    .fg(palette::DIM)
                    .add_modifier(Modifier::ITALIC),
            ))
        } else {
            let mut iter = chars.iter().copied();
            let left: String = iter.by_ref().take(cursor).collect();
            let right: String = iter.collect();
            Line::from(vec![
                Span::styled(left, Style::default().fg(palette::WHITE)),
                Span::styled(
                    "│",
                    Style::default()
                        .fg(palette::PRIMARY)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(right, Style::default().fg(palette::WHITE)),
            ])
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::SUCCESS))
            .title(" new task ");
        frame.render_widget(Paragraph::new(line).block(block), chunks[0]);

        // ── preview row ─────────────────────────────────────────────
        let preview = build_quickline_preview(ql, ctx);
        frame.render_widget(Paragraph::new(preview), chunks[1]);
    }

    fn render_list(&self, frame: &mut Frame, area: Rect, today: NaiveDate) {
        // Parse error short-circuits the list.
        if let ParseState::Err(msg) = &self.parse_state {
            let body = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "query parse error",
                    Style::default()
                        .fg(palette::ERROR)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(msg, Style::default().fg(palette::ERROR))),
                Line::from(""),
                Line::from(Span::styled(
                    "press / to edit the query",
                    Style::default()
                        .fg(palette::DIM)
                        .add_modifier(Modifier::ITALIC),
                )),
            ])
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(" tasks "));
            frame.render_widget(body, area);
            return;
        }

        if !self.loaded {
            let body = Paragraph::new(Line::from(Span::styled(
                "loading…",
                Style::default().fg(palette::DIM),
            )))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(" tasks "));
            frame.render_widget(body, area);
            return;
        }

        if self.matches.is_empty() {
            let body = Paragraph::new(Line::from(Span::styled(
                "no matching tasks",
                Style::default().fg(palette::DIM),
            )))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(" tasks "));
            frame.render_widget(body, area);
            return;
        }

        // Inner width inside the borders. Fixed columns: cursor(2)
        // + status glyph(2) + priority label(4) + due block(13)
        // + scheduled block(13) = 34. Description column flexes.
        let inner_width = area.width.saturating_sub(2);
        let desc_width = inner_width.saturating_sub(34).max(16) as usize;

        let lines = self.build_lines(today, desc_width);
        let scroll = self.scroll;
        let list = Paragraph::new(lines)
            .scroll((scroll, 0))
            .block(Block::default().borders(Borders::ALL).title(" tasks "));
        frame.render_widget(list, area);
    }

    fn build_lines(&self, today: NaiveDate, desc_width: usize) -> Vec<Line<'static>> {
        let mut lines: Vec<Line> = Vec::with_capacity(self.display.len() + 4);
        let split = self.overdue_display_count;
        // Divider labels count top-level matches, not display rows, so the
        // numbers stay stable as subtrees are expanded and collapsed.
        let overdue_n = self.overdue_count;
        let upcoming_n = self.matches.len().saturating_sub(self.overdue_count);

        if split > 0 {
            lines.push(divider_line(&format!("── overdue ({overdue_n}) ──")));
            for (i, row) in self.display[..split].iter().enumerate() {
                lines.push(task_line(
                    &self.tasks[row.task_idx],
                    today,
                    i == self.selected,
                    desc_width,
                    row,
                ));
            }
        }
        if split < self.display.len() {
            lines.push(divider_line(&format!("── upcoming ({upcoming_n}) ──")));
            for (i, row) in self.display[split..].iter().enumerate() {
                let sel = (i + split) == self.selected;
                lines.push(task_line(
                    &self.tasks[row.task_idx],
                    today,
                    sel,
                    desc_width,
                    row,
                ));
            }
        }
        lines
    }

    /// Compute the row index of `selected` within the rendered line sequence
    /// (which includes section dividers). Returns 0 when nothing is selected.
    fn selected_row(&self) -> u16 {
        if self.display.is_empty() {
            return 0;
        }
        let split = self.overdue_display_count;
        // Each non-empty section adds 1 divider row before its tasks.
        let mut row: usize = 0;
        if split > 0 {
            row += 1; // overdue divider
        }
        if self.selected < split {
            row += self.selected;
        } else {
            row += split; // skip overdue rows
            row += 1; // upcoming divider
            row += self.selected - split;
        }
        u16::try_from(row).unwrap_or(u16::MAX)
    }

    fn adjust_scroll(&mut self, body_height: u16) {
        // Body has a 1-row top border + 1-row bottom border ⇒ 2 reserved rows.
        let visible = body_height.saturating_sub(2).max(1);
        let row = self.selected_row();
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + visible {
            self.scroll = row + 1 - visible;
        }
    }
}

impl View for SearchView {
    fn title(&self) -> &str {
        "Search"
    }

    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()> {
        self.adopt_snapshot(ctx)
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        // Pick up a newer snapshot before acting on the key so task
        // line numbers come from the freshest installed build.
        self.adopt_snapshot(ctx)?;

        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };

        // Modal popup swallows everything until Esc / Ctrl+S.
        if self.popup.is_some() {
            return self.handle_popup_key(k, ctx);
        }

        // Quickline panel swallows everything until Esc / Enter (success).
        // Checked before edit_state because the quickline is a stronger
        // focus context — opening it from the query bar shouldn't happen
        // (the query bar is closed on `c` from normal mode anyway).
        if self.quickline.is_some() {
            return self.handle_quickline_key(k, ctx);
        }

        // Editing the query bar swallows everything except Apply/Cancel.
        if self.edit_state.is_some() {
            return Ok(self.handle_edit_key(k, ctx));
        }

        // Idle keymap lookup → SearchView::dispatch_command. Sub-modes
        // (popup, quickline, edit_state) handled their keys above and
        // returned early — only Idle reaches this point.
        let chord = KeyChord::from_key_event(k);
        let Some(cmd) = SEARCH_KEYMAP.lookup(chord).cloned() else {
            return Ok(EventOutcome::NotHandled);
        };
        match self.dispatch_idle_command(&cmd, ctx)? {
            CommandOutcome::Handled => Ok(EventOutcome::Consumed),
            CommandOutcome::NotHandled => Ok(EventOutcome::NotHandled),
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        SearchView::render(self, frame, area, ctx)
    }

    fn refresh(&mut self, ctx: &mut TabCtx) -> Result<()> {
        SearchView::refresh(self, ctx)
    }

    fn on_graph_ready(&mut self, ctx: &mut TabCtx) {
        let _ = self.adopt_snapshot(ctx);
    }
}

impl SearchView {
    /// Apply one Idle-state command. Returns `Result` because `reload`
    /// can fail; other arms are infallible.
    fn dispatch_idle_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> Result<CommandOutcome> {
        match cmd.name {
            "tasks.edit-query" => {
                self.enter_edit_mode();
                Ok(CommandOutcome::Handled)
            }
            "tasks.cursor-up" => {
                self.select_prev();
                Ok(CommandOutcome::Handled)
            }
            "tasks.cursor-down" => {
                self.select_next();
                Ok(CommandOutcome::Handled)
            }
            "tasks.expand" => {
                self.expand_selected();
                Ok(CommandOutcome::Handled)
            }
            "tasks.collapse" => {
                self.collapse_selected();
                Ok(CommandOutcome::Handled)
            }
            "tasks.reload" => {
                ctx.request_graph_refresh();
                Ok(CommandOutcome::Handled)
            }
            "tasks.due-next-day" => {
                let _ = self.nudge_field(ctx, Field::Due, 1)?;
                Ok(CommandOutcome::Handled)
            }
            "tasks.due-prev-day" => {
                let _ = self.nudge_field(ctx, Field::Due, -1)?;
                Ok(CommandOutcome::Handled)
            }
            "tasks.scheduled-next-day" => {
                let _ = self.nudge_field(ctx, Field::Scheduled, 1)?;
                Ok(CommandOutcome::Handled)
            }
            "tasks.scheduled-prev-day" => {
                let _ = self.nudge_field(ctx, Field::Scheduled, -1)?;
                Ok(CommandOutcome::Handled)
            }
            "tasks.priority-next" => {
                let _ = self.cycle_priority(ctx, 1)?;
                Ok(CommandOutcome::Handled)
            }
            "tasks.priority-prev" => {
                let _ = self.cycle_priority(ctx, -1)?;
                Ok(CommandOutcome::Handled)
            }
            "tasks.complete" => {
                let _ = self.complete_selected(ctx)?;
                Ok(CommandOutcome::Handled)
            }
            "tasks.cancel" => {
                let _ = self.cancel_selected(ctx)?;
                Ok(CommandOutcome::Handled)
            }
            "tasks.due-today" => {
                let _ = self.set_due_today(ctx)?;
                Ok(CommandOutcome::Handled)
            }
            "tasks.edit-popup" => {
                self.open_edit_popup();
                Ok(CommandOutcome::Handled)
            }
            "tasks.quickline" => {
                self.subtask_parent = None;
                self.quickline = Some(Quickline::default());
                Ok(CommandOutcome::Handled)
            }
            "tasks.new-blank-form" => {
                self.subtask_parent = None;
                self.popup = Some(EditPopup::new_blank());
                Ok(CommandOutcome::Handled)
            }
            "tasks.new-subtask" => {
                // Same quickline as `c`, but the new task nests under the
                // selected task. No selection ⇒ nothing to parent to.
                match self.selected_task_idx() {
                    Some(i) => {
                        let t = &self.tasks[i];
                        self.subtask_parent = Some((t.source_file.clone(), t.source_line));
                        self.quickline = Some(Quickline::default());
                    }
                    None => {
                        *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast {
                            text: "no task selected to add a subtask to".into(),
                            style: ToastStyle::Error,
                        });
                    }
                }
                Ok(CommandOutcome::Handled)
            }
            "tasks.open-in-editor" => {
                self.request_editor_open(ctx);
                Ok(CommandOutcome::Handled)
            }
            _ => Ok(CommandOutcome::NotHandled),
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        // When the quickline is open, slot it between the query bar and
        // the task list. 3-row bordered input + 1-row preview = 4 rows.
        let chunks = if self.quickline.is_some() {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(4),
                    Constraint::Min(1),
                ])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(1)])
                .split(area)
        };

        self.render_query_bar(frame, chunks[0]);
        if self.quickline.is_some() {
            self.render_quickline(frame, chunks[1], ctx);
            self.adjust_scroll(chunks[2].height);
            self.render_list(frame, chunks[2], ctx.today);
        } else {
            self.adjust_scroll(chunks[1].height);
            self.render_list(frame, chunks[1], ctx.today);
        }

        // Popup is drawn last so it floats above the list. Use the full body
        // area as the anchor — the helper centers the popup within it.
        if let Some(popup) = &mut self.popup {
            render_edit_popup(frame, area, popup);
        }
    }

    fn refresh(&mut self, ctx: &mut TabCtx) -> Result<()> {
        // Adopt anything newer already installed, then ask for a fresh
        // build (post-editor / post-sync content may have changed).
        self.adopt_snapshot(ctx)?;
        ctx.request_graph_refresh();
        Ok(())
    }
}

/// Which date column a `]`/`[`/`}`/`{` keypress targets.
#[derive(Debug, Clone, Copy)]
enum Field {
    Due,
    Scheduled,
}

/// Priority cycle order per plan: `p` walks None → Low → Medium → High → None;
/// `P` walks the other way. Highest/Lowest aren't on the cycle — they're
/// rarely used and the future edit popup will set them explicitly.
const PRIORITY_CYCLE: &[Option<Priority>] = &[
    None,
    Some(Priority::Low),
    Some(Priority::Medium),
    Some(Priority::High),
];

fn cycle_pos(p: Option<Priority>) -> usize {
    PRIORITY_CYCLE.iter().position(|x| *x == p).unwrap_or(0)
}

impl SearchView {
    /// Re-scan the vault and recompute matches, then restore the selection
    /// to the row whose `(path, line)` matches `anchor` if it's still in the
    /// list. Falls back to saturating at the last row.
    fn refresh_after_mutation(
        &mut self,
        ctx: &mut TabCtx,
        anchor: Option<(std::path::PathBuf, usize)>,
    ) -> Result<()> {
        self.pending_anchors = anchor.into_iter().collect();
        ctx.request_graph_refresh();
        Ok(())
    }

    /// Move the cursor to the display row for `key` (a `(path, line)`),
    /// returning whether it was found.
    fn select_display_row(&mut self, key: &(std::path::PathBuf, usize)) -> bool {
        if let Some(i) = self.display.iter().position(|r| {
            let t = &self.tasks[r.task_idx];
            t.source_file == key.0 && t.source_line == key.1
        }) {
            self.selected = i;
            true
        } else {
            false
        }
    }

    /// Keep `selected` within the current display bounds after a rebuild.
    fn clamp_selection(&mut self) {
        if !self.display.is_empty() && self.selected >= self.display.len() {
            self.selected = self.display.len() - 1;
        }
    }

    /// Refresh after a create. Prefer to anchor at the new task's
    /// `(path, line)`; if the new task doesn't pass the current filter,
    /// fall back to where the cursor was sitting before the write so the
    /// user doesn't lose their place.
    fn refresh_and_anchor_to_create(
        &mut self,
        ctx: &mut TabCtx,
        new: (std::path::PathBuf, usize),
        prior: Option<(std::path::PathBuf, usize)>,
    ) -> Result<()> {
        // Anchors are tried in order on the next snapshot adoption: the
        // new task first, then the prior cursor; neither matching falls
        // back to a clamped selection.
        self.pending_anchors = std::iter::once(new).chain(prior).collect();
        ctx.request_graph_refresh();
        Ok(())
    }

    fn with_selected_task<F>(&mut self, ctx: &mut TabCtx, op: F) -> Result<EventOutcome>
    where
        F: FnOnce(&std::path::Path, &Task, NaiveDate) -> Result<()>,
    {
        let Some(task_idx) = self.selected_task_idx() else {
            return Ok(EventOutcome::Consumed);
        };
        let task = &self.tasks[task_idx];
        // Tasks store paths relative to the vault root; ft-core mutators
        // need an absolute (or CWD-relative) path to read/write.
        let absolute = ctx.vault.path.join(&task.source_file);
        let anchor = Some((task.source_file.clone(), task.source_line));
        if let Err(e) = op(&absolute, task, ctx.today) {
            // Typically the expected-line guard: the file changed on
            // disk since this snapshot was built. Surface the error and
            // refresh so line numbers realign — never crash the loop.
            *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast {
                text: format!("{e}"),
                style: ToastStyle::Error,
            });
            ctx.request_graph_refresh();
            return Ok(EventOutcome::Consumed);
        }
        self.refresh_after_mutation(ctx, anchor)?;
        Ok(EventOutcome::Consumed)
    }

    fn nudge_field(
        &mut self,
        ctx: &mut TabCtx,
        field: Field,
        delta_days: i64,
    ) -> Result<EventOutcome> {
        let format = ctx.vault.task_format();
        self.with_selected_task(ctx, |path, task, today| {
            let line = task.source_line;
            ops::update_task_line(path, line, format, Some(task), move |t| {
                let current = match field {
                    Field::Due => t.due,
                    Field::Scheduled => t.scheduled,
                };
                let base = current.unwrap_or(today);
                let next = base + Duration::days(delta_days);
                match field {
                    Field::Due => t.due = Some(next),
                    Field::Scheduled => t.scheduled = Some(next),
                }
            })?;
            Ok(())
        })
    }

    fn set_due_today(&mut self, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let format = ctx.vault.task_format();
        self.with_selected_task(ctx, |path, task, today| {
            let line = task.source_line;
            ops::update_task_line(path, line, format, Some(task), move |t| {
                t.due = Some(today);
            })?;
            Ok(())
        })
    }

    fn cycle_priority(&mut self, ctx: &mut TabCtx, direction: i64) -> Result<EventOutcome> {
        let format = ctx.vault.task_format();
        self.with_selected_task(ctx, |path, task, _today| {
            let line = task.source_line;
            ops::update_task_line(path, line, format, Some(task), move |t| {
                let pos = cycle_pos(t.priority) as i64;
                let len = PRIORITY_CYCLE.len() as i64;
                let next = ((pos + direction).rem_euclid(len)) as usize;
                t.priority = PRIORITY_CYCLE[next];
            })?;
            Ok(())
        })
    }

    fn complete_selected(&mut self, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let format = ctx.vault.task_format();
        self.with_selected_task(ctx, |path, task, today| {
            // Already-done tasks are a no-op rather than an error so the user
            // can hammer `x` without ceremony.
            match ops::complete_task(
                path,
                task.source_line,
                format,
                Some(task),
                CompleteOptions { on: today },
            ) {
                Ok(_) => Ok(()),
                Err(ops::CompleteError::AlreadyDone { .. }) => Ok(()),
                Err(e) => Err(anyhow::Error::from(e)),
            }
        })
    }

    fn cancel_selected(&mut self, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let format = ctx.vault.task_format();
        self.with_selected_task(ctx, |path, task, today| {
            match ops::cancel_task(path, task.source_line, format, Some(task), today) {
                Ok(_) => Ok(()),
                Err(ops::CancelError::AlreadyCancelled { .. }) => Ok(()),
                Err(e) => Err(anyhow::Error::from(e)),
            }
        })
    }

    fn open_edit_popup(&mut self) {
        let Some(task_idx) = self.selected_task_idx() else {
            return;
        };
        self.popup = Some(EditPopup::from_task(&self.tasks[task_idx]));
    }

    fn request_editor_open(&self, ctx: &TabCtx) {
        let Some(task_idx) = self.selected_task_idx() else {
            return;
        };
        let task = &self.tasks[task_idx];
        let absolute = ctx.vault.path.join(&task.source_file);
        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
            path: absolute,
            line: task.source_line,
        });
    }

    fn handle_popup_key(&mut self, k: KeyEvent, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Some(popup) = self.popup.as_mut() else {
            return Ok(EventOutcome::Consumed);
        };

        // Open picker — all keys go to it, the form is paused beneath.
        if popup.target_picker.is_some() {
            return Ok(handle_target_picker_key(popup, k));
        }

        // Ctrl+S submits regardless of focused field.
        if k.code == KeyCode::Char('s') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return self.submit_popup(ctx);
        }

        // Picker triggers — only fire when target is the focused field and
        // we're in `New` mode (Edit mode hides the target field entirely).
        if popup.focus == EditField::Target && popup.mode == PopupMode::New {
            match (k.code, k.modifiers) {
                (KeyCode::Enter, _) => {
                    open_target_picker(popup, ctx, None);
                    return Ok(EventOutcome::Consumed);
                }
                (KeyCode::Char(c), m)
                    if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
                {
                    open_target_picker(popup, ctx, Some(c));
                    return Ok(EventOutcome::Consumed);
                }
                _ => {}
            }
        }

        // Popup-frame chords (focus moves, close) take precedence over
        // the buffer's EDIT_KEYMAP. `Tab`/`BackTab`/`Up`/`Down`
        // navigate between fields; the focused buffer never sees them.
        match (k.code, k.modifiers) {
            (KeyCode::Esc, _) => {
                self.popup = None;
            }
            (KeyCode::Tab, _) => popup.focus = popup.next_field(),
            (KeyCode::BackTab, _) => popup.focus = popup.prev_field(),
            (KeyCode::Down, _) => popup.focus = popup.next_field(),
            (KeyCode::Up, _) => popup.focus = popup.prev_field(),
            _ => {
                let _ = popup.focused_buffer_mut().handle_event(k);
            }
        }
        Ok(EventOutcome::Consumed)
    }

    fn submit_popup(&mut self, ctx: &mut TabCtx) -> Result<EventOutcome> {
        // Validate everything *before* mutating disk so a bad input keeps the
        // popup open with a clear message. Borrow popup immutably through the
        // validation phase, then drop the borrow before calling the mutator.
        let (validated, mode) = {
            let Some(popup) = self.popup.as_ref() else {
                return Ok(EventOutcome::Consumed);
            };
            let due = match parse_optional_date(&popup.due.text, ctx.today) {
                Ok(v) => v,
                Err(e) => {
                    self.popup.as_mut().unwrap().error = Some(format!("due: {e}"));
                    self.popup.as_mut().unwrap().focus = EditField::Due;
                    return Ok(EventOutcome::Consumed);
                }
            };
            let scheduled = match parse_optional_date(&popup.scheduled.text, ctx.today) {
                Ok(v) => v,
                Err(e) => {
                    self.popup.as_mut().unwrap().error = Some(format!("scheduled: {e}"));
                    self.popup.as_mut().unwrap().focus = EditField::Scheduled;
                    return Ok(EventOutcome::Consumed);
                }
            };
            let priority = match parse_priority(&popup.priority.text) {
                Ok(v) => v,
                Err(e) => {
                    self.popup.as_mut().unwrap().error = Some(e);
                    self.popup.as_mut().unwrap().focus = EditField::Priority;
                    return Ok(EventOutcome::Consumed);
                }
            };
            let recurrence = popup.recurrence.text.trim();
            let recurrence = (!recurrence.is_empty()).then(|| recurrence.to_string());
            let raw_description = popup.description.text.trim().to_string();
            let tags = parse_tags_field(&popup.tags.text);
            // Description carries inline `#tag` words; rewrite it so the
            // popup's tag field is the source of truth on save. Without this
            // `t.tags = ...` is a no-op (tags are re-derived from the
            // description on parse).
            let description = merge_tags_into_description(&raw_description, &tags);
            (
                (description, due, scheduled, priority, tags, recurrence),
                popup.mode.clone(),
            )
        };

        match mode {
            PopupMode::Edit => self.submit_popup_edit(ctx, validated),
            PopupMode::New => self.submit_popup_new(ctx, validated),
        }
    }

    fn submit_popup_edit(
        &mut self,
        ctx: &mut TabCtx,
        validated: PopupFields,
    ) -> Result<EventOutcome> {
        let format = ctx.vault.task_format();
        let outcome = self.with_selected_task(ctx, |path, task, _today| {
            let (description, due, scheduled, priority, tags, recurrence) = validated;
            ops::update_task_line(path, task.source_line, format, Some(task), move |t| {
                t.description = description;
                t.due = due;
                t.scheduled = scheduled;
                t.priority = priority;
                t.tags = tags;
                t.recurrence = recurrence;
            })?;
            Ok(())
        })?;
        self.popup = None;
        Ok(outcome)
    }

    fn submit_popup_new(
        &mut self,
        ctx: &mut TabCtx,
        validated: PopupFields,
    ) -> Result<EventOutcome> {
        let (description, due, scheduled, priority, tags, recurrence) = validated;
        if description.trim().is_empty() {
            self.popup.as_mut().unwrap().error = Some("description is empty".into());
            self.popup.as_mut().unwrap().focus = EditField::Description;
            return Ok(EventOutcome::Consumed);
        }

        // Parse the target field: supports `Path` and `Path#heading text`.
        // The optional `#heading` part translates to a `Position::UnderHeading`
        // write — letting users seed the new task into a specific section
        // without leaving the popup.
        let target_raw = self.popup.as_ref().unwrap().target.text.trim().to_string();
        let (target_path, heading): (Option<std::path::PathBuf>, Option<String>) =
            if target_raw.is_empty() {
                (None, None)
            } else {
                let q = ft_core::search::Query::parse(&target_raw);
                let path = if q.file_part.is_empty() {
                    None
                } else {
                    Some(std::path::PathBuf::from(&q.file_part))
                };
                (path, q.heading_part)
            };

        // In subtask mode the parent's file + indented position win over the
        // target field (the field is kept for parity but ignored here).
        let subtask = self.subtask_parent.clone();
        let (resolved, position) = match &subtask {
            Some((pfile, pline)) => (
                ctx.vault.path.join(pfile),
                ops::Position::Subtask {
                    parent_line: *pline,
                },
            ),
            None => {
                let (today_n, now_n) = ft_core::dates::now_pair();
                let resolved =
                    match ctx
                        .vault
                        .ensure_target(ctx.today, target_path.as_deref(), today_n, now_n)
                    {
                        Ok(p) => p,
                        Err(e) => {
                            self.popup.as_mut().unwrap().error = Some(e.to_string());
                            self.popup.as_mut().unwrap().focus = EditField::Target;
                            return Ok(EventOutcome::Consumed);
                        }
                    };
                let position = match &heading {
                    Some(h) => ops::Position::UnderHeading(h.clone()),
                    None => ops::auto_position(
                        &resolved,
                        ctx.vault.config.config.tasks.default_section.as_deref(),
                    ),
                };
                (resolved, position)
            }
        };

        let input = CreateInput {
            description,
            status: ft_core::task::Status::Open,
            priority,
            tags,
            created: None,
            start: None,
            scheduled,
            due,
            recurrence,
            id: None,
            depends_on: Vec::new(),
        };

        // Capture prior selection so a create that doesn't pass the filter
        // keeps the cursor where it was.
        let prior = self
            .selected_task_idx()
            .map(|i| (self.tasks[i].source_file.clone(), self.tasks[i].source_line));

        match ops::create_task(
            &resolved,
            ctx.vault.task_format(),
            input,
            ops::CreateOptions {
                position,
                force: false,
            },
        ) {
            Ok(outcome) => {
                self.popup = None;
                self.subtask_parent = None;
                if let Some(key) = subtask {
                    self.expanded.insert(key);
                }
                let rel_target = resolved
                    .strip_prefix(&ctx.vault.path)
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|_| resolved.clone());
                self.refresh_and_anchor_to_create(ctx, (rel_target.clone(), outcome.line), prior)?;
                *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast {
                    text: format!("created {}:{}", rel_target.display(), outcome.line),
                    style: ToastStyle::Success,
                });
                Ok(EventOutcome::Consumed)
            }
            Err(ops::CreateError::Duplicate { path, line }) => {
                let rel = path.strip_prefix(&ctx.vault.path).unwrap_or(&path);
                self.popup.as_mut().unwrap().error =
                    Some(format!("duplicate exists at {}:{line}", rel.display()));
                Ok(EventOutcome::Consumed)
            }
            Err(e) => {
                self.popup = None;
                *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast {
                    text: format!("create failed: {e}"),
                    style: ToastStyle::Error,
                });
                Ok(EventOutcome::Consumed)
            }
        }
    }

    fn handle_edit_key(&mut self, k: KeyEvent, ctx: &mut TabCtx) -> EventOutcome {
        match k.code {
            KeyCode::Esc => {
                self.cancel_edit();
            }
            KeyCode::Enter => {
                self.apply_edit(ctx);
            }
            _ => {
                if let Some(b) = self.edit_state.as_mut() {
                    let _ = b.handle_event(k);
                }
            }
        }
        EventOutcome::Consumed
    }

    // ── quickline (new task) ───────────────────────────────────────────

    fn handle_quickline_key(&mut self, k: KeyEvent, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Some(ql) = self.quickline.as_mut() else {
            return Ok(EventOutcome::Consumed);
        };

        // Quickline-specific chords: Esc/Enter/Ctrl+E. Everything else
        // is forwarded to the buffer's EDIT_KEYMAP, which clears the
        // error on any text mutation. Cursor-only moves preserve the
        // error so a stale message stays visible until the user retypes.
        match (k.code, k.modifiers) {
            (KeyCode::Esc, _) => {
                self.quickline = None;
                self.subtask_parent = None;
            }
            (KeyCode::Enter, _) => {
                return self.submit_quickline(ctx);
            }
            (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                // Expand to the full form. Parse what's in the quickline
                // so the popup opens already pre-populated; close the
                // quickline panel so its keys stop firing.
                let parse = parse_quickline(&ql.input.text, ctx.today);
                self.popup = Some(EditPopup::from_quickline(&parse));
                self.quickline = None;
            }
            _ => {
                let before = ql.input.text.clone();
                let _ = ql.input.handle_event(k);
                if ql.input.text != before {
                    ql.error = None;
                }
            }
        }
        Ok(EventOutcome::Consumed)
    }

    fn submit_quickline(&mut self, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Some(ql) = self.quickline.as_ref() else {
            return Ok(EventOutcome::Consumed);
        };
        let parse = parse_quickline(&ql.input.text, ctx.today);

        // Parse errors block the write; the preview already shows the
        // first error, but we copy it into the post-submit slot so the
        // user gets the same red `⚠` banner whether the failure was at
        // parse time or write time.
        if !parse.errors.is_empty() {
            self.quickline.as_mut().unwrap().error = Some(parse.errors[0].clone());
            return Ok(EventOutcome::Consumed);
        }
        if parse.description.trim().is_empty() {
            self.quickline.as_mut().unwrap().error = Some("description is empty".into());
            return Ok(EventOutcome::Consumed);
        }

        // Subtask mode forces the parent's file + an indented position;
        // otherwise resolve the quickline's own target field.
        let subtask = self.subtask_parent.clone();
        let (target, position) = match &subtask {
            Some((pfile, pline)) => (
                ctx.vault.path.join(pfile),
                ops::Position::Subtask {
                    parent_line: *pline,
                },
            ),
            None => {
                let (today_n, now_n) = ft_core::dates::now_pair();
                let t = match ctx.vault.ensure_target(
                    ctx.today,
                    parse.target.as_deref(),
                    today_n,
                    now_n,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        self.quickline.as_mut().unwrap().error = Some(e.to_string());
                        return Ok(EventOutcome::Consumed);
                    }
                };
                let position = ops::auto_position(
                    &t,
                    ctx.vault.config.config.tasks.default_section.as_deref(),
                );
                (t, position)
            }
        };

        let input = CreateInput {
            description: parse.description.clone(),
            status: ft_core::task::Status::Open,
            priority: parse.priority,
            tags: parse.tags.clone(),
            created: None,
            start: parse.start,
            scheduled: parse.scheduled,
            due: parse.due,
            recurrence: parse.recurrence.clone(),
            id: parse.id.clone(),
            depends_on: Vec::new(),
        };

        // Capture the prior cursor (if any) so a create that doesn't pass
        // the active filter can fall back to "stay where you were".
        let prior = self
            .selected_task_idx()
            .map(|i| (self.tasks[i].source_file.clone(), self.tasks[i].source_line));

        match ops::create_task(
            &target,
            ctx.vault.task_format(),
            input,
            ops::CreateOptions {
                position,
                force: false,
            },
        ) {
            Ok(outcome) => {
                self.quickline = None;
                self.subtask_parent = None;
                // Auto-expand the parent so the freshly created subtask is
                // visible and the cursor can anchor onto it.
                if let Some(key) = subtask {
                    self.expanded.insert(key);
                }
                let rel_target = target
                    .strip_prefix(&ctx.vault.path)
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|_| target.clone());
                self.refresh_and_anchor_to_create(ctx, (rel_target.clone(), outcome.line), prior)?;
                *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast {
                    text: format!("created {}:{}", rel_target.display(), outcome.line),
                    style: ToastStyle::Success,
                });
                Ok(EventOutcome::Consumed)
            }
            Err(ops::CreateError::Duplicate { path, line }) => {
                let rel = path.strip_prefix(&ctx.vault.path).unwrap_or(&path);
                self.quickline.as_mut().unwrap().error =
                    Some(format!("duplicate exists at {}:{line}", rel.display()));
                Ok(EventOutcome::Consumed)
            }
            Err(e) => {
                // Non-recoverable error (IO failure, etc.) — close the
                // panel and surface it as a red status-bar toast so the
                // user can act on it without staring at a panel they
                // can't fix from inside the quickline.
                self.quickline = None;
                *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast {
                    text: format!("create failed: {e}"),
                    style: ToastStyle::Error,
                });
                Ok(EventOutcome::Consumed)
            }
        }
    }
}

/// Build the preview line shown beneath the quickline input. Three states:
/// (1) post-submit error or parse error → red `⚠ <msg>`, (2) empty input →
/// dim hint, (3) parsed cleanly → the same emoji-format line `create_task`
/// would write, plus a `→ <target>` indicator on the right.
fn build_quickline_preview<'a>(ql: &Quickline, ctx: &TabCtx) -> Line<'a> {
    // Surfaced submit error (duplicate, IO) takes precedence so the user
    // sees the most recent failure instead of the live parse preview.
    if let Some(err) = &ql.error {
        return Line::from(vec![
            Span::styled("  ⚠ ", Style::default().fg(palette::ERROR)),
            Span::styled(err.clone(), Style::default().fg(palette::ERROR)),
        ]);
    }

    if ql.input.text.trim().is_empty() {
        return Line::from(Span::styled(
            "  Enter to save · Esc to cancel",
            Style::default()
                .fg(palette::DIM)
                .add_modifier(Modifier::ITALIC),
        ));
    }

    let parse = parse_quickline(&ql.input.text, ctx.today);
    if let Some(first) = parse.errors.first() {
        return Line::from(vec![
            Span::styled("  ⚠ ", Style::default().fg(palette::ERROR)),
            Span::styled(first.clone(), Style::default().fg(palette::ERROR)),
        ]);
    }

    let task = ops::build_task(&CreateInput {
        description: parse.description.clone(),
        status: Status::Open,
        priority: parse.priority,
        tags: parse.tags.clone(),
        created: None,
        start: parse.start,
        scheduled: parse.scheduled,
        due: parse.due,
        recurrence: parse.recurrence.clone(),
        id: parse.id.clone(),
        depends_on: Vec::new(),
    });
    use ft_core::task::{emoji::EmojiFormat, format::TaskFormat};
    let serialized = EmojiFormat.serialize_line(&task);

    // Target: shown on the right in dim text. We don't resolve to an
    // absolute path here — the relative `in:` value (or the daily-note
    // basename) is more useful than `/Users/.../Inbox.md`.
    let target_label = match &parse.target {
        Some(p) => p.display().to_string(),
        None => match ctx
            .vault
            .resolve_target(ctx.today, None)
            .ok()
            .and_then(|p| {
                p.strip_prefix(&ctx.vault.path)
                    .ok()
                    .map(|x| x.to_path_buf())
            }) {
            Some(p) => p.display().to_string(),
            None => "<daily note>".into(),
        },
    };

    Line::from(vec![
        Span::styled("  → ", Style::default().fg(palette::DIM)),
        Span::styled(serialized, Style::default().fg(palette::WHITE)),
        Span::styled(
            format!("   → {target_label}"),
            Style::default().fg(palette::DIM),
        ),
    ])
}

// --- row formatting ----------------------------------------------------------

/// Format a date relative to `today`. Near-term dates get human-readable
/// labels; dates further out fall back to ISO for precision.
fn divider_line(label: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {label}"),
        Style::default()
            .fg(palette::DIM)
            .add_modifier(Modifier::BOLD),
    ))
}

fn task_line(
    task: &Task,
    today: NaiveDate,
    selected: bool,
    desc_width: usize,
    row: &DisplayRow,
) -> Line<'static> {
    let pri_label = priority_label(task.priority);
    let pri_color = priority_color(task.priority);
    let (status_glyph, status_color) = status_marker(task.status);

    let due_str = task.due.map(|d| relative_date(d, today));
    let due_color = task
        .due
        .map(|d| {
            if d < today {
                palette::ERROR
            } else {
                palette::WHITE
            }
        })
        .unwrap_or(palette::DIM);

    let scheduled_str = task.scheduled.map(|d| relative_date(d, today));

    let cursor = if selected { "▶ " } else { "  " };
    let pri_text = if pri_label.is_empty() {
        "    ".to_string()
    } else {
        format!("{:<3} ", pri_label)
    };
    let status_text = format!("{status_glyph} ");

    // Tree affordance: indent per depth, then a ▾/▸ for expandable rows or a
    // blank gutter so leaf descriptions still line up under their siblings.
    let marker = match (row.has_children, row.expanded) {
        (true, true) => "▾ ",
        (true, false) => "▸ ",
        (false, _) if row.depth > 0 => "· ",
        (false, _) => "",
    };
    let tree_prefix = format!("{}{marker}", "  ".repeat(row.depth));

    // Description: truncate if too long, pad to fill the column budget.
    let desc = format!("{tree_prefix}{}", task.description.replace('\n', " "));
    let desc_count = desc.chars().count();
    let desc_trimmed = if desc_count > desc_width {
        let cut: String = desc.chars().take(desc_width.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        desc
    };
    let desc_padded = format!("{:<width$}", desc_trimmed, width = desc_width);

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(9);
    // Non-selected done/cancelled rows fade.
    let terminal_status = matches!(task.status, Status::Done | Status::Cancelled);
    let row_style = if selected {
        Style::default()
            .bg(Color::Rgb(50, 38, 30))
            .add_modifier(Modifier::BOLD)
    } else if terminal_status {
        Style::default().add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };

    spans.push(Span::styled(cursor.to_string(), row_style));
    spans.push(Span::styled(status_text, row_style.fg(status_color)));
    spans.push(Span::styled(pri_text, row_style.fg(pri_color)));
    spans.push(Span::styled(desc_padded, row_style));

    // Due and scheduled in fixed-width columns so the 📅 / ⏳ emoji align.
    if let Some(due) = &due_str {
        let cell = format!(" 📅 {:<10}", due);
        spans.push(Span::styled(cell, row_style.fg(due_color)));
    } else {
        spans.push(Span::styled("             ", row_style));
    }
    if let Some(sch) = &scheduled_str {
        let cell = format!(" ⏳ {:<10}", sch);
        spans.push(Span::styled(cell, row_style.fg(palette::PRIMARY)));
    }
    Line::from(spans)
}

/// Single-char status glyph + color. Open is a blank space so the row reads
/// uncluttered when the default `not done` query is active and every row is
/// open anyway; non-open statuses are immediately visible.
fn status_marker(status: Status) -> (&'static str, Color) {
    match status {
        Status::Open => (" ", palette::DIM),
        Status::Done => ("✓", palette::SUCCESS),
        Status::Cancelled => ("✗", palette::ERROR),
        Status::InProgress => ("▷", palette::SECONDARY),
    }
}

fn priority_label(p: Option<Priority>) -> &'static str {
    match p {
        Some(Priority::Highest) => "!!!",
        Some(Priority::High) => "!!",
        Some(Priority::Medium) => "!",
        Some(Priority::Low) => "v",
        Some(Priority::Lowest) => "vv",
        None => "",
    }
}

fn priority_color(p: Option<Priority>) -> Color {
    match p {
        Some(Priority::Highest | Priority::High) => palette::ERROR,
        Some(Priority::Medium) => palette::SECONDARY,
        Some(Priority::Low | Priority::Lowest) => palette::PRIMARY,
        None => palette::DIM,
    }
}

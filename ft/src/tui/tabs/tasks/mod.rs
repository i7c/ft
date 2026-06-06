mod quickline;
mod search;
mod view;

use std::sync::LazyLock;

use anyhow::Result;
use chrono::{DateTime, Local};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::{
    command::{ArgSpec, Command, CommandDef, CommandOutcome, CommandScope},
    event::Event,
    help::HelpSection,
    keymap::{KeyChord, KeyMap},
    palette,
    tab::{EventOutcome, Tab, TabCtx},
};

use search::SearchView;
use view::View;

// ── Commands ─────────────────────────────────────────────────────────

/// Every command the Tasks tab exposes. Includes both the tab-level
/// sidebar commands (handled by `TasksTab::dispatch_command` directly)
/// and the SearchView commands (delegated to the active view's
/// `dispatch_command`). Pre-declared here so the build-time
/// `CommandRegistry` sees the full surface in one slice.
pub(crate) static TASKS_COMMANDS: &[CommandDef] = &[
    // Tab-level: sidebar view selection.
    CommandDef {
        name: "tasks.select-prev-view",
        description: "Select the previous view in the sidebar",
        scope: CommandScope::Tab("tasks"),
        group: "Sidebar",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.select-next-view",
        description: "Select the next view in the sidebar",
        scope: CommandScope::Tab("tasks"),
        group: "Sidebar",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.confirm-view",
        description: "Confirm the sidebar's selected view (no-op for now)",
        scope: CommandScope::Tab("tasks"),
        group: "Sidebar",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    // SearchView: see `tabs/tasks/search.rs::SEARCH_COMMANDS` for the
    // canonical declarations. They're collected into this single slice
    // so the registry sees them.
    CommandDef {
        name: "tasks.edit-query",
        description: "Open the query editor",
        scope: CommandScope::Tab("tasks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.cursor-up",
        description: "Move the cursor up one task",
        scope: CommandScope::Tab("tasks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.cursor-down",
        description: "Move the cursor down one task",
        scope: CommandScope::Tab("tasks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.reload",
        description: "Reload the task list from the vault",
        scope: CommandScope::Tab("tasks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.open-in-editor",
        description: "Open the selected task in $EDITOR",
        scope: CommandScope::Tab("tasks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.due-next-day",
        description: "Bump the due date forward by one day",
        scope: CommandScope::Tab("tasks"),
        group: "Mutations",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.due-prev-day",
        description: "Bump the due date back by one day",
        scope: CommandScope::Tab("tasks"),
        group: "Mutations",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.scheduled-next-day",
        description: "Bump the scheduled date forward by one day",
        scope: CommandScope::Tab("tasks"),
        group: "Mutations",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.scheduled-prev-day",
        description: "Bump the scheduled date back by one day",
        scope: CommandScope::Tab("tasks"),
        group: "Mutations",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.priority-next",
        description: "Cycle priority forward",
        scope: CommandScope::Tab("tasks"),
        group: "Mutations",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.priority-prev",
        description: "Cycle priority back",
        scope: CommandScope::Tab("tasks"),
        group: "Mutations",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.complete",
        description: "Complete the selected task",
        scope: CommandScope::Tab("tasks"),
        group: "Mutations",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    // §9.4/§9.5 — first headless-handled command. Reachable from
    // `ft do tasks.complete-by-id --arg id=<id>` (see cmd/do.rs);
    // not bound to a chord in the TUI (cursor-driven completion is
    // `tasks.complete`).
    CommandDef {
        name: "tasks.complete-by-id",
        description: "Complete the task with the given id (headless)",
        scope: CommandScope::Tab("tasks"),
        group: "Mutations",
        args_schema: &[ArgSpec {
            name: "id",
            description: "Task id (the `🆔 xyz123` suffix)",
            required: true,
        }],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.cancel",
        description: "Cancel the selected task",
        scope: CommandScope::Tab("tasks"),
        group: "Mutations",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.due-today",
        description: "Set due date to today",
        scope: CommandScope::Tab("tasks"),
        group: "Mutations",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.edit-popup",
        description: "Open the edit-popup for the selected task",
        scope: CommandScope::Tab("tasks"),
        group: "Create / edit",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.quickline",
        description: "Open the quickline new-task entry",
        scope: CommandScope::Tab("tasks"),
        group: "Create / edit",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.new-blank-form",
        description: "Open the new-task form (blank)",
        scope: CommandScope::Tab("tasks"),
        group: "Create / edit",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
];

/// Tab-level keymap — sidebar navigation only. Looked up when the
/// active view returns `NotHandled` for an event (mirrors the
/// pre-migration fall-through). The view-level keymap lives in
/// `search::SEARCH_KEYMAP`.
pub(crate) static TASKS_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Up", "tasks.select-prev-view")
        .bind("Down", "tasks.select-next-view")
        .bind("Enter", "tasks.confirm-view")
});

/// Function pointer for "what time is it now?". Production uses
/// [`Local::now`]; tests inject a fixed value for deterministic snapshots.
pub type ClockFn = fn() -> DateTime<Local>;

fn local_now() -> DateTime<Local> {
    Local::now()
}

const SIDEBAR_WIDTH: u16 = 24;

pub struct TasksTab {
    views: Vec<Box<dyn View>>,
    active_view: usize,
    clock: ClockFn,
    keymap: crate::tui::keymap::KeyMap,
}

impl TasksTab {
    pub fn new() -> Self {
        Self::with_clock(local_now)
    }

    pub fn with_clock(clock: ClockFn) -> Self {
        let views: Vec<Box<dyn View>> = vec![Box::new(SearchView::new())];
        Self {
            views,
            active_view: 0,
            clock,
            keymap: TASKS_KEYMAP.clone(),
        }
    }

    pub fn with_keymap_overlay(mut self, overlay: &crate::tui::keymap::KeymapOverlay) -> Self {
        self.keymap = TASKS_KEYMAP.with_overlay(overlay);
        self
    }

    fn select_prev_view(&mut self) {
        if self.views.is_empty() {
            return;
        }
        if self.active_view == 0 {
            self.active_view = self.views.len() - 1;
        } else {
            self.active_view -= 1;
        }
    }

    fn select_next_view(&mut self) {
        if self.views.is_empty() {
            return;
        }
        self.active_view = (self.active_view + 1) % self.views.len();
    }

    fn render_sidebar(&self, frame: &mut Frame, area: Rect) {
        let now = (self.clock)();
        let date = now.format("%a %d %b").to_string();
        let time = now.format("%H:%M:%S").to_string();

        let mut lines: Vec<Line> = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!(" {date}"),
                Style::default().fg(palette::WHITE),
            )),
            Line::from(Span::styled(
                format!(" {time}"),
                Style::default()
                    .fg(palette::PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                " ── views ──",
                Style::default().fg(palette::DIM),
            )),
        ];

        for (i, v) in self.views.iter().enumerate() {
            let (marker, style) = if i == self.active_view {
                (
                    " ▶ ",
                    Style::default()
                        .fg(palette::PRIMARY)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("   ", Style::default().fg(palette::WHITE))
            };
            lines.push(Line::from(vec![
                Span::raw(marker),
                Span::styled(v.title().to_string(), style),
            ]));
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" sidebar ")
            .border_style(Style::default().fg(palette::DIM));
        let para = Paragraph::new(lines).block(block);
        frame.render_widget(para, area);
    }

    fn render_viewport(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        if let Some(v) = self.views.get_mut(self.active_view) {
            v.render(frame, area, ctx);
        }
    }
}

impl Tab for TasksTab {
    fn title(&self) -> &str {
        "Tasks"
    }

    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()> {
        if let Some(v) = self.views.get_mut(self.active_view) {
            v.on_focus(ctx)?;
        }
        Ok(())
    }

    fn commands(&self) -> &'static [CommandDef] {
        TASKS_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &self.keymap
    }

    fn dispatch_command(&mut self, cmd: &Command, _ctx: &mut TabCtx) -> CommandOutcome {
        // Tab-level commands only — view-level (`tasks.cursor-up` etc.)
        // are dispatched by SearchView before the keymap fall-through
        // reaches the tab. `_ctx` is unused because the sidebar
        // selection is purely tab-local state.
        match cmd.name {
            "tasks.select-prev-view" => {
                self.select_prev_view();
                CommandOutcome::Handled
            }
            "tasks.select-next-view" => {
                self.select_next_view();
                CommandOutcome::Handled
            }
            "tasks.confirm-view" => {
                // No-op for now; the sidebar dropdown is in-place
                // selection, not a confirm-then-apply flow.
                CommandOutcome::Handled
            }
            _ => CommandOutcome::NotHandled,
        }
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        // The active view gets first dibs — its selection model owns the same
        // keys (↑/↓/Enter) as the sidebar dropdown. The dropdown only handles
        // these keys when the view returns NotHandled (e.g. while the search
        // list is empty or the view has no opinion).
        let view_outcome = if let Some(v) = self.views.get_mut(self.active_view) {
            v.handle_event(ev.clone(), ctx)?
        } else {
            EventOutcome::NotHandled
        };
        if view_outcome != EventOutcome::NotHandled {
            return Ok(view_outcome);
        }

        // View didn't handle — try the tab-level keymap (sidebar).
        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };
        let chord = KeyChord::from_key_event(k);
        let Some(cmd) = self.keymap.lookup(chord).cloned() else {
            return Ok(EventOutcome::NotHandled);
        };
        Ok(match self.dispatch_command(&cmd, ctx) {
            CommandOutcome::Handled => EventOutcome::Consumed,
            CommandOutcome::NotHandled => EventOutcome::NotHandled,
        })
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(1)])
            .split(area);

        self.render_sidebar(frame, chunks[0]);
        self.render_viewport(frame, chunks[1], ctx);
    }

    fn refresh(&mut self, ctx: &mut TabCtx) -> Result<()> {
        if let Some(v) = self.views.get_mut(self.active_view) {
            v.refresh(ctx)?;
        }
        Ok(())
    }

    fn help_sections(&self) -> Vec<HelpSection> {
        vec![
            HelpSection::new(
                "Navigation",
                &[
                    ("↑ / ↓ · j / k", "select prev / next task"),
                    ("/", "edit query"),
                    ("R", "reload vault"),
                    ("Enter", "open task in $EDITOR"),
                ],
            ),
            HelpSection::new(
                "Mutations",
                &[
                    ("] / [", "due date +1d / -1d"),
                    ("} / {", "scheduled +1d / -1d"),
                    ("t", "set due to today"),
                    ("p / P", "priority cycle fwd / back"),
                    ("x / X", "complete / cancel"),
                ],
            ),
            HelpSection::new(
                "Create / edit",
                &[
                    ("c", "new task (quickline)"),
                    ("Shift+C", "new task (blank form)"),
                    ("e", "open edit popup"),
                    ("Ctrl+E", "expand quickline → form"),
                    ("Ctrl+S", "submit form"),
                    ("Tab / Shift+Tab", "next / prev field (form)"),
                    ("Enter (target)", "open file/heading picker"),
                ],
            ),
            HelpSection::new(
                "Text input",
                &[
                    ("← / →", "move cursor"),
                    ("Home / End", "jump to start / end"),
                    ("Ctrl+W / Ctrl+⌫", "delete previous word"),
                    ("Esc", "cancel input / close overlay"),
                ],
            ),
        ]
    }
}

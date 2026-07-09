pub(crate) mod edit_popup;
pub(crate) mod modals;
mod quickline;
mod search;
mod view;

use std::sync::LazyLock;

use anyhow::Result;
use ratatui::{layout::Rect, Frame};

use crate::tui::{
    command::{ArgSpec, Command, CommandDef, CommandOutcome, CommandScope},
    event::Event,
    help::HelpSection,
    keymap::KeyMap,
    tab::{EventOutcome, Tab, TabCtx, TabKind, TasksRequest},
};

/// Empty tab-level keymap (sidebar removed). Kept as a named static so
/// downstream sites that reference it (`app.rs`, `mod.rs`) compile without
/// change. The tab delegates all keys to the active view.
pub(crate) static TASKS_KEYMAP: LazyLock<KeyMap> = LazyLock::new(KeyMap::empty);

use search::SearchView;
use view::View;

// ── Commands ─────────────────────────────────────────────────────────

/// Every command the Tasks tab exposes. Pre-declared here so the
/// build-time `CommandRegistry` sees the full surface in one slice.
pub(crate) static TASKS_COMMANDS: &[CommandDef] = &[
    // SearchView commands (see `tabs/tasks/search.rs` for implementation).
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
        name: "tasks.preset-pick",
        description: "Load a task preset into the active query",
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
        name: "tasks.expand",
        description: "Expand the selected task's subtasks",
        scope: CommandScope::Tab("tasks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.collapse",
        description: "Collapse the selected task's subtasks",
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
        name: "tasks.cancel-by-id",
        description: "Cancel the task with the given id (headless)",
        scope: CommandScope::Tab("tasks"),
        group: "Mutations",
        args_schema: &[
            ArgSpec {
                name: "id",
                description: "Task id (the `🆔 xyz123` suffix)",
                required: true,
            },
            ArgSpec {
                name: "on",
                description: "Cancellation date (YYYY-MM-DD; defaults to today)",
                required: false,
            },
        ],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "tasks.edit-by-id",
        description: "Edit the task with the given id's fields (headless)",
        scope: CommandScope::Tab("tasks"),
        group: "Mutations",
        args_schema: &[
            ArgSpec {
                name: "id",
                description: "Task id (the `🆔 xyz123` suffix)",
                required: true,
            },
            ArgSpec {
                name: "due",
                description: "Set due date (YYYY-MM-DD or `none` to clear)",
                required: false,
            },
            ArgSpec {
                name: "scheduled",
                description: "Set scheduled date (YYYY-MM-DD or `none` to clear)",
                required: false,
            },
            ArgSpec {
                name: "priority",
                description: "Set priority (highest/high/medium/low/lowest or `none`)",
                required: false,
            },
            ArgSpec {
                name: "description",
                description: "Set the description text",
                required: false,
            },
        ],
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
    CommandDef {
        name: "tasks.new-subtask",
        description: "Create a subtask under the selected task (quickline)",
        scope: CommandScope::Tab("tasks"),
        group: "Create / edit",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
];

pub struct TasksTab {
    /// Views vec and active_view kept for future multi-view expansion
    /// (the sidebar was removed but the View trait abstraction remains).
    views: Vec<Box<dyn View>>,
    active_view: usize,
}

impl TasksTab {
    pub fn new() -> Self {
        let views: Vec<Box<dyn View>> = vec![Box::new(SearchView::new())];
        Self {
            views,
            active_view: 0,
        }
    }

    pub fn with_keymap_overlay(self, _overlay: &crate::tui::keymap::KeymapOverlay) -> Self {
        // Keymap overlay kept for API compatibility; the sidebar keymap
        // (which used it) is removed.
        self
    }
}

impl Tab for TasksTab {
    fn title(&self) -> &str {
        "Tasks"
    }

    fn kind(&self) -> TabKind {
        TabKind::Tasks
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
        &TASKS_KEYMAP
    }

    fn dispatch_command(&mut self, _cmd: &Command, _ctx: &mut TabCtx) -> CommandOutcome {
        // No tab-level commands — all go to the active view.
        CommandOutcome::NotHandled
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        // The sidebar is gone; route every event directly to the active view.
        if let Some(v) = self.views.get_mut(self.active_view) {
            v.handle_event(ev, ctx)
        } else {
            Ok(EventOutcome::NotHandled)
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        // Full-width viewport — no sidebar split.
        if let Some(v) = self.views.get_mut(self.active_view) {
            v.render(frame, area, ctx);
        }
    }

    fn refresh(&mut self, ctx: &mut TabCtx) -> Result<()> {
        if let Some(v) = self.views.get_mut(self.active_view) {
            v.refresh(ctx)?;
        }
        Ok(())
    }

    fn on_graph_ready(&mut self, ctx: &mut TabCtx) {
        if let Some(v) = self.views.get_mut(self.active_view) {
            v.on_graph_ready(ctx);
        }
    }

    fn handle_tasks_request(&mut self, req: TasksRequest, ctx: &mut TabCtx) {
        match req {
            TasksRequest::ApplyPreset(dsl) => {
                if let Some(v) = self.views.get_mut(self.active_view) {
                    v.apply_preset(&dsl, ctx.today);
                }
            }
        }
    }

    fn help_sections(&self) -> Vec<HelpSection> {
        vec![
            HelpSection::new(
                "Navigation",
                &[
                    ("↑ / ↓ · j / k", "select prev / next task"),
                    ("→ / l · ← / h", "expand / collapse subtasks"),
                    ("/", "edit query"),
                    ("Ctrl+P", "load preset into query"),
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
                    ("s", "new subtask of selected"),
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

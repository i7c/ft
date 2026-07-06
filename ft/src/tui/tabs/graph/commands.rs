//! Command registry rows + default keymap for the Graph tab, plus
//! the tab's query constants.

use super::*;

/// Fallback query the first view of the graph tab seeds itself with on
/// first focus when `[graph].default_query` isn't set in config. Shows
/// the vault root as a single directory line — pressing Enter / `l`
/// expands one hop. Kept here (and not in `ft-core`) because it's a
/// TUI-presentation default, not an engine concern.
pub(super) const BUILTIN_DEFAULT_QUERY: &str = concat!(
    "node where path = \"\"; ",
    "expand where edge.kind in {directory-contains, note-link};",
);

/// Width budget for a view's tab-strip label query snippet, in characters.
pub(super) const VIEW_LABEL_QUERY_WIDTH: usize = 20;

// ── Commands ─────────────────────────────────────────────────────────

/// Every command the Graph tab exposes through the command/keymap
/// layer. Modal-launch commands (`graph.create-blank`, `graph.append`,
/// `graph.quick-capture`, `graph.move`, `graph.rename`, `graph.related`,
/// `graph.search`, `graph.preset-pick`) are tagged `opens_modal: true`
/// — `ft do` rejects them since they need interactive input.
pub(crate) static GRAPH_COMMANDS: &[CommandDef] = &[
    // Multi-view bindings
    CommandDef {
        name: "graph.add-view",
        description: "Add a new view (pick preset or blank)",
        scope: CommandScope::Tab("graph"),
        group: "Views",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.preset-pick",
        description: "Load a preset into the active view",
        scope: CommandScope::Tab("graph"),
        group: "Views",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.close-view",
        description: "Close the active view",
        scope: CommandScope::Tab("graph"),
        group: "Views",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.next-view",
        description: "Switch to the next view",
        scope: CommandScope::Tab("graph"),
        group: "Views",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.prev-view",
        description: "Switch to the previous view",
        scope: CommandScope::Tab("graph"),
        group: "Views",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.switch-view",
        description: "Switch to the view at the given 0-based index",
        scope: CommandScope::Tab("graph"),
        group: "Views",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.related",
        description: "Open the Related panel for the selected note",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.journal",
        description: "Open the Journal tab for the selected note or ghost",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.add-to-journal-sources",
        description: "Append selected (or cursor) Note/Ghost rows to the Journal tab's sources",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    // Query bar
    CommandDef {
        name: "graph.query-bar",
        description: "Open the query bar to edit the active view's query",
        scope: CommandScope::Tab("graph"),
        group: "Query",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.rewrite-for-root",
        description: "Re-root the active view's query on the selected node",
        scope: CommandScope::Tab("graph"),
        group: "Query",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.search",
        description: "Open the in-tree fuzzy search picker",
        scope: CommandScope::Tab("graph"),
        group: "Query",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    // Navigation
    CommandDef {
        name: "graph.cursor-down",
        description: "Move the cursor down one row",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.cursor-up",
        description: "Move the cursor up one row",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.expand-or-collapse",
        description: "Expand the selected node (or collapse if already expanded)",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.collapse-or-jump-parent",
        description: "Collapse the selected node (or jump to parent)",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.cursor-first",
        description: "Jump to the first row",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.cursor-last",
        description: "Jump to the last row",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.cursor-half-page-down",
        description: "Move the cursor down half a page",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.cursor-half-page-up",
        description: "Move the cursor up half a page",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    // Notes — open / create / append / capture / move / rename
    CommandDef {
        name: "graph.open-in-editor",
        description: "Open the selected note in $EDITOR",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.open-in-obsidian",
        description: "Open the selected note in Obsidian",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.create-blank",
        description: "Create a new note (blank) in the selected folder",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.create-from-template",
        description: "Create a new note from a template",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.promote-ghost",
        description:
            "Promote the selected ghost into a synth note seeded with every paragraph mentioning it",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.append",
        description: "Append a template to the selected note",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.quick-capture",
        description: "Quick capture (run a preset)",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.move",
        description: "Enter the move-section flow (source from selected)",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.rename-or-multi-move",
        description: "Rename the selected node (or move multi-selection)",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.refresh",
        description: "Refresh the graph from disk",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.delete",
        description: "Delete the selected note or directory",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.create-subdir",
        description: "Create a subdirectory under the selected directory",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    // Periodic notes
    CommandDef {
        name: "graph.periodic-leader",
        description: "Navigate to periodic note in graph (then d/w/m/q/y)",
        scope: CommandScope::Tab("graph"),
        group: "Periodic notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.today",
        description: "Navigate to today's daily note in graph",
        scope: CommandScope::Tab("graph"),
        group: "Periodic notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    // Tasks — interaction verbs on focused Task rows
    // (graph-task-interaction §7). All are no-ops (toast) on non-Task rows.
    CommandDef {
        name: "graph.task-complete",
        description: "Complete the focused task",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-cancel",
        description: "Cancel the focused task",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-due-next",
        description: "Advance the focused task's due date by one day",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-due-prev",
        description: "Move the focused task's due date back one day",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-scheduled-next",
        description: "Advance the focused task's scheduled date by one day",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-scheduled-prev",
        description: "Move the focused task's scheduled date back one day",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-priority-next",
        description: "Cycle the focused task's priority up",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-priority-prev",
        description: "Cycle the focused task's priority down",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-due-today",
        description: "Set the focused task's due date to today",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-edit-popup",
        description: "Open the task edit form on the focused task",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-leader",
        description: "Task-create leader (then c=create, s=subtask)",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-create",
        description: "Create a new top-level task (via the task leader)",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-new-subtask",
        description: "Create a subtask under the focused task (via the task leader)",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.tasks-of-note",
        description: "Rewrite the view to the focused note's task subtree",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    // Multi-select
    CommandDef {
        name: "graph.toggle-multi-select",
        description: "Toggle multi-selection on the focused row",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.clear-multi-select",
        description: "Clear the multi-selection (Esc)",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
];

/// Default keymap for the Graph tab. Per-modal flows are routed
/// through the App-level `ActiveModal` slot and bypass this keymap
/// entirely (the modal driver dispatches keys to the modal first).
pub(crate) static GRAPH_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        // Views
        .bind("Ctrl+n", "graph.add-view")
        .bind("Ctrl+p", "graph.preset-pick")
        .bind("Ctrl+w", "graph.close-view")
        .bind("Ctrl+PageDown", "graph.next-view")
        .bind("Ctrl+PageUp", "graph.prev-view")
        // Cross-tab
        .bind("R", "graph.related")
        .bind("J", "graph.journal")
        .bind("Ctrl+j", "graph.add-to-journal-sources")
        // Query bar / search
        .bind("/", "graph.query-bar")
        .bind("z", "graph.rewrite-for-root")
        .bind("f", "graph.search")
        // Navigation — vim + arrow aliases
        .bind("j", "graph.cursor-down")
        .bind("Down", "graph.cursor-down")
        .bind("k", "graph.cursor-up")
        .bind("Up", "graph.cursor-up")
        .bind("Enter", "graph.expand-or-collapse")
        .bind("l", "graph.expand-or-collapse")
        .bind("h", "graph.collapse-or-jump-parent")
        .bind("g", "graph.cursor-first")
        .bind("G", "graph.cursor-last")
        .bind("Ctrl+d", "graph.cursor-half-page-down")
        .bind("Ctrl+u", "graph.cursor-half-page-up")
        // Notes
        .bind("o", "graph.open-in-editor")
        .bind("Ctrl+o", "graph.open-in-obsidian")
        .bind("c", "graph.create-blank")
        .bind("C", "graph.create-from-template")
        .bind("P", "graph.promote-ghost")
        .bind("A", "graph.append")
        .bind("Q", "graph.quick-capture")
        .bind("m", "graph.move")
        .bind("r", "graph.rename-or-multi-move")
        .bind("Ctrl+r", "graph.refresh")
        .bind("d", "graph.delete")
        .bind("n", "graph.create-subdir")
        // Periodic
        .bind("p", "graph.periodic-leader")
        .bind("t", "graph.today")
        // Tasks — interaction verbs on focused Task rows
        // (graph-task-interaction §7). `p`/`t` are taken by the periodic
        // flow, so priority cycles on `=`/`-` and due-today on `T`.
        .bind("x", "graph.task-complete")
        .bind("X", "graph.task-cancel")
        .bind("]", "graph.task-due-next")
        .bind("[", "graph.task-due-prev")
        .bind("}", "graph.task-scheduled-next")
        .bind("{", "graph.task-scheduled-prev")
        .bind("=", "graph.task-priority-next")
        .bind("-", "graph.task-priority-prev")
        .bind("T", "graph.task-due-today")
        .bind("e", "graph.task-edit-popup")
        .bind("a", "graph.task-leader")
        .bind("v", "graph.tasks-of-note")
        // Multi-select
        .bind("Space", "graph.toggle-multi-select")
        .bind("Esc", "graph.clear-multi-select")
        // Alt+1..9 → switch view (with `index` arg)
        .bind_with_args("Alt+1", "graph.switch-view", &[("index", "0")])
        .bind_with_args("Alt+2", "graph.switch-view", &[("index", "1")])
        .bind_with_args("Alt+3", "graph.switch-view", &[("index", "2")])
        .bind_with_args("Alt+4", "graph.switch-view", &[("index", "3")])
        .bind_with_args("Alt+5", "graph.switch-view", &[("index", "4")])
        .bind_with_args("Alt+6", "graph.switch-view", &[("index", "5")])
        .bind_with_args("Alt+7", "graph.switch-view", &[("index", "6")])
        .bind_with_args("Alt+8", "graph.switch-view", &[("index", "7")])
        .bind_with_args("Alt+9", "graph.switch-view", &[("index", "8")])
});

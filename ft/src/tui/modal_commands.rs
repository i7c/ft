#![allow(dead_code)] // some commands and keymaps land here ahead of their consumers

//! Per-modal commands and keymaps.
//!
//! Each [`ActiveModal`](crate::tui::modal::ActiveModal) variant
//! contributes a `<MODAL>_COMMANDS` slice and (where the bindings are
//! stable across the modal's internal state) a `<MODAL>_KEYMAP`. The
//! Modal trait's `commands()` and `keymap()` methods return these so
//! the build-time `CommandRegistry`, the `?` overlay, the docs
//! generator, and `ft commands list` see the modal-level verbs.
//!
//! ## Scoping
//!
//! Command names are prefixed with the modal's [`Modal::name()`] so
//! they're globally unique (the registry panics on duplicates). Two
//! modals that share a verb conceptually (e.g. `confirm` on Create
//! and on Append) get distinct names (`create.confirm`,
//! `append.confirm`); the "generic modal verb" framing from the
//! design doc is reified per-modal, trading naming brevity for
//! registry uniqueness.
//!
//! ## State-machine modals
//!
//! Multi-step flows (Create, Append, SectionMove, MoveOuter) keep
//! the same chord set across their steps (Enter / Esc / arrows /
//! Space) but the chord *means* something different per step.
//! The keymap declares the common chord → command binding; the
//! command's *implementation* (in the existing `handle_event` /
//! per-step handlers) interprets the verb against the current step.

use std::sync::LazyLock;

use crate::tui::command::{CommandDef, CommandScope};
use crate::tui::keymap::KeyMap;

// ── Helpers ──────────────────────────────────────────────────────────

const fn nav_def(name: &'static str, description: &'static str, scope: CommandScope) -> CommandDef {
    CommandDef {
        name,
        description,
        scope,
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    }
}

const fn confirm_def(name: &'static str, scope: CommandScope) -> CommandDef {
    CommandDef {
        name,
        description: "Confirm the current step",
        scope,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    }
}

const fn cancel_def(name: &'static str, scope: CommandScope) -> CommandDef {
    CommandDef {
        name,
        description: "Cancel / step back",
        scope,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    }
}

// ── Create ───────────────────────────────────────────────────────────

const CREATE_SCOPE: CommandScope = CommandScope::Modal("create");

pub static CREATE_COMMANDS: &[CommandDef] = &[
    confirm_def("create.confirm", CREATE_SCOPE),
    cancel_def("create.cancel", CREATE_SCOPE),
    nav_def(
        "create.cursor-up",
        "Select the previous candidate",
        CREATE_SCOPE,
    ),
    nav_def(
        "create.cursor-down",
        "Select the next candidate",
        CREATE_SCOPE,
    ),
];

pub static CREATE_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Enter", "create.confirm")
        .bind("Esc", "create.cancel")
        .bind("Up", "create.cursor-up")
        .bind("Down", "create.cursor-down")
});

// ── Append ───────────────────────────────────────────────────────────

const APPEND_SCOPE: CommandScope = CommandScope::Modal("append");

pub static APPEND_COMMANDS: &[CommandDef] = &[
    confirm_def("append.confirm", APPEND_SCOPE),
    cancel_def("append.cancel", APPEND_SCOPE),
    nav_def(
        "append.cursor-up",
        "Select the previous candidate",
        APPEND_SCOPE,
    ),
    nav_def(
        "append.cursor-down",
        "Select the next candidate",
        APPEND_SCOPE,
    ),
];

pub static APPEND_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Enter", "append.confirm")
        .bind("Esc", "append.cancel")
        .bind("Up", "append.cursor-up")
        .bind("Down", "append.cursor-down")
});

// ── Section move ─────────────────────────────────────────────────────

const SECTION_MOVE_SCOPE: CommandScope = CommandScope::Modal("section-move");

pub static SECTION_MOVE_COMMANDS: &[CommandDef] = &[
    confirm_def("section-move.confirm", SECTION_MOVE_SCOPE),
    cancel_def("section-move.cancel", SECTION_MOVE_SCOPE),
    nav_def(
        "section-move.cursor-up",
        "Focus the previous heading",
        SECTION_MOVE_SCOPE,
    ),
    nav_def(
        "section-move.cursor-down",
        "Focus the next heading",
        SECTION_MOVE_SCOPE,
    ),
    CommandDef {
        name: "section-move.toggle",
        description: "Toggle the focused heading's selection",
        scope: SECTION_MOVE_SCOPE,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
];

pub static SECTION_MOVE_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Enter", "section-move.confirm")
        .bind("Esc", "section-move.cancel")
        .bind("Up", "section-move.cursor-up")
        .bind("Down", "section-move.cursor-down")
        .bind("Space", "section-move.toggle")
});

// ── Capture-var prompt ──────────────────────────────────────────────

const CAPTURE_VAR_SCOPE: CommandScope = CommandScope::Modal("capture-var");

pub static CAPTURE_VAR_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "capture-var.next-or-commit",
        description: "Advance to the next var (or commit the capture)",
        scope: CAPTURE_VAR_SCOPE,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    cancel_def("capture-var.cancel", CAPTURE_VAR_SCOPE),
];

pub static CAPTURE_VAR_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Enter", "capture-var.next-or-commit")
        .bind("Esc", "capture-var.cancel")
});

// ── Periodic leader ─────────────────────────────────────────────────

const PERIODIC_LEADER_SCOPE: CommandScope = CommandScope::Modal("periodic-leader");

pub static PERIODIC_LEADER_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "periodic-leader.daily",
        description: "Open today's daily note",
        scope: PERIODIC_LEADER_SCOPE,
        group: "Periodic",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    CommandDef {
        name: "periodic-leader.weekly",
        description: "Open this week's weekly note",
        scope: PERIODIC_LEADER_SCOPE,
        group: "Periodic",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    CommandDef {
        name: "periodic-leader.monthly",
        description: "Open this month's monthly note",
        scope: PERIODIC_LEADER_SCOPE,
        group: "Periodic",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "periodic-leader.quarterly",
        description: "Open this quarter's quarterly note",
        scope: PERIODIC_LEADER_SCOPE,
        group: "Periodic",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "periodic-leader.yearly",
        description: "Open this year's yearly note",
        scope: PERIODIC_LEADER_SCOPE,
        group: "Periodic",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    cancel_def("periodic-leader.cancel", PERIODIC_LEADER_SCOPE),
];

pub static PERIODIC_LEADER_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("d", "periodic-leader.daily")
        .bind("w", "periodic-leader.weekly")
        .bind("m", "periodic-leader.monthly")
        .bind("q", "periodic-leader.quarterly")
        .bind("y", "periodic-leader.yearly")
        .bind("Esc", "periodic-leader.cancel")
});

// ── Query bar ───────────────────────────────────────────────────────

const QUERY_BAR_SCOPE: CommandScope = CommandScope::Modal("query-bar");

pub static QUERY_BAR_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "query-bar.apply",
        description: "Apply the edited query",
        scope: QUERY_BAR_SCOPE,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    cancel_def("query-bar.cancel", QUERY_BAR_SCOPE),
];

pub static QUERY_BAR_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Enter", "query-bar.apply")
        .bind("Esc", "query-bar.cancel")
});

// ── Rename ──────────────────────────────────────────────────────────

const RENAME_SCOPE: CommandScope = CommandScope::Modal("rename");

pub static RENAME_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "rename.commit",
        description: "Commit the rename",
        scope: RENAME_SCOPE,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    cancel_def("rename.cancel", RENAME_SCOPE),
];

pub static RENAME_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Enter", "rename.commit")
        .bind("Esc", "rename.cancel")
});

// ── Picker family (Search / Preset / Capture) ───────────────────────

const SEARCH_SCOPE: CommandScope = CommandScope::Modal("search");
const PRESET_PICKER_SCOPE: CommandScope = CommandScope::Modal("preset-picker");
const CAPTURE_PICKER_SCOPE: CommandScope = CommandScope::Modal("capture-picker");
const TASK_PRESET_PICKER_SCOPE: CommandScope = CommandScope::Modal("task-preset-picker");

pub static SEARCH_COMMANDS: &[CommandDef] = &[
    confirm_def("search.confirm", SEARCH_SCOPE),
    cancel_def("search.cancel", SEARCH_SCOPE),
    nav_def(
        "search.cursor-up",
        "Select the previous candidate",
        SEARCH_SCOPE,
    ),
    nav_def(
        "search.cursor-down",
        "Select the next candidate",
        SEARCH_SCOPE,
    ),
];

pub static SEARCH_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Enter", "search.confirm")
        .bind("Esc", "search.cancel")
        .bind("Up", "search.cursor-up")
        .bind("Down", "search.cursor-down")
});

pub static PRESET_PICKER_COMMANDS: &[CommandDef] = &[
    confirm_def("preset-picker.confirm", PRESET_PICKER_SCOPE),
    cancel_def("preset-picker.cancel", PRESET_PICKER_SCOPE),
    nav_def(
        "preset-picker.cursor-up",
        "Select the previous preset",
        PRESET_PICKER_SCOPE,
    ),
    nav_def(
        "preset-picker.cursor-down",
        "Select the next preset",
        PRESET_PICKER_SCOPE,
    ),
];

pub static PRESET_PICKER_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Enter", "preset-picker.confirm")
        .bind("Esc", "preset-picker.cancel")
        .bind("Up", "preset-picker.cursor-up")
        .bind("Down", "preset-picker.cursor-down")
});

pub static TASK_PRESET_PICKER_COMMANDS: &[CommandDef] = &[
    confirm_def("task-preset-picker.confirm", TASK_PRESET_PICKER_SCOPE),
    cancel_def("task-preset-picker.cancel", TASK_PRESET_PICKER_SCOPE),
    nav_def(
        "task-preset-picker.cursor-up",
        "Select the previous preset",
        TASK_PRESET_PICKER_SCOPE,
    ),
    nav_def(
        "task-preset-picker.cursor-down",
        "Select the next preset",
        TASK_PRESET_PICKER_SCOPE,
    ),
];

pub static TASK_PRESET_PICKER_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Enter", "task-preset-picker.confirm")
        .bind("Esc", "task-preset-picker.cancel")
        .bind("Up", "task-preset-picker.cursor-up")
        .bind("Down", "task-preset-picker.cursor-down")
});

pub static CAPTURE_PICKER_COMMANDS: &[CommandDef] = &[
    confirm_def("capture-picker.confirm", CAPTURE_PICKER_SCOPE),
    cancel_def("capture-picker.cancel", CAPTURE_PICKER_SCOPE),
    nav_def(
        "capture-picker.cursor-up",
        "Select the previous capture preset",
        CAPTURE_PICKER_SCOPE,
    ),
    nav_def(
        "capture-picker.cursor-down",
        "Select the next capture preset",
        CAPTURE_PICKER_SCOPE,
    ),
];

pub static CAPTURE_PICKER_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Enter", "capture-picker.confirm")
        .bind("Esc", "capture-picker.cancel")
        .bind("Up", "capture-picker.cursor-up")
        .bind("Down", "capture-picker.cursor-down")
});

// ── Related ─────────────────────────────────────────────────────────

const RELATED_SCOPE: CommandScope = CommandScope::Modal("related");

pub static RELATED_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "related.toggle",
        description: "Toggle the focused candidate's selection",
        scope: RELATED_SCOPE,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    CommandDef {
        name: "related.commit",
        description: "Append the checked concepts to the note's Related section",
        scope: RELATED_SCOPE,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    cancel_def("related.cancel", RELATED_SCOPE),
    nav_def(
        "related.cursor-up",
        "Focus the previous candidate",
        RELATED_SCOPE,
    ),
    nav_def(
        "related.cursor-down",
        "Focus the next candidate",
        RELATED_SCOPE,
    ),
];

pub static RELATED_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Space", "related.toggle")
        .bind("Enter", "related.commit")
        .bind("Esc", "related.cancel")
        .bind("Up", "related.cursor-up")
        .bind("Down", "related.cursor-down")
});

// ── Move outer (graph-tab section-move wrapper) ─────────────────────

const MOVE_OUTER_SCOPE: CommandScope = CommandScope::Modal("move");

pub static MOVE_OUTER_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "move.use-selected-as-source",
        description: "Confirm the tree's selected note as the move source",
        scope: MOVE_OUTER_SCOPE,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    CommandDef {
        name: "move.use-selected-as-target",
        description: "Confirm the tree's selected note as the move target",
        scope: MOVE_OUTER_SCOPE,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    CommandDef {
        name: "move.pick-from-list",
        description: "Open the fuzzy picker for source/target selection",
        scope: MOVE_OUTER_SCOPE,
        group: "Flow",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    cancel_def("move.cancel", MOVE_OUTER_SCOPE),
];

pub static MOVE_OUTER_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    // `m`/`t`/`Esc` are the stable chords across all 7 move-outer
    // variants; their *meaning* depends on the current step (source
    // confirm vs target confirm vs picker open). The keymap declares
    // the chord-to-command mapping; the modal's handle_event
    // interprets the verb against the active variant.
    KeyMap::new()
        .bind("m", "move.use-selected-as-source")
        .bind("t", "move.pick-from-list")
        .bind("Esc", "move.cancel")
});

// ── Create-subdir ───────────────────────────────────────────────────

const CREATE_SUBDIR_SCOPE: CommandScope = CommandScope::Modal("create-subdir");

pub static CREATE_SUBDIR_COMMANDS: &[CommandDef] = &[
    confirm_def("create-subdir.confirm", CREATE_SUBDIR_SCOPE),
    cancel_def("create-subdir.cancel", CREATE_SUBDIR_SCOPE),
];

pub static CREATE_SUBDIR_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Enter", "create-subdir.confirm")
        .bind("Esc", "create-subdir.cancel")
});

// ── Task edit popup (graph-task-edit-modal §2.4) ────────────────────

const TASK_EDIT_SCOPE: CommandScope = CommandScope::Modal("task-edit");

pub static TASK_EDIT_COMMANDS: &[CommandDef] = &[
    confirm_def("task-edit.confirm", TASK_EDIT_SCOPE),
    cancel_def("task-edit.cancel", TASK_EDIT_SCOPE),
];

pub static TASK_EDIT_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Enter", "task-edit.confirm")
        .bind("Ctrl+s", "task-edit.confirm")
        .bind("Esc", "task-edit.cancel")
});

// ── Task create popup (graph-task-edit-modal §4) ───────────────────

const TASK_CREATE_SCOPE: CommandScope = CommandScope::Modal("task-create");

pub static TASK_CREATE_COMMANDS: &[CommandDef] = &[
    confirm_def("task-create.confirm", TASK_CREATE_SCOPE),
    cancel_def("task-create.cancel", TASK_CREATE_SCOPE),
];

pub static TASK_CREATE_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Ctrl+s", "task-create.confirm")
        .bind("Esc", "task-create.cancel")
});

// ── Task create leader (graph-task-edit-modal §4) ──────────────────

const TASK_LEADER_SCOPE: CommandScope = CommandScope::Modal("task-leader");

pub static TASK_LEADER_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "task-leader.create",
        description: "Create a new top-level task",
        scope: TASK_LEADER_SCOPE,
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    CommandDef {
        name: "task-leader.new-subtask",
        description: "Create a new subtask under the focused task",
        scope: TASK_LEADER_SCOPE,
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
];

pub static TASK_LEADER_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("c", "task-leader.create")
        .bind("s", "task-leader.new-subtask")
        .bind("Esc", "task-edit.cancel")
});

// ── Confirm-delete ──────────────────────────────────────────────────

const CONFIRM_DELETE_SCOPE: CommandScope = CommandScope::Modal("confirm-delete");

pub static CONFIRM_DELETE_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "confirm-delete.yes",
        description: "Confirm deletion",
        scope: CONFIRM_DELETE_SCOPE,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    CommandDef {
        name: "confirm-delete.no",
        description: "Cancel deletion",
        scope: CONFIRM_DELETE_SCOPE,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    nav_def(
        "confirm-delete.cursor-left",
        "Focus the previous choice",
        CONFIRM_DELETE_SCOPE,
    ),
    nav_def(
        "confirm-delete.cursor-right",
        "Focus the next choice",
        CONFIRM_DELETE_SCOPE,
    ),
];

pub static CONFIRM_DELETE_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("y", "confirm-delete.yes")
        .bind("n", "confirm-delete.no")
        .bind("Esc", "confirm-delete.no")
        .bind("q", "confirm-delete.no")
        .bind("Enter", "confirm-delete.yes")
        .bind("Left", "confirm-delete.cursor-left")
        .bind("h", "confirm-delete.cursor-left")
        .bind("Right", "confirm-delete.cursor-right")
        .bind("l", "confirm-delete.cursor-right")
});

// ── Journal sources manager ─────────────────────────────────────────

const JOURNAL_SOURCES_SCOPE: CommandScope = CommandScope::Modal("journal-sources");

pub static JOURNAL_SOURCES_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "journal-sources.add",
        description: "Open the inner fuzzy picker to add a source",
        scope: JOURNAL_SOURCES_SCOPE,
        group: "Source",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    CommandDef {
        name: "journal-sources.remove",
        description: "Remove the focused source",
        scope: JOURNAL_SOURCES_SCOPE,
        group: "Source",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    CommandDef {
        name: "journal-sources.clear",
        description: "Clear all sources",
        scope: JOURNAL_SOURCES_SCOPE,
        group: "Source",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    confirm_def("journal-sources.commit", JOURNAL_SOURCES_SCOPE),
    cancel_def("journal-sources.cancel", JOURNAL_SOURCES_SCOPE),
    nav_def(
        "journal-sources.cursor-up",
        "Focus the previous source row",
        JOURNAL_SOURCES_SCOPE,
    ),
    nav_def(
        "journal-sources.cursor-down",
        "Focus the next source row",
        JOURNAL_SOURCES_SCOPE,
    ),
];

pub static JOURNAL_SOURCES_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("a", "journal-sources.add")
        .bind("d", "journal-sources.remove")
        .bind("c", "journal-sources.clear")
        .bind("Enter", "journal-sources.commit")
        .bind("Esc", "journal-sources.cancel")
        .bind("Up", "journal-sources.cursor-up")
        .bind("k", "journal-sources.cursor-up")
        .bind("Down", "journal-sources.cursor-down")
        .bind("j", "journal-sources.cursor-down")
});

// ── Journal append-or-replace prompt ────────────────────────────────

const JOURNAL_APPEND_REPLACE_SCOPE: CommandScope = CommandScope::Modal("journal-append-or-replace");

pub static JOURNAL_APPEND_REPLACE_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "journal-append-or-replace.append",
        description: "Union the incoming targets with the current sources",
        scope: JOURNAL_APPEND_REPLACE_SCOPE,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    CommandDef {
        name: "journal-append-or-replace.replace",
        description: "Replace the current sources with the incoming targets",
        scope: JOURNAL_APPEND_REPLACE_SCOPE,
        group: "Flow",
        args_schema: &[],
        opens_modal: false,
        is_primary: true,
    },
    cancel_def(
        "journal-append-or-replace.cancel",
        JOURNAL_APPEND_REPLACE_SCOPE,
    ),
    confirm_def(
        "journal-append-or-replace.commit",
        JOURNAL_APPEND_REPLACE_SCOPE,
    ),
    nav_def(
        "journal-append-or-replace.cursor-left",
        "Focus the previous choice",
        JOURNAL_APPEND_REPLACE_SCOPE,
    ),
    nav_def(
        "journal-append-or-replace.cursor-right",
        "Focus the next choice",
        JOURNAL_APPEND_REPLACE_SCOPE,
    ),
];

pub static JOURNAL_APPEND_REPLACE_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("a", "journal-append-or-replace.append")
        .bind("r", "journal-append-or-replace.replace")
        .bind("c", "journal-append-or-replace.cancel")
        .bind("Esc", "journal-append-or-replace.cancel")
        .bind("Enter", "journal-append-or-replace.commit")
        .bind("Left", "journal-append-or-replace.cursor-left")
        .bind("Tab", "journal-append-or-replace.cursor-right")
        .bind("Right", "journal-append-or-replace.cursor-right")
});

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Every modal command name is unique across all modal slices.
    #[test]
    fn all_modal_command_names_unique() {
        let all: Vec<&'static [CommandDef]> = vec![
            CREATE_COMMANDS,
            APPEND_COMMANDS,
            SECTION_MOVE_COMMANDS,
            CAPTURE_VAR_COMMANDS,
            PERIODIC_LEADER_COMMANDS,
            QUERY_BAR_COMMANDS,
            RENAME_COMMANDS,
            SEARCH_COMMANDS,
            PRESET_PICKER_COMMANDS,
            TASK_PRESET_PICKER_COMMANDS,
            CAPTURE_PICKER_COMMANDS,
            RELATED_COMMANDS,
            MOVE_OUTER_COMMANDS,
            CONFIRM_DELETE_COMMANDS,
            CREATE_SUBDIR_COMMANDS,
            JOURNAL_SOURCES_COMMANDS,
            JOURNAL_APPEND_REPLACE_COMMANDS,
        ];
        let mut seen: HashSet<&'static str> = HashSet::new();
        let mut total = 0;
        for slice in all {
            for def in slice {
                assert!(
                    seen.insert(def.name),
                    "duplicate modal command name: {}",
                    def.name
                );
                total += 1;
            }
        }
        assert!(total > 0);
    }

    /// Every chord bound in a modal keymap resolves to a registered
    /// command in that modal's slice.
    #[test]
    fn modal_keymap_bindings_resolve() {
        let pairs: Vec<(&'static LazyLock<KeyMap>, &'static [CommandDef])> = vec![
            (&CREATE_KEYMAP, CREATE_COMMANDS),
            (&APPEND_KEYMAP, APPEND_COMMANDS),
            (&SECTION_MOVE_KEYMAP, SECTION_MOVE_COMMANDS),
            (&CAPTURE_VAR_KEYMAP, CAPTURE_VAR_COMMANDS),
            (&PERIODIC_LEADER_KEYMAP, PERIODIC_LEADER_COMMANDS),
            (&QUERY_BAR_KEYMAP, QUERY_BAR_COMMANDS),
            (&RENAME_KEYMAP, RENAME_COMMANDS),
            (&SEARCH_KEYMAP, SEARCH_COMMANDS),
            (&PRESET_PICKER_KEYMAP, PRESET_PICKER_COMMANDS),
            (&TASK_PRESET_PICKER_KEYMAP, TASK_PRESET_PICKER_COMMANDS),
            (&CAPTURE_PICKER_KEYMAP, CAPTURE_PICKER_COMMANDS),
            (&RELATED_KEYMAP, RELATED_COMMANDS),
            (&MOVE_OUTER_KEYMAP, MOVE_OUTER_COMMANDS),
        ];
        for (keymap, commands) in pairs {
            let names: HashSet<&'static str> = commands.iter().map(|d| d.name).collect();
            for (_, cmd) in keymap.iter() {
                assert!(
                    names.contains(cmd.name),
                    "keymap binding to unknown command: {}",
                    cmd.name
                );
            }
        }
    }
}

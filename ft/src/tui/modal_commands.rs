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
            CAPTURE_PICKER_COMMANDS,
            RELATED_COMMANDS,
            MOVE_OUTER_COMMANDS,
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

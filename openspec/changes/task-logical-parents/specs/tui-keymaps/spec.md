## ADDED Requirements

### Requirement: Tasks-tab link bindings

The Tasks tab's `KeyMap` SHALL bind `L` → `tasks.link`, `U` → `tasks.unlink` and `S` → `tasks.new-linked-subtask`. `tasks.link` and `tasks.unlink` SHALL be in the "Restructure" group, and `tasks.new-linked-subtask` in "Create / edit". None of them SHALL collide with an existing Tasks-tab binding, and all three SHALL appear in the `?` overlay and in `docs/keybindings.md`.

#### Scenario: Chords resolve
- **WHEN** the Tasks tab is idle and the user presses `L`
- **THEN** the keymap lookup returns `tasks.link`

#### Scenario: Docs in sync
- **WHEN** `ft commands docs --check` runs
- **THEN** it passes with the three new commands listed under `tab/tasks`

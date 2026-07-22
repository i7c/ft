## ADDED Requirements

### Requirement: `M` binding on the Tasks tab resolves to `tasks.move`

The Tasks tab's `KeyMap` SHALL bind the chord `M` (Shift-m) to the `tasks.move` command. The binding SHALL be declared in the Tasks-tab `KEYMAP` static and surfaced through the tab's `with_keymap_overlay` wiring so it appears in the `?` overlay, `ft commands check-keymap`, and `docs/keybindings.md`. The chord `M` SHALL NOT collide with any other binding in the same Tasks-tab scope.

#### Scenario: `M` dispatches `tasks.move`
- **WHEN** the Tasks tab is active, no modal is up, and the user presses `M`
- **THEN** the Tasks tab's `keymap().lookup(chord)` returns `Some(Command { name: "tasks.move", … })` and the move picker opens

#### Scenario: `M` appears in generated docs
- **WHEN** `ft commands docs --check` is run
- **THEN** the committed `docs/keybindings.md` contains a Tasks-tab row binding `M` to `tasks.move`

#### Scenario: No collision in the Tasks-tab keymap
- **WHEN** the Tasks-tab `KeyMap` is constructed
- **THEN** `M` is bound exactly once and no other Tasks-tab binding uses `M`

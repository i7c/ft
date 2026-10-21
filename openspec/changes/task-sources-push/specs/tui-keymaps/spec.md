## ADDED Requirements

### Requirement: Tasks-tab promote binding

The Tasks tab's `KeyMap` SHALL bind `I` → `tasks.promote` (group "Restructure"). It SHALL NOT collide with any existing binding, and SHALL be listed in the `?` overlay and in `docs/keybindings.md`. The `ConflictResolve` modal SHALL declare its own keymap (`j`, `k`, `l`, `r`, `s`, `Enter`, `Esc`) under the `modal/conflict-resolve` scope.

#### Scenario: Docs in sync
- **WHEN** `ft commands docs --check` runs
- **THEN** it passes with `tasks.promote` and the `modal/conflict-resolve` scope documented

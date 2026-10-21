## ADDED Requirements

### Requirement: Tasks-tab restructure bindings

The Tasks tab's `KeyMap` SHALL bind:
- `d` → `tasks.cut-toggle`
- `p` → `tasks.paste-child`
- `P` → `tasks.paste-sibling`
- `>` → `tasks.indent`
- `<` → `tasks.outdent`
- `Esc` → `tasks.cut-clear`

These SHALL be declared in the Tasks-tab keymap static, surfaced through `with_keymap_overlay`, and SHALL NOT collide with any other Tasks-tab binding.

#### Scenario: Restructure chords dispatch
- **WHEN** the Tasks tab is active, no modal or sub-mode is open, and the user presses `p`
- **THEN** the keymap lookup returns `tasks.paste-child`

#### Scenario: Generated docs in sync
- **WHEN** `ft commands docs --check` runs
- **THEN** the committed `docs/keybindings.md` lists the six restructure bindings for `tab/tasks`

### Requirement: Tasks-tab priority cycling on `+` / `-`

The Tasks tab SHALL bind `+` to `tasks.priority-next` (one step up the existing None → Low → Medium → High cycle) and `-` to `tasks.priority-prev`. The chords `p` and `P` SHALL no longer resolve to priority commands. The command names themselves SHALL NOT change, so `ft do` invocations and user `[keymap]` overrides that name them keep working.

#### Scenario: `+` raises priority
- **WHEN** the cursor task has priority Medium and the user presses `+`
- **THEN** the task's priority becomes High

#### Scenario: `p` no longer cycles priority
- **WHEN** the cursor task has priority Medium, the cut set is empty, and the user presses `p`
- **THEN** the priority is unchanged and the toast `nothing cut — press d on a task first` is shown

## ADDED Requirements

### Requirement: Load a task preset into the active Tasks view

The Tasks tab SHALL provide a `tasks.preset-pick` command bound to `Ctrl+P` (outside of input/popup/quickline/edit-query sub-modes) that opens a fuzzy preset picker modal listing all available task presets: user-defined presets from `Config::presets` followed by the built-ins from `ft_core::query::preset::builtin`, with user presets shadowing built-ins of the same name. On selection of a preset, the active `SearchView`'s query text SHALL be replaced with the preset's DSL string and the view SHALL recompile and recompute its matches against the current shared snapshot without rebuilding the graph; the tab SHALL remain in normal mode. On dismissal (Esc) or when no presets exist, the active view's query SHALL remain unchanged.

The picker source SHALL be tasks-specific: it reads `Config::presets` and `query::preset::builtin` (not the graph preset maps `Config::graph.presets` / `ft_core::graph::preset::builtin`). The selected preset is parsed under `Profile::Tasks`, the same profile the Tasks tab's inline query bar uses.

#### Scenario: Ctrl+P opens the tasks preset picker
- **WHEN** the user presses `Ctrl+P` on the Tasks tab with no modal active and not in a sub-mode (query edit, popup, quickline)
- **THEN** a fuzzy preset-picker modal opens listing user task presets and the built-in task presets (`done-today`, `not-done`, `overdue`, `today`, `upcoming`)

#### Scenario: Selecting a preset replaces the active query
- **WHEN** the tasks preset picker is open and the user selects a preset (e.g. `overdue`)
- **THEN** the active SearchView's query text becomes the preset's DSL string (e.g. `(status in {Open, InProgress}) and due < today`), the matches list is recomputed, and the tab is in normal mode (not query-edit mode)

#### Scenario: Dismissing the picker leaves the query unchanged
- **WHEN** the tasks preset picker is open and the user presses Esc
- **THEN** the picker closes and the active view's query text is not modified

#### Scenario: Ctrl+P appears in the Tasks help overlay
- **WHEN** the user opens the `?` help overlay on the Tasks tab
- **THEN** `Ctrl+P` is listed with a description of loading a preset into the active query

#### Scenario: Ctrl+P is a no-op when no presets exist
- **WHEN** the user presses `Ctrl+P` and no user-defined or built-in task presets exist
- **THEN** no picker opens and the active view is unchanged

#### Scenario: User preset shadows a built-in of the same name
- **WHEN** `Config::presets` defines a preset named `today` and the user selects it from the tasks picker
- **THEN** the DSL applied to the active view is the user-defined string, not the built-in `today` DSL

### Requirement: Task preset picker is a registered modal

The tasks preset picker SHALL be expressed as an `ActiveModal::TaskPresetPicker` variant implementing the `Modal` trait, managed by the App's single `active_modal` slot (not an inline `Option<...>` field on the SearchView). It SHALL expose a `modal/task-preset-picker` command scope with `task-preset-picker.confirm` / `task-preset-picker.cancel` / `task-preset-picker.cursor-up` / `task-preset-picker.cursor-down` commands bound to `Enter` / `Esc` / `Up` / `Down`, registered in the central `CommandRegistry` and surfaced in the `?` overlay and `ft commands list`.

#### Scenario: Picker is a modal, not a tab-resident field
- **WHEN** the user opens the tasks preset picker
- **THEN** `App::active_modal_name()` returns `Some("task-preset-picker")` and no `Option<...>` preset-picker field exists on `SearchView`

#### Scenario: Arrow keys navigate within the picker
- **WHEN** the tasks preset picker is open and the user presses `Up` or `Down`
- **THEN** the picker's cursor moves among presets and the key is consumed by the modal (the Tasks tab's `j`/`k` cursor movement does not fire)

#### Scenario: Docs stay in sync
- **WHEN** `cargo run --release -q -- commands docs --check` runs after the change
- **THEN** it passes (the new `tasks.preset-pick` and `task-preset-picker.*` commands are reflected in `docs/keybindings.md`)

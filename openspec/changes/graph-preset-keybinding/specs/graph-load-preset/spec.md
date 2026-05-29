## ADDED Requirements

### Requirement: Load preset into active graph view
The graph tab SHALL provide a `Ctrl+P` keybinding that opens the fuzzy preset picker scoped to the currently active view. On selection of a preset, the active view's query SHALL be replaced with the preset's DSL string and the graph SHALL re-run. On dismissal (Esc or no presets available), the active view SHALL remain unchanged.

#### Scenario: Ctrl+P opens preset picker
- **WHEN** the user presses `Ctrl+P` on the graph tab (outside input mode)
- **THEN** the fuzzy preset picker modal opens listing all available presets (user-defined and built-in)

#### Scenario: Selecting a preset applies DSL to active view
- **WHEN** the preset picker is open (via `Ctrl+P`) and the user selects a preset
- **THEN** the active view's query is set to the preset's DSL and the graph re-runs with that query

#### Scenario: Dismissing the picker leaves active view unchanged
- **WHEN** the preset picker is open (via `Ctrl+P`) and the user presses Esc
- **THEN** the picker closes and the active view's query is not modified

#### Scenario: Ctrl+P appears in help overlay
- **WHEN** the user opens the `?` help overlay on the graph tab
- **THEN** `Ctrl+P` is listed under "Query" with the description "load preset into this view"

#### Scenario: Ctrl+P is a no-op when no presets exist
- **WHEN** the user presses `Ctrl+P` and no user-defined or built-in presets exist
- **THEN** no picker opens and the active view is unchanged

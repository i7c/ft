# tui-keymaps

## ADDED Requirements

### Requirement: Graph tab keymap includes delete and create-subdir bindings

The Graph tab's default keymap (`GRAPH_KEYMAP`) SHALL include bindings for `graph.delete` on `d` and `graph.create-subdir` on `n`.

#### Scenario: d bound to graph.delete

- **WHEN** the Graph tab's keymap is built
- **THEN** `d` resolves to `graph.delete`

#### Scenario: n bound to graph.create-subdir

- **WHEN** the Graph tab's keymap is built
- **THEN** `n` resolves to `graph.create-subdir`

#### Scenario: Existing bindings unchanged

- **WHEN** the Graph tab's keymap is built
- **THEN** all existing bindings (`c`, `C`, `r`, `m`, `Space`, `j`, `k`, etc.) remain in place with their current command mappings

# graph-presets Specification

## Purpose
TBD - created by archiving change config-graph-presets. Update Purpose after archive.
## Requirements
### Requirement: Graph preset config field
The system SHALL store graph-query presets as a `presets` field on `GraphCfg` — a map of preset name to graph-DSL string. User-defined presets under `[graph.presets]` in TOML SHALL be deserialized into this map.

#### Scenario: User defines a graph preset in TOML
- **WHEN** the config contains `[graph.presets]\nmy-backlinks = "node where title includes \"Foo\"; expand where edge.kind = link;"`
- **THEN** `config.graph.presets["my-backlinks"]` yields that DSL string

#### Scenario: No graph presets defined
- **WHEN** the config has no `[graph.presets]` section
- **THEN** `config.graph.presets` is an empty `HashMap`

### Requirement: Built-in graph presets
The system SHALL provide built-in graph presets accessible via `ft_core::graph::query::preset::builtin(name)`. Each built-in SHALL parse cleanly through `graph::query::parse`. The `builtin_names()` function SHALL return all built-in names sorted alphabetically.

#### Scenario: Resolve a built-in preset
- **WHEN** `builtin("orphans")` is called
- **THEN** it returns `Some("node where indegree = 0 and kind = Note;")`

#### Scenario: Resolve unknown built-in
- **WHEN** `builtin("nonexistent")` is called
- **THEN** it returns `None`

#### Scenario: Every built-in round-trips through the parser
- **WHEN** each string returned by `builtin_names()` is passed to `parse()`
- **THEN** parsing succeeds without error

### Requirement: CLI preset resolution
`ft graph query` SHALL accept a `--preset <name>` flag mutually exclusive with `QUERY`, `--query`, and `--from-file`. Resolution SHALL check the user config map first, then fall back to built-ins. If no preset matches, the command SHALL exit with code 2 and print an error message.

#### Scenario: Resolve user-defined preset from CLI
- **WHEN** `ft graph query --preset my-backlinks` is run and `[graph.presets.my-backlinks]` is defined in config
- **THEN** the DSL string from config is used as the query source

#### Scenario: Resolve built-in preset from CLI
- **WHEN** `ft graph query --preset orphans` is run and no user preset named "orphans" exists
- **THEN** the built-in `orphans` DSL string is used as the query source

#### Scenario: Unknown preset name from CLI
- **WHEN** `ft graph query --preset nonexistent` is run and no matching preset exists in config or built-ins
- **THEN** the command exits with code 2 and prints "unknown preset: nonexistent"

#### Scenario: Preset flag conflicts with positional query
- **WHEN** `ft graph query "node where kind = Note" --preset orphans` is run
- **THEN** the command exits with an error indicating mutually exclusive arguments

### Requirement: User preset shadows built-in
When a user-defined preset has the same name as a built-in, the user-defined preset SHALL take precedence. The built-in definition SHALL still be available through `builtin()` directly for callers that want the canonical definition.

#### Scenario: User overrides a built-in name
- **WHEN** config contains `[graph.presets.orphans] = "node where kind = Note and outdegree = 0;"` and the user runs `ft graph query --preset orphans`
- **THEN** the user's DSL string is used, not the built-in

### Requirement: TUI preset quick-pick
When creating a new graph view in the TUI (`Ctrl+N`), the system SHALL offer preset names (user-defined + built-in, user-defined first) as quick-pick entries. Selecting a preset SHALL pre-fill the query input with the resolved DSL string. The user SHALL be able to edit the string before applying.

#### Scenario: User selects a preset in new-view creation
- **WHEN** the user presses `Ctrl+N` and selects "orphans" from the preset list
- **THEN** the query input is pre-filled with `node where indegree = 0 and kind = Note;`

#### Scenario: User creates blank view
- **WHEN** the user presses `Ctrl+N` and dismisses the preset list without selecting
- **THEN** a new view is created with the default query (current behavior)


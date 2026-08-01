## MODIFIED Requirements

### Requirement: CLI preset resolution
`ft graph query` SHALL accept a `--preset <name>` flag mutually exclusive with `QUERY`, `--query`, and `--from-file`. Resolution SHALL check the user config map first, then fall back to built-ins. If no preset matches, the command SHALL exit with code 2 and print an error message. The resolved preset DSL string SHALL be run through sigil interpolation (`query-sigil-interpolation`) before parsing, so stored graph presets MAY contain dynamic `@`-sigil placeholders (`@today`, `@daily`, …) that resolve fresh on every run. Built-in presets contain no sigils and are unaffected by interpolation.

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

#### Scenario: Stored graph preset with a sigil resolves dynamically
- **WHEN** `config.graph.presets["today-note"]` is `node where path includes @today` and the user runs `ft graph query --preset today-note` with `FT_TODAY=2026-07-29`
- **THEN** the query used is `node where path includes "2026-07-29"` (the `@today` sigil expanded at run time)

#### Scenario: Stored graph preset re-resolves the next day
- **WHEN** the same `today-note` preset is run with `FT_TODAY=2026-07-30`
- **THEN** the query used references `2026-07-30`, not the previous day's date

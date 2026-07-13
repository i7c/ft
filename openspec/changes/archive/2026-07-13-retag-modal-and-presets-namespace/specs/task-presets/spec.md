## ADDED Requirements

### Requirement: Task query presets stored under `[tasks.presets]`

The system SHALL store task query presets as a `presets` field on the
`Tasks` config struct — a map of preset name to task-DSL string — serialized
as `[tasks.presets]` in TOML. Top-level `[presets]` SHALL NOT be accepted:
because `Config` is `deny_unknown_fields`, a config file containing a
top-level `[presets]` section SHALL fail to load with an error naming the
unknown field.

#### Scenario: User defines a task preset under tasks.presets
- **WHEN** the config contains `[tasks.presets]\nwork = "tags includes \"work\" and not done"`
- **THEN** `config.tasks.presets["work"]` yields that DSL string

#### Scenario: No task presets defined
- **WHEN** the config has no `[tasks.presets]` section
- **THEN** `config.tasks.presets` is an empty `HashMap`

#### Scenario: Legacy top-level presets section is rejected
- **WHEN** the config contains a top-level `[presets]` section (the pre-change location)
- **THEN** config loading fails with an error naming `presets` as an unknown field, guiding the user to rename to `[tasks.presets]`

### Requirement: CLI and TUI resolve task presets from `[tasks.presets]`

`ft tasks list --preset <name>` and the TUI task-preset picker SHALL resolve
a preset name by checking `config.tasks.presets` first, then the built-in
task presets (`ft_core::query::preset::builtin`). User presets SHALL shadow
built-ins of the same name. If no preset matches, the CLI SHALL exit with
code 2 and print an error message.

#### Scenario: Resolve user-defined preset from CLI
- **WHEN** `ft tasks list --preset work` is run and `[tasks.presets.work]` is defined in config
- **THEN** the DSL string from `config.tasks.presets["work"]` is used as the query source

#### Scenario: Resolve built-in preset from CLI
- **WHEN** `ft tasks list --preset today` is run and no user preset named "today" exists
- **THEN** the built-in `today` DSL string is used as the query source

#### Scenario: Unknown preset name from CLI
- **WHEN** `ft tasks list --preset nonexistent` is run and no matching preset exists in config or built-ins
- **THEN** the command exits with code 2 and prints an error naming the unknown preset

#### Scenario: User preset shadows a built-in
- **WHEN** `config.tasks.presets["today"]` is defined and the user runs `ft tasks list --preset today`
- **THEN** the user's DSL string is used, not the built-in

#### Scenario: TUI picker reads from the new location
- **WHEN** the user opens the task-preset picker (`Ctrl+P`) in the Tasks tab
- **THEN** the picker lists user presets from `config.tasks.presets` (first, shadowing) followed by built-in task presets

### Requirement: `ft vault config` dumps task presets under the new key

The `ft vault config` command SHALL report task presets from
`config.tasks.presets`, labeled under the `tasks.presets` key (mirroring how
`graph.presets` is reported). It SHALL NOT read a top-level `presets` field.

#### Scenario: Config dump shows task presets
- **WHEN** the user runs `ft vault config` and `[tasks.presets]` contains one or more entries
- **THEN** the output lists those presets under a `tasks.presets` heading

#### Scenario: Empty task presets reported
- **WHEN** the user runs `ft vault config` and no task presets are defined
- **THEN** the output shows `tasks.presets = {}` (or equivalent empty representation)

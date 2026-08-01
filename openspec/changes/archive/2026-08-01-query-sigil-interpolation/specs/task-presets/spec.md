## MODIFIED Requirements

### Requirement: CLI and TUI resolve task presets from `[tasks.presets]`

`ft tasks list --preset <name>` and the TUI task-preset picker SHALL resolve
a preset name by checking `config.tasks.presets` first, then the built-in
task presets (`ft_core::query::preset::builtin`). User presets SHALL shadow
built-ins of the same name. If no preset matches, the CLI SHALL exit with
code 2 and print an error message. The resolved preset DSL string SHALL be
run through sigil interpolation (`query-sigil-interpolation`) before
parsing, so stored presets MAY contain dynamic `@`-sigil placeholders
(`@today`, `@daily`, …) that resolve fresh on every run. Built-in presets
contain no sigils and are unaffected by interpolation.

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

#### Scenario: Stored preset with a sigil resolves dynamically
- **WHEN** `config.tasks.presets["daily-open"]` is `path includes @daily and status in {Open, InProgress}` and the user runs `ft tasks list --preset daily-open` with `FT_TODAY=2026-07-29` and `[periodic_notes.daily]` resolving to `journal/2026/2026-07-29.md`
- **THEN** the query used is `path includes "journal/2026/2026-07-29.md" and status in {Open, InProgress}` (the `@daily` sigil expanded at run time)

#### Scenario: Stored preset with a sigil re-resolves the next day
- **WHEN** the same `daily-open` preset is run with `FT_TODAY=2026-07-30`
- **THEN** the query used references `journal/2026/2026-07-30.md`, not the previous day's path

#### Scenario: Stored preset whose sigil lacks periodic config exits with code 2
- **WHEN** `config.tasks.presets["daily-open"]` contains `@daily` and the vault has no `[periodic_notes.daily]` block and the user runs `ft tasks list --preset daily-open`
- **THEN** the command exits with code 2 and prints an error naming the missing `[periodic_notes.daily]` config

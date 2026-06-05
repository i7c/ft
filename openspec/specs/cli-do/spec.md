# cli-do Specification

## Purpose
TBD - created by archiving change commands-and-keymaps. Update Purpose after archive.
## Requirements
### Requirement: `ft commands list` introspects the command registry

The `ft commands list` subcommand SHALL print every `CommandDef` in the registry. Output format SHALL be controllable via `--format table|json|ndjson` and filterable via `--scope <scope>` and `--opens-modal true|false`.

#### Scenario: Default table output
- **WHEN** the user runs `ft commands list`
- **THEN** stdout contains a terminal-aware table with columns `Name`, `Scope`, `Opens modal`, `Description`, grouped by scope

#### Scenario: ndjson output
- **WHEN** the user runs `ft commands list --format ndjson`
- **THEN** stdout contains one JSON object per line, each with keys `name`, `scope`, `opens_modal`, `description`, `args_schema`, `group`

#### Scenario: Scope filter
- **WHEN** the user runs `ft commands list --scope tab`
- **THEN** only commands with `CommandDef.scope` matching a tab scope are printed

### Requirement: `ft do <command>` dispatches a non-modal command

The `ft do <command> [--arg key=value …]` subcommand SHALL look up the command in the registry, validate args against `args_schema`, and invoke the same handler the TUI would invoke. Commands with `opens_modal = true` SHALL be rejected with a clear non-zero-exit error.

#### Scenario: Atomic command succeeds
- **WHEN** the user runs `ft do tasks.complete-by-id --arg id=xyz123`
- **THEN** the task with id `xyz123` is completed via the same `ft_core::task::ops::complete_task` path used by the TUI's `tasks.complete-current`, and stdout reports success

#### Scenario: Modal-opening command rejected
- **WHEN** the user runs `ft do graph.create-note`
- **THEN** `ft do` exits with code 2 and prints `command 'graph.create-note' opens an interactive flow; use 'ft tui' (this is a v1 limitation)`

#### Scenario: Unknown command
- **WHEN** the user runs `ft do nonexistent.command`
- **THEN** `ft do` exits with code 2 and prints `unknown command 'nonexistent.command'; see 'ft commands list'`

#### Scenario: Args validate against schema
- **WHEN** the user runs `ft do tasks.complete-by-id` without `--arg id=…`
- **THEN** `ft do` exits with code 2 and prints a message naming the missing required arg

### Requirement: `ft do` honours `--json-errors` and `--format`

`ft do` SHALL respect the top-level `--json-errors` flag (errors as `{"error":…,"chain":[…]}` on stderr) and SHALL support `--format text|json` for successful output (text is human-readable; json is a structured outcome object).

#### Scenario: JSON error output
- **WHEN** the user runs `ft --json-errors do unknown.command`
- **THEN** stderr contains a JSON object describing the error and exit code is non-zero

#### Scenario: JSON success output
- **WHEN** the user runs `ft do tasks.complete-by-id --arg id=xyz123 --format json`
- **THEN** stdout contains a JSON object with at least `{"command":"tasks.complete-by-id","outcome":"ok","details":…}`

## ADDED Requirements

### Requirement: `tasks.move` command registered on the Tasks tab

The Tasks tab's `COMMANDS` slice SHALL include a `CommandDef` with `name: "tasks.move"`, a human-readable `description`, `opens_modal: true` (it opens the file/heading picker modal), `scope` scoped to the Tasks tab, and a `group` consistent with other Tasks-tab mutation commands. The command SHALL be dispatchable via `ft do tasks.move` metadata and SHALL appear in the `?` overlay grouped with its peers.

#### Scenario: Command appears in the registry
- **WHEN** `ft commands list` is invoked
- **THEN** the output includes `tasks.move` with `opens_modal: true`

#### Scenario: `ft do` rejects a modal-opening command
- **WHEN** the user runs `ft do tasks.move`
- **THEN** `ft do` exits non-zero with a message instructing the user to use `ft tui` for interactive flows (per the `opens_modal` gate)

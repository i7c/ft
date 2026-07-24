## ADDED Requirements

### Requirement: `graph.synth-from-note` command registered on the Graph tab
The Graph tab's `COMMANDS` slice SHALL include a `CommandDef` with `name: "graph.synth-from-note"`, a human-readable `description`, `opens_modal: true` (it opens the paragraph-synth modal), `scope` scoped to the Graph tab, and a `group` consistent with other Graph-tab mutation commands. The command SHALL be bound to the `y` chord on the Graph tab. The command SHALL be dispatchable via `ft do` metadata and SHALL appear in the `?` overlay grouped with its peers.

#### Scenario: Command appears in the registry
- **WHEN** `ft commands list` is invoked
- **THEN** the output includes `graph.synth-from-note` with `opens_modal: true`

#### Scenario: `y` is bound on the Graph tab
- **WHEN** the Graph tab's keymap is built
- **THEN** the `y` chord resolves to `graph.synth-from-note`

#### Scenario: `ft do` rejects the modal-opening command
- **WHEN** the user runs `ft do graph.synth-from-note`
- **THEN** `ft do` exits non-zero with a message instructing the user to use `ft tui` for interactive flows (per the `opens_modal` gate)

### Requirement: `notes.synth-from-note` command registered on the Notes tab
The Notes tab's `COMMANDS` slice SHALL include a `CommandDef` with `name: "notes.synth-from-note"`, a human-readable `description`, `opens_modal: true`, `scope` scoped to the Notes tab, and a `group` consistent with other Notes-tab mutation commands. The command SHALL be bound to the `y` chord on the Notes tab. The command SHALL be dispatchable via `ft do` metadata and SHALL appear in the `?` overlay grouped with its peers.

#### Scenario: Command appears in the registry
- **WHEN** `ft commands list` is invoked
- **THEN** the output includes `notes.synth-from-note` with `opens_modal: true`

#### Scenario: `y` is bound on the Notes tab
- **WHEN** the Notes tab's keymap is built
- **THEN** the `y` chord resolves to `notes.synth-from-note`

#### Scenario: `ft do` rejects the modal-opening command
- **WHEN** the user runs `ft do notes.synth-from-note`
- **THEN** `ft do` exits non-zero with a message instructing the user to use `ft tui` for interactive flows (per the `opens_modal` gate)

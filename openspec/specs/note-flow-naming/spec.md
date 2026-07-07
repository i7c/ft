# note-flow-naming Specification

## Purpose
The naming contract for the note-flow surface: verbs that say what
they do, one namespace (ft notes), a tab order that reads as the
workflow, and no "ritual" anywhere. Created by archiving change
note-flow-renames.

## Requirements
### Requirement: The workflow lives under ft notes
The note-flow commands SHALL all be subcommands of `ft notes`:
`gather` (topic-shaped paragraph feed, formerly `journal`), `recent`
(time-shaped paragraph feed, formerly `history`), `pulse` (windowed
mention ranking, formerly top-level `review`), and `synth` with its
subcommands `scaffold`, `grow`, `verify`, `repair`, `reslice`
(formerly top-level `synth`). The top-level `ft review` and `ft synth`
commands SHALL NOT exist. No aliases for removed names SHALL be
provided.

#### Scenario: New names dispatch
- **WHEN** `ft notes gather`, `ft notes recent`, `ft notes pulse`, or
  `ft notes synth verify --all` is invoked
- **THEN** each runs the behavior previously reachable under its old
  name, flags unchanged

#### Scenario: Old names are gone
- **WHEN** `ft review` or `ft synth scaffold x.md` or
  `ft notes journal` is invoked
- **THEN** the command fails with clap's unknown-command error

### Requirement: TUI tabs and command IDs follow the names
The TUI tabs SHALL be titled Gather, Recent, and Pulse, and their
command IDs SHALL be `gather.*`, `recent.*`, and `pulse.*` (verb parts
and default chords unchanged). The registry, `?` overlay,
`ft commands list`, `ft do`, and `docs/keybindings.md` SHALL agree on
the new IDs; `[keymap]` overrides referencing old IDs fail validation
exactly like any unknown command.

#### Scenario: Registry coherence
- **WHEN** `cargo run --release -q -- commands docs --check` runs
  after the change
- **THEN** it passes, and `ft commands list` contains
  `gather.toggle-uncited` and no `journal.*` entries

### Requirement: Tab order reads capture → resurface → consolidate
The TUI tab order SHALL be Graph, Notes, Pulse, Recent, Gather, then
(when enabled) Tasks and Timeblocks. Tab digits and cycling SHALL
derive from the built tab list.

#### Scenario: Default layout
- **WHEN** the TUI starts with no `[tui]` config
- **THEN** the tab bar shows exactly
  `1 Graph  2 Notes  3 Pulse  4 Recent  5 Gather`

### Requirement: Tasks and Timeblocks tabs are opt-in
A `[tui]` config table SHALL provide `tasks_tab` and `timeblocks_tab`
booleans, both defaulting to `false`. When `false`, the corresponding
tab SHALL not be built; when `true`, it appends after Gather in the
order Tasks, Timeblocks. Unknown keys in `[tui]` SHALL be rejected
like every other config table. Headless task/timeblock CLI commands
SHALL be unaffected by the tab toggles.

#### Scenario: Enabling the adjacent tabs
- **WHEN** config sets `tasks_tab = true` and `timeblocks_tab = true`
- **THEN** the tab bar ends with `6 Tasks  7 Timeblocks`

#### Scenario: CLI unaffected
- **WHEN** `tasks_tab = false` and `ft tasks list` is invoked
- **THEN** it works exactly as before

### Requirement: No ritual wording anywhere
No CLI help string, code comment, module doc, user-facing doc,
developer doc (CLAUDE.md, AGENTS.md, docs/architecture.md), or live
openspec spec SHALL describe the workflow as a "ritual". Archived
openspec changes are historical records and are exempt.

#### Scenario: Grep gate
- **WHEN** `rg -i ritual` runs over the repo excluding
  `openspec/changes/archive/` and `openspec/explorations/`
- **THEN** it returns no matches

### Requirement: Internal names match the surface
Core and TUI internals SHALL use the new vocabulary (`gather`,
`recent`, `pulse` modules and types); no `journal`/`history`/
`link_review` identifiers remain for the workflow concepts. On-disk
vault formats SHALL be byte-identical: `ft-synth: true`,
`ft-synth-targets`, `[!ft-source]` callout grammar, the blame cache,
and the `[synth]` config table are unchanged, and "synth note" remains
the name of the artifact format.

#### Scenario: Vault round-trip unchanged
- **WHEN** a synth note created before the rename is verified after it
- **THEN** `ft notes synth verify` reports `ok` with no rewrites

### Requirement: Docs show the new surface with real output
README and guide pages SHALL reference only the new command names,
with the README demo re-verified against a real vault. The docs SHALL
mention how to re-enable the Tasks/Timeblocks tabs.

#### Scenario: No stale command references
- **WHEN** `rg "ft review|ft synth |notes journal|notes history"
  README.md docs/guide/` runs (excluding intentional "formerly" notes)
- **THEN** it returns no matches

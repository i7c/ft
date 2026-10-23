## MODIFIED Requirements

### Requirement: The workflow lives under ft notes
The synthesis commands SHALL be subcommands of `ft notes`: `synth` with its
subcommands `scaffold`, `grow`, `verify`, `repair`, `reslice`. The top-level
`ft synth` command SHALL NOT exist, and no aliases for removed names SHALL be
provided. (`gather`/`journal` were removed by the gather change; `pulse`
/`recent` are removed by this change, so neither namespace survives.)

#### Scenario: New names dispatch
- **WHEN** `ft notes synth verify --all` is invoked
- **THEN** it runs the behavior of the former top-level `ft synth verify`

#### Scenario: Old names are gone
- **WHEN** `ft synth scaffold x.md`, `ft notes journal`, `ft notes pulse`, or
  `ft notes recent` is invoked
- **THEN** the command fails with clap's unknown-subcommand error (exit 2)

### Requirement: TUI tabs and command IDs follow the names
Every TUI tab SHALL carry a `<tab>` scope whose command IDs share the tab's
name (`search.*`, `graph.*`, `notes.*`, …), and the registry, `?` overlay,
`ft commands list`, `ft do`, and `docs/keybindings.md` SHALL agree on those
IDs; `[keymap]` overrides referencing an unknown command or scope fail
validation exactly like any other unknown command.

#### Scenario: Registry coherence
- **WHEN** `cargo run --release -q -- commands docs --check` runs
- **THEN** it passes, and `ft commands list` contains `search.reload` and no
  `pulse.*` or `recent.*` entries

#### Scenario: Stale override rejected
- **WHEN** `[keymap]` contains a binding for `recent.reload`
- **THEN** `ft commands check-keymap` reports it as an unknown command

### Requirement: Tab order reads capture → resurface → consolidate
The TUI tab order SHALL be Graph, Notes, then Search, then (when enabled)
Tasks and Timeblocks. Tab digits and cycling SHALL derive from the built tab
list.

#### Scenario: Default layout
- **WHEN** the TUI starts with no `[tui]` config
- **THEN** the tab bar shows exactly `1 Graph  2 Notes  3 Search`

### Requirement: Tasks and Timeblocks tabs are opt-in
A `[tui]` config table SHALL provide `tasks_tab` and `timeblocks_tab`
booleans, both defaulting to `false`. When `false`, the corresponding
tab SHALL not be built; when `true`, it appends after Search in the
order Tasks, Timeblocks. Unknown keys in `[tui]` SHALL be rejected
like every other config table. Headless task/timeblock CLI commands
SHALL be unaffected by the tab toggles.

#### Scenario: Enabling the adjacent tabs
- **WHEN** config sets `tasks_tab = true` and `timeblocks_tab = true`
- **THEN** the tab bar ends with `4 Tasks  5 Timeblocks`

#### Scenario: CLI unaffected
- **WHEN** `tasks_tab = false` and `ft tasks list` is invoked
- **THEN** it works exactly as before

### Requirement: Internal names match the surface
Core and TUI internals SHALL use the same vocabulary as the surface: the
`synth` module family for the synth workflow, `search` for paragraph search,
and `recents` only for the recently-opened log. No `journal`/`history`/
`link_review` identifiers remain for workflow concepts, and no `pulse`
/`recent` feed modules remain. On-disk vault formats SHALL be unchanged:
`ft-synth: true`, `[!ft-source]` callout grammar, the blame cache, the
`recents.jsonl` log, and the `[synth]` config table, and "synth note" remains
the name of the artifact format.

#### Scenario: Vault round-trip unchanged
- **WHEN** a synth note created before this change is verified after it
- **THEN** `ft notes synth verify` reports `ok` with no rewrites

### Requirement: Docs show the new surface with real output
README and guide pages SHALL reference only current command names, with the
README demo re-verified against a real vault. The docs SHALL mention how to
re-enable the Tasks/Timeblocks tabs.

#### Scenario: No stale command references
- **WHEN** `rg "ft review|ft synth |notes journal|notes history|notes pulse|notes recent"
  README.md docs/guide/` runs (excluding intentional "formerly" notes)
- **THEN** it returns no matches

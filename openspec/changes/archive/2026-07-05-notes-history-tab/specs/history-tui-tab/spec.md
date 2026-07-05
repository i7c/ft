## ADDED Requirements

### Requirement: History tab registration
The TUI SHALL register a new top-level tab titled `History`, implementing the `Tab`
trait with its own `TabKind` routing key and declaring `HISTORY_COMMANDS` +
`HISTORY_KEYMAP` static slices. It SHALL be pushed into `build_tabs_with_overlays`
wrapped with `.with_keymap_overlay(...)`, and its keymap SHALL appear in
`docs/keybindings.md` after regeneration.

#### Scenario: Tab appears in the tab strip
- **WHEN** the TUI starts
- **THEN** the tab strip lists a `History` tab that can receive focus

#### Scenario: Keymap overlay is registered
- **WHEN** `ft commands check-keymap` runs
- **THEN** the History tab's keymap is included (the overlay wiring is present)

### Requirement: Shared-snapshot participation
The History tab SHALL read graph/scan data only from `ctx.snapshot` and SHALL NOT
call `vault.scan()` or `Graph::build` itself. It SHALL re-derive its feed on graph
generation change (`on_graph_ready` / `on_focus`), and after any mutation it
performs SHALL raise `ctx.request_graph_refresh()`.

#### Scenario: No direct scan
- **WHEN** the History tab loads its feed
- **THEN** it uses `ctx.snapshot` and issues no `vault.scan()` / `Graph::build` call

#### Scenario: Refresh after mutation
- **WHEN** the tab performs a section move that changes files
- **THEN** it raises `ctx.request_graph_refresh()` so the feed re-derives on the new generation

### Requirement: Windowed feed rendering
The History tab SHALL render the `build_history` feed for a window that defaults to
`7d`, showing each paragraph entry with its date and source title, ordered
reverse-chronologically. It SHALL reuse a `BlameCache` for the session and hold the
current window so the feed can be recomputed on reload.

#### Scenario: Feed renders on focus
- **WHEN** the History tab is focused in a git-backed vault
- **THEN** it displays the last-7-days paragraph feed

#### Scenario: Empty window state
- **WHEN** no paragraphs were edited within the window
- **THEN** the tab shows an empty-state message rather than a blank body

#### Scenario: Reload recomputes the feed
- **WHEN** the user triggers the tab's reload action after committing an edit
- **THEN** `build_history` is re-run and newly edited paragraphs appear

### Requirement: Row selection for synth
The History tab SHALL let the user select one, several, or all feed rows and hand
the selected paragraphs to the existing synth scaffold flow, producing protected
`[!ft-source]` callout sections in a chosen target note. The flow SHALL NOT write
an `ft-synth-target` frontmatter key (there is no target), and SHALL NOT offer the
synth-grow/accrete step — History synth is scaffold-only.

#### Scenario: Select and scaffold
- **WHEN** the user selects two rows and confirms the synth action against target note `T`
- **THEN** the synth scaffold flow writes protected `[!ft-source]` callouts for those two paragraphs into `T`

#### Scenario: Select-all
- **WHEN** the user invokes select-all and confirms synth
- **THEN** every current feed row is included in the scaffold

#### Scenario: No synth-target frontmatter
- **WHEN** a History synth scaffold writes into target note `T`
- **THEN** `T` gains no `ft-synth-target` frontmatter key (the marker written for target-based synth is omitted)

### Requirement: Seeded section-move action (TUI only)
On a focused feed row, the History tab SHALL offer an action that opens the
existing section-move modal (`ActiveModal::SectionMove`) seeded to the row's source
note, skipping the source picker and starting at heading multi-select for that
note. This SHALL reuse the existing move machinery via a thin `begin_for_source`
entry point built on `advance_to_multiselect`; no new movement primitive is
introduced. This action is available in the TUI only (the CLI `ft notes history`
is read-only).

#### Scenario: Move opens seeded to the row's note
- **WHEN** the user invokes the move action on a row whose source note is `Daily-2026-07-01.md`
- **THEN** the section-move modal opens already scoped to that note's headings (no source-picker step)

#### Scenario: Move completes via the existing flow
- **WHEN** the user selects a heading and a target in the seeded modal and confirms
- **THEN** the section is moved using the existing `move_sections` apply path, and the tab raises a graph refresh

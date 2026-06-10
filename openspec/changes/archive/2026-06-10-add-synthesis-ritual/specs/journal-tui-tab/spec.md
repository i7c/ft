## ADDED Requirements

### Requirement: Multi-target mode queueing
The Journal tab SHALL accept a queued multi-target request from another tab (specifically the Review tab) via a new typed slot `queued_targets: RefCell<Option<MultiTargetRequest>>` on `App`, where `MultiTargetRequest` carries the resolved `Vec<NoteId>` of selected targets and an optional `window: WindowRange`. On `on_focus`, if the slot is `Some`, the tab SHALL consume it, call `build_journal` with the slice of targets, and replace its body with the resulting multi-target journal.

#### Scenario: Multi-target queue consumed on focus
- **WHEN** the Review tab handed off three targets and the user switches focus to the Journal tab
- **THEN** the tab consumes the slot, runs `build_journal` for the three targets, and displays the merged entries

#### Scenario: Existing single-note queue still works
- **WHEN** `queued_journal_for_path` is set (legacy single-note queue) and no `queued_targets` is set
- **THEN** the existing single-note flow runs unchanged

#### Scenario: Both slots set prefers multi-target
- **WHEN** both `queued_targets` and `queued_journal_for_path` are set simultaneously
- **THEN** the multi-target queue takes precedence and the single-note queue is cleared without execution

### Requirement: Matched-targets badge rendering
When rendering a multi-target journal (loaded via `queued_targets`), entries whose `matched` field contains more than one target SHALL display a `matched: X, Y` badge after the date line. The badge SHALL list target display titles (not raw `[[wikilink]]` syntax), comma-separated, in selection order. Entries with `matched.len() == 1` SHALL NOT display the badge.

#### Scenario: Badge shown for multi-match entry
- **WHEN** an entry's paragraph links to both `Foo` and `Bar` (both selected)
- **THEN** the rendered entry shows a `matched: Foo, Bar` line after the date

#### Scenario: No badge for single-match entry
- **WHEN** an entry's paragraph links to only `Foo` (with `Foo` and `Bar` selected)
- **THEN** no `matched:` line appears for that entry

### Requirement: In-window-only toggle
The Journal tab SHALL provide a key (`w`) to toggle in-window-only filtering. The toggle SHALL be active only when the tab is in multi-target mode AND a window range was passed in the queued request. When active, only entries whose `(source_file, line_start..=line_end)` overlaps an added-line in the queued window SHALL be displayed. When inactive (default), all-time entries are shown. The current toggle state SHALL be reflected in the tab header.

#### Scenario: Toggle filters to in-window entries
- **WHEN** the user presses `w` after a Review handoff with a 14-day window
- **THEN** only entries with paragraph lines overlapping added-lines from the last 14 days are shown

#### Scenario: Toggle off shows all-time
- **WHEN** the user presses `w` a second time
- **THEN** all-time matching entries are shown again

#### Scenario: Toggle has no effect outside multi-target with window
- **WHEN** the tab is in single-note mode (no window context)
- **THEN** pressing `w` has no effect and the help overlay does not advertise the toggle

### Requirement: Entry multi-select
The Journal tab SHALL support multi-selecting journal entries with `<space>`. Selected entries SHALL be visually distinguished and the selection SHALL persist across cursor movement within the tab.

#### Scenario: Toggle selection
- **WHEN** the user navigates to an entry and presses `<space>`
- **THEN** the entry's selection state flips

#### Scenario: Selection persists on cursor move
- **WHEN** the user selects three entries and moves the cursor
- **THEN** all three remain selected

### Requirement: Send-to-synth action
The Journal tab SHALL provide a key (`s`) that opens an inline prompt for a target synth note. The prompt SHALL support: (a) fuzzy-picking an existing note marked `ft-synth: true`, or (b) entering a new note name (resolved against `synth.folder`). On confirmation, the tab SHALL call `plan_synth_scaffold` with the currently selected entries (or all displayed entries when no selection), `apply_synth_scaffold` to write the changes, then trigger the existing editor-handoff path opening `$EDITOR` at the bottom of the target file.

#### Scenario: Send selected entries to new synth note
- **WHEN** the user selects two entries, presses `s`, types `topic`, and confirms with the "new note" option
- **THEN** `Synthesis/topic.md` is created with `ft-synth: true` frontmatter and protected sections for the two entries; `$EDITOR` opens at the bottom

#### Scenario: Send to existing synth note
- **WHEN** the user presses `s` and picks an existing `Synthesis/topic.md` from the fuzzy picker
- **THEN** the scaffold is appended to that note and `$EDITOR` opens at the new bottom

#### Scenario: No selection sends all displayed entries
- **WHEN** the user presses `s` with no entries selected
- **THEN** all currently displayed entries are sent as the scaffold source

#### Scenario: Cancel prompt aborts action
- **WHEN** the user presses `s` and then `Esc`
- **THEN** the prompt closes and no file is modified

### Requirement: Help overlay covers new bindings
The Journal tab's `Tab::help_sections()` SHALL include entries for: `<space>` (toggle entry selection), `s` (send to synth), and `w` (in-window-only toggle, when applicable).

#### Scenario: Help overlay lists new bindings
- **WHEN** the user opens the `?` help overlay on the Journal tab
- **THEN** the overlay lists `space`, `s`, and `w` with their descriptions

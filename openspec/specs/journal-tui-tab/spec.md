# journal-tui-tab Specification

## Purpose
TBD - created by archiving change notes-journal-tui. Update Purpose after archive.
## Requirements
### Requirement: Gather tab registration
The TUI SHALL register a new top-level tab titled `Journal`, slotted after the existing `Graph` tab in `App::new`. The tab SHALL implement the `Tab` trait alongside the existing tabs.

#### Scenario: Tab appears in the tab strip
- **WHEN** the TUI starts
- **THEN** the tab strip lists `Graph`, `Tasks`, `Notes`, `Timeblocks`, and `Journal` (in that order)

#### Scenario: Tab can receive focus
- **WHEN** the user presses the digit key for the Gather tab's position
- **THEN** focus switches to the Gather tab and `on_focus` runs

### Requirement: Empty-state picker prompt
When no note has been selected, the Gather tab SHALL display a prompt instructing the user to press `/` to open a fuzzy note picker.

#### Scenario: Empty state on first focus
- **WHEN** the Gather tab is focused for the first time and no note has been queued
- **THEN** the tab body shows a "press `/` to pick a note" prompt and no entries

### Requirement: Note selection via fuzzy picker
Pressing `/` on the Gather tab SHALL open the existing `FuzzyPicker<VaultFilePickerSource>` overlay. Selecting a note from the picker SHALL trigger `ft_core::journal::build_journal` for that note's vault-relative path and replace the tab's body with the resulting entries.

#### Scenario: Picker opens on `/`
- **WHEN** the user presses `/` with no other overlay active
- **THEN** the fuzzy picker overlay opens on the Gather tab

#### Scenario: Picker selection loads the journal
- **WHEN** the user selects a note in the picker
- **THEN** the picker closes and the Gather tab renders the entries returned by `build_journal` for that note

#### Scenario: Picker dismissal preserves prior state
- **WHEN** the user presses `Esc` to dismiss the picker
- **THEN** the picker closes and the tab's previous state (empty prompt or prior entries) is preserved

### Requirement: BlameCache reuse across loads
The Gather tab SHALL hold a `BlameCache` instance for the duration of its session, loading from `.ft/cache/blame.msgpack` on first use and saving back after each successful `build_journal` call (best-effort; failures are logged but non-fatal).

#### Scenario: Subsequent loads warm the cache
- **WHEN** the user loads two different notes' journals in the same TUI session and no commits have happened between them
- **THEN** the second load reuses cached blame data for any overlapping source files (no extra `git blame` subprocesses for those files)

### Requirement: Entries persist across tab switches
Once a journal has been loaded, switching to another tab and back SHALL preserve `target_path` and `entries`. The tab SHALL NOT re-run `build_journal` on `on_focus` unless `queued_journal_for_path` is set or the user presses `R`.

#### Scenario: Tab switch round-trip
- **WHEN** the user loads a journal, switches to the Graph tab, and switches back to the Gather tab
- **THEN** the same entries are visible immediately without recomputation

### Requirement: Reload (`R`) re-runs build_journal
Pressing `R` on the Gather tab when a note is loaded SHALL re-invoke `build_journal` for the current `target_path` and replace `entries` with the result.

#### Scenario: Reload after editing the source
- **WHEN** the user has a journal loaded and presses `R`
- **THEN** `build_journal` is called again and any newly committed paragraphs appear in the refreshed entries

### Requirement: Clear (`c`) returns to picker prompt
Pressing `c` on the Gather tab SHALL clear `target_path` and `entries`, returning the tab to its empty-state prompt.

#### Scenario: Clear after a load
- **WHEN** the user has a journal loaded and presses `c`
- **THEN** the tab shows the empty-state prompt again

### Requirement: Entry navigation
The Gather tab SHALL support cursor movement through entries with `j`/`k` and `Up`/`Down` for one-entry steps, `Ctrl+D`/`Ctrl+U` for half-page jumps, and `g`/`G` for first/last entry.

#### Scenario: Step through entries
- **WHEN** the journal has multiple entries and the user presses `j`
- **THEN** the selected entry advances by one and the viewport scrolls if needed

#### Scenario: Jump to last entry
- **WHEN** the user presses `G`
- **THEN** the cursor moves to the last entry and the viewport scrolls to show it

### Requirement: Enter opens source in editor
Pressing `Enter` on a selected entry SHALL raise an editor-open request with the entry's `source_path` and a line target of the paragraph's first line, using the same editor-spawn path as other tabs' `o` keybinding.

#### Scenario: Editor opens at paragraph line
- **WHEN** the user selects an entry whose paragraph starts on line 42 of `Daily.md` and presses `Enter`
- **THEN** `$EDITOR` opens `Daily.md` jumping to line 42

#### Scenario: Empty journal: Enter is a no-op
- **WHEN** the tab is in its empty state (no entries) and the user presses `Enter`
- **THEN** nothing happens (no editor invocation, no error)

### Requirement: Help overlay lists Journal keybindings
The Gather tab's `help_sections()` SHALL contribute at least one `HelpSection` to the `?` overlay covering: picker open (`/`), reload (`R`), clear (`c`), navigation (`j`/`k`, `g`/`G`, `Ctrl+D`/`Ctrl+U`), and entry open (`Enter`).

#### Scenario: Help overlay shows Journal keymap
- **WHEN** the user presses `?` on the Gather tab
- **THEN** the overlay contains a section listing the Journal-specific bindings above

### Requirement: queue_journal_for hook on Tab trait
The `Tab` trait SHALL gain a `queue_journal_for(&mut self, note_path: &Path)` method with a default no-op implementation. The Gather tab SHALL override it to store the requested path; the path SHALL be consumed and turned into a load on the next `on_focus`.

#### Scenario: Queued path triggers load on focus
- **WHEN** `queue_journal_for("Foo.md")` is called on a Gather tab that is not currently focused, then focus switches to it
- **THEN** the tab loads the journal for `Foo.md` automatically (no manual `/` step required)

### Requirement: Multi-target mode queueing
The Gather tab SHALL accept a queued multi-target request from another tab (specifically the Pulse tab) via a new typed slot `queued_targets: RefCell<Option<MultiTargetRequest>>` on `App`, where `MultiTargetRequest` carries the resolved `Vec<NoteId>` of selected targets and an optional `window: WindowRange`. On `on_focus`, if the slot is `Some`, the tab SHALL consume it, call `build_journal` with the slice of targets, and replace its body with the resulting multi-target journal.

#### Scenario: Multi-target queue consumed on focus
- **WHEN** the Pulse tab handed off three targets and the user switches focus to the Gather tab
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
The Gather tab SHALL provide a key (`w`) to toggle in-window-only filtering. The toggle SHALL be active only when the tab is in multi-target mode AND a window range was passed in the queued request. When active, only entries whose `(source_file, line_start..=line_end)` overlaps an added-line in the queued window SHALL be displayed. When inactive (default), all-time entries are shown. The current toggle state SHALL be reflected in the tab header.

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
The Gather tab SHALL support multi-selecting journal entries with `<space>`. Selected entries SHALL be visually distinguished and the selection SHALL persist across cursor movement within the tab.

#### Scenario: Toggle selection
- **WHEN** the user navigates to an entry and presses `<space>`
- **THEN** the entry's selection state flips

#### Scenario: Selection persists on cursor move
- **WHEN** the user selects three entries and moves the cursor
- **THEN** all three remain selected

### Requirement: Send-to-synth action

The Gather tab SHALL provide a key (`s`) that opens an inline prompt for a target synth note. The prompt SHALL support: (a) fuzzy-picking an existing note marked `ft-synth: true`, or (b) entering a new note name (resolved against `synth.folder`). On confirmation, the tab SHALL call `plan_synth_scaffold` with the currently selected entries (or all displayed entries when no selection), `apply_synth_scaffold` to write the changes, then trigger the existing editor-handoff path opening `$EDITOR` at the bottom of the target file. When sending to an existing note, the scaffold SHALL apply the dedup-on-append invariant: entries whose `(source_path, body)` is already pinned in the picked note SHALL be dropped before planning (handled by `plan_synth_scaffold`'s append path). A second key (`n`) SHALL trigger the "send-to-synth-new-only" flow: after the user picks an existing note, the tab SHALL compute that note's last-synth watermark and ship only entries whose `date` is strictly greater than the watermark's date (in addition to the dedup-on-append invariant); when the watermark is `None`, it SHALL fall back to shipping all missing entries with an informational toast.

#### Scenario: Send selected entries to new synth note

- **WHEN** the user selects two entries, presses `s`, types `topic`, and confirms with the "new note" option
- **THEN** `Synthesis/topic.md` is created with `ft-synth: true` frontmatter and protected sections for the two entries; `$EDITOR` opens at the bottom

#### Scenario: Send to existing synth note dedups

- **WHEN** the user presses `s` and picks an existing `Synthesis/topic.md` that already pins some of the selected entries
- **THEN** only the not-yet-pinned entries are appended and `$EDITOR` opens at the new bottom; existing callouts are unchanged

#### Scenario: No selection sends all displayed entries

- **WHEN** the user presses `s` with no entries selected
- **THEN** all currently displayed entries are sent as the scaffold source (subject to dedup against the picked note)

#### Scenario: Cancel prompt aborts action

- **WHEN** the user presses `s` and then `Esc`
- **THEN** the prompt closes and no file is modified

#### Scenario: send-to-synth-new-only scopes to entries newer than the watermark

- **WHEN** the user presses `n`, picks a synth note whose last callout was pinned at a commit dated 2026-06-01, and confirms
- **THEN** only entries whose `date` is greater than 2026-06-01 are appended (after dedup)

#### Scenario: send-to-synth-new-only on a note with no watermark falls back

- **WHEN** the user presses `n`, picks a synth note with no callouts (or all-unreachable SHAs), and confirms
- **THEN** all missing entries are appended and an informational toast explains the watermark was unavailable

### Requirement: Help overlay covers new bindings

The Gather tab's `Tab::help_sections()` SHALL include entries for: `<space>` (toggle entry selection), `s` (send to synth), `n` (send only entries newer than the picked note's last synth), and `w` (in-window-only toggle, when applicable).

#### Scenario: Help overlay lists new bindings

- **WHEN** the user opens the `?` help overlay on the Gather tab
- **THEN** the overlay lists `space`, `s`, `n`, and `w` with their descriptions

### Requirement: Citation badge on journal rows
Gather tab rows SHALL render the citation state from the shared
snapshot's citation index: a `cited` marker (with note stem) for
exact citations, a visually distinct `cited*` marker for stale ones,
nothing for uncited. Badge data SHALL come from `TabCtx::snapshot` —
the tab performs no scanning of its own.

#### Scenario: Rows re-badge on snapshot refresh
- **WHEN** the user appends an entry to a synth note and the graph
  refresh completes
- **THEN** that entry's row shows the `cited` marker without reloading
  the tab

### Requirement: journal.toggle-uncited command
A `gather.toggle-uncited` command (bound to `u` by default) SHALL
toggle the feed between all entries and uncited-only (stale counts as
uncited, matching the CLI). The command SHALL be declared in the
tab's command/keymap statics so the `?` overlay, `docs/keybindings.md`,
and `ft commands list` pick it up.

#### Scenario: Toggle filters the visible feed
- **WHEN** the user presses `u` on a feed with cited and uncited
  entries
- **THEN** only non-`Cited` entries remain visible, and pressing `u`
  again restores the full feed

### Requirement: Note-context badges
When a target synth note is in play — the append-to-existing (`s`)
picker flow, or a journal opened from a synth note's
`ft-synth-targets` — row badges SHALL re-scope to that note:
`in note` when the entry is already pinned there (per the
filter_missing rule), `missing` otherwise, with a status-line
indicator naming the target note. Leaving the flow SHALL restore
global badges.

#### Scenario: Working a journal toward a note
- **WHEN** the user enters the `s` flow and picks synth note N
- **THEN** every feed row shows `in note` or `missing` relative to N,
  matching exactly what plan-time dedup would drop or keep

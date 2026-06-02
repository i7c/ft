## ADDED Requirements

### Requirement: Journal tab registration
The TUI SHALL register a new top-level tab titled `Journal`, slotted after the existing `Graph` tab in `App::new`. The tab SHALL implement the `Tab` trait alongside the existing tabs.

#### Scenario: Tab appears in the tab strip
- **WHEN** the TUI starts
- **THEN** the tab strip lists `Graph`, `Tasks`, `Notes`, `Timeblocks`, and `Journal` (in that order)

#### Scenario: Tab can receive focus
- **WHEN** the user presses the digit key for the Journal tab's position
- **THEN** focus switches to the Journal tab and `on_focus` runs

### Requirement: Empty-state picker prompt
When no note has been selected, the Journal tab SHALL display a prompt instructing the user to press `/` to open a fuzzy note picker.

#### Scenario: Empty state on first focus
- **WHEN** the Journal tab is focused for the first time and no note has been queued
- **THEN** the tab body shows a "press `/` to pick a note" prompt and no entries

### Requirement: Note selection via fuzzy picker
Pressing `/` on the Journal tab SHALL open the existing `FuzzyPicker<VaultFilePickerSource>` overlay. Selecting a note from the picker SHALL trigger `ft_core::journal::build_journal` for that note's vault-relative path and replace the tab's body with the resulting entries.

#### Scenario: Picker opens on `/`
- **WHEN** the user presses `/` with no other overlay active
- **THEN** the fuzzy picker overlay opens on the Journal tab

#### Scenario: Picker selection loads the journal
- **WHEN** the user selects a note in the picker
- **THEN** the picker closes and the Journal tab renders the entries returned by `build_journal` for that note

#### Scenario: Picker dismissal preserves prior state
- **WHEN** the user presses `Esc` to dismiss the picker
- **THEN** the picker closes and the tab's previous state (empty prompt or prior entries) is preserved

### Requirement: BlameCache reuse across loads
The Journal tab SHALL hold a `BlameCache` instance for the duration of its session, loading from `.ft/cache/blame.msgpack` on first use and saving back after each successful `build_journal` call (best-effort; failures are logged but non-fatal).

#### Scenario: Subsequent loads warm the cache
- **WHEN** the user loads two different notes' journals in the same TUI session and no commits have happened between them
- **THEN** the second load reuses cached blame data for any overlapping source files (no extra `git blame` subprocesses for those files)

### Requirement: Entries persist across tab switches
Once a journal has been loaded, switching to another tab and back SHALL preserve `target_path` and `entries`. The tab SHALL NOT re-run `build_journal` on `on_focus` unless `queued_journal_for_path` is set or the user presses `R`.

#### Scenario: Tab switch round-trip
- **WHEN** the user loads a journal, switches to the Graph tab, and switches back to the Journal tab
- **THEN** the same entries are visible immediately without recomputation

### Requirement: Reload (`R`) re-runs build_journal
Pressing `R` on the Journal tab when a note is loaded SHALL re-invoke `build_journal` for the current `target_path` and replace `entries` with the result.

#### Scenario: Reload after editing the source
- **WHEN** the user has a journal loaded and presses `R`
- **THEN** `build_journal` is called again and any newly committed paragraphs appear in the refreshed entries

### Requirement: Clear (`c`) returns to picker prompt
Pressing `c` on the Journal tab SHALL clear `target_path` and `entries`, returning the tab to its empty-state prompt.

#### Scenario: Clear after a load
- **WHEN** the user has a journal loaded and presses `c`
- **THEN** the tab shows the empty-state prompt again

### Requirement: Entry navigation
The Journal tab SHALL support cursor movement through entries with `j`/`k` and `Up`/`Down` for one-entry steps, `Ctrl+D`/`Ctrl+U` for half-page jumps, and `g`/`G` for first/last entry.

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
The Journal tab's `help_sections()` SHALL contribute at least one `HelpSection` to the `?` overlay covering: picker open (`/`), reload (`R`), clear (`c`), navigation (`j`/`k`, `g`/`G`, `Ctrl+D`/`Ctrl+U`), and entry open (`Enter`).

#### Scenario: Help overlay shows Journal keymap
- **WHEN** the user presses `?` on the Journal tab
- **THEN** the overlay contains a section listing the Journal-specific bindings above

### Requirement: queue_journal_for hook on Tab trait
The `Tab` trait SHALL gain a `queue_journal_for(&mut self, note_path: &Path)` method with a default no-op implementation. The Journal tab SHALL override it to store the requested path; the path SHALL be consumed and turned into a load on the next `on_focus`.

#### Scenario: Queued path triggers load on focus
- **WHEN** `queue_journal_for("Foo.md")` is called on a Journal tab that is not currently focused, then focus switches to it
- **THEN** the tab loads the journal for `Foo.md` automatically (no manual `/` step required)

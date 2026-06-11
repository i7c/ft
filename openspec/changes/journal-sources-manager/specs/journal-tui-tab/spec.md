## MODIFIED Requirements

### Requirement: Empty-state picker prompt

When no source has been loaded, the Journal tab SHALL display the sources strip in its empty state (`Sources (0)` on line one and `no sources loaded — press / to manage sources` on line two) and SHALL show no entry body below the strip. The legacy "press `/` to pick a note" centered prompt is removed.

#### Scenario: Empty state on first focus
- **WHEN** the Journal tab is focused for the first time and no sources have been queued
- **THEN** the tab body shows the two-line empty Sources strip and no entries below it

### Requirement: Note selection via fuzzy picker

Pressing `/` on the Journal tab SHALL open the Sources Manager modal (landed on its add-source fuzzy picker) instead of the legacy single-source picker overlay. Selecting a note from the manager's picker SHALL **append** that note to the current source set (not replace it) and SHALL keep the manager open for further edits. On manager commit (Enter or Esc) the Journal tab SHALL run `ft_core::journal::build_journal` against the resulting source set and replace the entry body with the result.

#### Scenario: Picker opens on `/`
- **WHEN** the user presses `/` with no other overlay active
- **THEN** the Sources Manager modal opens and its add-source fuzzy picker is in focus

#### Scenario: Picker selection appends to source set
- **WHEN** the user selects a note in the manager's picker
- **THEN** the selected note is appended to the current source set; the picker closes back to the manager's source list; the journal is NOT yet rebuilt (rebuild waits for manager commit)

#### Scenario: Manager commit rebuilds the journal
- **WHEN** the user presses Enter or Esc to close the manager after any source-set mutation
- **THEN** the Journal tab calls `build_journal` with the current source set and renders the resulting entries

#### Scenario: Picker dismissal preserves prior state
- **WHEN** the user presses `Esc` to dismiss the add-source picker without selecting
- **THEN** focus returns to the manager's source list with the source set unchanged

### Requirement: Entries persist across tab switches

Once a journal has been built, switching to another tab and back SHALL preserve the `sources` slot and the rendered `entries`. The tab SHALL NOT re-run `build_journal` on `on_focus` unless a queued single-target, multi-target, or AddSources request is set, or the user presses `R`.

#### Scenario: Tab switch round-trip
- **WHEN** the user loads a journal, switches to the Graph tab, and switches back to the Journal tab
- **THEN** the same entries and the same Sources strip are visible immediately without recomputation

### Requirement: Reload (`R`) re-runs build_journal

Pressing `R` on the Journal tab when at least one source is loaded SHALL re-invoke `build_journal` for the current source set and replace the entry body with the result. With zero sources, `R` SHALL be a no-op.

#### Scenario: Reload after editing the source
- **WHEN** the user has a journal loaded and presses `R`
- **THEN** `build_journal` is called again and any newly committed paragraphs appear in the refreshed entries

#### Scenario: Reload with empty source set is a no-op
- **WHEN** the user has zero sources loaded and presses `R`
- **THEN** nothing happens (no rebuild, no error)

### Requirement: Clear (`c`) returns to picker prompt

Pressing `c` on the Journal tab SHALL clear the source set and the entry body, returning the tab to its empty Sources-strip state. The same action is also reachable from the Sources Manager modal via its `c` action.

#### Scenario: Clear after a load
- **WHEN** the user has a journal loaded and presses `c`
- **THEN** the source set is emptied and the tab shows the empty Sources strip with no entries

### Requirement: Multi-target mode queueing

The Journal tab SHALL accept a queued multi-target request from another tab (specifically the Review tab) via a `RefCell<Option<MultiTargetRequest>>` slot, where `MultiTargetRequest` carries the resolved `Vec<JournalTarget>` of selected targets and an optional `window: JournalWindow`. On `on_focus`, if the slot is `Some`, the tab SHALL **replace** the current source set with `request.targets`, set the window, and call `build_journal` to render the merged entries.

#### Scenario: Multi-target queue consumed on focus
- **WHEN** the Review tab handed off three targets and the user switches focus to the Journal tab
- **THEN** the tab consumes the slot, replaces the source set with the three targets, attaches the window, and renders the multi-target journal

#### Scenario: Existing single-note queue still works
- **WHEN** `queued_journal_for` is set (legacy single-note queue from Graph `Shift+J`) and no multi-target or AddSources slot is set
- **THEN** the single-note queue replaces the source set with a one-element vector and renders unchanged

#### Scenario: Both single and multi slots set prefers multi
- **WHEN** both `queued_multi` and `queued_journal_for` are set simultaneously
- **THEN** the multi-target queue takes precedence and the single-note queue is cleared without execution

### Requirement: Help overlay lists Journal keybindings

The Journal tab's `help_sections()` SHALL contribute at least one `HelpSection` to the `?` overlay covering: open Sources Manager (`/`), add a source (`+`), clear (`c`), reload (`R`), entry navigation (`j`/`k`, `g`/`G`, `Ctrl+D`/`Ctrl+U`), and entry open (`Enter`).

#### Scenario: Help overlay shows Journal keymap
- **WHEN** the user presses `?` on the Journal tab
- **THEN** the overlay contains a section listing each of the above bindings

## ADDED Requirements

### Requirement: Sources slot replaces single/multi target fields

The Journal tab SHALL hold its loaded sources in a single mutable `sources: Vec<JournalTarget>` slot. All entry rebuilds SHALL read from this slot. Empty `sources` corresponds to the empty-state. The legacy `target: Option<JournalTarget>` and `multi_targets: Vec<JournalTarget>` fields SHALL be removed.

#### Scenario: Sources slot drives rebuild
- **WHEN** any path mutates `sources` (manager modal, single-target queue, multi-target queue, AddSources request) and `rebuild_journal` is called
- **THEN** `build_journal` is invoked with `sources.iter()` and the resulting entries replace the body

#### Scenario: Empty sources is the empty state
- **WHEN** the sources slot is empty
- **THEN** the entry body renders nothing and the Sources strip shows `(0)` with the empty-state hint

### Requirement: Append-or-Replace prompt for external AddSources requests

When the Journal tab receives `AppRequest::JournalAddSources { targets, default_mode }`, it SHALL raise an `AppRequest::OpenModal(JournalAppendOrReplace { incoming_targets, default_mode })` prompt rather than mutating sources directly. The prompt SHALL render at the bottom of the tab area with three actions: `[a] append`, `[r] replace`, and `[c] cancel`. The initial focus SHALL match `default_mode`.

#### Scenario: Append commits union
- **WHEN** the prompt is up with current sources `[Foo.md]` and incoming targets `[Bar.md, Baz (ghost)]` and the user presses `a` (or Enter while `Append` is focused)
- **THEN** the sources slot becomes `[Foo.md, Bar.md, Baz (ghost)]` (incoming order preserved, no duplicates) and the journal rebuilds

#### Scenario: Replace commits replacement
- **WHEN** the prompt is up with current sources `[Foo.md]` and incoming targets `[Bar.md]` and the user presses `r`
- **THEN** the sources slot becomes `[Bar.md]` (replacing the previous set) and the journal rebuilds

#### Scenario: Cancel preserves current sources
- **WHEN** the prompt is up and the user presses `c` or `Esc`
- **THEN** the sources slot is unchanged and the journal is not rebuilt

#### Scenario: Append deduplicates against current sources
- **WHEN** current sources are `[Foo.md, Bar.md]` and incoming targets are `[Bar.md, Baz.md]` and the user appends
- **THEN** the final sources are `[Foo.md, Bar.md, Baz.md]` (Bar.md not duplicated)

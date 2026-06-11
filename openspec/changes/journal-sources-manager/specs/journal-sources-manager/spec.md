## ADDED Requirements

### Requirement: Sources strip is always visible on the Journal tab

The Journal tab SHALL render a `Sources` strip at the top of its inner area (inside the existing block border) in every state — empty, single-source, and multi-source. The strip SHALL occupy exactly two terminal rows so that the entry-list scroll math remains stable across state transitions.

#### Scenario: Strip rendered in empty state
- **WHEN** the Journal tab has no sources loaded and the user focuses the tab
- **THEN** the first two rows of the inner area read `Sources (0)` on line one and `no sources loaded — press / to manage sources` (in dim) on line two

#### Scenario: Strip rendered with one source
- **WHEN** the Journal tab has exactly one source, the note `Inbox/Foo.md`
- **THEN** line one reads `Sources (1)` and line two reads `Inbox/Foo.md`

#### Scenario: Strip rendered with multiple sources
- **WHEN** the Journal tab has three sources `[Foo.md, Bar.md, Baz (ghost)]`
- **THEN** line one reads `Sources (3)` and line two reads `Foo.md, Bar.md, Baz (ghost)` (comma-separated, in source-set insertion order)

#### Scenario: Strip surfaces window when attached
- **WHEN** the source set carries an attached window range of "last 7 days"
- **THEN** line one reads `Sources (N) [window: since 7d]`

#### Scenario: Strip surfaces in-window filter cue when active
- **WHEN** the user has toggled the in-window-only filter and a window is attached
- **THEN** line one reads `Sources (N) [window: since 7d] [filter: in-window]`

#### Scenario: Strip truncates long source lists
- **WHEN** the joined source-label string is wider than the inner area's column count
- **THEN** line two is truncated and ends with `…, +K more` where K is the number of sources elided

### Requirement: Sources Manager modal lists every current source

The Journal tab SHALL provide a Sources Manager modal that lists every current source as one navigable row, opened via the `/` chord or the `+` chord. The modal SHALL be installed on the App's `ActiveModal` slot via `AppRequest::OpenModal`.

#### Scenario: Manager lists current sources in insertion order
- **WHEN** the user has loaded three sources `[Foo.md, Bar.md, Baz (ghost)]` and presses `/`
- **THEN** the modal opens listing three rows in the same order, with the first row visually focused

#### Scenario: `+` chord opens the manager landed on add-source mode
- **WHEN** the user presses `+` on the Journal tab
- **THEN** the manager opens with the add-source fuzzy picker already in focus

#### Scenario: `/` chord opens the manager landed on add-source mode
- **WHEN** the user presses `/` on the Journal tab
- **THEN** the manager opens with the add-source fuzzy picker already in focus (matching the legacy `/` muscle memory of "press `/`, type, Enter to add")

### Requirement: Sources Manager supports remove, clear, and add actions

The Sources Manager modal SHALL provide keyboard actions to remove the focused source, clear all sources, and add a new source via a ghost-aware fuzzy picker.

#### Scenario: Remove focused source
- **WHEN** the focused row in the manager is `Bar.md` and the user presses `d`
- **THEN** `Bar.md` is removed from the source set and the modal redraws with the remaining rows; focus moves to the next remaining row (or the previous one if the removed row was the last)

#### Scenario: Clear all sources
- **WHEN** the user presses `c` (clear) in the manager
- **THEN** the source set is emptied, the modal redraws showing no rows and a `no sources` placeholder

#### Scenario: Add a new source via fuzzy picker
- **WHEN** the user presses `a` in the manager
- **THEN** an inner fuzzy picker opens; selecting a result from it appends the chosen source to the set without closing the manager

#### Scenario: Modal stays open across mutations
- **WHEN** the user removes one source and then adds another in the same manager session
- **THEN** both mutations are reflected in the modal's row list without the modal closing between them

### Requirement: Sources Manager picker is ghost-aware

The add-source fuzzy picker hosted by the Sources Manager SHALL surface both real-note hits (from the vault's fuzzy index) and ghost names (from the current `Graph`) in a single ranked result list, with each item carrying a typed `JournalSourceHit` payload.

#### Scenario: Ghost row appears alongside real-note rows
- **WHEN** the vault has `Phantasm.md` and the graph contains a ghost named `Phantom` and the user types `pha` in the manager's add-source picker
- **THEN** the result list includes both `Phantasm.md` and `Phantom (ghost)` rows, ranked by the same fuzzy matcher

#### Scenario: Selecting a ghost adds a JournalTarget::Ghost
- **WHEN** the user selects `Phantom (ghost)` from the picker
- **THEN** the source set gains a `JournalTarget::Ghost("Phantom")` entry (not a `JournalTarget::Note`)

#### Scenario: Selecting a real-note hit adds a JournalTarget::Note
- **WHEN** the user selects `Phantasm.md` from the picker
- **THEN** the source set gains a `JournalTarget::Note("Phantasm.md")` entry with the vault-relative path

### Requirement: Manager commit triggers a journal rebuild

When the Sources Manager modal closes via `Enter` or `Esc`, the Journal tab SHALL rebuild the journal feed from the current source set. The entry cursor SHALL reset to 0 and the entry multi-select set SHALL be cleared. The in-window-only filter state SHALL be preserved iff `sources.len() >= 2` and a window is attached.

#### Scenario: Rebuild after add
- **WHEN** the user adds a new source and presses Enter to close the manager
- **THEN** the journal entries are rebuilt to include matches against the new source set, the cursor is at entry 0, and no entries are multi-selected

#### Scenario: Rebuild after remove
- **WHEN** the user removes a source and presses Esc to close the manager
- **THEN** the journal entries are rebuilt without that source, the cursor is at entry 0, and no entries are multi-selected

#### Scenario: In-window-only preserved when still meaningful
- **WHEN** the user enters the manager with `in_window_only = true` and a 7-day window, removes one of three sources (leaving two), and closes the manager
- **THEN** `in_window_only` remains `true` and the filter is reapplied

#### Scenario: In-window-only cleared when no longer meaningful
- **WHEN** the user enters the manager with `in_window_only = true` and removes sources until only one remains
- **THEN** `in_window_only` is cleared to `false` (the filter requires multi-target mode with a window)

### Requirement: Sources Manager visualizes the modal title and key cues

The Sources Manager modal SHALL render with a titled block (`Journal Sources — N source(s)`) and a footer key cue line listing the available actions (`a add  d remove  c clear  Enter commit  Esc cancel`).

#### Scenario: Title reflects count
- **WHEN** the user opens the manager with 2 sources loaded
- **THEN** the modal's title bar reads `Journal Sources — 2 source(s)`

#### Scenario: Footer cue lists actions
- **WHEN** the manager is open (not inside the add-source picker)
- **THEN** the bottom row of the modal area reads `a add  d remove  c clear  Enter commit  Esc cancel`

## MODIFIED Requirements

### Requirement: Graph tab `J` keybinding opens the Journal tab

When the graph tab is focused and the currently-selected row is a `NodeKind::Note` or `NodeKind::Ghost`, pressing `Shift+J` SHALL raise a request that switches the App's active tab to the Journal tab and queues the selected target on it. The Journal tab SHALL **replace** its current source set with the single queued target on consume.

#### Scenario: Jump from Note row
- **WHEN** the user has the Graph tab focused with a `NodeKind::Note` row selected and presses `Shift+J`
- **THEN** focus switches to the Journal tab and the Journal tab's source set becomes `[Note(path)]` with that note's journal rendered

#### Scenario: Jump from Ghost row
- **WHEN** the user has the Graph tab focused with a `NodeKind::Ghost` row selected and presses `Shift+J`
- **THEN** focus switches to the Journal tab and the Journal tab's source set becomes `[Ghost(raw)]` with that ghost's journal rendered

#### Scenario: Jump from non-Note/non-Ghost row produces a toast
- **WHEN** the user presses `Shift+J` with a Directory, Task, or Paragraph row selected
- **THEN** focus does NOT switch and a "select a Note or Ghost row" toast is queued

#### Scenario: Jump with empty selection produces a toast
- **WHEN** the user presses `Shift+J` with no row selected (empty tree)
- **THEN** focus does NOT switch and an informational toast is queued

## ADDED Requirements

### Requirement: Graph tab `Shift+A` appends multi-selection to Journal sources

When the Graph tab is focused, pressing `Shift+A` SHALL raise an `AppRequest::JournalAddSources` carrying the active view's `multi_selected` set (or the cursor row if `multi_selected` is empty), with each entry resolved to a `JournalTarget::Note` or `JournalTarget::Ghost` depending on its node kind. Directory, Task, and Paragraph rows in the selection SHALL be filtered out silently. The default mode on the resulting AppRequest SHALL be `Append` (the Journal tab raises a prompt; user can still pick Replace there).

#### Scenario: Multi-selection mapped to targets
- **WHEN** the user has multi-selected `Foo.md` (Note) and `Phantom` (Ghost) and presses `Shift+A`
- **THEN** an `AppRequest::JournalAddSources { targets: [Note("Foo.md"), Ghost("Phantom")], default_mode: Append }` is raised

#### Scenario: Empty multi-selection falls back to cursor row
- **WHEN** `multi_selected` is empty and the cursor is on `Foo.md` (Note) and the user presses `Shift+A`
- **THEN** an `AppRequest::JournalAddSources { targets: [Note("Foo.md")], default_mode: Append }` is raised

#### Scenario: Non-Note/Ghost rows in selection are filtered
- **WHEN** `multi_selected` contains `Foo.md` and a Directory row, and the user presses `Shift+A`
- **THEN** only `Note("Foo.md")` is in the resulting targets list (Directory silently skipped)

#### Scenario: Selection of only non-eligible rows yields a toast
- **WHEN** `multi_selected` contains only Directory rows and the user presses `Shift+A`
- **THEN** no AppRequest is raised and an error toast is queued (`no Note or Ghost rows selected`)

### Requirement: Help overlay on graph tab lists `Shift+A`

The graph tab's `help_sections()` SHALL include `Shift+A: append selected (or cursor) to Journal sources` in the same section that lists `Shift+J`.

#### Scenario: Help mentions the new chord
- **WHEN** the user presses `?` on the graph tab
- **THEN** the overlay contains a row mentioning `Shift+A` and the append-to-Journal-sources action

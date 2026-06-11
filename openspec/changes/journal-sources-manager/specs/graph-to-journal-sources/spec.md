## ADDED Requirements

### Requirement: Graph tab `Ctrl+J` adds selected nodes to Journal sources

The Graph tab SHALL provide a `graph.add-to-journal-sources` command (bound to `Ctrl+J` in the default keymap; `+` was the original spec choice but conflicts with the keymap parser's modifier separator, and `Shift+A` collides with `graph.append`) that ships the active view's multi-selection (or the cursor row when the multi-selection is empty) to the Journal tab as additional sources.

#### Scenario: Multi-selection drives the target list
- **WHEN** the user has three rows multi-selected on the Graph tab (`Foo.md`, `Bar.md`, ghost `Phantom`) and presses `Ctrl+J`
- **THEN** an `AppRequest::JournalAddSources { targets, default_mode: Append }` is raised with `targets = [Note("Foo.md"), Note("Bar.md"), Ghost("Phantom")]`

#### Scenario: Cursor row used when multi-selection is empty
- **WHEN** no rows are multi-selected and the cursor is on `Foo.md` and the user presses `Ctrl+J`
- **THEN** an `AppRequest::JournalAddSources { targets: [Note("Foo.md")], default_mode: Append }` is raised

#### Scenario: Non-Note/non-Ghost rows are filtered out
- **WHEN** the multi-selection includes a Directory row and a Task row alongside `Foo.md`
- **THEN** the Directory and Task rows are silently skipped and `targets = [Note("Foo.md")]`

#### Scenario: Empty resolved list yields a toast and no request
- **WHEN** the multi-selection contains only Directory and Task rows (all skipped)
- **THEN** no `AppRequest::JournalAddSources` is raised and an error toast is queued ("no Note or Ghost rows selected")

### Requirement: AppRequest::JournalAddSources is routed by the App to the Journal tab

`App::service_request` SHALL handle `AppRequest::JournalAddSources { targets, default_mode }` by switching the active tab to the Journal tab and calling `Tab::queue_journal_add_sources` on it. The Journal tab's hook SHALL store the request; on next `on_focus` (i.e. immediately after the tab switch) the request SHALL be consumed to open the Append/Replace prompt.

#### Scenario: Tab switches to Journal on AddSources
- **WHEN** the Graph tab raises `AppRequest::JournalAddSources { targets: [...], default_mode: Append }`
- **THEN** on the next event-loop iteration the App calls `queue_journal_add_sources` on the Journal tab and switches the active tab to it

#### Scenario: Queued request consumed on focus
- **WHEN** the Journal tab gains focus with a queued AddSources request stored
- **THEN** the tab consumes the slot and raises `AppRequest::OpenModal(JournalAppendOrReplace { incoming_targets, default_mode })` so the prompt renders next frame

### Requirement: Help overlay on Graph tab lists `Ctrl+J`

The Graph tab's `Tab::help_sections()` SHALL include a row for `Ctrl+J` describing the "append selected to Journal sources" action, in the same section that already documents `Shift+J`.

#### Scenario: Help overlay lists the chord
- **WHEN** the user presses `?` on the Graph tab
- **THEN** the overlay contains a row reading `Ctrl+J: append selected (or cursor) to Journal sources`

### Requirement: Tab trait gains a `queue_journal_add_sources` hook

The `Tab` trait SHALL gain a `queue_journal_add_sources(&mut self, targets: Vec<JournalTarget>, default_mode: AppendOrReplaceMode)` method with a default no-op implementation. The Journal tab SHALL override it to store the pending request; other tabs SHALL retain the default.

#### Scenario: Default is a no-op on non-Journal tabs
- **WHEN** the App routes `JournalAddSources` to any tab that does not override `queue_journal_add_sources`
- **THEN** the call is a no-op and no panic occurs

#### Scenario: Journal tab override stores the request
- **WHEN** the App calls `queue_journal_add_sources(targets, Append)` on the Journal tab
- **THEN** the targets and mode are held in a typed slot until the next `on_focus`, at which point they are consumed

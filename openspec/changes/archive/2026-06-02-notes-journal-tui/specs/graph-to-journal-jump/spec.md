## ADDED Requirements

### Requirement: Graph tab `J` keybinding opens the Journal tab
When the graph tab is focused and the currently-selected row is a `NodeKind::Note`, pressing `Shift+J` SHALL raise a request that switches the App's active tab to the Journal tab and queues the selected note's vault-relative path on it.

#### Scenario: Jump from Note row
- **WHEN** the user has the graph tab focused with a Note row selected and presses `Shift+J`
- **THEN** focus switches to the Journal tab and that note's journal is loaded automatically

#### Scenario: Jump from non-Note row produces a toast
- **WHEN** the user presses `Shift+J` with a Directory, Ghost, Task, or Paragraph row selected
- **THEN** focus does NOT switch and a "select a Note row" toast is queued

#### Scenario: Jump with empty selection produces a toast
- **WHEN** the user presses `Shift+J` with no row selected (empty tree)
- **THEN** focus does NOT switch and an informational toast is queued

### Requirement: App services the Journal-jump request via existing pending_request channel
The graph tab's `Shift+J` handler SHALL raise the jump as an `AppRequest` variant (e.g. `AppRequest::JournalForNote { path }`). `App::service_request` SHALL handle it by calling `queue_journal_for(&path)` on the Journal tab and switching the active tab index to the Journal tab.

#### Scenario: AppRequest delivered and serviced
- **WHEN** the graph tab raises `AppRequest::JournalForNote { path }`
- **THEN** on the next event-loop iteration the App calls `queue_journal_for(&path)` on the Journal tab and switches to it

### Requirement: Help overlay on graph tab lists `Shift+J`
The graph tab's `help_sections()` SHALL include `Shift+J: open Journal for selected note` (or equivalent wording) in an appropriate section so the binding is discoverable via the `?` overlay.

#### Scenario: Help mentions the jump binding
- **WHEN** the user presses `?` on the graph tab
- **THEN** the overlay contains a row mentioning `Shift+J` and the Journal jump

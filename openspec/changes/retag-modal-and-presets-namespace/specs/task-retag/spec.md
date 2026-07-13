## ADDED Requirements

### Requirement: `retag_tags` config field

The system SHALL store a `retag_tags` field on the `Tasks` config struct as
a list of tag name strings (no leading `#`). It SHALL default to an empty
list when `[tasks.retag_tags]` is absent from config.

#### Scenario: Default empty when absent
- **WHEN** the config contains no `[tasks]` section (or no `retag_tags` key)
- **THEN** `config.tasks.retag_tags` is an empty `Vec`

#### Scenario: User defines the list
- **WHEN** the config contains `[tasks]\nretag_tags = ["wait", "computer", "physical"]`
- **THEN** `config.tasks.retag_tags` equals `["wait", "computer", "physical"]`

#### Scenario: Bare names, no leading hash
- **WHEN** the config contains `[tasks]\nretag_tags = ["#wait", "computer"]`
- **THEN** loading succeeds and each entry's leading `#` is preserved verbatim as stored (the system does not strip it; documented expectation is bare names)

### Requirement: `tasks.retag` command opens the retag picker

The Tasks SearchView SHALL expose a `tasks.retag` command that, when the
`retag_tags` list is non-empty, opens a fuzzy picker modal over the configured
tags. The picker SHALL be an `ActiveModal` variant implemented via the
existing `Modal` trait, riding the same modal→view seam as the task-preset
picker.

#### Scenario: Open picker when list is non-empty
- **WHEN** `config.tasks.retag_tags` is non-empty and the user triggers `tasks.retag`
- **THEN** a fuzzy picker modal opens listing every entry in `retag_tags`, labeled with a leading `#`

#### Scenario: Toast when list is empty
- **WHEN** `config.tasks.retag_tags` is empty and the user triggers `tasks.retag`
- **THEN** no modal opens and a toast informs the user that no retag tags are configured

#### Scenario: Picker appears in help overlay
- **WHEN** the user opens the `?` help overlay on the Tasks tab
- **THEN** the `tasks.retag` chord is listed under the "Mutations" group with its description

### Requirement: Selecting a tag swaps it into the selected task

On selection of a tag in the retag picker, the system SHALL rewrite the
selected task's description so that every inline `#tag` word whose bare form
is in `retag_tags` is removed and the selected tag is appended. Inline tags
not in `retag_tags` SHALL be preserved verbatim, as SHALL all non-tag
description text. The change SHALL be persisted via the existing
`update_task_line` path with the expected-`Task` guard.

#### Scenario: Swap replaces prior list tag, preserves others
- **WHEN** the selected task's description is `"Pay invoice #finance #computer"`, `retag_tags = ["wait", "computer", "physical"]`, and the user picks `wait`
- **THEN** the persisted description is `"Pay invoice #finance #wait"` — `#computer` (a list member) removed, `#finance` (not a list member) preserved, `#wait` appended

#### Scenario: Picking when no list tag is present just appends
- **WHEN** the selected task's description is `"Email Sarah #finance"`, `retag_tags = ["wait", "computer", "physical"]`, and the user picks `computer`
- **THEN** the persisted description is `"Email Sarah #finance #computer"`

#### Scenario: Non-list tags glued to punctuation are preserved
- **WHEN** the selected task's description is `"fix #computer-thing"`, `retag_tags = ["computer"]`, and the user picks `wait`
- **THEN** the persisted description is `"fix #computer-thing #wait"` — `#computer-thing` is not the word `#computer` and is not matched/removed

#### Scenario: Esc cancels with no write
- **WHEN** the retag picker is open and the user presses Esc
- **THEN** the picker closes and the selected task's file is unchanged

#### Scenario: Stale-line guard surfaces an error
- **WHEN** the selected task's line changed on disk since the snapshot was built and the user picks a tag
- **THEN** the write fails with the `LineChanged` error surfaced as a toast and the graph is refreshed; no panic

### Requirement: Retag routes through `TasksRequest::RetagSelected`

A new `TasksRequest::RetagSelected(String)` variant SHALL carry the picked
tag (bare, no `#`) from the picker modal to the SearchView via
`AppRequest::Tasks`. The `TasksTab::handle_tasks_request` SHALL route it to
a `SearchView::apply_retag(tag)` method that performs the write. This mirrors
the existing `ApplyPreset(String)` seam.

#### Scenario: Picker commits via the request channel
- **WHEN** the user picks a tag in the retag picker
- **THEN** the modal posts `AppRequest::Tasks(TasksRequest::RetagSelected(tag))` and returns `ModalOutcome::Closed`

#### Scenario: SearchView services the retag
- **WHEN** `TasksTab::handle_tasks_request` receives `RetagSelected(tag)`
- **THEN** the active SearchView's `apply_retag(tag)` runs, persisting the retag against the cursor's selected task

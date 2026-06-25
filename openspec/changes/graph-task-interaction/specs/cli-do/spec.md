# cli-do

## ADDED Requirements

### Requirement: `ft do` dispatches `tasks.cancel-by-id` headlessly

The `ft do` headless dispatcher SHALL handle `tasks.cancel-by-id` by resolving
a single task by its `id` selector argument, calling
`ft_core::task::ops::cancel_task` against its absolute path and source line
(with `--arg on=<date>` optional, defaulting to today), and reporting the
result. This mirrors the existing `tasks.complete-by-id` headless handler.

#### Scenario: cancel a task by id headlessly

- **WHEN** `ft do tasks.cancel-by-id --arg id=abc123` is run in a vault where a task with `id = "abc123"` exists
- **THEN** `ops::cancel_task` is called against that task's path and line
- **AND** the task is marked `Cancelled` on disk
- **AND** the command exits 0 and reports the cancelled task

#### Scenario: cancel-by-id with a done-date override

- **WHEN** `ft do tasks.cancel-by-id --arg id=abc123 --arg on=2026-06-25` is run
- **THEN** the cancelled task's `cancelled` date is set to `2026-06-25`

### Requirement: `ft do` dispatches `tasks.edit-by-id` headlessly

The `ft do` headless dispatcher SHALL handle `tasks.edit-by-id` by resolving a
single task by its `id` selector argument and applying field updates
(`--arg due=…`, `--arg scheduled=…`, `--arg priority=…`, `--arg tags=…`,
`--arg description=…`) via `ft_core::task::ops::update_task_line`. Only the
supplied fields are changed; omitted fields are preserved.

#### Scenario: edit a task's due date by id headlessly

- **WHEN** `ft do tasks.edit-by-id --arg id=abc123 --arg due=2026-07-01` is run
- **THEN** `ops::update_task_line` sets the task's `due` to `2026-07-01` and leaves all other fields unchanged
- **AND** the command exits 0 and reports the updated task

#### Scenario: edit multiple fields by id headlessly

- **WHEN** `ft do tasks.edit-by-id --arg id=abc123 --arg priority=High --arg due=2026-07-01` is run
- **THEN** the task's `priority` becomes `High` and `due` becomes `2026-07-01`

### Requirement: `ft do` rejects task create/open modal commands

`ft do` SHALL reject (exit 2) any `graph.task-*` command with `opens_modal: true`
or any command with no headless handler, consistent with the existing policy.
The actionable task verbs that DO get headless handlers are `tasks.complete-by-id`,
`tasks.cancel-by-id`, and `tasks.edit-by-id`.

#### Scenario: graph task verbs are not headless-dispatchable

- **WHEN** `ft do graph.task-complete` is run
- **THEN** the command exits 2 with a message that the command opens a modal / has no headless handler

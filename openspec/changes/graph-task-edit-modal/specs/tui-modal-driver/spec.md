# tui-modal-driver

## ADDED Requirements

### Requirement: `TaskEdit`, `TaskLeader`, and `TaskCreate` active-modal variants

The `ActiveModal` enum SHALL include `TaskEdit(Box<TaskEditState>)`,
`TaskLeader(Box<TaskLeader>)`, and `TaskCreate(Box<TaskCreateState>)`
variants. Each SHALL implement the `Modal` trait and be wired into the
`ActiveModal` dispatch (`handle_event`/`render`/`keymap_help`/`name`/
`commands`/`keymap`/`dispatch_command`).

#### Scenario: TaskEdit modal dispatch

- **WHEN** `ActiveModal::TaskEdit(s)` is the active modal
- **THEN** `handle_event`/`render`/`name`/`commands`/`keymap` delegate to `TaskEditState`'s `Modal` impl, and `name()` returns `"task-edit"`

#### Scenario: TaskLeader modal dispatch

- **WHEN** `ActiveModal::TaskLeader(s)` is the active modal
- **THEN** `handle_event`/`render`/`name`/`commands`/`keymap` delegate to `TaskLeader`'s `Modal` impl, and `name()` returns `"task-leader"`

#### Scenario: TaskCreate modal dispatch

- **WHEN** `ActiveModal::TaskCreate(s)` is the active modal
- **THEN** `handle_event`/`render`/`name`/`commands`/`keymap` delegate to `TaskCreateState`'s `Modal` impl, and `name()` returns `"task-create"`

### Requirement: `TaskEdit` commit routes through `AppRequest`

The `TaskEdit` modal's commit SHALL post
`AppRequest::GraphTaskEdit { path, line, fields }` rather than mutating
disk directly, so the Graph tab host can plan/apply/refresh against
in-memory graph state — matching the established modal→`AppRequest`
pattern used by `GraphCommitRename`, `GraphCreateSubdir`, etc.

#### Scenario: TaskEdit commit posts GraphTaskEdit

- **WHEN** the user commits the `TaskEdit` modal (Enter/Ctrl+S) with valid fields
- **THEN** `AppRequest::GraphTaskEdit { path, line, fields }` is posted via `ctx.pending_request` and the modal closes

### Requirement: `TaskLeader` opens `TaskCreate`; create commit routes through `AppRequest`

The `TaskLeader` modal SHALL be seeded at open time with the focused
row's note path and (when a Task is focused) its `(file, line)`. Its
`c`/`s` SHALL swap to the `TaskCreate` modal via
`ModalOutcome::OpenSibling` (so the popup opens in the same event pass,
without an inter-frame `pending_request` hop). The `TaskCreate` modal's
commit SHALL post `AppRequest::GraphTaskCommitCreate { fields, target,
subtask_parent }` rather than mutating disk directly, so the Graph tab
host resolves the target/position and applies via `ops::create_task`.

#### Scenario: TaskLeader c opens a seeded TaskCreate

- **WHEN** the `TaskLeader` modal is active and the user presses `c`
- **THEN** it returns `ModalOutcome::OpenSibling(TaskCreate)` with the popup's `target` field seeded from the focused note

#### Scenario: TaskLeader s with no focused task toasts

- **WHEN** the `TaskLeader` modal is active, no Task is focused, and the user presses `s`
- **THEN** a "select a task first" toast is queued and the modal closes without opening `TaskCreate`

#### Scenario: TaskCreate commit posts GraphTaskCommitCreate

- **WHEN** the user commits the `TaskCreate` modal (Ctrl+S) with a non-empty description
- **THEN** `AppRequest::GraphTaskCommitCreate { fields, target, subtask_parent }` is posted via `ctx.pending_request` and the modal closes

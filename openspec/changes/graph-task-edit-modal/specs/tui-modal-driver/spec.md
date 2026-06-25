# tui-modal-driver

## ADDED Requirements

### Requirement: `TaskEdit` and `TaskLeader` active-modal variants

The `ActiveModal` enum SHALL include `TaskEdit(TaskEditState)` and
`TaskLeader` variants. Each SHALL implement the `Modal` trait and be
wired into the `ActiveModal` dispatch (`handle_event`/`render`/
`keymap_help`/`name`/`commands`/`keymap`/`dispatch_command`).

#### Scenario: TaskEdit modal dispatch

- **WHEN** `ActiveModal::TaskEdit(s)` is the active modal
- **THEN** `handle_event`/`render`/`name`/`commands`/`keymap` delegate to `TaskEditState`'s `Modal` impl, and `name()` returns `"task-edit"`

#### Scenario: TaskLeader modal dispatch

- **WHEN** `ActiveModal::TaskLeader` is the active modal
- **THEN** `handle_event`/`render`/`name`/`commands`/`keymap` delegate to `TaskLeader`'s `Modal` impl, and `name()` returns `"task-leader"`

### Requirement: `TaskEdit` commit routes through `AppRequest`

The `TaskEdit` modal's commit SHALL post
`AppRequest::GraphTaskEdit { path, line, fields }` rather than mutating
disk directly, so the Graph tab host can plan/apply/refresh against
in-memory graph state — matching the established modal→`AppRequest`
pattern used by `GraphCommitRename`, `GraphCreateSubdir`, etc.

#### Scenario: TaskEdit commit posts GraphTaskEdit

- **WHEN** the user commits the `TaskEdit` modal (Enter/Ctrl+S) with valid fields
- **THEN** `AppRequest::GraphTaskEdit { path, line, fields }` is posted via `ctx.pending_request` and the modal closes

### Requirement: `TaskLeader` routes create through `AppRequest`

The `TaskLeader` modal's `c`/`s` SHALL post
`AppRequest::GraphTaskCreate { kind }` rather than opening a quickline
directly, so the Graph tab host owns the quickline seeding.

#### Scenario: TaskLeader c posts GraphTaskCreate

- **WHEN** the `TaskLeader` modal is active and the user presses `c`
- **THEN** `AppRequest::GraphTaskCreate { kind: TopLevel { seed_path } }` is posted and the modal closes

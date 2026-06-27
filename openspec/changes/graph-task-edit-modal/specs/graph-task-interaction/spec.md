# graph-task-interaction

## ADDED Requirements

### Requirement: Graph tab `graph.task-edit-popup` opens the shared edit form

The Graph tab SHALL open the shared `EditPopup` (the same form the Tasks
tab uses) in edit mode when `graph.task-edit-popup` (`e`) is dispatched on
a `NodeKind::Task` row. On commit (`Enter`/`Ctrl+S`) it SHALL post an
`AppRequest::GraphTaskEdit { path, line, fields }` serviced by the Graph
tab via `ops::update_task_line`, then refresh and restore the cursor. On a
non-Task row `e` SHALL show an error toast.

#### Scenario: e opens the edit popup on a Task row

- **WHEN** the focused row is a `NodeKind::Task` and the user presses `e`
- **THEN** the `TaskEdit` modal opens pre-populated with the task's current fields
- **AND** on `Enter`, the task is updated on disk via `ops::update_task_line`, the graph refreshes, and the cursor returns to the task

#### Scenario: e toasts on a non-Task row

- **WHEN** the user presses `e` and the focused row is not a `NodeKind::Task`
- **THEN** an error toast is shown and no popup opens

### Requirement: Graph tab task create leader chord

The Graph tab SHALL provide an `a` leader chord for creating tasks: `ac`
creates a new top-level task (the shared `EditPopup` in New mode, with its
`target` field seeded from the focused note's path), `as` creates a new
subtask under the focused task. The leader is a transient `TaskLeader`
modal seeded at open time with the focused row; `c`/`s` swap to the
`TaskCreate` popup via `ModalOutcome::OpenSibling`; any other key
(including `Esc`) cancels. On `Ctrl+S` the popup posts
`AppRequest::GraphTaskCommitCreate`, serviced via `ops::create_task`.

#### Scenario: ac creates a top-level task

- **WHEN** the user presses `a` then `c`
- **THEN** the `TaskCreate` popup opens with its `target` field seeded from the focused note's path
- **AND** on `Ctrl+S` `ops::create_task` inserts the task and the graph refreshes

#### Scenario: as creates a subtask under the focused task

- **WHEN** the focused row is a `NodeKind::Task` and the user presses `a` then `s`
- **THEN** the `TaskCreate` popup opens with its subtask parent set to the focused task's `(source_file, source_line)`
- **AND** on `Ctrl+S` `ops::create_task` inserts the subtask nested under the parent

#### Scenario: as on a non-Task row toasts

- **WHEN** the focused row is not a `NodeKind::Task` and the user presses `a` then `s`
- **THEN** a "select a task first" toast is shown and no popup opens

#### Scenario: a leader cancels on other key

- **WHEN** the `a` leader is active and the user presses a key other than `c` or `s` (including `Esc`)
- **THEN** the leader cancels and no popup opens

### Requirement: Graph tab note-scoped task view

The Graph tab SHALL provide a `v` command that, on a `NodeKind::Note` or
`NodeKind::Directory` row, rewrites the active view's query to show that
note's (or directory's) task subtree: top-level tasks via `has-task` and
their subtasks via `subtask`. The resulting view SHALL be deduplicated by
construction.

#### Scenario: v on a Note shows its task subtree

- **WHEN** the focused row is a `NodeKind::Note` with path `projects/foo.md` and the user presses `v`
- **THEN** the active view's query becomes `node where kind = Note and path = "projects/foo.md"; expand where edge.kind in {has-task, subtask} and to.kind in {Task};`
- **AND** the tree shows each of the note's tasks exactly once

#### Scenario: v on a Task scopes to its source note

- **WHEN** the focused row is a `NodeKind::Task` and the user presses `v`
- **THEN** the view is scoped to the task's source note's task subtree

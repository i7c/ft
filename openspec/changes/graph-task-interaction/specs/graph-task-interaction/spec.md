# graph-task-interaction

## Purpose

Enable task interaction and inspection parity on the Graph tab: complete,
cancel, nudge/clear dates, cycle priority, open the full edit popup, open the
source note at the task line, and view a note's task subtree — all routed
through the shared `ft-core::task::ops` primitives the Tasks tab and CLI
already use.

## Requirements

### Requirement: Graph tab task verbs are row-kind-gated

The Graph tab SHALL register task-interaction commands that act on the
focused `NodeKind::Task` row and are no-ops (with an error toast) on other
row kinds. Each task verb SHALL resolve the task's `(source_file,
source_line)` from its `TaskData`, call the corresponding
`ft_core::task::ops` primitive against the absolute path, refresh the graph,
re-materialize the active view, and restore the cursor to the same
`(source_file, source_line)` anchor.

#### Scenario: graph.task-complete completes a task

- **WHEN** the focused row is a `NodeKind::Task` and the user presses `x`
- **THEN** `ops::complete_task` is called with the task's path and line
- **AND** the task's status becomes `Done` on disk
- **AND** the graph refreshes and the cursor returns to the same task (or its recurring next instance)

#### Scenario: graph.task-cancel cancels a task

- **WHEN** the focused row is a `NodeKind::Task` and the user presses `X`
- **THEN** `ops::cancel_task` is called and the task is marked `Cancelled` on disk
- **AND** the graph refreshes and the cursor is restored

#### Scenario: graph.task-due-next nudges the due date

- **WHEN** the focused row is a `NodeKind::Task` and the user presses `]`
- **THEN** `ops::update_task_line` advances the task's `due` by one day (from today if unset)
- **AND** the graph refreshes and the cursor is restored

#### Scenario: graph.task-due-prev nudges the due date backwards

- **WHEN** the focused row is a `NodeKind::Task` and the user presses `[`
- **THEN** the task's `due` is moved back one day

#### Scenario: graph.task-scheduled-next / prev nudge the scheduled date

- **WHEN** the user presses `}` / `{` on a `NodeKind::Task` row
- **THEN** `ops::update_task_line` advances / retreats the task's `scheduled` by one day

#### Scenario: graph.task-priority-next / prev cycle priority

- **WHEN** the user presses `=` / `-` on a `NodeKind::Task` row
- **THEN** `ops::update_task_line` cycles the task's `priority` forward / backward through the priority cycle

#### Scenario: graph.task-due-today sets due to today

- **WHEN** the user presses `T` on a `NodeKind::Task` row
- **THEN** `ops::update_task_line` sets the task's `due` to today

#### Scenario: task verbs are no-ops on non-Task rows

- **WHEN** any `graph.task-*` command is dispatched and the focused row is not a `NodeKind::Task`
- **THEN** an error toast is shown (e.g. "select a task first") and no file is modified

### Requirement: Graph tab opens the source note at the task line

When `graph.open-in-editor` is dispatched on a `NodeKind::Task` row, the
system SHALL open the task's owning note (`ctx.vault.path.join(source_file)`)
in `$EDITOR` positioned at the task's `source_line`.

#### Scenario: open-in-editor on a Task row

- **WHEN** the focused row is a `NodeKind::Task` with `source_file = "projects/foo.md"` and `source_line = 7` and the user presses `o`
- **THEN** `projects/foo.md` opens in `$EDITOR` at line 7

### Requirement: Graph tab task edit popup

The Graph tab SHALL open the shared `EditPopup` (the same form the Tasks tab
uses) when `graph.task-edit-popup` is dispatched on a `NodeKind::Task` row.
On commit the popup SHALL post an `AppRequest::GraphTaskEdit { path, line,
fields }` serviced by the Graph tab via `ops::update_task_line` (and
`plan_move`/`apply_move_plan` for the target/move field).

#### Scenario: e opens the edit popup on a Task row

- **WHEN** the focused row is a `NodeKind::Task` and the user presses `e`
- **THEN** the `TaskEdit` modal opens pre-populated with the task's current fields (description, due, scheduled, priority, tags, recurrence)
- **AND** on save, the task is updated on disk via `ops::update_task_line` and the graph refreshes

#### Scenario: e toasts on non-Task row

- **WHEN** the user presses `e` and the focused row is not a `NodeKind::Task`
- **THEN** an error toast is shown and no popup opens

### Requirement: Graph tab task create leader chords

The Graph tab SHALL provide a `t` leader chord for creating tasks: `tc`
creates a new top-level task (quickline seeded with the focused note's path,
or the vault default), `ts` creates a new subtask under the focused task. The
leader is a transient state (mirroring the `PeriodicLeader` pattern); any key
other than `c`/`s`, or `Esc`, cancels it.

#### Scenario: tc creates a top-level task

- **WHEN** the user presses `t` then `c`
- **THEN** a quickline opens seeded with the focused note's path (or the default target)
- **AND** on submit `ops::create_task` inserts the task and the graph refreshes

#### Scenario: ts creates a subtask under the focused task

- **WHEN** the focused row is a `NodeKind::Task` and the user presses `t` then `s`
- **THEN** a quickline opens with the parent set to the focused task's `(source_file, source_line)`
- **AND** on submit `ops::create_task` inserts the subtask nested under the parent and the graph refreshes

#### Scenario: t leader cancels on other key

- **WHEN** the `t` leader is active and the user presses a key other than `c` or `s` (including `Esc`)
- **THEN** the leader cancels and no quickline opens

### Requirement: Graph tab note-scoped task view

The Graph tab SHALL provide a `g` then `t` leader chord that, on a
`NodeKind::Note` or `NodeKind::Directory` row, rewrites the active view's
query to show that note's (or directory's) task subtree: top-level tasks via
`has-task` and their subtasks via `subtask`. The resulting view SHALL be
deduplicated by construction (each task appears once).

#### Scenario: gt on a Note shows its task subtree

- **WHEN** the focused row is a `NodeKind::Note` with path `projects/foo.md` and the user presses `g` then `t`
- **THEN** the active view's query becomes `node where kind = Note and path = "projects/foo.md"; expand where edge.kind in {has-task, subtask} and to.kind in {Task};`
- **AND** the tree shows each of the note's tasks exactly once (top-level tasks as direct children, subtasks nested)

#### Scenario: gt on a Directory shows tasks under it

- **WHEN** the focused row is a `NodeKind::Directory` with path `projects/` and the user presses `g` then `t`
- **THEN** the active view's query is scoped to notes under `projects/` and their task subtrees

#### Scenario: gt toasts on a Task row

- **WHEN** the focused row is a `NodeKind::Task` and the user presses `g` then `t`
- **THEN** an informational toast is shown and the view is unchanged (or scoped to the task's parent, per implementation)

### Requirement: Graph tab Task row display parity

The Graph tab SHALL render `NodeKind::Task` rows with a status marker prefix
followed by the description, and SHALL append compact relative due, scheduled,
and priority fields when present, reusing the Tasks tab's relative-date
formatting. Each appended field is omitted when its value is `None`.

#### Scenario: Task row shows due and priority

- **WHEN** a `NodeKind::Task` row has `status = "Open"`, `description = "Fix login bug"`, `due` 3 days ago, and `priority = "High"`
- **THEN** the row display begins with `[ ] Fix login bug` and includes a relative due label (e.g. `📅 3d ago`) and the priority (e.g. `⏩ High`)

#### Scenario: Task row omits absent fields

- **WHEN** a `NodeKind::Task` row has no `due`, no `scheduled`, and no `priority`
- **THEN** the row display is just the status marker and description (e.g. `[ ] Fix login bug`)

### Requirement: Graph tab task verbs use shared ops primitives

Every Graph-tab task mutation SHALL be implemented by calling the existing
`ft_core::task::ops` primitives (`complete_task`, `cancel_task`,
`update_task_line`, `create_task`, `plan_move`/`apply_move_plan`). No
task-mutation logic SHALL be duplicated in the Graph tab; the only
Graph-tab-specific code is keymap/dispatch/refresh/cursor-restore.

#### Scenario: no duplicated mutation logic

- **WHEN** the Graph tab completes, cancels, nudges, edits, or creates a task
- **THEN** the on-disk result is identical to the corresponding Tasks-tab or CLI operation on the same `(source_file, source_line)`

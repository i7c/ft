# tui-commands

## ADDED Requirements

### Requirement: Graph tab exposes `graph.task-*` commands

The Graph tab SHALL register the following task-interaction commands in
`GRAPH_COMMANDS` (scope `Tab("graph")`, group `"Tasks"`):

- `graph.task-complete` (`x`) — `ops::complete_task` on the focused Task row
- `graph.task-cancel` (`X`) — `ops::cancel_task` on the focused Task row
- `graph.task-due-next` (`]`) / `graph.task-due-prev` (`[`) — nudge due ±1 day
- `graph.task-scheduled-next` (`}`) / `graph.task-scheduled-prev` (`{`) — nudge scheduled ±1 day
- `graph.task-priority-next` (`=`) / `graph.task-priority-prev` (`-`) — cycle priority
- `graph.task-due-today` (`T`) — set due to today
- `graph.task-edit-popup` (`e`) — open the shared `EditPopup` modal (`opens_modal: true`)
- `graph.task-create` (`t` then `c` leader) — create a top-level task
- `graph.task-new-subtask` (`t` then `s` leader) — create a subtask under the focused task
- `graph.tasks-of-note` (`g` then `t` leader) — rewrite the view to the note's task subtree

Each command SHALL be a no-op with an error toast when the focused row is not
a `NodeKind::Task` (except `graph.tasks-of-note`, which acts on Note/Directory
rows, and `graph.task-create`/`graph.task-new-subtask`, which open a quickline).

#### Scenario: graph.task-* commands appear in the registry

- **WHEN** `GRAPH_COMMANDS` is built
- **THEN** it contains `CommandDef`s named `graph.task-complete`, `graph.task-cancel`, `graph.task-due-next`, `graph.task-due-prev`, `graph.task-scheduled-next`, `graph.task-scheduled-prev`, `graph.task-priority-next`, `graph.task-priority-prev`, `graph.task-due-today`, `graph.task-edit-popup`, `graph.task-create`, `graph.task-new-subtask`, and `graph.tasks-of-note`, each with scope `Tab("graph")` and group `"Tasks"`

#### Scenario: graph.task-edit-popup opens a modal

- **WHEN** the `graph.task-edit-popup` `CommandDef` is inspected
- **THEN** `opens_modal` is `true`

### Requirement: `TaskEdit` and `TaskLeader` modals expose commands

The `TaskEdit` modal SHALL expose `task-edit.confirm` and `task-edit.cancel`
commands. The `TaskLeader` transient SHALL expose `task-leader.create` and
`task-leader.new-subtask` commands (the second key of the `t` leader). Each
has corresponding `CommandDef` entries and keymap bindings.

#### Scenario: TaskEdit commands registered

- **WHEN** `TaskEdit::commands()` is called
- **THEN** it returns a slice containing `task-edit.confirm` (group "Flow", is_primary: true) and `task-edit.cancel` (group "Flow", is_primary: true)

#### Scenario: TaskLeader commands registered

- **WHEN** `TaskLeader::commands()` is called
- **THEN** it returns a slice containing `task-leader.create` and `task-leader.new-subtask`

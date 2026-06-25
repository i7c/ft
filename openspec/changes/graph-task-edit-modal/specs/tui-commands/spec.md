# tui-commands

## ADDED Requirements

### Requirement: Graph tab exposes task edit/create/view commands

The Graph tab SHALL register the following commands in `GRAPH_COMMANDS`
(scope `Tab("graph")`, group `"Tasks"`):

- `graph.task-edit-popup` (`e`) — open the shared `EditPopup` modal (`opens_modal: true`)
- `graph.task-leader` (`a`) — open the task-create leader modal (`opens_modal: true`)
- `graph.task-create` — create a top-level task (posted by the `a`+`c` leader)
- `graph.task-new-subtask` — create a subtask (posted by the `a`+`s` leader)
- `graph.tasks-of-note` (`v`) — rewrite the view to the note's task subtree

#### Scenario: commands appear in the registry

- **WHEN** `GRAPH_COMMANDS` is built
- **THEN** it contains `CommandDef`s named `graph.task-edit-popup`, `graph.task-leader`, `graph.task-create`, `graph.task-new-subtask`, and `graph.tasks-of-note`, each with scope `Tab("graph")` and group `"Tasks"`

#### Scenario: task-edit-popup and task-leader open modals

- **WHEN** the `graph.task-edit-popup` and `graph.task-leader` `CommandDef`s are inspected
- **THEN** both have `opens_modal = true`

### Requirement: `TaskEdit` and `TaskLeader` modals expose commands

The `TaskEdit` modal SHALL expose `task-edit.confirm` and `task-edit.cancel`.
The `TaskLeader` modal SHALL expose `task-leader.create` and
`task-leader.new-subtask`. Each has corresponding `CommandDef` entries and
keymap bindings.

#### Scenario: TaskEdit commands registered

- **WHEN** `TaskEdit::commands()` is called
- **THEN** it returns a slice containing `task-edit.confirm` (group "Flow", is_primary: true) and `task-edit.cancel` (group "Flow", is_primary: true)

#### Scenario: TaskLeader commands registered

- **WHEN** `TaskLeader::commands()` is called
- **THEN** it returns a slice containing `task-leader.create` and `task-leader.new-subtask`

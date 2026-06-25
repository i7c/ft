# tui-keymaps

## ADDED Requirements

### Requirement: Graph tab binds task edit/create/view keys

`GRAPH_KEYMAP` SHALL bind:

| Chord | Command |
|---|---|
| `e` | `graph.task-edit-popup` |
| `a` | `graph.task-leader` |
| `v` | `graph.tasks-of-note` |

The `a` leader enters a transient `TaskLeader` state where only `c`/`s`
complete a command; any other key (including `Esc`) cancels. The `e` and
`v` bindings are single-key. None collide with existing `GRAPH_KEYMAP`
bindings.

#### Scenario: task edit/create/view bound

- **WHEN** `GRAPH_KEYMAP` is built
- **THEN** `e`→`graph.task-edit-popup`, `a`→`graph.task-leader`, and `v`→`graph.tasks-of-note` are present

#### Scenario: no chord collisions

- **WHEN** `GRAPH_KEYMAP` is constructed
- **THEN** the `KeyMap::bind` builder does not panic on duplicate chords (`e`, `a`, `v` do not duplicate any existing binding)

### Requirement: TaskEdit and TaskLeader keymaps

The `TaskEdit` modal SHALL bind `Enter`/`Ctrl+S`→`task-edit.confirm`,
`Esc`→`task-edit.cancel`, plus the shared edit-buffer field-navigation
keys. The `TaskLeader` modal SHALL bind `c`→`task-leader.create`,
`s`→`task-leader.new-subtask`, `Esc`→cancel.

#### Scenario: TaskEdit keymap

- **WHEN** the `TaskEdit` modal is active
- **THEN** `Enter` and `Ctrl+S` dispatch `task-edit.confirm`, `Esc` dispatches `task-edit.cancel`

#### Scenario: TaskLeader keymap

- **WHEN** the `TaskLeader` modal is active
- **THEN** `c` dispatches `task-leader.create`, `s` dispatches `task-leader.new-subtask`, and `Esc` cancels

# tui-keymaps

## ADDED Requirements

### Requirement: Graph tab keymap binds task-interaction verbs

`GRAPH_KEYMAP` SHALL bind the following chords to the corresponding
`graph.task-*` commands. The `x`, `X`, `]`, `[`, `}`, `{`, `=`, `-`, `T`, and
`e` chords are single-key bindings; `tc`, `ts`, and `gt` are two-key leader
chords implemented via a transient `TaskLeader` modal.

| Chord | Command |
|---|---|
| `x` | `graph.task-complete` |
| `X` | `graph.task-cancel` |
| `]` | `graph.task-due-next` |
| `[` | `graph.task-due-prev` |
| `}` | `graph.task-scheduled-next` |
| `{` | `graph.task-scheduled-prev` |
| `=` | `graph.task-priority-next` |
| `-` | `graph.task-priority-prev` |
| `T` | `graph.task-due-today` |
| `e` | `graph.task-edit-popup` |
| `t` then `c` | `graph.task-create` |
| `t` then `s` | `graph.task-new-subtask` |
| `g` then `t` | `graph.tasks-of-note` |

The single-key task bindings (`x`,`X`,`]`,`[`,`}`,`{`,`=`,`-`,`T`,`e`) SHALL
not collide with existing `GRAPH_KEYMAP` bindings (`p`, `t`, `c`, `C`, `s`,
`r`, `m`, `d`, `n`, `o`, etc. remain unchanged). The `t` and `g` leaders
enter a transient state where only the documented second key completes the
command; any other key (including `Esc`) cancels.

#### Scenario: task verbs bound in GRAPH_KEYMAP

- **WHEN** `GRAPH_KEYMAP` is built
- **THEN** `x`→`graph.task-complete`, `X`→`graph.task-cancel`, `]`→`graph.task-due-next`, `[`→`graph.task-due-prev`, `}`→`graph.task-scheduled-next`, `{`→`graph.task-scheduled-prev`, `=`→`graph.task-priority-next`, `-`→`graph.task-priority-prev`, `T`→`graph.task-due-today`, and `e`→`graph.task-edit-popup` are all present

#### Scenario: existing graph bindings are unchanged

- **WHEN** `GRAPH_KEYMAP` is built
- **THEN** `p`→`graph.periodic-leader`, `t`→(task leader entry), `c`→`graph.create-blank`, `C`→`graph.create-from-template`, `r`→`graph.rename-or-multi-move`, `m`→`graph.move`, `d`→`graph.delete`, `n`→`graph.create-subdir`, `o`→`graph.open-in-editor` remain as before

#### Scenario: no chord collisions at build time

- **WHEN** `GRAPH_KEYMAP` is constructed
- **THEN** the `KeyMap::bind` builder does not panic on duplicate chords (the new task chords do not duplicate any existing binding)

### Requirement: Task leader and edit modal keymaps

The `TaskLeader` transient SHALL bind `c`→`task-leader.create` and
`s`→`task-leader.new-subtask`, with `Esc`/any-other-key cancelling. The
`TaskEdit` modal SHALL bind `Enter`→`task-edit.confirm` and
`Esc`→`task-edit.cancel` (plus the shared edit buffer bindings for field
navigation).

#### Scenario: TaskLeader keymap

- **WHEN** the `TaskLeader` modal is active
- **THEN** `c` dispatches `task-leader.create`, `s` dispatches `task-leader.new-subtask`, and `Esc` cancels

#### Scenario: TaskEdit keymap

- **WHEN** the `TaskEdit` modal is active
- **THEN** `Enter` dispatches `task-edit.confirm` and `Esc` dispatches `task-edit.cancel`

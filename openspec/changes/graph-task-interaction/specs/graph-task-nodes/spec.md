# graph-task-nodes

## MODIFIED Requirements

### Requirement: HasTask edge kind

The graph SHALL support `EdgeKind::HasTask` as an edge kind. For each
**top-level** task node (a task whose `parent` is `None`) whose
`TaskData.source_file` matches a note node's `NoteData.path`, an
`EdgeKind::HasTask` edge SHALL be created from that note node to the task
node. **Subtasks** (tasks whose `parent` is `Some(_)`) SHALL NOT receive a
`HasTask` edge; they are reachable from their owning note exclusively via the
chain `Note →[HasTask]→ top-level task →[Subtask]→ …`. Task nodes whose
`source_file` does not match any note SHALL still exist in the graph with no
incoming `HasTask` edge. The `EdgeKind::Subtask` edge kind (parent task →
child task, intra-file, indentation-derived) is unchanged.

#### Scenario: Note-to-top-level-task edge creation

- **WHEN** a top-level task has `source_file = "projects/foo.md"` and a note node exists with `NoteData.path = "projects/foo.md"`
- **THEN** an `EdgeKind::HasTask` edge SHALL exist from that note node to the task node

#### Scenario: Subtasks receive no HasTask edge

- **WHEN** a task has `parent = Some(parent_line)` (i.e. it is a subtask) and `source_file = "projects/foo.md"` matching a note node
- **THEN** NO `EdgeKind::HasTask` edge SHALL be created from that note node to the subtask
- **AND** the subtask SHALL be reachable from its note only via `Note →[HasTask]→ parent task →[Subtask]→ subtask`

#### Scenario: Task with no matching note

- **WHEN** a scan contains a task whose `source_file` does not match any note node's path (for example because the file was deleted between scan and build, or the path was constructed by hand)
- **THEN** the task node SHALL still be created in the graph, **AND** no `HasTask` edge SHALL terminate at that task node, **AND** the task SHALL still be returned by `node where kind = "Task"` queries

### Requirement: Task attribute queries in graph DSL

The graph query DSL SHALL support attribute-based filtering on task nodes.
The following attributes SHALL be recognized for `NodeKind::Task` nodes:
`status`, `priority`, `due`, `scheduled`, `created`, `start`, `completed`,
`description`, `tags`, and `path`. The `path` attribute on a task node SHALL
return the vault-relative `source_file` of the note that owns the task, so
that `path includes "Areas/"` selects tasks whose owning note lives under
`Areas/`. String attributes (`status`, `priority`, `due`, `scheduled`,
`created`, `start`, `completed`, `description`, `path`) SHALL support
equality, inequality, `in`, `includes`, `starts_with`, and `ends_with`
operators. The `tags` attribute SHALL support `in` and `includes` operators.
The DSL strings projected for `status` SHALL be exactly `"Open"`, `"Done"`,
`"InProgress"`, `"Cancelled"`; the DSL strings projected for `priority`
(when present) SHALL be exactly `"Highest"`, `"High"`, `"Medium"`, `"Low"`,
`"Lowest"`. These spellings are a stable contract and SHALL NOT be coupled to
the Rust `Debug` representation of the underlying enum variants.

#### Scenario: Filter tasks by status

- **WHEN** a user writes `node where kind = "Task" and status = "Open"`
- **THEN** the query SHALL return only task nodes whose `TaskData.status` is `"Open"`

#### Scenario: Filter tasks by priority

- **WHEN** a user writes `node where kind = "Task" and priority = "High"`
- **THEN** the query SHALL return only task nodes whose `TaskData.priority` is `Some("High")`

#### Scenario: Filter tasks by due date

- **WHEN** a user writes `node where kind = "Task" and due = "2025-01-15"`
- **THEN** the query SHALL return only task nodes whose `TaskData.due` is `Some("2025-01-15")`

#### Scenario: Filter tasks by tag

- **WHEN** a user writes `node where kind = "Task" and tags includes "work"`
- **THEN** the query SHALL return only task nodes whose `TaskData.tags` contain `"work"`

#### Scenario: Filter tasks by description substring

- **WHEN** a user writes `node where kind = "Task" and description starts_with "Fix"`
- **THEN** the query SHALL return only task nodes whose `TaskData.description` starts with `"Fix"`

#### Scenario: Filter tasks by description suffix

- **WHEN** a user writes `node where kind = "Task" and description ends_with "report"`
- **THEN** the query SHALL return only task nodes whose `TaskData.description` ends with `"report"`

#### Scenario: Filter tasks by inequality on status

- **WHEN** a user writes `node where kind = "Task" and status != "Done"`
- **THEN** the query SHALL return only task nodes whose `TaskData.status` is not `"Done"`

#### Scenario: Filter tasks by status in set

- **WHEN** a user writes `node where kind = "Task" and status in {"Open", "InProgress"}`
- **THEN** the query SHALL return only task nodes whose `TaskData.status` is `"Open"` or `"InProgress"`

#### Scenario: Path attribute on task node yields the owning note's path

- **WHEN** a user writes `node where kind = "Task" and path = "root.md"` against a graph in which task nodes have `TaskData.source_file = "root.md"`
- **THEN** the condition SHALL evaluate to true for those task nodes, because `path` on a task node returns its `source_file`

#### Scenario: Path-includes selects tasks of a note

- **WHEN** a user writes `node where kind = "Task" and path includes "Areas/finance"`
- **THEN** the query SHALL return every task whose owning note lives under a path containing `Areas/finance` (top-level tasks and subtasks alike)

#### Scenario: Title attribute on task node yields no match

- **WHEN** a user writes `node where kind = "Task" and title = "anything"`
- **THEN** the condition SHALL evaluate to false for every task node, because `title` is not a queryable attribute on task nodes

#### Scenario: Stable DSL spelling for Status

- **WHEN** `Graph::build` constructs a `TaskData` from a `Task` whose status is any `Status` variant
- **THEN** `TaskData.status` SHALL equal exactly one of `"Open"`, `"Done"`, `"InProgress"`, `"Cancelled"`, **AND** this spelling SHALL be produced by an explicit conversion on `Status` rather than by `format!("{:?}", …)`

#### Scenario: Stable DSL spelling for Priority

- **WHEN** `Graph::build` constructs a `TaskData` from a `Task` whose priority is `Some(p)` for any `Priority` variant `p`
- **THEN** `TaskData.priority` SHALL equal exactly `Some(s)` where `s` is one of `"Highest"`, `"High"`, `"Medium"`, `"Low"`, `"Lowest"`, **AND** this spelling SHALL be produced by an explicit conversion on `Priority` rather than by `format!("{:?}", …)`

### Requirement: Built-in graph preset `tasks-in-fs`

The built-in graph preset named `tasks-in-fs` (previously referred to in this
spec as `tasks-in-tree`; the implementation name `tasks-in-fs` is canonical)
SHALL start at the vault root and expand via `directory-contains`, `has-task`,
and `subtask` edges, so that walking the vault reaches a note's full task
subtree (top-level tasks via `has-task`, their subtasks via `subtask`). The
`fs` and `tree` presets SHALL remain unchanged and SHALL NOT follow `has-task`
or `subtask` edges, so they exclude task nodes.

#### Scenario: tasks-in-fs walks the full task subtree

- **WHEN** the `tasks-in-fs` preset is applied against a graph built from a vault whose scan contained a note with one top-level task and one nested subtask
- **THEN** the resulting walk SHALL contain both the top-level task and the subtask

#### Scenario: fs preset excludes tasks

- **WHEN** the `fs` preset is applied against the same graph
- **THEN** the resulting walk SHALL contain zero `NodeKind::Task` rows

#### Scenario: tree preset excludes tasks

- **WHEN** the `tree` preset is applied against the same graph
- **THEN** the resulting walk SHALL contain zero `NodeKind::Task` rows

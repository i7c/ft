## ADDED Requirements

### Requirement: Task node kind in graph
The graph SHALL support `NodeKind::Task(TaskData)` as a fourth node kind alongside `Note`, `Ghost`, and `Directory`. `TaskData` SHALL carry `description` (String), `status` (String), `priority` (Option<String>), `due` (Option<String>), `scheduled` (Option<String>), `tags` (Vec<String>), `source_file` (PathBuf), and `source_line` (usize).

#### Scenario: Task nodes created from scan data
- **WHEN** `Graph::build(&vault, &scan)` is called and the scan contains tasks
- **THEN** for each task in `scan.tasks`, a `NodeKind::Task(TaskData)` node SHALL be inserted into the graph with fields populated from the corresponding `Task` struct

#### Scenario: No task nodes without scan data
- **WHEN** `Graph::build(&vault, &scan)` is called and the scan has zero tasks
- **THEN** no task nodes SHALL exist in the resulting graph

#### Scenario: Task node deduplication
- **WHEN** two task entries in the scan share the same `source_file` and `source_line`
- **THEN** a single task node SHALL be created (not duplicated)

### Requirement: HasTask edge kind
The graph SHALL support `EdgeKind::HasTask` as a new edge kind. For each task node, an `EdgeKind::HasTask` edge SHALL be created from the note node whose path matches the task's `source_file`.

#### Scenario: Note-to-task edge creation
- **WHEN** a task has `source_file = "projects/foo.md"` and a note node exists with `NoteData.path = "projects/foo.md"`
- **THEN** an `EdgeKind::HasTask` edge SHALL exist from that note node to the task node

#### Scenario: Task with no matching note
- **WHEN** a task's `source_file` does not match any note node's path
- **THEN** the task node SHALL still be created, but no `HasTask` edge SHALL be added for that task

### Requirement: Graph build accepts scan data
`Graph::build` SHALL accept a `&Scan` parameter containing task data alongside the existing `&Vault` parameter. The previous signature `Graph::build(&Vault)` SHALL be replaced with `Graph::build(&Vault, &Scan)`.

#### Scenario: Build with scan data
- **WHEN** `Graph::build(&vault, &scan)` is called
- **THEN** the resulting graph SHALL contain note, directory, ghost, and task nodes with appropriate edges including `HasTask` edges

#### Scenario: Build with empty scan
- **WHEN** `Graph::build(&vault, &Scan { tasks: vec![], errors: vec![] })` is called
- **THEN** the graph SHALL behave identically to the pre-change `Graph::build(&vault)` (no task nodes, no HasTask edges)

### Requirement: Task kind in graph query DSL
The graph query DSL SHALL recognize `"Task"` as a valid `kind` value for node conditions. `node_kind_str` SHALL return `"Task"` for `NodeKind::Task` nodes.

#### Scenario: Filter by task kind
- **WHEN** a user writes `node where kind = "Task"`
- **THEN** the query SHALL return only task nodes

#### Scenario: Expand revealing tasks
- **WHEN** a user writes `node where kind = "Directory"; expand where to.kind in {"Note", "Directory", "Task"}`
- **THEN** the expansion SHALL include task nodes that are reachable via `HasTask` edges from contained note nodes

#### Scenario: Task kind in DSL validation
- **WHEN** a user writes `node where kind = "Task"` in a query string
- **THEN** the DSL parser SHALL accept it without returning `UnknownKindValue`

### Requirement: Task attribute queries in graph DSL
The graph query DSL SHALL support attribute-based filtering on task nodes. The following attributes SHALL be recognized for `NodeKind::Task` nodes: `status`, `priority`, `due`, `scheduled`, `tags`, `description`. String attributes (`status`, `priority`, `due`, `scheduled`, `description`) SHALL support equality, inequality, `in`, `includes`, `starts_with`, and `ends_with` operators. The `tags` attribute SHALL support `in` and `includes` operators.

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

#### Scenario: Unknown attribute on task node
- **WHEN** a user writes `node where kind = "Task" and nonexistent = "foo"`
- **THEN** the condition SHALL evaluate to false (no match), consistent with unknown attributes on other node types

### Requirement: TUI graph renders task nodes
The TUI graph tab SHALL render `NodeKind::Task` nodes with `kind_char = 'T'` and display text set to the task description (truncated to fit the display width).

#### Scenario: Task node in graph tree
- **WHEN** the graph contains task nodes and the user expands a note that has `HasTask` edges
- **THEN** the task nodes SHALL appear as children with `T` as the kind character and the task description as the display text

#### Scenario: Task node selection
- **WHEN** a user selects a task node in the graph tree
- **THEN** the task SHALL be highlighted in the same manner as note/directory/ghost nodes

### Requirement: Has-task edge kind string
`edge_kind_str` SHALL return `"has-task"` for `EdgeKind::HasTask` edges. The DSL SHALL accept `"has-task"` as a valid edge kind value.

#### Scenario: Filter edges by has-task kind
- **WHEN** a user writes `expand where edge.kind = "has-task"`
- **THEN** the expansion SHALL traverse only `HasTask` edges

### Requirement: Built-in graph preset for tasks
A new built-in graph preset named `tasks-in-tree` SHALL be added. It SHALL be identical to the `tree` preset except that its expand block includes `Task` in the `to.kind` set.

#### Scenario: tasks-in-tree preset
- **WHEN** a user applies the `tasks-in-tree` preset
- **THEN** the graph tree SHALL show notes, directories, ghosts, and tasks

#### Scenario: Existing presets unchanged
- **WHEN** a user applies the `tree` preset
- **THEN** task nodes SHALL NOT appear (same behavior as before this change)
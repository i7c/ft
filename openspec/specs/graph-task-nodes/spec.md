# graph-task-nodes Specification

## Purpose
TBD - created by archiving change graph-task-nodes. Update Purpose after archive.
## Requirements
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
The graph SHALL support `EdgeKind::HasTask` as a new edge kind. For each task node whose `TaskData.source_file` matches a note node's `NoteData.path`, an `EdgeKind::HasTask` edge SHALL be created from that note node to the task node. Task nodes whose `source_file` does not match any note SHALL still exist in the graph with no incoming `HasTask` edge.

#### Scenario: Note-to-task edge creation
- **WHEN** a task has `source_file = "projects/foo.md"` and a note node exists with `NoteData.path = "projects/foo.md"`
- **THEN** an `EdgeKind::HasTask` edge SHALL exist from that note node to the task node

#### Scenario: Task with no matching note
- **WHEN** a scan contains a task whose `source_file` does not match any note node's path (for example because the file was deleted between scan and build, or the path was constructed by hand)
- **THEN** the task node SHALL still be created in the graph, **AND** no `HasTask` edge SHALL terminate at that task node, **AND** the task SHALL still be returned by `node where kind = "Task"` queries

### Requirement: Graph build accepts scan data
`Graph::build` SHALL accept a `&Scan` parameter alongside the `&Vault` parameter. The scan carries both the task data and the per-file parse artifacts (`Scan::files`: links, paragraphs, headings) from `Vault::scan`'s single read pass; `Graph::build` SHALL construct the graph entirely from those artifacts, performing no file reads of its own (only the directory walk for `Directory` nodes).

#### Scenario: Build with scan data
- **WHEN** `Graph::build(&vault, &vault.scan())` is called
- **THEN** the resulting graph SHALL contain note, directory, ghost, and task nodes with appropriate edges including `HasTask` edges

#### Scenario: Build with empty scan
- **WHEN** `Graph::build(&vault, &Scan::default())` is called (no tasks and no parse artifacts)
- **THEN** the graph SHALL contain no note or task nodes — note nodes come from `Scan::files`, not from a disk walk — and only the directory nodes the vault walk yields

### Requirement: Task kind in graph query DSL
The graph query DSL SHALL recognize `"Task"` as a valid `kind` value for node conditions. `node_kind_str` SHALL return `"Task"` for `NodeKind::Task` nodes. Expansion blocks SHALL accept `"Task"` as a member of `to.kind` sets, and traversal across `EdgeKind::HasTask` SHALL include task nodes as expansion targets when the `to.kind` filter permits them.

#### Scenario: Filter by task kind
- **WHEN** a user writes `node where kind = "Task"`
- **THEN** the query SHALL return only task nodes

#### Scenario: Expand revealing tasks via to.kind
- **WHEN** a user writes `node where kind = "Directory" and path = ""; expand where edge.kind in {"directory-contains", "has-task"} and to.kind in {"Note", "Directory", "Task"};`
- **THEN** the resulting walk SHALL contain at least one `NodeKind::Task` row reachable via a `HasTask` edge from a contained note

#### Scenario: Expand omitting tasks via to.kind
- **WHEN** the same expansion is written but `"Task"` is omitted from the `to.kind` set (while `"has-task"` remains in `edge.kind`)
- **THEN** the resulting walk SHALL contain zero `NodeKind::Task` rows

#### Scenario: Task kind in DSL validation
- **WHEN** a user writes `node where kind = "Task"` in a query string
- **THEN** the DSL parser SHALL accept it without returning `UnknownKindValue`

### Requirement: Task attribute queries in graph DSL
The graph query DSL SHALL support attribute-based filtering on task nodes. The following attributes SHALL be recognized for `NodeKind::Task` nodes: `status`, `priority`, `due`, `scheduled`, `tags`, `description`. String attributes (`status`, `priority`, `due`, `scheduled`, `description`) SHALL support equality, inequality, `in`, `includes`, `starts_with`, and `ends_with` operators. The `tags` attribute SHALL support `in` and `includes` operators. Any other attribute name evaluated against a task node — including `path` and `title` — SHALL yield no value, causing the condition to evaluate to false (consistent with unknown attributes on any other node kind). The DSL strings projected for `status` SHALL be exactly `"Open"`, `"Done"`, `"InProgress"`, `"Cancelled"`; the DSL strings projected for `priority` (when present) SHALL be exactly `"Highest"`, `"High"`, `"Medium"`, `"Low"`, `"Lowest"`. These spellings are a stable contract and SHALL NOT be coupled to the Rust `Debug` representation of the underlying enum variants.

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

#### Scenario: Path attribute on task node yields no match
- **WHEN** a user writes `node where kind = "Task" and path = "root.md"` against a graph in which task nodes have `TaskData.source_file = "root.md"`
- **THEN** the condition SHALL evaluate to false for every task node, because `path` is not a queryable attribute on task nodes

#### Scenario: Title attribute on task node yields no match
- **WHEN** a user writes `node where kind = "Task" and title = "anything"`
- **THEN** the condition SHALL evaluate to false for every task node, because `title` is not a queryable attribute on task nodes

#### Scenario: Stable DSL spelling for Status
- **WHEN** `Graph::build` constructs a `TaskData` from a `Task` whose status is any `Status` variant
- **THEN** `TaskData.status` SHALL equal exactly one of `"Open"`, `"Done"`, `"InProgress"`, `"Cancelled"`, **AND** this spelling SHALL be produced by an explicit conversion on `Status` rather than by `format!("{:?}", …)`

#### Scenario: Stable DSL spelling for Priority
- **WHEN** `Graph::build` constructs a `TaskData` from a `Task` whose priority is `Some(p)` for any `Priority` variant `p`
- **THEN** `TaskData.priority` SHALL equal exactly `Some(s)` where `s` is one of `"Highest"`, `"High"`, `"Medium"`, `"Low"`, `"Lowest"`, **AND** this spelling SHALL be produced by an explicit conversion on `Priority` rather than by `format!("{:?}", …)`

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
A new built-in graph preset named `tasks-in-tree` SHALL be added. It SHALL be identical to the `tree` preset except that its expand block includes `Task` in the `to.kind` set and `has-task` in the `edge.kind` set. The two presets SHALL produce observably different walks against a graph that contains task nodes: `tasks-in-tree` SHALL include them, and `tree` SHALL exclude them.

#### Scenario: tasks-in-tree preset includes tasks
- **WHEN** a user applies the `tasks-in-tree` preset against a graph built from a vault whose scan contained at least one task
- **THEN** the resulting walk SHALL contain at least one `NodeKind::Task` row

#### Scenario: tree preset excludes tasks
- **WHEN** a user applies the `tree` preset against the same graph
- **THEN** the resulting walk SHALL contain zero `NodeKind::Task` rows


## 1. Data Model

- [x] 1.1 Define `TaskData` struct in `ft-core/src/graph/mod.rs` with fields: `description: String`, `status: String`, `priority: Option<String>`, `due: Option<String>`, `scheduled: Option<String>`, `tags: Vec<String>`, `source_file: PathBuf`, `source_line: usize`
- [x] 1.2 Add `Task(TaskData)` variant to `NodeKind` enum in `ft-core/src/graph/mod.rs`
- [x] 1.3 Add `HasTask` variant to `EdgeKind` enum in `ft-core/src/graph/mod.rs`
- [x] 1.4 Add `task_index: HashMap<(PathBuf, usize), NoteId>` field to `Graph` struct for task node lookup by `(source_file, source_line)`

## 2. Graph Build

- [x] 2.1 Change `Graph::build` signature from `build(vault: &Vault)` to `build(vault: &Vault, scan: &Scan)`
- [x] 2.2 Add `insert_task_node` method to `Graph` that creates a `NodeKind::Task(TaskData)` node, updates `task_index`, and returns `NoteId`
- [x] 2.3 In `Graph::build`, after inserting note and directory nodes, iterate `scan.tasks` to insert task nodes using `insert_task_node`
- [x] 2.4 Add `insert_hastask_edges` method that creates `EdgeKind::HasTask` edges from each note node to its task nodes (matching by `NoteData.path == TaskData.source_file`)
- [x] 2.5 In `Graph::build`, call `insert_hastask_edges` after link/embed edge insertion
- [ ] 2.6 Update all callers of `Graph::build` (`ft/src/tui/tabs/graph.rs`, `ft/src/cmd/graph.rs`, tests) to pass `&Scan`

## 3. DSL Parser

- [x] 3.1 Add `"Task"` to valid node kind values in `check_kind_values` (`ft-core/src/graph/query.rs`)
- [x] 3.2 Add `"has-task"` to valid edge kind values in `check_kind_values` (`ft-core/src/graph/query.rs`)

## 4. DSL Evaluator

- [x] 4.1 Add `NodeKind::Task` arm to `node_kind_str` returning `"Task"` (`ft-core/src/graph/query.rs`)
- [x] 4.2 Add `EdgeKind::HasTask` arm to `edge_kind_str` returning `"has-task"` (`ft-core/src/graph/query.rs`)
- [x] 4.3 Extend `node_string_attr` for `NodeKind::Task` to recognize `"status"`, `"priority"`, `"due"`, `"scheduled"`, `"description"` attribute names and return corresponding `TaskData` field values (`None` for `Option` fields that are `None`)
- [x] 4.4 Handle `"tags"` attribute on `NodeKind::Task` nodes — support `includes` and `in` operators by adding a `NodeKind::Task` arm to the tag evaluation logic

## 5. TUI Rendering

- [x] 5.1 Update `make_row` in `ft/src/tui/tabs/graph.rs` to handle `NodeKind::Task` — `kind_char = 'T'`, display = task description (truncated to column width)
- [x] 5.2 Verify task node selection/highlighting works with existing selection logic

## 6. Presets and CLI

- [x] 6.1 Add `tasks-in-tree` built-in preset in `ft-core/src/graph/preset.rs` — same as `tree` but with `Task` in expand `to.kind`
- [x] 6.2 Add `tasks-in-tree` to `builtin_names()` return list
- [x] 6.3 Update CLI graph subcommand output rendering to handle task nodes (if it has its own rendering path separate from TUI)

## 7. Tests

- [x] 7.1 Unit test: `TaskData` construction from `&Task` preserves all fields correctly
- [x] 7.2 Unit test: `Graph::build` with non-empty scan creates task nodes and `HasTask` edges
- [x] 7.3 Unit test: `Graph::build` with empty scan produces no task nodes (matches old behavior)
- [x] 7.4 Unit test: task node deduplication by `(source_file, source_line)`
- [x] 7.5 Unit test: `node_kind_str` returns `"Task"` for task nodes
- [x] 7.6 Unit test: `edge_kind_str` returns `"has-task"` for `HasTask` edges
- [x] 7.7 Unit test: DSL `node where kind = "Task"` returns only task nodes
- [x] 7.8 Unit test: DSL task attribute filters (`status`, `priority`, `due`, `scheduled`, `description`, `tags`)
- [x] 7.9 Unit test: DSL expand with `to.kind` including `"Task"` reveals task children
- [x] 7.10 Unit test: `tasks-in-tree` preset parses correctly and differs from `tree` in expand targets
- [x] 7.11 Unit test: TUI `TreeRow` task nodes render with `kind_char = 'T'` and description as display
## Why

ft has a rich task system (parse, query, mutate) and a rich graph system (nodes, edges, walk, expand), but they're completely separate. `Vault::scan()` returns tasks; `Graph::build()` returns notes/directories/ghosts. There's no way to see tasks in the graph, walk from a note to its tasks, or query for tasks using the graph DSL. The `NodeKind` comment at `graph/mod.rs:17` already envisions this — it's time to wire it up.

## What Changes

- Add `NodeKind::Task(TaskData)` variant to the graph, where `TaskData` carries task-identifying fields (description, status, priority, source_file, source_line, due, scheduled, tags)
- Add `EdgeKind::HasTask` edge from notes (and directories via containment) to their task nodes
- Extend `Graph::build()` to accept or produce task nodes alongside the existing note/directory/ghost/link/embed graph
- Extend the graph query DSL to recognize `"Task"` as a valid `kind` value and support task-specific attribute filters (`status`, `priority`, `due`, `scheduled`, `tags`, `description`)
- Extend `node_kind_str` to return `"Task"` for task nodes
- Update TUI graph rendering to display task nodes with a distinct `kind_char` and display text
- Add/update built-in graph presets to show tasks when expanding

## Capabilities

### New Capabilities
- `graph-task-nodes`: Task nodes in the graph — `NodeKind::Task(TaskData)`, `EdgeKind::HasTask`, and DSL support for `kind = Task` plus task-specific attribute queries

### Modified Capabilities
- None (no existing specs to modify — this is the first spec-driven change)

## Impact

- **ft-core/src/graph/mod.rs**: New `NodeKind::Task` variant, `TaskData` struct, `EdgeKind::HasTask` variant, updated `Graph::build()` signature/logic, new task-node insertion methods
- **ft-core/src/graph/query.rs**: New `kind = Task` valid value, task attribute parsing/evaluation (`Attr::Status`, `Attr::Priority`, `Attr::Due`, `Attr::Scheduled`, `Attr::Tags`, `Attr::Description` on nodes), updated `node_kind_str`
- **ft-core/src/vault.rs**: `Graph::build()` needs task data — either accepts `&Scan` or parses tasks inline during file read
- **ft/src/tui/tabs/graph.rs**: `make_row()` handles `NodeKind::Task`, new `kind_char` for tasks
- **ft-core/src/graph/preset.rs**: Built-in presets updated to include `Task` in expand targets
- **CLI graph subcommand**: Output rendering for task nodes
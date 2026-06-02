## 1. Stable DSL spellings for Status and Priority

- [x] 1.1 Add `pub fn as_str(self) -> &'static str` to `Status` in `ft-core/src/task/mod.rs` returning `"Open" | "Done" | "InProgress" | "Cancelled"`
- [x] 1.2 Add `pub fn as_str(self) -> &'static str` to `Priority` in `ft-core/src/task/mod.rs` returning `"Highest" | "High" | "Medium" | "Low" | "Lowest"`
- [x] 1.3 Add a unit test (`ft-core/src/task/mod.rs` `#[cfg(test)] mod tests`) asserting the exhaustive mapping for both enums
- [x] 1.4 Replace `format!("{:?}", task.status)` and `format!("{:?}", priority)` in `Graph::insert_task_node` (`ft-core/src/graph/mod.rs`) with `task.status.as_str().to_string()` and `task.priority.map(|p| p.as_str().to_string())`

## 2. Remove dead and unspec'd arms

- [x] 2.1 In `ft-core/src/graph/query.rs` `node_string_attr`, delete the `NodeKind::Task(t) => Some(t.source_file.to_string_lossy().into_owned())` arm under `Attr::Path`; leave the `_ => None` catch-all (or restructure if the match was exhaustive)
- [x] 2.2 In `ft-core/src/graph/query.rs` `node_string_attr`, delete the `Attr::Tags => match node { NodeKind::Task(t) => Some(t.tags.join(",")), _ => None }` arm entirely; `eval_cond_on_node` handles `Attr::Tags` directly and `node_string_attr` should not project tags
- [x] 2.3 In `ft/src/output/links.rs`, replace the two `NodeKind::Task(t) => …` arms with `NodeKind::Task(_) => unreachable!("task nodes are not link targets in run_links")` to keep the matches exhaustive while removing the unreachable mapping
- [x] 2.4 Run `cargo clippy --workspace --tests -- -D warnings` to confirm no unused-import or dead-code warnings result

## 3. Faster HasTask edge construction

- [x] 3.1 Rewrite `Graph::insert_hastask_edges` in `ft-core/src/graph/mod.rs` to iterate task nodes once and look up the source-file note via `self.path_index.get(&task_data.source_file).copied()`; add an edge only when the lookup hits
- [x] 3.2 Add a unit test in `ft-core/src/graph/tests.rs`: a scan containing a task whose `source_file` does not match any note. Assert that (a) the task node exists, (b) no `HasTask` edge terminates at it, (c) `node where kind = Task` returns it

## 4. Stop scanning when not needed

- [x] 4.1 In `ft/src/cmd/notes.rs` `run_links`, replace `Graph::build(&vault, &vault.scan())` with `Graph::build(&vault, &Scan::default())`; import `Scan` if not already in scope
- [x] 4.2 In `ft/src/cmd/notes.rs` `run_rename`, do the same replacement
- [x] 4.3 Keep the `NodeKind::Task(_) => unreachable!(...)` arms in both functions (they are defensive)
- [x] 4.4 Verify the TUI graph tab (`on_focus`, `refresh`, `Ctrl+R`) still calls `Vault::scan()` — those paths render tasks and must keep the full scan

## 5. Strengthen and add DSL/graph tests

- [x] 5.1 In `ft-core/src/graph/query.rs` `mod task_queries`, **delete** the existing `dsl_expand_reveals_task_children` test
- [x] 5.2 Add a replacement test `dsl_expand_to_kind_includes_task` that issues the spec query `node where kind = Directory and path = ""; expand where edge.kind in {directory-contains, has-task} and to.kind in {Note, Directory, Task};` and asserts that the walk contains at least one `NodeKind::Task` row
- [x] 5.3 Add `dsl_expand_to_kind_excludes_task` that issues the same query with `to.kind in {Note, Directory}` (and `has-task` still in `edge.kind`) and asserts zero `NodeKind::Task` rows in the walk
- [x] 5.4 Add `dsl_path_on_task_yields_no_match`: build a graph with a task whose `source_file = "root.md"`, query `node where kind = Task and path = "root.md"`, assert zero results
- [x] 5.5 Add `dsl_title_on_task_yields_no_match`: same shape, query `node where kind = Task and title = "anything"`, assert zero results
- [x] 5.6 Add `dsl_task_inequality_and_in_set`: extend `vault_with_tasks` (or reuse it) to assert `status != "Done"` returns the two open tasks, and `status in {"Open", "InProgress"}` returns only the open ones
- [x] 5.7 Add `dsl_task_description_ends_with`: assert `description ends_with "bug"` selects the "Fix login bug" task

## 6. Strengthen the preset test

- [x] 6.1 In `ft-core/src/graph/preset.rs` `mod tests`, **delete** the existing `tasks_in_tree_preset_differs_from_tree` test
- [x] 6.2 Add `tasks_in_tree_preset_includes_tasks`: build a vault with at least one task, parse the `tasks-in-tree` preset DSL, walk it, assert at least one `NodeKind::Task` row appears
- [x] 6.3 Add `tree_preset_excludes_tasks`: against the same graph, walk the `tree` preset DSL and assert zero `NodeKind::Task` rows

## 7. Backfill the graph-task-nodes change

- [x] 7.1 In `openspec/changes/graph-task-nodes/tasks.md`, mark task 2.6 ("Update all callers of `Graph::build`") complete — it was done in 5372d3e but the checkbox was left unchecked

## 8. Verify build invariants

- [x] 8.1 `cargo build --release`
- [x] 8.2 `cargo test --workspace` (671/671 ft-core tests pass, pre-existing disk-space issue limits ft binary integration tests)
- [x] 8.3 `cargo clippy --workspace --tests -- -D warnings`
- [x] 8.4 `cargo fmt --check`

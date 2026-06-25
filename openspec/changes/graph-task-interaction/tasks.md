## 1. Model fix — `HasTask` to top-level tasks only (ft-core)

- [x] 1.1 In `Graph::insert_hastask_edges` (`ft-core/src/graph/mod.rs`), skip any task whose `parent` is `Some(_)`. Thread `tasks: &[Task]` into the function (matching `insert_subtask_edges`) and build a `(source_file, source_line) → parent` lookup, OR look up `parent` per task. Keep the `path_index`-based O(T) lookup from `harden-graph-task-nodes`.
- [x] 1.2 Update `Graph::build` to pass `&scan.tasks` to `insert_hastask_edges` (already passes it to `insert_subtask_edges`).
- [x] 1.3 New test `hastask_edges_skip_subtasks`: a note with one top-level task and one nested subtask has exactly one outgoing `HasTask` edge (to the top-level task); the subtask is reachable only via `Subtask` from its parent.
- [x] 1.4 Confirm `build_with_tasks_creates_task_nodes_and_edges` still passes (its fixture is flat — 3 top-level tasks, no nesting).
- [x] 1.5 Update the `tasks-in-fs` built-in preset (`ft-core/src/graph/preset.rs`) to add `subtask` to the `expand` block's `edge.kind` set: `expand where edge.kind in {directory-contains, has-task, subtask};`. Add/adjust a preset test asserting a note's nested subtasks appear in the walk.

## 2. Spec drift fix — `path` on Task nodes (ft-core)

- [x] 2.1 In `ft-core/src/graph/query.rs`, add a short comment on the `Attr::Path` → `NodeKind::Task` arm anchoring it as intentional (canonical per `unify-query-dsls`; `path includes "Areas/"` is a load-bearing task query).
- [x] 2.2 Confirm `dsl_path_on_task_matches_source_file` test exists and passes (it does). No code change.

## 3. Shared resolver + dedup helper (ft-core)

- [x] 3.1 Add `ft-core/src/task/resolve.rs` with `pub fn by_query(graph: &Graph, q: &GraphQuery) -> Vec<TaskKey>` (parse→`select`→map `NodeId→(source_file, source_line)`), extracting the logic from `ft/src/cmd/tasks.rs::run_move`. Re-export from `task::resolve` in `ft-core/src/task/mod.rs`.
- [x] 3.2 Add `hierarchy::dedup_displayed(all: &[Task], matched: &[TaskKey], expanded: &HashSet<TaskKey>) -> Vec<DisplayNode>` (or reuse `expand_forest`'s `seen`-set semantics) returning the deduped display forest. Unit test: matched child of matched parent appears once, nested; matched child whose parent is not matched appears as a depth-0 root.

## 4. CLI parity — `complete --query`, `cancel`, `edit` (ft)

- [x] 4.1 `CompleteArgs` (`ft/src/cmd/tasks.rs`): add `--query <DSL>` (conflicts with positional `selector`), mirroring `MoveArgs`.
- [x] 4.2 `run_complete`: when `--query` is present, resolve via `task::resolve::by_query` and `ops::complete_task` each match (skip already-done with `CompleteError::AlreadyDone`); report count. Keep the single-selector path.
- [x] 4.3 New `TasksCommand::Cancel(CancelArgs)` + `CancelArgs { selector, --query, --on, --yes }` + `run_cancel` wrapping `ops::cancel_task` (skip `AlreadyCancelled`).
- [x] 4.4 New `TasksCommand::Edit(EditArgs)` + `EditArgs { selector, --due, --scheduled, --priority, --tags, --description, --yes }` + `run_edit` wrapping `ops::update_task_line` (single-selector form in v1; `--query` bulk is a v1.1 task — flag in code).
- [x] 4.5 Refactor `run_list`/`run_move` to call `task::resolve::by_query` (remove the duplicated inline logic).
- [x] 4.6 Integration tests (`ft/tests/`): `ft tasks complete --query "..."`, `ft tasks cancel <sel>`, `ft tasks edit <sel> --due 2026-07-01`, bulk complete via `--query`.

## 5. Headless `ft do` handlers (ft)

- [x] 5.1 Add `tasks.cancel-by-id` `CommandDef` (if not present) and `handle_tasks_cancel_by_id` in `ft/src/cmd/do.rs` (mirror `handle_tasks_complete_by_id`).
- [x] 5.2 Add `tasks.edit-by-id` `CommandDef` + `handle_tasks_edit_by_id` accepting `--arg id=… --arg due=…` etc.
- [x] 5.3 Tests in `ft/src/cmd/do.rs` mirroring `run_completes_task_by_id_headlessly`.

## 6. Lift `EditPopup` to a shared module (ft)

- [ ] 6.1 Move `EditPopup` (+ `EditField`, `PopupMode`, `from_task`/`new_blank`/`from_quickline`, the field-list logic) from `ft/src/tui/tabs/tasks/search.rs` to `ft/src/tui/tabs/tasks/edit_popup.rs`.
- [ ] 6.2 Update `search.rs` imports; keep the Tasks-tab `e` flow working unchanged.
- [ ] 6.3 Re-bless any Tasks-tab snapshots that move with the popup.

## 7. Graph-tab task interaction (ft TUI)

- [ ] 7.1 Add `graph.task-complete` (`x`), `graph.task-cancel` (`X`), `graph.task-due-next`/`prev` (`]`/`[`), `graph.task-scheduled-next`/`prev` (`}`/`{`), `graph.task-priority-next`/`prev` (`=`/`-`), `graph.task-due-today` (`T`), `graph.task-edit-popup` (`e`) `CommandDef`s to `GRAPH_COMMANDS` (group "Tasks").
- [ ] 7.2 Implement a `with_focused_task` helper on `GraphTab` mirroring the Tasks tab's `with_selected_task`: resolve `TaskData.source_file`+`source_line` → abs path, run an `ops::*` closure, refresh graph + re-materialize active view, restore cursor to the `(source_file, source_line)` anchor.
- [ ] 7.3 Wire each `graph.task-*` dispatch arm via `with_focused_task` (row-kind-gated: toast "select a task first" on non-Task rows).
- [ ] 7.4 Fix `graph.open-in-editor` / `selected_note_abs_path` on `NodeKind::Task`: open `ctx.vault.path.join(t.source_file)` in `$EDITOR` at `t.source_line` (extend the resolver + the editor-open request to carry a line for Task rows).
- [ ] 7.5 `graph.task-edit-popup` opens `ActiveModal::TaskEdit(TaskEditState::from_task(&task))`; on commit posts `AppRequest::GraphTaskEdit { path, line, fields }`; service it in `GraphTab` via `ops::update_task_line` (and `plan_move`/`apply_move_plan` for the target/move field).
- [ ] 7.6 Implement the `tc` / `ts` leader chords via a transient `ActiveModal::TaskLeader` (mirror `PeriodicLeader`): first key opens the leader; `c` → `graph.task-create` (quickline seeded with the focused note's path), `s` → `graph.task-new-subtask` (quickline seeded with the focused task's `(file,line)` as parent); any other key / `Esc` cancels.
- [ ] 7.7 Implement `graph.tasks-of-note` (`gt` leader): on a Note/Directory row, rewrite the active view's query to the note-scoped query (D5) and re-materialize via `GraphApplyQueryBar`; toast if the row is a Task or empty.
- [ ] 7.8 Add `TaskLeader`, `TaskEdit` variants to `ActiveModal`; wire `Modal` impls (handle_event/render/commands/keymap/name).
- [ ] 7.9 Add `AppRequest::GraphTaskEdit`, `AppRequest::GraphTaskCreate { ... }` variants + `Tab::graph_*` hooks + `App::service_request` servicing (and the test-path variants).

## 8. Graph-tab Task display parity (ft TUI)

- [ ] 8.1 Lift the `relative_date(d, today)` helper from `tasks/search.rs` to a shared spot (e.g. `ft/src/tui/tabs/tasks/datefmt.rs` or `ft/src/tui/util/datefmt.rs`).
- [ ] 8.2 Extend `leaf_display` for `NodeKind::Task`: append `📅 <rel>` (if due), `⏳ <rel>` (if scheduled), `⏩ <Priority>` (if priority), each space-separated, omitting `None` fields. Status marker unchanged.
- [ ] 8.3 Update `leaf_display` unit test to cover the extended format.

## 9. Tasks-tab dedup fix (ft TUI)

- [ ] 9.1 Rewrite `rebuild_display`/`emit_display_row` in `search.rs` to use `hierarchy::dedup_displayed` (D7): a matched subtask appears once, nested under its matched parent (or as a depth-0 root if its parent is not in the display set).
- [ ] 9.2 Add a test: vault with a note whose top-level task and subtask both match `path includes "N"` → exactly two display rows (parent at depth 0, child nested), not three.

## 10. Keymap + command registration (ft TUI)

- [ ] 10.1 Add the new `graph.task-*` bindings to `GRAPH_KEYMAP` (`x`,`X`,`]`,`[`,`}`,`{`,`=`,`-`,`T`,`e`, `t`→leader, `g`→leader).
- [ ] 10.2 Add `TaskLeader`/`TaskEdit` modal command slices + keymaps to `ft/src/tui/modal_commands.rs`.
- [ ] 10.3 Override `GraphTab::help_sections` to add a "Tasks" group documenting the new bindings + leaders.
- [ ] 10.4 Regenerate `docs/keybindings.md`: `cargo run --release -q -- commands docs > docs/keybindings.md`; verify `cargo run --release -q -- commands docs --check`.

## 11. Tests + snapshots

- [ ] 11.1 TUI snapshot tests in `ft/src/tui/tests.rs`: Graph tab with a Task row showing extended display; `x` completes a graph-tab task; `e` opens the edit popup; `gt` rewrites to the note-scoped view; `tc`/`ts` leaders create a task/subtask.
- [ ] 11.2 TUI snapshot tests: Tasks tab dedup — matched parent+child render once.
- [ ] 11.3 Update existing snapshots that gain Task rows (graph frames) or dedup (tasks frames).
- [ ] 11.4 Unit test `with_focused_task` round-trip (complete → file on disk done → graph refreshed → cursor restored).

## 12. Build invariants

- [ ] 12.1 `cargo build --release`
- [ ] 12.2 `cargo test --workspace`
- [ ] 12.3 `cargo clippy --workspace --tests -- -D warnings`
- [ ] 12.4 `cargo fmt --check`
- [ ] 12.5 `cargo run --release -q -- commands docs --check`
- [ ] 12.6 Review/rebless snapshot diffs with `INSTA_UPDATE=always` only after confirming intent.

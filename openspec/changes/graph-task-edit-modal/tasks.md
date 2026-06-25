## 1. Lift popup render + validation helpers (ft TUI)

- [x] 1.1 Move `render_edit_popup`, `parse_optional_date`, `parse_priority`, `parse_tags_field`, `merge_tags_into_description`, `centered_rect` from `ft/src/tui/tabs/tasks/search.rs` to `ft/src/tui/tabs/tasks/edit_popup.rs`, made `pub(crate)`.
- [x] 1.2 Update `search.rs` to import them from `edit_popup`; confirm the Tasks-tab `e` flow still works.
- [x] 1.3 `cargo test --workspace` + `cargo clippy` clean.

## 2. `TaskEdit` modal (ft TUI)

- [x] 2.1 Add `ActiveModal::TaskEdit(TaskEditState)` variant to `ft/src/tui/modal.rs`; define `TaskEditState { popup: EditPopup, path: PathBuf, line: usize }`.
- [x] 2.2 Implement `Modal` for `TaskEditState`: `handle_event` (Tab/Shift+Tab/Up/Down field nav, printable → `focused_buffer_mut`, `Enter`/`Ctrl+S` → validate + post `AppRequest::GraphTaskEdit`, `Esc` → Closed); `render` calls the lifted `render_edit_popup`; `commands`/`keymap` return `TASK_EDIT_*`; `name` = `"task-edit"`.
- [x] 2.3 Wire `TaskEdit` into the `ActiveModal` dispatch (handle_event/render/keymap_help/name/commands/keymap/dispatch_command).
- [x] 2.4 Add `TASK_EDIT_COMMANDS` + `TASK_EDIT_KEYMAP` to `ft/src/tui/modal_commands.rs` (commands `task-edit.confirm`/`task-edit.cancel`; keys `Enter`/`Ctrl+S`→confirm, `Esc`→cancel, Tab/Up/Down/Shift+Tab for nav).
- [x] 2.5 In `graph.rs`, add `graph.task-edit-popup` (`e`) `CommandDef` + dispatch arm: on a Task row, build `TaskEditState` (reuse `focused_task_edit_state`, adding `path`+`line`) and post `OpenModal(TaskEdit)`; toast on non-Task.

## 3. `AppRequest::GraphTaskEdit` + servicing (ft TUI)

- [x] 3.1 Add `AppRequest::GraphTaskEdit { path: PathBuf, line: usize, fields: PopupFields }` to `ft/src/tui/tab.rs` (+ `Display` arm).
- [x] 3.2 Add `Tab::graph_task_edit(&mut self, ctx, path, line, fields)` default no-op to the `Tab` trait.
- [x] 3.3 Service `GraphTaskEdit` in `App::service_request` + `drain_simple_requests` + `service_pending_for_test` + `service_request_for_test` (call `ops::update_task_line`, refresh graph, restore cursor).
- [x] 3.4 Override `graph_task_edit` on `GraphTab`.

## 4. `TaskLeader` chord + create (ft TUI)

- [x] 4.1 Add `ActiveModal::TaskLeader` variant + `struct TaskLeader;` to `modal.rs`; implement `Modal` mirroring `PeriodicLeader` (`c`→`GraphTaskCreate{TopLevel}`, `s`→`GraphTaskCreate{Subtask{...}}`, any other→Closed).
- [x] 4.2 Add `AppRequest::GraphTaskCreate { kind: TaskCreateKind }` where `TaskCreateKind` is `TopLevel { seed_path: Option<PathBuf> }` | `Subtask { parent_file: PathBuf, parent_line: usize }` to `tab.rs`.
- [x] 4.3 Add `Tab::graph_task_create(&mut self, ctx, kind)` default no-op; service `GraphTaskCreate` in App. **v1 delegates to the Tasks tab** (toast directing the user there); a full in-graph create quickline is a follow-up — the leader + request plumbing are in place for it.
- [x] 4.4 In `graph.rs`, add `graph.task-create`/`graph.task-new-subtask` CommandDefs + `a`→`graph.task-leader` binding + dispatch arm opening `TaskLeader`.
- [x] 4.5 `as` on a non-Task row toasts "select a task first" (the leader posts an empty parent; `graph_task_create` resolves the focused task or toasts).

## 5. `v` note-scoped task view (ft TUI)

- [x] 5.1 Add `graph.tasks-of-note` (`v`) CommandDef + dispatch arm: on Note/Directory/Task, rewrite the active view's query (D4) and re-materialize via `apply_query`; toast on empty.
- [x] 5.2 Bind `v` → `graph.tasks-of-note` in `GRAPH_KEYMAP`.

## 6. Keymap + help + docs (ft TUI)

- [x] 6.1 Bind `e`, `a`, `v` in `GRAPH_KEYMAP`.
- [x] 6.2 Add `TASK_LEADER_COMMANDS`/`KEYMAP` to `modal_commands.rs`.
- [x] 6.3 Update `GraphTab::help_sections`: document `e`, `a`+`c`/`s`, `v`.
- [x] 6.4 Regenerate `docs/keybindings.md`; verify `commands docs --check`.

## 7. Tests + build invariants

- [x] 7.1 TUI snapshot/unit tests: `e` opens edit popup on a Task row and commit updates the task on disk; `ac` opens a quickline seeded with the focused note's path; `as` under a task seeds the parent; `v` on a note rewrites the view to its tasks.
- [x] 7.2 `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `commands docs --check`.

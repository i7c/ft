## 1. Move modal state + `Modal` impl

- [x] 1.1 Create `ft/src/tui/tabs/tasks/move_modal.rs` (or `notes_actions/task_move.rs` — match the closest existing sibling) with a `TaskMoveState` struct holding `FuzzyPicker<VaultFilePickerSource>` plus the captured source `(path: PathBuf, line: usize, task: Task)`.
- [x] 1.2 Add a constructor `TaskMoveState::new(ctx: &TabCtx, source_path: PathBuf, source_line: usize, task: Task)` that builds `VaultFilePickerSource::new(Arc::clone(ctx.vault), Arc::clone(ctx.recents))`, wraps it in `FuzzyPicker::new(...)`, and stores the source identity.
- [x] 1.3 Implement `Modal for TaskMoveState`: `handle_event` drives `picker.handle_key` and maps `PickerOutcome` → `ModalOutcome` (Selected → commit per task 3.x; Cancelled → Closed; StillOpen → Consumed; NotHandled → NotHandled); `render` delegates to the shared move-overlay renderer (reuse `notes_view::render_move_overlay` or a thin tasks-tab renderer); `name` returns `"task-move"`; `keymap_help`/`commands`/`keymap` return the picker's chords.
- [x] 1.4 Add `ActiveModal::TaskMove(TaskMoveState)` to `ft/src/tui/modal.rs` and wire all `match ActiveModal` arms (`handle_event`, `render`, `keymap_help`, `name`, `commands`, `keymap`, `dispatch_command`).

## 2. Command + keymap registration

- [x] 2.1 Add a `CommandDef { name: "tasks.move", description: "...", opens_modal: true, scope: Tasks, group: <mutations group> }` to the Tasks-tab `COMMANDS` slice in `ft/src/tui/tabs/tasks/commands.rs`.
- [x] 2.2 Bind `M` → `tasks.move` in the Tasks-tab `KEYMAP` (`ft/src/tui/tabs/tasks/search.rs` next to the existing `.bind(...)` rows).
- [x] 2.3 Add a `dispatch_command` arm for `tasks.move` in the Tasks tab: on a task row, capture `(path, line, task)` from the cursor and open `ActiveModal::TaskMove(...)` via `AppRequest::OpenModal`; on a non-task row, toast "select a task first".
- [x] 2.4 Confirm the Tasks tab is registered with `with_keymap_overlay` in `build_tabs_with_overlays` (`ft/src/tui/app.rs`) so the new binding is visible to `?` / `ft commands check-keymap` (it already is; verify no regression).

## 3. Commit path (Hit → MoveTarget → plan → apply → refresh)

- [x] 3.1 Add a `Hit → MoveTarget` helper in the move modal: `abs = ctx.vault.path.join(&hit.path)`; `MoveTarget::UnderHeading(abs, h.text)` when `hit.heading.is_some()`, else `MoveTarget::Append(abs)`.
- [x] 3.2 Same-file guard: if `abs == source_path`, toast "can't move to the same file — pick a different target" (error style), keep the picker open, return `ModalOutcome::Consumed` (no plan, no write).
- [x] 3.3 Build `MoveSource { path: source_path, line: source_line, expected: Some(task) }` and call `ops::plan_move(&[source], &target, ctx.vault.task_format())`.
- [x] 3.4 On `Ok(plan)`: `ops::apply_move_plan(&plan)?`, then `ctx.request_graph_refresh()`, toast `moved to <path>` or `moved to <path>#<heading>` (vault-relative), return `ModalOutcome::Closed`.
- [x] 3.5 On `Err(MoveError::LineChanged)`: toast the drift message, `ctx.request_graph_refresh()`, return `ModalOutcome::Closed`.
- [x] 3.6 On other `MoveError` variants (`Read`/`Write`/`NotATask`/`LineMissing`): toast the error, return `ModalOutcome::Closed`.

## 4. Tests

- [x] 4.1 Unit test for the `Hit → MoveTarget` mapping (file-only → `Append`; file+heading → `UnderHeading`).
- [x] 4.2 Unit test for the same-file guard (target path == source path → no plan, no write, toast text).
- [x] 4.3 `TestBackend` snapshot under `ft/src/tui/tests/`: open the move modal from a task row, type a query, assert the picker frame renders.
- [x] 4.4 `TestBackend` snapshot: pick a target file → assert the task moved (source file lost the line, target file gained it) and the success toast rendered.
- [x] 4.5 `TestBackend` snapshot: cancel the picker (Esc) → assert no files changed and focus returned to the tab.

## 5. Docs + build invariants

- [x] 5.1 Regenerate the committed reference: `cargo run --release -q -- commands docs > docs/keybindings.md`.
- [x] 5.2 `cargo build --release`
- [x] 5.3 `cargo test --workspace`
- [x] 5.4 `cargo clippy --workspace --tests -- -D warnings`
- [x] 5.5 `cargo fmt --check`
- [x] 5.6 `cargo run --release -q -- commands docs --check`

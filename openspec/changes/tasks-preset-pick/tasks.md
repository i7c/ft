## 1. Cross-tab request channel (ft-core → TUI shell)

- [x] 1.1 Define `TasksRequest` enum in `ft/src/tui/tab.rs` with a single `ApplyPreset(String)` variant; add `AppRequest::Tasks(TasksRequest)` variant to `AppRequest`; extend the manual `Debug for AppRequest` impl with an `AppRequest::Tasks(req)` arm.
- [x] 1.2 Add `fn handle_tasks_request(&mut self, _req: TasksRequest, _ctx: &mut TabCtx)` to the `Tab` trait with a default no-op body, placed next to `handle_graph_request`.
- [x] 1.3 Add one `AppRequest::Tasks(req)` match arm in `App::service_simple` (`ft/src/tui/app.rs`) that does `with_tab(TabKind::Tasks, |tab, ctx| tab.handle_tasks_request(req, ctx))`, mirroring the existing `AppRequest::Graph` arm.

## 2. Task preset picker modal (types + ActiveModal wiring)

- [x] 2.1 Create `ft/src/tui/tabs/tasks/modals.rs` with `TaskPresetPickerSource` (reads `ctx.vault.config.config.presets` then `ft_core::query::preset::builtin`, dedups with user-shadows-built-in, same `PickerSource` impl shape as `PresetPickerSource`).
- [x] 2.2 In `tabs/tasks/modals.rs` add `TaskPresetPickerModal` wrapping `FuzzyPicker<TaskPresetPickerSource>`; implement `Modal` so on `PickerOutcome::Selected(name)` it resolves name→DSL (user map first, then `query::preset::builtin`) and posts `AppRequest::Tasks(TasksRequest::ApplyPreset(dsl))` then returns `Closed`; on `Cancelled` returns `Closed` with no request; `StillOpen`/`NotHandled` map to `Consumed`/`NotHandled`. `name()` returns `"task-preset-picker"`.
- [x] 2.3 Add `ActiveModal::TaskPresetPicker(TaskPresetPickerModal)` variant in `ft/src/tui/modal.rs` and extend the six `ActiveModal`-level dispatch arms (`handle_event`, `render`, `keymap_help`, `name`, `commands`, `keymap`, and `dispatch_command` if present) to forward to it.
- [x] 2.4 Re-export `TaskPresetPickerModal`/`TaskPresetPickerSource` from `tabs/tasks/mod.rs` so `modal.rs` can name the variant.

## 3. Command / keymap registry

- [x] 3.1 Add `CommandDef { name: "tasks.preset-pick", opens_modal: true, scope: Tab("tasks"), group: "Navigation", ... }` to `TASKS_COMMANDS` in `tabs/tasks/mod.rs`.
- [x] 3.2 Add `TASK_PRESET_PICKER_COMMANDS` + `TASK_PRESET_PICKER_KEYMAP` (scope `CommandScope::Modal("task-preset-picker")`, bound `Enter`/`Esc`/`Up`/`Down`) to `ft/src/tui/modal_commands.rs`; add them to the `all_modal_command_names_unique` test slice list and the `(&KEYMAP, COMMANDS)` registry-pairs list.
- [x] 3.3 Add `"modal/task-preset-picker" => Some(CommandScope::Modal("task-preset-picker"))` to `scope_for_command_name` in `ft/src/tui/keymap.rs`.

## 4. Tasks tab wiring (open + apply)

- [x] 4.1 Add `fn apply_preset(&mut self, dsl: &str, today: NaiveDate)` to `SearchView` in `tabs/tasks/search.rs` that sets `query_text = dsl.to_string()`, calls `recompile(today)` + `recompute_matches(today)`, and leaves `edit_state = None` (normal mode).
- [x] 4.2 Add `fn apply_preset(&mut self, _dsl: &str, _today: NaiveDate)` with a default no-op to the `view::View` trait (`tabs/tasks/view.rs`); override it in `SearchView` to call the method from 4.1.
- [x] 4.3 Implement `TasksTab::handle_tasks_request` (`tabs/tasks/mod.rs`) overriding the `Tab` trait default: on `TasksRequest::ApplyPreset(dsl)` call `self.views[self.active_view].apply_preset(dsl, ctx.today)`.
- [x] 4.4 Add a `"tasks.preset-pick"` arm to `SearchView::dispatch_idle_command` (`tabs/tasks/search.rs`) that builds `TaskPresetPickerSource::new(ctx.vault)`, early-returns `Handled` (no-op) if `items.is_empty()`, otherwise posts `AppRequest::OpenModal(ActiveModal::TaskPresetPicker(TaskPresetPickerModal::new(src)))` and returns `Handled`.
- [x] 4.5 Bind `Ctrl+p` → `tasks.preset-pick` in `SEARCH_KEYMAP` (`tabs/tasks/search.rs`).

## 5. Help overlay + docs

- [x] 5.1 Add a `("Ctrl+P", "load preset into query")` row to the Tasks tab `help_sections()` in `tabs/tasks/mod.rs` (Navigation section).
- [x] 5.2 Regenerate `docs/keybindings.md` via `cargo run --release -q -- commands docs > docs/keybindings.md` and verify `cargo run --release -q -- commands docs --check` passes.

## 6. Tests + snapshots

- [x] 6.1 Add a unit test in `tabs/tasks/` mirroring `ctrl_p_preset_replaces_active_view_query` (graph): build a `TasksTab` with a snapshot, call the open helper, assert `OpenModal(ActiveModal::TaskPresetPicker)` is queued; feed Enter to the modal, assert `AppRequest::Tasks(TasksRequest::ApplyPreset(_))` is queued; call `handle_tasks_request` and assert the active view's `query_text` matches the selected preset DSL and `edit_state` is `None`.
- [x] 6.2 Add an `insta` TUI snapshot of the tasks tab with the `task-preset-picker` modal open (TestBackend), modeled on `graph_tab_preset_picker_open.snap`.
- [x] 6.3 Update/bless the tasks help-overlay snapshot (`help_overlay_over_tasks_80x24.snap`) to include the new `Ctrl+P` row.
- [x] 6.4 Verify `App::active_modal_name()` returns `Some("task-preset-picker")` when the modal is open (assert in the snapshot test or a focused routing test).

## 7. Build invariants

- [x] 7.1 `cargo build --release`
- [x] 7.2 `cargo test --workspace`
- [x] 7.3 `cargo clippy --workspace --tests -- -D warnings`
- [x] 7.4 `cargo fmt --check`
- [x] 7.5 `cargo run --release -q -- commands docs --check`

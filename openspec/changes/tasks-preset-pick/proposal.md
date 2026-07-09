## Why

The Graph tab has a `Ctrl+P` "load preset into active view" flow; the Tasks tab — whose query bar is arguably more preset-friendly (built-ins like `today`, `overdue`, `not-done`) — has no preset support at all. Task presets already exist in `ft-core` (`query::preset::builtin` + user `Config::presets`) and power the CLI (`ft tasks --preset`), but they are unreachable from the Tasks TUI. This change brings the TUI Tasks tab to parity with the Graph tab's preset picker.

## What Changes

- Add a `tasks.preset-pick` command (`opens_modal: true`, scope `tab/tasks`, group `Navigation`) to `TASKS_COMMANDS`, bound to `Ctrl+p` in the Tasks `SEARCH_KEYMAP`.
- Add a tasks-specific preset picker: a new `TaskPresetPickerSource` (reads `Config::presets` + `query::preset::builtin`, user shadows built-in) and a new `TaskPresetPickerModal` wrapping `FuzzyPicker`, plus a new `ActiveModal::TaskPresetPicker` variant.
- Add a new `modal/task-preset-picker` command scope + `TASK_PRESET_PICKER_COMMANDS` / `TASK_PRESET_PICKER_KEYMAP` (parallel to the existing graph `preset-picker` modal registry) so `?`-overlay, `ft commands list`, and `ft do` cover the tasks picker.
- Add a new cross-tab routing path for the Tasks tab: an `AppRequest::Tasks(TasksRequest)` variant + a `TasksRequest::ApplyPreset(dsl)` payload + a `Tab::handle_tasks_request` hook. The App routes `AppRequest::Tasks` by looking up the `TabKind::Tasks` tab (mirroring the existing `AppRequest::Graph` + `handle_graph_request` pattern).
- On selection, the modal resolves the preset name → DSL string and posts `AppRequest::Tasks(TasksRequest::ApplyPreset(dsl))`; the Tasks tab's `handle_tasks_request` sets the active `SearchView`'s `query_text` and recompiles + recomputes matches against the shared snapshot (no graph rebuild), then stays in normal mode.
- Update the Tasks tab `help_sections()` to list `Ctrl+P` and regenerate `docs/keybindings.md`.
- Keep the graph-tab empty-presets no-op guard on the tasks side (unreachable in practice — built-ins always exist — but consistent).

## Capabilities

### New Capabilities
- `tasks-load-preset`: Tasks-tab `Ctrl+P` flow that opens a fuzzy picker over task presets (user `Config::presets` + built-in `query::preset::builtin`) and applies the selected preset's DSL to the active SearchView's query, mirroring the graph tab's `graph-load-preset` capability.

### Modified Capabilities
- `tui-tab-request-routing`: adds a parallel typed routing channel for the Tasks tab — `AppRequest::Tasks(TasksRequest)` + `Tab::handle_tasks_request` — so modal-raised, Tasks-targeted requests can return to the owning tab. (Today only the Graph tab has such a channel.)
- `tui-modal-driver`: adds one new `ActiveModal::TaskPresetPicker` variant (a picker modal committing via `AppRequest::Tasks`), following the established picker-modal pattern.

## Impact

- **Code:** `ft/src/tui/tab.rs` (new `TasksRequest` enum, `AppRequest::Tasks` variant, `Tab::handle_tasks_request` hook, `Debug` arm); `ft/src/tui/app.rs` (one new `service_simple` match arm routing to `TabKind::Tasks`); `ft/src/tui/modal.rs` (new `ActiveModal` variant + dispatch arms); `ft/src/tui/modal_commands.rs` (new scope + commands + keymap); `ft/src/tui/keymap.rs` (new `modal/task-preset-picker` scope mapping); `ft/src/tui/tabs/tasks/mod.rs` (new command def + help section); `ft/src/tui/tabs/tasks/search.rs` (`Ctrl+p` binding, dispatch arm, open + apply helpers, `handle_tasks_request` impl on `TasksTab`); a new `ft/src/tui/tabs/tasks/modals.rs` (or alongside search.rs) hosting `TaskPresetPickerSource` + `TaskPresetPickerModal`.
- **Docs:** `docs/keybindings.md` regenerated via `ft commands docs`; one tasks help-overlay snapshot updated (the `?` overlay gains a `Ctrl+P` row).
- **Tests:** a unit test mirroring `ctrl_p_preset_replaces_active_view_query` in the graph tab tests (open picker → Enter → assert active view's query replaced); a TUI snapshot of the open tasks preset picker; the `all_modal_command_names_unique` test extended with the new commands slice; the modal-name routing in `keymap.rs`.
- **No CLI change:** `ft tasks --preset` already resolves the same built-in + user presets; the TUI reuses the same `ft_core::query::preset` source of truth. No new config keys.
- **No breaking changes.** Purely additive: new command, new modal variant, new routing variant, new keybinding on a previously-free chord.

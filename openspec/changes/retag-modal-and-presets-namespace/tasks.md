## 1. Config: move presets and add retag_tags

- [x] 1.1 In `ft-core/src/config.rs`, move the `presets: HashMap<String, String>` field off `Config` and onto the `Tasks` struct (so it reads from `[tasks.presets]`). Update the field doc comment to reference `[tasks.presets]`.
- [x] 1.2 Add `retag_tags: Vec<String>` to the `Tasks` struct in `ft-core/src/config.rs`, `#[serde(default)]`, doc'd as "bare tag names (no leading `#`) offered by the retag picker; empty by default."
- [x] 1.3 Update the existing config test `presets_loaded_correctly` (around line 670) to read `[tasks.presets]` and assert against `lc.config.tasks.presets`.
- [x] 1.4 Add a config test asserting a legacy top-level `[presets]` section fails to load with a `deny_unknown_fields` error naming `presets`.
- [x] 1.5 Add a config test that `[tasks]\nretag_tags = ["wait","computer","physical"]` round-trips into `config.tasks.retag_tags`, and that an absent key defaults to `Vec::new()`.

## 2. ft-core: retag op

- [x] 2.1 Add `fn is_tag_word(w: &str) -> bool` to `ft-core/src/task/ops.rs` (or a small shared helper) matching the predicate in `tabs/tasks/edit_popup.rs`: a word starting with `#` whose remainder is non-empty and all-alphanumeric/`_`/`-`/`/`.
- [x] 2.2 Add `pub fn retag_task(target_path, line, format, expected: Option<&Task>, list: &[String], tag: &str) -> Result<Task, UpdateError>` in `ft-core/src/task/ops.rs`. It wraps `update_task_line` with a mutate closure that rebuilds `task.description`: keep every whitespace-delimited word that is not a list-member `#tag`, then append `#<tag>` when `tag` is non-empty. Reuse the existing `extract_tags` / `is_tag_word` for the membership check.
- [x] 2.3 Add unit tests in `ft-core/src/task/ops.rs` (or a `mod tests` near it) covering: swap-replaces-prior-list-tag, append-when-no-list-tag-present, glued-punctuation-preserved, empty-`tag`-clears-list-tags, non-list-tag-preserved. Use the `EmojiFormat` and `write_atomic` against a temp file.

## 3. TUI: request routing and modal variant

- [x] 3.1 Add `RetagSelected(String)` variant to `TasksRequest` in `ft/src/tui/tab.rs` with a doc comment mirroring `ApplyPreset(String)` (carries the bare tag picked by the retag modal).
- [x] 3.2 Add `TaskRetagPicker(ActiveModal)` variant to the `ActiveModal` enum in `ft/src/tui/modal.rs`; wire it into the `handle_event`, `render`, `commands`/`keymap`/`help` dispatch (match arms) following the `TaskPresetPicker` pattern exactly.
- [x] 3.3 Add `TASK_RETAG_PICKER_COMMANDS` and `TASK_RETAG_PICKER_KEYMAP` static slices to `ft/src/tui/modal_commands.rs` mirroring `TASK_PRESET_PICKER_*`; register them in the central registry list (the `&[...]` array near line 746 and the `(&KEYMAP, COMMANDS)` tuple near line 784).

## 4. TUI: retag modal and SearchView wiring

- [x] 4.1 Add `TaskRetagPickerSource` + `TaskRetagPickerModal` to `ft/src/tui/tabs/tasks/modals.rs`, parallel to `TaskPresetPickerSource`/`TaskPresetPickerModal`. Source reads `config.tasks.retag_tags`; items are bare names labeled with a leading `#` in the picker. On Enter, post `AppRequest::Tasks(TasksRequest::RetagSelected(name))` and return `Closed`; on Esc return `Closed` with no request.
- [x] 4.2 Add `tasks.retag` `CommandDef` to `TASKS_COMMANDS` in `ft/src/tui/tabs/tasks/mod.rs` (group "Mutations", `opens_modal: true`) and a binding in `SEARCH_KEYMAP` (`ft/src/tui/tabs/tasks/search.rs`) on an unbound chord (confirm it doesn't collide; `g` is a global leader, `T` is free on the Tasks tab — pick one and record it).
- [x] 4.3 Implement `SearchView::apply_retag(&mut self, tag: &str, ctx: &mut TabCtx) -> Result<EventOutcome>` mirroring `apply_preset`'s shape but calling `ops::retag_task` inside `with_selected_task` (pass `config.tasks.retag_tags` from `ctx.vault.config.config.tasks.retag_tags`). It reuses the existing anchor-restore + graph-refresh machinery in `with_selected_task`.
- [x] 4.4 Add the `RetagSelected(tag)` arm to `TasksTab::handle_tasks_request` in `ft/src/tui/tabs/tasks/mod.rs`, routing to the active view's `apply_retag`.
- [x] 4.5 In `SearchView::dispatch_idle_command` (`ft/src/tui/tabs/tasks/search.rs`), add the `"tasks.retag"` arm: if `ctx.vault.config.config.tasks.retag_tags` is empty, post a toast ("no retag tags configured — add [tasks.retag_tags]"); otherwise open the `TaskRetagPicker` modal via `AppRequest::OpenModal`.

## 5. CLI + docs

- [x] 5.1 Update `resolve_preset` in `ft/src/cmd/tasks.rs` to read `vault.config.config.tasks.presets` instead of `vault.config.config.presets`.
- [x] 5.2 Update the config dump in `ft/src/cmd/vault.rs` to report `config.tasks.presets` under a `tasks.presets` heading (mirroring the `graph.presets` reporting style), and stop reading the removed top-level `cfg.presets`.
- [x] 5.3 Update the `tasks.preset-pick` source `TaskPresetPickerSource::new` in `ft/src/tui/tabs/tasks/modals.rs` to read `vault.config.config.tasks.presets`.
- [x] 5.4 Regenerate `docs/keybindings.md`: `cargo run --release -q -- commands docs > docs/keybindings.md`, then verify `cargo run --release -q -- commands docs --check` passes.

## 6. Build invariants + tests

- [x] 6.1 Run `cargo build --release` and fix any remaining references to the old `config.presets` field across the workspace (`rg -n "\.presets" ft-core/src ft/src`).
- [x] 6.2 Run `cargo test --workspace`; add/adjust TUI snapshot tests in `ft/src/tui/tests/tasks.rs` for the retag picker open state (mirror `tasks_tab_preset_picker_open_80x24`).
- [x] 6.3 Run `cargo clippy --workspace --tests -- -D warnings` and `cargo fmt --check`.
- [x] 6.4 Run `cargo run --release -q -- commands docs --check` and confirm the keybindings doc is in sync.

## Why

Task batching by context (physical vs computer, or "waiting" follow-ups) only
works if reclassifying a task is fast. Today, switching a task from `#computer`
to `#wait` means opening the edit popup, retyping the tags field, and
submitting — enough friction that the queue split degrades into one big list.
A picker over a user-defined short tag list (default empty) makes the swap one
chord + one Enter, replacing only tags drawn from that list and leaving every
other tag untouched.

Separately, task query presets live at the top-level `[presets]` in config
while graph presets live at `[graph.presets]`. The asymmetry is confusing.
This change moves task presets to `[tasks.presets]`, matching the graph
convention and the existing `[tasks]` config block.

## What Changes

- **New** `[tasks.retag_tags]` config field: a list of tag names (default
  empty) offered by the retag picker. Tags are stored without a leading `#`.
- **New** TUI command `tasks.retag` on the Tasks SearchView, opening a fuzzy
  picker modal over `tasks.retag_tags`. Selecting an entry swaps the selected
  task's inline `#tag` words: any tag from the configured list is removed and
  the picked tag is appended. Tags not in the list are left untouched. Esc
  cancels with no write.
- **New** `TasksRequest::RetagSelected(String)` variant carrying the picked
  tag from the modal to the SearchView, mirroring the existing
  `ApplyPreset(String)` routing seam.
- **New** `ft_core::task::ops::retag_task` op performing the surgical
  description rewrite (strip list-members, append the selected tag), going
  through the existing `update_task_line` + expected-`Task` guard path.
- **BREAKING**: task query presets move from top-level `[presets]` to
  `[tasks.presets]`. `Config` uses `deny_unknown_fields`, so a leftover
  top-level `[presets]` section is a startup error, not a silent ignore.
  Users update their config TOML by renaming the section.
- `ft vault config` dump and the CLI `--preset` resolver read from the new
  `config.tasks.presets` location.

## Capabilities

### New Capabilities
- `task-retag`: A picker modal that swaps one tag from a configured short
  list into the selected task, replacing any prior tag from that list while
  preserving all other tags.
- `task-presets`: Storage of task query presets under `[tasks.presets]`
  (mirroring `[graph.presets]`), with CLI/TUI resolution preferring user
  presets over built-ins.

### Modified Capabilities
<!-- None. Task presets config had no prior spec; the new location is
     specced fresh under task-presets. The TasksRequest channel gains a
     variant but the routing mechanism (tui-tab-request-routing) is about
     the GraphRequest pattern and is not altered. -->

## Impact

- **ft-core**: `config.rs` (move `presets` into `Tasks`, add `retag_tags` to
  `Tasks`); `task/ops.rs` (new `retag_task` op); config round-trip tests.
- **ft binary**: `cmd/tasks.rs` and `cmd/vault.rs` read `config.tasks.presets`;
  `tui/tabs/tasks/modals.rs` (new `TaskRetagPickerModal` +
  `TaskRetagPickerSource`); `tui/tabs/tasks/search.rs` (handle
  `TasksRequest::RetagSelected`, wire the `tasks.retag` command into
  `dispatch_idle_command` and `SEARCH_KEYMAP`); `tui/tab.rs`
  (`TasksRequest::RetagSelected` variant); `tui/modal.rs` (`ActiveModal`
  variant); `tui/modal_commands.rs` (picker command/keymap slice); new
  `tasks.retag` `CommandDef` in `tabs/tasks/mod.rs`.
- **docs**: regenerate `docs/keybindings.md` via `ft commands docs`.
- **Users**: rename `[presets]` → `[tasks.presets]` in their config TOML;
  optionally populate `[tasks.retag_tags]`.
- **Build invariants**: new command/keymap rows require `ft commands docs
  --check` to pass; the config rename touches `cargo test` config tests.

## Why

`ft tasks move` (relocate a task to another file, optionally under a
heading) is CLI-only today. The TUI's Tasks tab can complete, cancel,
re-date, re-priority, retag, and edit a task in place, but cannot move it
to a different file — the user must drop to the shell. Every primitive
needed already exists: `ops::plan_move` / `ops::apply_move_plan`
(`ft-core/src/task/ops.rs`), the `MoveTarget::{Append, UnderHeading}`
model, and a fuzzy file+heading picker (`VaultFilePickerSource`) already
wired into the Tasks tab for new-task creation. This change closes the
gap by reusing that picker to drive a single-task move from the Tasks
tab.

## What Changes

- **`tasks.move` command + `M` keybinding (Tasks tab).** With the
  cursor on a task row, `M` opens the existing `VaultFilePickerSource`
  fuzzy picker (the same one `New`-mode task creation uses) to choose a
  target file and optional `#heading`. On selection the flow builds a
  `MoveTarget` from the `Hit` (`Append(path)` or
  `UnderHeading(path, heading)`), a single-element `MoveSource` from the
  cursor task (`expected: Some(task)` guard), calls `ops::plan_move` +
  `ops::apply_move_plan`, then `ctx.request_graph_refresh()` and toasts
  success/failure.
- **Same-file guard.** If the picked target resolves to the same file
  the task already lives in, the flow toasts an error and stays open
  (no plan, no write) — matching the `section_move` flow's stance rather
  than the CLI's permissive no-op. Bulk move (`--query`) is explicitly
  out of scope: the Tasks tab has a single cursor and no multi-select;
  bulk would require adding marking first, which is a separate change.
- **Modal-driven flow.** The move is a new `ActiveModal` variant (per
  the modal-driver pattern) rather than a per-tab `Option<...>` field.
  It wraps the existing `FuzzyPicker<VaultFilePickerSource>`; the
  `open_target_picker` / `handle_target_picker_key` helpers in
  `tasks/edit_popup.rs` are the reference for `Hit` → target-string
  composition, though the move flow builds a `MoveTarget` directly
  rather than round-tripping through an `EditBuffer`.
- **Docs sync.** `ft commands docs` regenerated so `docs/keybindings.md`
  reflects the new `M` binding.

## Capabilities

### New Capabilities

- `tui-tasks-move`: Move the cursor task to a different file (optionally
  under a heading) from the Tasks tab via a fuzzy file+heading picker,
  reusing the existing `VaultFilePickerSource` and `ops::plan_move` /
  `ops::apply_move_plan` primitives.

### Modified Capabilities

- `tui-commands`: new `tasks.move` `CommandDef` on the Tasks tab.
- `tui-keymaps`: `M` binding on the Tasks tab → `tasks.move`.

## Impact

- **New code:** a move modal (state + `Modal` impl) under
  `ft/src/tui/notes_actions/` or `ft/src/tui/tabs/tasks/`, an
  `ActiveModal` variant in `ft/src/tui/modal.rs`, a `tasks.move`
  `CommandDef` + keymap row, a dispatch arm in the Tasks tab, and a
  commit path calling `ops::plan_move` / `ops::apply_move_plan` +
  `ctx.request_graph_refresh()`.
- **Reused, unchanged:** `VaultFilePickerSource` + `FuzzyPicker`
  (`ft/src/tui/widgets/picker.rs`), `ops::plan_move` /
  `ops::apply_move_plan` / `MoveTarget` / `MoveSource`
  (`ft-core/src/task/ops.rs`), `ctx.request_graph_refresh()`.
- **No core API changes:** no new params to widely-called functions;
  `MoveSource`/`MoveTarget` are constructed, not modified.
- **Tests:** a `TestBackend` snapshot of the move flow (open → pick →
  commit) under `ft/src/tui/tests/`, plus a unit test for the
  same-file guard. `cargo run --release -q -- commands docs >
  docs/keybindings.md` regenerated.
- **Build invariants:** all five (`build`, `test`, `clippy`, `fmt`,
  `commands docs --check`) stay clean.

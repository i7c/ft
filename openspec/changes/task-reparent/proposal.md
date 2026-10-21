## Why

Restructuring a task tree is the most common reorganisation in a task
vault, and ft can't do it. `ops::plan_move` can only target a file
(`MoveTarget::Append`) or a heading (`MoveTarget::UnderHeading`)
(`ft-core/src/task/ops.rs:715`). To move a subtask to another parent,
or to promote or demote a task one level, you have to open `$EDITOR`
and fix indentation by hand. The TUI's `tasks.move` (`M`) only offers a
file/heading picker, so even file-level moves take several steps.

This is phase 1 of a larger task revamp (see design.md §"Roadmap").
It adds reparenting as a first-class move operation; later phases
build cross-file parents and Jira/GitHub sources on top of it.

## What Changes

- **Core: task-relative move targets.** `MoveTarget` gains
  `UnderTask { path, line, expected, placement }`, where `placement` is
  `Child` (append as the parent's last child) or `SiblingAfter` (insert
  after the target's whole block, at the target's indent). Moved blocks
  are re-indented: the head line takes the new depth, and nested lines
  keep their relative depth, converted to the target file's indent unit.
  Moving a task under itself or one of its descendants is rejected with
  a new `MoveError::IntoOwnSubtree`. A stale target line fails with
  `MoveError::TargetChanged`.
- **Core: indent and outdent as moves.** `ops::plan_indent` makes the
  task the last child of its previous sibling. `ops::plan_outdent`
  makes it the next sibling of its parent, placed after the parent's
  block. Both produce a `MovePlan` through `plan_move`, so the apply
  path, guards and atomic writes are shared.
- **Core: indent-unit detection.** A `detect_indent_unit(lines)` helper
  infers the file's indent unit (tab, or N spaces) and falls back to
  two spaces. Both `UnderTask` and the existing subtask placement in
  `create_task` (`ft tasks create --parent`) use it. A first child in a
  file indented with 4 spaces or tabs now matches that file instead of
  always getting 2 spaces.
- **CLI.**
  - `ft tasks move <SEL> --under <SEL> [--sibling]` is added; `--to` and
    `--under` are mutually exclusive.
  - `ft tasks indent <SEL>` and `ft tasks outdent <SEL>` are added.
  - All three gain `--format text|json`. The JSON form reports each
    moved task's old and new `path:line`, so editor clients can follow
    the task. `--dry-run`, `--query` bulk and `--yes` keep working with
    `--under`.
- **TUI Tasks tab, cut and paste:**
  - `d` toggles the cursor task in a cut set (multiple tasks allowed;
    cut rows are marked).
  - `p` pastes the cut set as children of the cursor task, and `P`
    pastes it as siblings after the cursor task. This works within a
    file and across files.
  - `Esc` clears the cut set.
- **TUI Tasks tab, indent and outdent:** `>` indents and `<` outdents
  the cursor task.
- **BREAKING (keymap):** `tasks.priority-next` / `tasks.priority-prev`
  move from `p` / `P` to `+` / `-`. Users who override these in
  `[keymap]` are unaffected; everyone else needs to retrain.
- **[ft.nvim]** A move-under command that shells out to
  `ft tasks move --under … --format json`, plus indent and outdent.

## Capabilities

### New Capabilities

- `task-reparent`: Core and CLI reparenting. Covers `MoveTarget::UnderTask`
  child/sibling placement, re-indentation, the subtree and stale-target
  guards, indent/outdent planners, indent-unit detection, and the
  `ft tasks move --under`, `ft tasks indent` and `ft tasks outdent` CLI
  surfaces with JSON output.
- `tui-tasks-reparent`: The Tasks-tab cut set (`d`), paste as child or
  sibling (`p`/`P`), clearing it (`Esc`), and indent/outdent (`>`/`<`).

### Modified Capabilities

- `tui-keymaps`: Tasks-tab priority cycling moves from `p`/`P` to
  `+`/`-`; new bindings `d`, `p`, `P`, `>`, `<`, `Esc`.
- `tasks-create-parent`: the first subtask's indent follows the
  detected indent unit instead of a hard-coded two spaces.

## Impact

- **ft-core:** `task/ops.rs` (`MoveTarget` variant, `MoveError`
  variants, `plan_move` insertion path, `plan_indent`/`plan_outdent`,
  `detect_indent_unit`, `subtask_placement`). `MoveTarget` is matched in
  only a few places (`MoveTarget::path`, `splice_into_target`, the CLI's
  `parse_move_target`, and the TUI move modal), so adding a variant
  ripples very little. `plan_move`'s signature does not change.
- **ft binary:** `cmd/tasks.rs` (`MoveArgs`, new `indent`/`outdent`
  subcommands, JSON output), and the Tasks tab (`tabs/tasks/search.rs`
  keymap, cut-set state, dispatch arms, row rendering, `help_sections`).
- **Docs:** `docs/keybindings.md` regenerated; README task section
  updated.
- **Tests:** unit tests and a proptest-style table in `ops.rs` for
  re-indentation; CLI integration tests under `ft/tests/`; `TestBackend`
  snapshots for the cut marker and for paste and indent results.
- **No new dependencies, no network, no change to scan performance.**

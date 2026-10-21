## 1. Core: indent unit

- [x] 1.1 Add `IndentUnit { Tab, Spaces(usize) }` and `detect_indent_unit(lines: &[String]) -> IndentUnit` in `ft-core/src/task/ops.rs` (list-item lines only, stack-based parent indent, min positive delta, `Spaces(2)` fallback).
- [x] 1.2 Unit tests: 2-space, 4-space, tab, flat file, mixed list/non-list lines, ordered lists.
- [x] 1.3 Switch `subtask_placement`'s childless fallback from `"  "` to parent whitespace + detected unit; update/add `create_task` tests (tab-indented file, 4-space file).

## 2. Core: `MoveTarget::UnderTask`

- [x] 2.1 Add `Placement { Child, SiblingAfter }` and `MoveTarget::UnderTask { path, line, expected, placement }`; extend `MoveTarget::path`.
- [x] 2.2 Add `MoveError::{IntoOwnSubtree, TargetChanged, NoPreviousSibling, AlreadyTopLevel}` with user-facing messages (vault-relative paths are the caller's job).
- [x] 2.3 In `plan_move`, validate the target line on the original target lines: parse via `format`, `check_expected` → `TargetChanged`, not-a-task → `NotATask`, compute target block end.
- [x] 2.4 Subtree guard: if the target index falls within any kept source range of the same file → `IntoOwnSubtree`.
- [x] 2.5 Map the insertion index through same-file removals (`e - removed_before(e)`) and splice after it.
- [x] 2.6 Re-indent moved blocks per design D3 (head whitespace per placement; relative whitespace verbatim when units equal, depth-converted otherwise). Leave `Append`/`UnderHeading` behaviour unchanged.
- [x] 2.7 Add `MovedBlock::dest: Option<(PathBuf, usize)>` and fill it for all target kinds.
- [x] 2.8 Table-driven tests: {source above / below / in another file} × {Child (with/without existing children), SiblingAfter} × {same unit, tab→spaces, spaces→tab}; own-subtree and self-target rejection; stale target; non-task block lines travel; multiple sources incl. ancestor+descendant.
- [x] 2.9 Proptest: for random small task trees and random valid `UnderTask` moves, the multiset of trimmed non-blank lines is unchanged and the result re-parses with the moved task's `parent` equal to the target (Child) or the target's parent (SiblingAfter).

## 3. Core: indent / outdent

- [x] 3.1 `plan_indent(source: &MoveSource, format) -> Result<MovePlan, MoveError>`: parse the file's tasks, `resolve_hierarchy`, find the previous sibling (same parent and indent) → `UnderTask{Child}`; else `NoPreviousSibling`.
- [x] 3.2 `plan_outdent(...)`: parent lookup → `UnderTask{parent, SiblingAfter}`; top-level → `AlreadyTopLevel`.
- [x] 3.3 Tests: indent under sibling with/without children, indent first child errors, outdent keeps later siblings with the old parent, outdent top-level errors, `LineChanged` on a stale source.

## 4. CLI

- [ ] 4.1 `MoveArgs`: add `--under <SEL>` (an `ArgGroup` with `--to`, required + exclusive) and `--sibling` (`requires = "under"`); add `--format text|json`.
- [ ] 4.2 Factor the unique-task resolver used by `create --parent` into a shared helper; use it for `--under` (candidate listing on ambiguity, id→fuzzy fallback).
- [ ] 4.3 `run_move`: build `UnderTask` with the resolved target as the `expected` guard; keep `--query`, `--dry-run` and `--yes` working; map new `MoveError`s to messages.
- [ ] 4.4 Add `ft tasks indent <SEL>` / `ft tasks outdent <SEL>` (`--dry-run`, `--format`).
- [ ] 4.5 JSON output: an array of `{description, from:{path,line}, to:{path,line}}`, vault-relative, from `MovedBlock::dest`.
- [ ] 4.6 Integration tests in `ft/tests/` (assert_cmd + assert_fs): cross-file `--under`, `--sibling`, ambiguous `--under`, `--to` + `--under` conflict, indent/outdent, `--dry-run` diff, `--format json` shape (insta snapshot).

## 5. TUI: keymap + commands

- [ ] 5.1 Add `CommandDef`s `tasks.cut-toggle`, `tasks.cut-clear`, `tasks.paste-child`, `tasks.paste-sibling`, `tasks.indent`, `tasks.outdent` (group "Restructure") to `TASKS_COMMANDS`.
- [ ] 5.2 `SEARCH_KEYMAP`: bind `d`, `Esc`, `p`, `P`, `>`, `<`; move `tasks.priority-next`/`-prev` to `+`/`-`.
- [ ] 5.3 Add the "Restructure" section to `help_sections()`.

## 6. TUI: cut set + paste + indent/outdent

- [ ] 6.1 Add `cut: Vec<CutEntry { key: TaskKey, snapshot: Task }>` to `SearchView`; `tasks.cut-toggle` / `tasks.cut-clear` dispatch arms (`Esc` is consumed only when the set is non-empty).
- [ ] 6.2 Render a cut marker in the row gutter and a status hint `N cut — p child · P sibling · Esc clear`.
- [ ] 6.3 `tasks.paste-child` / `tasks.paste-sibling`: empty-set toast; build sources in cut order + guarded `UnderTask` target from the cursor; plan → apply → clear set → `request_graph_refresh` → pending anchor at `blocks[0].dest` → toast. On error: toast, keep the set, refresh on `LineChanged`/`TargetChanged`.
- [ ] 6.4 `tasks.indent` / `tasks.outdent` dispatch arms via `plan_indent` / `plan_outdent`, pending anchor on the moved task, info toasts for `NoPreviousSibling` / `AlreadyTopLevel`.
- [ ] 6.5 Re-resolve the cut set on graph generation change (same file, equal serialized line, nearest line); drop the rest with a toast.
- [ ] 6.6 `TestBackend` tests under `ft/src/tui/tests/`: cut marker snapshot (2 rows), paste-child across files (files + cursor), paste-sibling, paste into own subtree toast, cut survives an unrelated line-inserting mutation, `>` / `<` results, `+` raises priority, `p` with an empty set toasts.

## 7. ft.nvim

- [ ] 7.1 [ft.nvim] Move-under command: pick a target task, run `ft tasks move <file>:<line> --under <file>:<line> [--sibling] --format json`, reload the affected buffers, jump to `to`.
- [ ] 7.2 [ft.nvim] Indent / outdent commands on the cursor task line via `ft tasks indent|outdent … --format json`.
- [ ] 7.3 [ft.nvim] Document the new CLI surface in ft.nvim's `ARCHITECTURE.md` protocol contract.

## 8. Docs + build invariants

- [ ] 8.1 README tasks section: reparent, indent/outdent, and the BREAKING note on priority chords (`+`/`-`).
- [ ] 8.2 `cargo run --release -q -- commands docs > docs/keybindings.md`
- [ ] 8.3 `cargo build --release`
- [ ] 8.4 `cargo test --workspace`
- [ ] 8.5 `cargo clippy --workspace --tests -- -D warnings`
- [ ] 8.6 `cargo fmt --check`
- [ ] 8.7 `cargo run --release -q -- commands docs --check`

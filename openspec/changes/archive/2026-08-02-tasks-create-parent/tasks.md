# tasks-create-parent tasks

## 1. CLI flag + resolution

- [x] 1.1 Add `--parent <SELECTOR>` to `CreateArgs` in `ft/src/cmd/tasks.rs` with `#[arg(long, value_name = "SELECTOR", conflicts_with_all = ["file", "under_heading", "at_line", "append"])]` and a doc comment describing the selector forms (id, `file:line`, fuzzy) and the unique-match requirement.
- [x] 1.2 Add a private `fn resolve_parent<'a>(selector: &str, scan: &'a ft_core::vault::Scan) -> Result<&'a Task>` helper in `tasks.rs` (near `pick_task`): `selector::parse` → `selector::resolve`; if empty and the parsed form is `Selector::Id`, retry with `Selector::Fuzzy` (id→fuzzy fallback, matching `resolve_targets`); empty → `anyhow!("no tasks match selector `{s}`")`; multiple → error listing up to five `file:line  description` candidates plus `… and N more`, mirroring the `--yes` non-TTY path of `pick_task`.

## 2. Wire into `run_create`

- [x] 2.1 In `run_create`, when `args.parent` is `Some`: run `vault.scan()` (warn on scan errors via `tracing::warn!`, as in `run_complete`), call `resolve_parent`, set `target = vault.path.join(parent.source_file)` and `position = Position::Subtask { parent_line: parent.source_line }`, bypassing `resolve_target_path`/`auto_position`. Keep the existing duplicate-check/`--force` path unchanged.
- [x] 2.2 Confirm the `Created task at …` output line and `--edit` handoff still work with the subtask target (they operate on `target`/`outcome.line`, which `create_task` already returns correctly for `Subtask` placement).

## 3. Integration tests (`ft/tests/tasks_create.rs`)

- [x] 3.1 Parent by `file:line`: seed `inbox.md` with a top-level task; `--parent inbox.md:<line>`; assert the new line is indented two spaces, immediately after the parent, in `inbox.md`.
- [x] 3.2 Parent by task id: seed a task with `🆔 abc123`; `--parent abc123`; assert the subtask lands under it.
- [x] 3.3 Parent by fuzzy substring and id→fuzzy fallback: single description match via `--parent build` (id-shaped, no such id); assert subtask lands under the matching task.
- [x] 3.4 Ambiguity error: seed two tasks matching the same fuzzy needle; assert the command fails with an error naming both candidates and the file is unchanged.
- [x] 3.5 No-match error: `--parent nope`; assert failure naming the selector, file unchanged.
- [x] 3.6 Clap conflicts: `--parent` with `--file`, with `--at-line`, with `--under-heading`, and with `--append` each fail with a conflict error and no file is modified.
- [x] 3.7 Indent matching existing children: parent with a four-space child; assert the new task is written after the last child at four spaces.
- [x] 3.8 Duplicate interaction: same description+dates as an existing task elsewhere in the parent's file → error naming the duplicate `file:line`; retry with `--force` → inserted.
- [x] 3.9 Done parent via `file:line`: parent task is `- [x]`; assert subtask still created under it.

## 4. Build invariants

- [x] 4.1 `cargo build --release`
- [x] 4.2 `cargo test --workspace`
- [x] 4.3 `cargo clippy --workspace --tests -- -D warnings`
- [x] 4.4 `cargo fmt --check`
- [x] 4.5 `cargo run --release -q -- commands docs --check` (no keymap change expected — verifies nothing drifted)

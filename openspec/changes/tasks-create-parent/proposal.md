# tasks-create-parent

## Why

`ft tasks create` has no way to create a task as a child of another task, even
though the core machinery (`ops::Position::Subtask`) is complete and the TUI
quickline already uses it. The CLI gap means the primary poweruser path (shell
scripts, ft.nvim) cannot create subtasks without hand-editing indentation.

## What Changes

- Add `--parent <SELECTOR>` to `ft tasks create`. `SELECTOR` uses the
  existing task-selector forms shared by `complete`/`cancel`/`edit`:
  task id (`abc123`), `<file>:<line>`, or fuzzy substring.
- The parent is resolved against the full vault scan. It must match
  **exactly one** task; zero or multiple matches are hard errors listing
  the candidates (no interactive picker — `create` is scriptable).
  The standard id→fuzzy fallback applies (an id-shaped selector with no
  id match falls through to substring matching), consistent with the other
  selector commands.
- The new task is written as an indented subtask at the end of the
  parent's block in the parent's file, reusing `ops::Position::Subtask`:
  indentation matches the parent's existing children (or two spaces deeper
  when the parent has none yet).
- `--parent` **conflicts** with `--file`, `--under-heading`, `--at-line`,
  and `--append` (clap `conflicts_with_all`). The parent alone decides the
  target file and position. Without `--parent`, behavior is unchanged.
- The file-wide duplicate check (`--force` to bypass) is unchanged and
  applies to subtask creation too, matching the TUI quickline behavior.
- Done tasks remain selectable as parents via `id`/`file:line` (fuzzy
  matching already excludes them), consistent with the selector rules.

## Capabilities

### New Capabilities
- `tasks-create-parent`: CLI creation of a subtask under a uniquely
  resolved parent task via `ft tasks create --parent <SELECTOR>`,
  including the selector resolution rules (unique-match-or-error,
  `file:line` support, id→fuzzy fallback) and the position/`--file`
  conflict contract.

### Modified Capabilities
- None. (Existing capability specs are unaffected; this is a purely
  additive CLI surface. The TUI quickline subtask flow and
  `ops::Position::Subtask` are unchanged and already covered.)

## Impact

- **Code:**
  - `ft/src/cmd/tasks.rs` — `CreateArgs` gains `--parent` (with
    `conflicts_with_all` against `file`/`under_heading`/`at_line`/`append`);
    `run_create` resolves the parent via `vault.scan()` +
    `selector::parse`/`selector::resolve` (with id→fuzzy fallback), errors
    on zero/multiple matches, and builds `Position::Subtask { parent_line }`
    with target `vault.path.join(parent.source_file)`. A small helper
    mirroring `pick_task`'s uniqueness logic keeps `run_create` readable.
  - No `ft-core` changes: `ops::Position::Subtask` (indent derivation,
    block-end placement) and `selector` are already in place and tested.
- **Docs:** `ft tasks create` help text via clap; no keybindings impact.
- **Tests:** integration tests under `ft/tests/tasks_create.rs` — parent
  via `file:line`, via id, ambiguous-selector error, no-match error,
  conflict-with-`--file` error, indent derivation (first child vs matching
  existing children), duplicate-check interaction with `--force`, done
  parent via `file:line`.
- **ft.nvim:** no protocol change required — the CLI is the contract and
  the new flag is generic; the plugin can pass it through as a plain
  argument.
- **No breaking changes.** Purely additive flag on an existing command.

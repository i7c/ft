## Why

whatnow (a task hub that syncs the vault two-way with Todoist, Jira and
GitHub) drives the vault through the `ft` CLI instead of parsing the Tasks
format itself. The CLI is the contract (AGENTS.md §Editor integrations), and
a sync tool needs a few things from it that it lacks today:

- `ft tasks edit` cannot set the start date, the status (reopen, in
  progress), the `🆔` or the `⛔` set, which are fields a sync writes.
- `ft tasks edit --tags` exits 0 and prints "Edited" but writes nothing: tags
  are an index rebuilt from the description, and the serializer emits the
  description verbatim (the trap `ops::retag_task` documents).
- Writes print human text only, so a script cannot read back what was stored.
- A script holds tasks from an earlier `list`; nothing lets it say "only if
  the task still looks like this", and a changed line fails with the same
  exit code as every other error.
- Giving ids to many tasks takes one process, and one full scan, per task.
- `list` can include paths (`--path`) but not exclude them, and the query DSL
  has no `not`.

## What Changes

- `ft tasks edit` gains `--title`, `--start`, `--status open|in-progress`,
  `--id` and `--depends-on`. `--tags` is fixed: it rewrites the tag words in
  the description.
- `ft tasks edit`, `complete` and `cancel` gain `--expect <JSON>` (the task
  as `list --format json` printed it; the write is refused if the line on
  disk differs) and `--format json` (the task as written, in the `list` row
  shape).
- A refused write because the line changed on disk exits with code 3.
- New `ft tasks assign-ids`: gives a fresh `🆔` to every matching task that
  has none, in one scan and one write per file.
- `ft tasks list` and `assign-ids` gain `--exclude-path <PREFIX>`
  (repeatable).

## Capabilities

### New Capabilities
- `tasks-edit-for-sync`: the CLI surface a two-way sync drives: the new edit
  fields, the guarded and machine-readable writes, bulk id assignment and
  path exclusion.

### Modified Capabilities
- None. All additions; the `--tags` fix makes the flag do what its help text
  always said.

## Impact

- **Code:** `ft/src/cmd/tasks.rs` (flags, output, exit code), a new
  `ops::assign_ids` planner in `ft-core/src/task/ops.rs` (plan/apply, through
  `write_atomic`). `update_task_line` is reused for every edit.
- **Tests:** `ft/tests/tasks_edit.rs`, `tasks_complete.rs`,
  `tasks_cancel.rs`, `tasks_list.rs`, and a new `tasks_assign_ids.rs`.
- **ft.nvim:** no change needed; nothing existing changes shape.
- **Not changed:** completing a recurring task still leaves its `🆔` on both
  the done line and the next instance (`tasks_complete.rs` asserts it). A
  consumer takes the open one.

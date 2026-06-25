## Why

The `graph-task-interaction` change landed task-interaction verbs on the
Graph tab (complete/cancel/date-nudge/priority/due-today, plus
open-source-note and display parity), but three pieces were deferred to
keep that change reviewable:

- **A. The full edit popup (`e`).** The shared `EditPopup` form was lifted
  to `tasks/edit_popup.rs` and a `focused_task_edit_state` extraction is
  in place, but no `TaskEdit` modal renders/commits it from the Graph tab.
  So a user on a Graph-tab Task row still can't edit the description,
  recurrence, or clear a date in place — they must switch to the Tasks tab
  or drop to the CLI.
- **B. Create-task leader chords (`tc` / `ts`).** `c`/`C`/`s` are taken
  on the Graph tab, so creating a task or subtask from the graph requires
  a two-key leader. None exists yet.
- **C. Note-scoped task view (`gt`).** "Show me note N's tasks" is a
  pre-built query rewrite, deduped by construction (the `HasTask`→top-level
  model fix already landed). No entry point exists on the Graph tab.

## What Changes

- **`TaskEdit` modal (`e`).** A new `ActiveModal::TaskEdit(TaskEditState)`
  wraps the shared `EditPopup` (edit mode only — moving a task stays a
  separate `m` op). Render reuses the Tasks-tab popup renderer (lifted to
  `edit_popup.rs`). On `Enter`/`Ctrl+S` it validates and posts
  `AppRequest::GraphTaskEdit { path, line, fields }`; the Graph tab services
  it via `ops::update_task_line`, then refreshes + restores the cursor.
  On non-Task rows `e` toasts.
- **`TaskLeader` chord (`t`).** A transient `ActiveModal::TaskLeader`
  (mirroring `PeriodicLeader`): `t` opens it; `c` posts
  `AppRequest::GraphTaskCreate { kind: TopLevel }`, `s` posts
  `GraphTaskCreate { kind: Subtask { parent_file, parent_line } }`; any
  other key / `Esc` cancels. The Graph tab services `GraphTaskCreate` by
  opening a quickline seeded with the focused note's path (top-level) or
  the focused task's `(file, line)` (subtask).
- **`gt` note-scoped view.** A new `graph.tasks-of-note` command: on a
  Note/Directory row, rewrite the active view's query to
  `node where kind = Note and path = "<p>"; expand where edge.kind in
  {has-task, subtask} and to.kind in {Task};` and re-materialize. On a
  Task row it scopes to that task's parent's note.
- **Shared popup helpers.** `render_edit_popup`, `parse_optional_date`,
  `parse_priority`, `parse_tags_field`, `merge_tags_into_description`,
  and `centered_rect` move from `tasks/search.rs` to
  `tasks/edit_popup.rs` (made `pub(crate)`), so the Graph-tab `TaskEdit`
  modal reuses the exact render + validation path.

## Capabilities

### Modified Capabilities

- `graph-task-interaction`: the `e` edit-popup, `tc`/`ts` create leaders,
  and `gt` note-scoped view requirements deferred from the original change
  are now satisfied.
- `tui-commands`: new `graph.task-edit-popup`, `graph.task-create`,
  `graph.task-new-subtask`, `graph.tasks-of-note` `CommandDef`s.
- `tui-keymaps`: `e`, `t`→leader, `g`→leader bindings on the Graph tab.
- `tui-modal-driver`: new `TaskEdit` and `TaskLeader` `ActiveModal`
  variants.

## Impact

- **ft/src/tui/modal.rs** — `ActiveModal::TaskEdit(TaskEditState)`,
  `ActiveModal::TaskLeader` variants + `Modal` impl dispatch arms.
- **ft/src/tui/tabs/graph.rs** — `TaskEditState`, `TaskLeader` state types
  (or simple `struct TaskLeader;`), `graph.task-edit-popup` /
  `graph.task-create` / `graph.task-new-subtask` / `graph.tasks-of-note`
  dispatch arms, `AppRequest::GraphTaskEdit`/`GraphTaskCreate` servicing
  via `Tab::graph_*` hooks, `gt` query-rewrite helper, help-section
  additions.
- **ft/src/tui/tabs/tasks/edit_popup.rs** — gains `render_edit_popup`,
  `parse_optional_date`, `parse_priority`, `parse_tags_field`,
  `merge_tags_into_description`, `centered_rect` (moved from `search.rs`).
- **ft/src/tui/tabs/tasks/search.rs** — imports the moved helpers.
- **ft/src/tui/tab.rs** — `AppRequest::GraphTaskEdit`,
  `AppRequest::GraphTaskCreate` variants + `Tab::graph_task_edit` /
  `Tab::graph_task_create` hooks.
- **ft/src/tui/app.rs** — service the new requests in `service_request`
  + `drain_simple_requests` + test-path variants.
- **ft/src/tui/modal_commands.rs** — `TASK_EDIT_COMMANDS`/`KEYMAP`,
  `TASK_LEADER_COMMANDS`/`KEYMAP`.
- **docs/keybindings.md** — regenerated.
- **Snapshot tests** — Graph-tab frames with `e`/`tc`/`ts`/`gt`.
- **No breaking changes**; the `t`/`g`/`e` keys are newly bound on the
  Graph tab (previously `t` was `graph.today` — **breaking**: `t` is
  repurposed to the task-leader entry, and `graph.today` moves to `T`…
  see design D3 for the collision resolution).

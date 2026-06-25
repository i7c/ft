## Why

Tasks live in the graph as `NodeKind::Task` nodes (reachable from notes via
`HasTask` edges and from each other via `Subtask` edges), but the two places
users look at them — the Graph tab and the Tasks tab — are inconsistent and
both have a duplication bug:

- **A. No task interaction parity.** On the Tasks tab you can complete, cancel,
  nudge due/scheduled dates, cycle priority, open the full edit popup, and
  create tasks/subtasks. On the Graph tab a Task row is inert: `graph.delete`
  toasts "cannot delete a task node", `graph.open-in-editor` silently does
  nothing (the path resolver only matches `NodeKind::Note`), and there is no
  way to mark a task done or see/edit its due date without leaving for the
  Tasks tab. The `ft-core::task::ops` primitives that power every Tasks-tab
  mutation are keyed by `(source_file, source_line)` — and `TaskData` on a
  graph Task node already carries exactly those fields — so the engine is
  shared; only the Graph-tab keymap/dispatch/popup is missing.
- **B. Examining tasks from a specific note shows duplicates.** Querying
  `path includes "N"` in the Tasks tab lists a subtask both as a top-level row
  and (if its parent is expanded) nested under the parent, because
  `rebuild_display` emits every matched index at depth 0 with no `seen`-set.
  The CLI's `--tree` mode (`hierarchy::expand_forest`) does not have this bug.
  In the Graph tab the same note's tasks appear multiple times because every
  task — including subtasks — gets a `HasTask` edge from its note, so a subtask
  is reachable both directly (`Note →[HasTask]→ Subtask`) and transitively
  (`Note →[HasTask]→ Parent →[Subtask]→ Subtask`), and the expand-as-you-go
  tree has no visited-set.

## What Changes

- **Model fix: `HasTask` edges reach top-level tasks only.**
  `Graph::insert_hastask_edges` SHALL skip tasks whose `parent` is `Some(_)`.
  Subtasks remain reachable from their note exclusively via the existing
  `Subtask` edge chain (`Note →[HasTask]→ top task →[Subtask]→ …`). This makes
  "the tasks of note N" a deduped tree by construction and advances the
  direction the Tasks tab is already heading: a special query over the same
  graph. No new edge kind, no DSL surface change (`subtask` is already an
  accepted `edge.kind`).
- **Graph-tab task interaction (parity).** New row-kind-gated `graph.task-*`
  commands on the Graph tab — complete, cancel, due/scheduled ±1 day, due-today,
  priority cycle, full edit popup, open-source-note-at-line, and note-scoped
  task view — that call the *same* `ft-core::task::ops` the Tasks tab uses.
  Task verbs are no-ops (or a brief toast) on non-Task rows, matching the
  existing pattern where `graph.create-subdir` toasts on non-directory rows.
  The `EditPopup` is lifted to a shared module so both tabs open the same form.
- **Graph-tab Task display parity.** `leaf_display` for `NodeKind::Task`
  extends beyond `[ ] description` to also show due/scheduled (relative, as
  the Tasks tab now does) and priority, compact on one line.
- **CLI parity.** `ft tasks complete` gains `--query`; new `ft tasks cancel`
  and `ft tasks edit` verbs wrap `ops::cancel_task` and `ops::update_task_line`
  so the CLI is never behind the TUI. `ft do` gains headless
  `tasks.cancel-by-id` / `tasks.edit-by-id` handlers.
- **Tasks-tab dedup fix.** `rebuild_display` ports `expand_forest`'s
  `seen`-set so a matched subtask appears only nested under its matched
  parent (or as a depth-0 root if its parent is not in the display set), never
  twice — bringing the TUI in line with `ft tasks list --tree`.
- **Spec drift fix.** The `graph-task-nodes` spec scenario "Path attribute on
  task node yields no match" is wrong: `unify-query-dsls` deliberately made
  `self.path` on a Task return its `source_file`. The spec is amended to match
  the code, since `path includes "N"` is the load-bearing workflow for B.

## Capabilities

### New Capabilities

- `graph-task-interaction`: Row-kind-gated task verbs on the Graph tab
  (complete/cancel/date-nudge/priority/edit-popup/open-source-note) and a
  note-scoped task view, all routing through `ft-core::task::ops`.
- `tui-task-list`: The Tasks tab's display list is deduplicated (a matched
  subtask appears once, nested under its matched parent), matching the CLI
  `--tree` forest.

### Modified Capabilities

- `graph-task-nodes`: `HasTask` edges reach top-level tasks only (amend); the
  `path` attribute on Task nodes returns `source_file` (fix the stale scenario).
- `tui-commands`: New `graph.task-*` `CommandDef`s in `GRAPH_COMMANDS`.
- `tui-keymaps`: New Graph-tab bindings (`x`,`X`,`]`,`[`,`}`,`{`,`=`,`-`,`T`,
  `e`, and leader chords `tc`/`ts`/`gt`) for task interaction.
- `cli-do`: Headless handlers for `tasks.cancel-by-id` and `tasks.edit-by-id`.

## Impact

- **ft-core/src/graph/mod.rs** — `insert_hastask_edges` gains a `parent`
  guard (needs the task's `parent` field; the `task_index` holds
  `(source_file, source_line) → NoteId`, so the scan's `&[Task]` is threaded
  in, mirroring `insert_subtask_edges`).
- **ft-core/src/graph/tests.rs** — the
  `build_with_tasks_creates_task_nodes_and_edges` fixture is flat (3
  top-level tasks, no nesting) so its `HasTask` count assertion is unaffected;
  add a new test asserting subtasks get *no* `HasTask` edge and are reachable
  only via `Subtask`.
- **ft-core/src/graph/query.rs** — no change (the `Attr::Path` Task arm is
  already canonical); a doc/comment note anchors it as intentional.
- **ft-core/src/task/resolve.rs (new, or in task/mod.rs)** —
  `by_query(graph, &GraphQuery) -> Vec<TaskKey>` shared by CLI bulk verbs and
  the Tasks-tab/TUI; extracts the pattern already in
  `ft/src/cmd/tasks.rs::run_move`.
- **ft-core/src/task/hierarchy.rs** — add (or expose) a `dedup_displayed`
  helper used by the Tasks-tab `rebuild_display`, reusing `expand_forest`'s
  `seen`-set semantics.
- **ft/src/cmd/tasks.rs** — `CompleteArgs` gains `--query`; new `CancelArgs`
  and `EditArgs` structs + `run_cancel`/`run_edit`; `TasksCommand` gains
  `Cancel` / `Edit` variants.
- **ft/src/cmd/do.rs** — `handle_tasks_cancel_by_id`,
  `handle_tasks_edit_by_id`.
- **ft/src/tui/tabs/graph.rs** — new `graph.task-*` `dispatch_command` arms
  (row-kind-gated), `leaf_display` Task arm extension, fix
  `graph.open-in-editor`/`selected_note_abs_path` to open the source note at
  the task's line, `graph.tasks-of-note` leader-chord command, new
  `CommandDef`s + keymap rows.
- **ft/src/tui/tabs/tasks/edit_popup.rs (new)** — `EditPopup` lifted out of
  `search.rs`; both tabs import it.
- **ft/src/tui/tabs/tasks/search.rs** — `rebuild_display`/`emit_display_row`
  rewritten to dedup via the shared `hierarchy::dedup_displayed`.
- **docs/keybindings.md** — regenerated via `ft commands docs`.
- **Snapshot tests** in `ft/src/tui/tests.rs` and `ft/src/tui/snapshots/` —
  Task rows change display text; graph frames gain task-interaction
  snapshots; Tasks-tab frames dedup.
- **Breaking changes**: One observable behaviour change — `HasTask` no longer
  edges to subtasks. Any user query relying on reaching a subtask directly via
  `has-task` from a note must instead follow `has-task` then `subtask`. The
  built-in `tasks-in-fs` preset is updated to include `subtask` in its
  `edge.kind` set so note→task→subtask traversal is preserved out of the box.

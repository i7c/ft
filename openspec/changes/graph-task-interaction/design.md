## Context

Tasks appear in two TUI tabs and the CLI. The `Task` model and the
`ft-core::task::ops` mutation primitives are shared, but the surrounding
machinery diverges:

- The **Tasks tab** (`ft/src/tui/tabs/tasks/search.rs`) holds a flat
  `tasks: Vec<Task>` from `vault.scan()`, builds a `children` index via
  `hierarchy::child_index_map`, and flattens matches into `display` via
  `rebuild_display`/`emit_display_row`. It has the full interaction keymap
  (`x`/`X`/`]`/`[`/`}`/`{`/`p`/`P`/`t`/`e`/`c`/`C`/`s`) calling
  `ops::complete_task` / `cancel_task` / `update_task_line` / `create_task`,
  each keyed by `(source_file, source_line)`.
- The **Graph tab** (`ft/src/tui/tabs/graph.rs`) renders `NodeKind::Task`
  rows but has no task verbs: `graph.delete` toasts "cannot delete a task
  node", `graph.open-in-editor` silently fails on Task rows
  (`selected_note_abs_path` only matches `NodeKind::Note`), and there is no
  complete/cancel/edit path. `leaf_display` shows only `[ ] description`.
- The **CLI** (`ft/src/cmd/tasks.rs`) has `list`/`create`/`complete`/`move`;
  `move` supports `--query` bulk, `complete` does not, and `cancel`/`edit`
  verbs do not exist.

Duplication today has two independent causes:

1. **Graph tab:** `Graph::insert_hastask_edges` edges *every* task —
   including subtasks — to its note. A subtask is thus reachable two ways
   (`Note →[HasTask]→ Subtask` and `Note →[HasTask]→ Parent →[Subtask]→
   Subtask`). `TreeState::expand_at` inserts children with no visited-set,
   so the subtask is materialized twice. (`collect_search_candidates`, the
   `f` picker, *does* dedup via a `visited: HashSet<NoteId>`.)
2. **Tasks tab:** `rebuild_display` calls `emit_display_row(idx, 0, …)` for
   each matched index. A subtask that also matches appears both as a depth-0
   row and (under an expanded parent) nested. The CLI's `--tree` mode
   (`hierarchy::expand_forest`) avoids this with a `seen: HashSet<usize>`,
   emitting matched children only nested under a matched parent.

The `graph-task-nodes` spec requires `HasTask` for *every* task matching a
note; this change amends that to top-level only. The same spec has a stale
scenario asserting `self.path` on a Task yields no value; `unify-query-dsls`
deliberately made it return `source_file` (verified: re-added in commit
`731c79e`, with test `dsl_path_on_task_matches_source_file`), so the code is
canonical and the spec is fixed here.

## Goals / Non-Goals

**Goals:**

- Graph tab reaches task-interaction parity with the Tasks tab (complete,
  cancel, date-nudge, priority, edit popup, open source note, note-scoped
  view), using the same `ft-core::task::ops` primitives.
- Graph-tab Task rows show the same information density as Tasks-tab rows
  (status, due, scheduled, priority) compactly.
- "Tasks of note N" is deduped in both tabs and the CLI by construction.
- CLI verbs match the TUI: `complete --query`, `cancel`, `edit`.
- Fix the stale `graph-task-nodes` `path`-on-task spec scenario.

**Non-Goals:**

- No new edge kind; `Subtask` already exists and is already a DSL `edge.kind`.
- No change to the Tasks tab's overdue/upcoming bucketed list layout — the
  Graph tab stays a structural tree; parity means "any visible task is
  actionable and inspectable", not "the graph tab renders the flat list".
- No change to the `Task`/`TaskData` struct shapes; no `Graph::build`
  signature change beyond threading `&[Task]` into `insert_hastask_edges`
  (already passed to `insert_subtask_edges`).
- No new task format; `EmojiFormat` remains the only impl.
- No multi-select task bulk operations on the Graph tab in v1 (the
  multi-select set is `Note`/`Directory` only today).

## Decisions

### D1: Fix the model — `HasTask` to top-level tasks only

`insert_hastask_edges` skips a task when its `parent` is `Some(_)`. Subtasks
are reachable from their note only via `Note →[HasTask]→ top task →[Subtask]→
…`. `insert_subtask_edges` is unchanged.

**Rationale:** This is the structural fix that makes "tasks of note N" a
deduped tree by construction — no `seen`-set needed in the graph tree for the
common case. It also advances the convergence of the Tasks tab toward "a
special query over the same graph". Verified safe: no test asserts a
note→subtask `HasTask` edge; `build_creates_subtask_edges_from_parent_pointers`
checks only `Subtask` edges; the flat `dirs_fixture_scan_with_tasks` (3
top-level tasks, no nesting) leaves `build_with_tasks_creates_task_nodes_and_edges`'s
`HasTask`-count assertions unchanged.

**Breaking observable:** A query reaching a subtask directly via `has-task`
from a note must now follow `has-task` then `subtask`. Mitigation: the
built-in `tasks-in-fs` preset is updated to include `subtask` in its
`edge.kind` set, preserving note→task→subtask traversal out of the box.

**Alternatives considered:**
- *Dedup in the view only (`seen`-set in `expand_at`).* Rejected: it papers
  over a model redundancy (two paths to every subtask) and leaves the DSL
  walk and the `f` picker with inconsistent reachability semantics. The model
  fix is one line and removes the ambiguity at the source.
- *A new `OwnsTask` edge to top-level tasks only, keep `HasTask` to all.*
  Rejected: two edge kinds for the same relationship is exactly the
  duplication we're removing.

### D2: Row-kind-gated dispatch on the Graph tab (consistency with graph nodes)

Every new `graph.task-*` command checks the focused row's `NodeKind` and:
- On `NodeKind::Task(t)`: resolves `ctx.vault.path.join(&t.source_file)` +
  `t.source_line`, calls the matching `ops::*`, then refreshes (rebuild
  graph + re-materialize the active view) and restores the cursor to the
  same `(source_file, source_line)` via the existing `select_display_row`-
  style anchor pattern (the Tasks tab's `refresh_after_mutation` already
  does exactly this).
- On any other row: a brief toast ("select a task first" or similar) or a
  silent no-op, and `CommandOutcome::Handled`.

**Rationale:** This mirrors the established graph-tab pattern
(`graph.create-subdir` toasts "select a directory first"; `graph.delete`
toasts per row kind). It keeps task verbs from surprising users on non-task
rows and avoids a separate per-row keymap mode.

### D3: Keymap — distinct keys for priority/due-today; leader chords for create

The Tasks tab binds `p`/`P` (priority) and `t` (due-today), but the Graph tab
already binds `p`→periodic-leader and `t`→today. The `KeyMap` resolves
modal→tab→global, not per-row, so context-sensitive reuse would need
dispatch-time checks. Decisions (keys verified free in `GRAPH_KEYMAP`):

| Action | Graph-tab key | Command |
|---|---|---|
| complete | `x` | `graph.task-complete` |
| cancel | `X` | `graph.task-cancel` |
| due +1 / −1 day | `]` / `[` | `graph.task-due-next` / `prev` |
| scheduled +1 / −1 day | `}` / `{` | `graph.task-scheduled-next` / `prev` |
| priority next / prev | `=` / `-` | `graph.task-priority-next` / `prev` |
| due-today | `T` | `graph.task-due-today` |
| edit popup | `e` | `graph.task-edit-popup` |
| open source note at line | `o` (existing `graph.open-in-editor`, fixed) | — |
| new top-level task (leader) | `t` then `c` | `graph.task-create` |
| new subtask (leader) | `t` then `s` | `graph.task-new-subtask` |
| note-scoped task view (leader) | `g` then `t` | `graph.tasks-of-note` |

`x`/`X`/`]`/`[`/`}`/`{`/`e` are free and reuse the Tasks-tab muscle memory.
`=`/`-` and `T` (shift) are free and avoid colliding with the periodic flow.
Create and note-scoped-view use two-key leader chords (`tc`/`ts`/`gt`)
because `c`/`C`/`s`/`t`/`g` are all taken. The leader is implemented as a
transient `ActiveModal`-style state (reusing the `PeriodicLeader` pattern:
first key opens a transient "task leader" awaiting the second; any other key
or `Esc` cancels), *not* a new long-lived mode.

### D4: Shared `EditPopup`

`EditPopup` (description/due/scheduled/priority/tags/recurrence + target
picker) is lifted from `ft/src/tui/tabs/tasks/search.rs` to
`ft/src/tui/tabs/tasks/edit_popup.rs`. The Graph tab opens it via a new
`ActiveModal::TaskEdit(TaskEditState)` variant wrapping the same popup state
(`from_task(&Task)` for edit mode). On commit, the modal posts an
`AppRequest::GraphTaskEdit { path, line, fields }` which the Graph tab
services by calling `ops::update_task_line` (or, for the target/move field,
`plan_move`+`apply_move_plan`).

**Rationale:** One form, two hosts — the modal-sharing pattern already used
for `Create`/`Append`/`SectionMove`. Keeps the snapshot tests of the popup
itself in one place.

### D5: Note-scoped task view rewrites the active view's query

`graph.tasks-of-note` (`gt`) on a `Note`/`Directory` row rewrites the active
view's query to:

```
node where kind = Note and path = "<path>";   // or kind = Directory and path starts_with "<dir>/"
expand where edge.kind in {has-task, subtask} and to.kind in {Task};
```

and re-materializes. Reuses the existing `GraphApplyQueryBar` /
root-rewrite (`z`) machinery — no new view-management code. On a Task row it
scopes to that task's siblings + descendants (query rooted at the task's
parent). With D1 this view is deduped by construction.

**Alternatives considered:**
- A dedicated "tasks of note" sub-panel. Rejected: it duplicates the tree
  widget and the query engine already expresses this exactly.
- A new built-in preset. Rejected as the primary entry: presets can't take a
  path argument; the leader chord resolves the path from the focused row. A
  `tasks-of-note` built-in is still added for CLI/scripting convenience as
  a *template* users adapt, but the TUI entry is the chord.

### D6: Display parity — extend `leaf_display` for Task

`leaf_display` for `NodeKind::Task` becomes:

```
[<status>] <description>  <relative due>  <relative scheduled>  <priority>
```

e.g. `[x] Fix login bug  📅 3d ago  ⏳ tomorrow  ⏩ High`, omitting any field
that is `None`. Reuses the Tasks tab's `relative_date` helper (lifted to a
shared spot under `ft/src/tui/tabs/tasks/` or a small `datefmt` util). Status
marker (`[ ]`/`[x]`/`[/]`/`[-]`) unchanged from `improve-graph-node-display`.
Keeps one line + kind char `T`, consistent with other graph leaves; the `e`
popup is the full-detail view.

### D7: Tasks-tab dedup ports `expand_forest`'s `seen`-set

`rebuild_display` builds a `display: HashSet<task_idx>` of "to be shown"
(matched ∪ descendants-of-matched, exactly like `expand_forest`), computes
roots as matched tasks whose parent is *not* in the display set, then emits
each root's subtree once with a `seen` guard. A matched subtask whose parent
is also matched appears only nested. Reuses
`hierarchy::dedup_displayed` (new thin helper, or inline mirroring
`expand_forest`).

### D8: CLI parity extracts a shared resolver

`task::resolve::by_query(graph, q) -> Vec<TaskKey>` extracts the
parse→`select`→map-`NoteId`-to-`(source_file, source_line)` logic currently
duplicated in `run_list` and `run_move`. `complete --query`, `cancel`, and
`edit` (single-selector and `--query`) all call it. `complete` keeps its
existing single-selector path; `--query` follows `run_move`'s bulk pattern
(intersect, filter `scan.tasks`).

### D9: `tasks-in-fs` preset updated

Because subtasks no longer have direct `HasTask` edges, the built-in
`tasks-in-fs` preset's `expand` block adds `subtask` to its `edge.kind` set
so walking from a note reaches the full task subtree. This is the only
built-in preset affected; `tree` and `fs` are unchanged (they never followed
`has-task`).

## Risks / Trade-offs

- **Breaking `HasTask`-to-subtask reachability.** Mitigated by the
  `tasks-in-fs` preset update (D9) and the fact that `Subtask` was already
  the documented parent→child edge. Any user query reaching subtasks via
  `has-task` directly is a misuse the model now prevents. Documented in the
  proposal's breaking-change note.
- **Snapshot churn.** Every Graph-tab frame with a Task row changes display
  text; Tasks-tab frames dedup; new task-interaction snapshots added.
  Review diffs carefully; rebless with `INSTA_UPDATE=always`.
- **`EditPopup` extraction touches Tasks-tab snapshots.** Mechanical but
  wide; the popup's own snapshot tests move with it.
- **Leader chords add a transient state.** Reusing the `PeriodicLeader`
  modal pattern keeps it within the modal driver; the `?` overlay lists the
  chords so they're discoverable.
- **`insert_hastask_edges` needs the task's `parent`.** The function today
  iterates `task_index` (which maps `(file,line) → NoteId` but not `parent`).
  Resolution: thread `tasks: &[Task]` in (same as `insert_subtask_edges`)
  and look up `parent` from the slice, or build a small `(file,line) →
  parent` map once. The latter is O(T) and avoids a second scan.
- **CLI `edit` field set.** `--due`/`--scheduled`/`--priority`/`--tags`/
  `--description` map to `ops::update_task_line` closures. Single-selector
  form only in v1; `--query` bulk for `edit` can follow. Flagged as a v1.1
  task.

## Context

`graph-task-interaction` landed the Graph-tab task verbs (complete/cancel/
date-nudge/priority/due-today), open-source-note, display parity, and the
shared `EditPopup` extraction. Three deferred pieces remain: the `e`
edit popup, create-task leaders, and the `gt` note-scoped view.

The Graph tab's keymap is now fairly full. The already-landed task verbs
took `x X ] [ } { = - T` (with `T` chosen for due-today specifically to
avoid `t`, which is `graph.today`). Still free as single keys: `e`, `a`,
`i`, `u`, `w`, `y`, `b`, `v` and their capitals (minus those already
bound). `t` (graph.today), `g` (graph.cursor-first), `c`/`C` (create note),
`s`, `r`, `m`, `d`, `n`, `o`, `p` are all taken.

The modal driver is the established pattern: `ActiveModal` enum + `Modal`
trait + `AppRequest` routing. `PeriodicLeader` is the reference for a
two-key leader; `CreateSubdir` is the reference for a single-line prompt;
the Tasks-tab `submit_popup` is the reference for `EditPopup` validation +
commit.

## Goals / Non-Goals

**Goals:**
- `e` opens the shared `EditPopup` on a focused Graph-tab Task row; commit
  applies fields via `ops::update_task_line` and refreshes.
- `a` leader: `ac` creates a top-level task, `as` a subtask under the
  focused task, via a quickline seeded with the right target/parent.
- `v` on a Note/Directory row rewrites the active view to that note's
  task subtree (deduped by construction via the `HasTask`→top-level model).
- Reuse the Tasks-tab popup render + validation verbatim (no duplication).

**Non-Goals:**
- No new task format; `EmojiFormat` only.
- No `--query` bulk for `ft tasks edit` (single-selector CLI already
  exists from `graph-task-interaction` §4).
- No moving a task from the edit popup (move stays a separate `m` op).
- No multi-select task bulk ops.

## Decisions

### D1: Keys — `e`, `a` leader, `v`

| Action | Key | Command |
|---|---|---|
| edit popup | `e` | `graph.task-edit-popup` |
| create top-level task | `a` then `c` | `graph.task-create` |
| create subtask | `a` then `s` | `graph.task-new-subtask` |
| note-scoped task view | `v` | `graph.tasks-of-note` |

`e` and `v` are free single keys. `a` (free) is the leader entry ("add");
its second keys `c`/`s` are consumed by the leader modal before the tab
keymap sees them (same as `PeriodicLeader` reusing `d`/`w`/`m`). This
avoids the `t`/`g` collisions the original spec draft assumed.

**Rejected:** `tc`/`ts`/`gt` from the original proposal — `t` is
`graph.today` and `g` is `graph.cursor-first`; repurposing them would be
a breaking change to existing graph navigation. `a`/`v` are free and
mnemonic ("add", "view note's tasks").

### D2: `TaskEdit` modal reuses the shared popup render + validation

`render_edit_popup`, `parse_optional_date`, `parse_priority`,
`parse_tags_field`, `merge_tags_into_description`, and `centered_rect`
move from `tasks/search.rs` to `tasks/edit_popup.rs` (made `pub(crate)`).
`TaskEditState { popup: EditPopup, path: PathBuf, line: usize }` wraps
the shared `EditPopup` in edit mode. On `Enter`/`Ctrl+S` it validates
(reusing the lifted helpers) and posts
`AppRequest::GraphTaskEdit { path, line, fields }`; the Graph tab services
it via `ops::update_task_line` + `refresh` + `restore_task_cursor`.

### D3: `TaskLeader` mirrors `PeriodicLeader`

`struct TaskLeader;` — `a` opens it; `handle_event` maps `c`→`GraphTaskCreate
{ kind: TopLevel }`, `s`→`GraphTaskCreate { kind: Subtask { parent_file,
parent_line } }`; any other key (incl. `Esc`) closes without action. The
Graph tab services `GraphTaskCreate` by opening a quickline seeded with
the focused note's path (top-level) or the focused task's `(file, line)`
(subtask). If no task is focused for `as`, it toasts.

### D4: `v` rewrites the active view's query

On a `Note` row: query becomes
`node where kind = Note and path = "<p>"; expand where edge.kind in
{has-task, subtask} and to.kind in {Task};`. On a `Directory` row: scopes
to `path starts_with "<dir>/"`. On a `Task` row: scopes to its source
note. Reuses `apply_query` (the `z` root-rewrite already swaps queries).
Deduped by construction (model fix from `graph-task-interaction`).

## Risks / Trade-offs

- **Keymap churn**: adds `e`, `a`, `v` bindings; none collide. `?` overlay
  + `docs/keybindings.md` updated.
- **Popup helper move**: touches Tasks-tab imports; snapshot tests for the
  popup itself stay in one place (now `edit_popup.rs` or unchanged in
  `search.rs` via re-import).
- **`t`/`g` not reused**: users who read the original proposal's `tc`/
  `ts`/`gt` get `a`/`v` instead — documented in the help overlay.

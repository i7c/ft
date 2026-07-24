## Context

`ft tasks move` relocates a task (and its subtask block) to another file,
optionally under a heading. It is CLI-only today (`ft/src/cmd/tasks.rs::run_move`).
The TUI's Tasks tab can complete/cancel/re-date/re-priority/retag/edit a task
in place but cannot move it to a different file.

Every primitive the TUI needs already exists:

- **Core planner:** `ops::plan_move(&[MoveSource], &MoveTarget, &dyn TaskFormat)`
  → `MovePlan`, applied by `ops::apply_move_plan(&MovePlan)`
  (`ft-core/src/task/ops.rs`). `MoveSource { path, line, expected: Option<Task> }`
  carries the `LineChanged` guard; `MoveTarget::{Append(PathBuf),
  UnderHeading(PathBuf, String)}` is the file/heading target. `MoveError`
  surfaces `LineChanged` / `Read` / `Write` / `NotATask` / `LineMissing`.
- **Fuzzy file+heading picker:** `VaultFilePickerSource`
  (`ft/src/tui/widgets/picker.rs:442`) wrapped in `FuzzyPicker`. A `Hit`
  carries `path: PathBuf` + `heading: Option<Heading>`. The query parser
  (`search::Query::parse`) splits `file#heading`, flipping
  `include_headings` on when the user types `#`.
- **In-tab precedent:** `open_target_picker` / `handle_target_picker_key`
  (`ft/src/tui/tabs/tasks/edit_popup.rs:467/490`) already wire this picker
  into the Tasks tab for `New`-mode task creation, composing
  `Hit` → `"{path}#{heading}"`.
- **Modal driver:** one `ActiveModal` enum + `RefCell<Option<ActiveModal>>`
  slot on `App`; dispatch precedence is modal → tab → global
  (`ft/src/tui/modal.rs`, `docs/architecture.md` §"Modal driver").
- **Refresh pattern:** after a mutation the tab raises
  `ctx.request_graph_refresh()`; a background worker rebuilds the shared
  `Arc<GraphSnapshot>` and tabs re-derive on generation change.

The Tasks tab has a single cursor (`selected: usize`) and no multi-select, so
this design covers single-task move only. Bulk move (`--query`) is deferred.

## Goals / Non-Goals

**Goals:**
- From the Tasks tab, move the cursor task to a different file (optionally
  under a heading) via the existing fuzzy file+heading picker, with a
  `LineChanged` guard, atomic writes, graph refresh, and success/error
  toasts.
- Reuse `VaultFilePickerSource` + `FuzzyPicker` + `ops::plan_move` /
  `ops::apply_move_plan` unchanged.
- Follow the modal-driver pattern (new `ActiveModal` variant) and the
  command/keymap registry (`tasks.move` `CommandDef` + `M` binding).
- Keep all five build invariants clean; regenerate `docs/keybindings.md`.

**Non-Goals:**
- Bulk move via query DSL (requires Tasks-tab multi-select first — separate
  change).
- Wikilink rewriting on cross-folder moves (deferred to plan 003 per the
  existing `TODO` in `plan_move`; the TUI inherits the CLI's current
  behavior of moving bytes verbatim).
- Moving a task to the same file it already lives in (rejected by a guard;
  see Decisions).
- A dry-run / diff preview (the CLI has `--dry-run`; the TUI commits
  directly, consistent with every other Tasks-tab mutation).

## Decisions

### D1. New `ActiveModal::TaskMove` variant, not a per-tab `Option<...>`

The move is a multi-step flow (open picker → pick → commit) that must
capture keyboard ahead of the tab. Per AGENTS.md §"Modal driver", the
pattern is a new `ActiveModal` variant wrapping a small state struct,
**not** a field on `SearchView`. The state struct holds the
`FuzzyPicker<VaultFilePickerSource>` and the source task identity
(`(path, line, Task)` snapshot taken at open time).

**Alternative considered:** reuse the `EditPopup.target_picker` field
and drive the move from the edit popup. Rejected — the edit popup is
explicitly edit-in-place (`PopupMode::Edit`), and `edit_popup.rs:51`
states "edits don't move the task to a different file." Mixing move into
the popup would muddy that seam and force a `PopupMode::Move` variant
plus target-field reuse that the popup's validation wasn't designed for.

### D2. Build `MoveTarget` directly from the `Hit`, not via an `EditBuffer`

`open_target_picker` / `handle_target_picker_key` round-trip the `Hit`
through a `"{path}#{heading}"` string into an `EditBuffer` because the
new-task flow needs a text field the user can keep editing. The move flow
has no editable target field — picking is the commit trigger — so it
builds a `MoveTarget` directly from `hit.path` + `hit.heading`:

```text
MoveTarget::UnderHeading(abs, h.text)   // when hit.heading.is_some()
MoveTarget::Append(abs)                 // otherwise
```

where `abs = ctx.vault.path.join(&hit.path)`. This avoids the
`search::Query::parse` round-trip the new-task commit path uses
(`tasks/search.rs:1331`). The picker helpers in `edit_popup.rs` are the
reference but are **not** reused verbatim; the move modal owns its own
thin `Hit` → `MoveTarget` mapping.

**Alternative considered:** factor a shared `hit_to_target_string` helper
and reuse it. Rejected — the two consumers want different output types
(`EditBuffer` text vs `MoveTarget`), and a stringly-typed intermediate
adds a parse step for no gain.

### D3. Same-file move is rejected with a toast, flow stays open

If `abs == source_path` (the task's current file), the flow toasts an
error ("can't move to the same file — pick a different target") and keeps
the picker open so the user can pick again. No plan is built, no write
happens.

**Rationale:** the CLI permits same-file moves (they reposition under a
heading or no-op for append), but `section_move` rejects them
(`modals.rs:1581` "same-file move is out of scope"). For an interactive
flow, rejecting is clearer than a silent no-op or a confusing in-file
reposition. The user explicitly asked for this stance.

**Alternative considered:** allow same-file `UnderHeading` moves to
match the CLI. Rejected per user direction (simpler MVP, clearer UX).

### D4. Commit path: `plan_move` + `apply_move_plan` + `request_graph_refresh`

On `PickerOutcome::Selected(hit)` (after the same-file guard):

1. Build `MoveSource { path: source_path, line: source_line, expected:
   Some(task_snapshot) }` from the cursor task captured at open time.
2. `let plan = ops::plan_move(&[source], &target, ctx.vault.task_format())?`
3. `ops::apply_move_plan(&plan)?`
4. `ctx.request_graph_refresh()`
5. Toast success ("moved to {path}#{heading}") or error.

`MoveError::LineChanged` is surfaced as a toast and closes the modal
(the task changed on disk; the user should rescan). Other `MoveError`
variants (`Read`/`Write`/`NotATask`/`LineMissing`) are surfaced as
error toasts and close the modal.

This mirrors the Tasks-tab mutation idiom (`with_selected_task` + `ops::*`
+ `ctx.request_graph_refresh()`, e.g. `complete_selected` at
`tasks/search.rs:1143`) and the `TaskEdit` modal's commit-via-`AppRequest`
pattern — except move is simple enough to call `ops` directly from the
modal's `handle_event` rather than posting an `AppRequest`. The modal
holds everything it needs (the `Task` snapshot, the `TabCtx`); no
host-side `GraphRequest` arm is required.

**Alternative considered:** post an `AppRequest::Tasks(TasksRequest::Move
{ ... })` and service it on the tab, mirroring `GraphRequest::TaskEdit`.
Rejected as over-indirection for a single-tab, self-contained mutation —
the modal already has the `Task` and the `TabCtx`, and the Tasks tab's
other mutations (`complete`/`cancel`/`retag`) call `ops::*` directly
rather than round-tripping through a request enum. If a later bulk-move
change needs host coordination, the request-enum seam can be added then.

### D5. Source `Task` captured at open time, not re-read at commit

The modal captures `(source_path, source_line, Task)` from the cursor at
open time and passes `expected: Some(task)` to `plan_move`. If the line
shifted on disk between open and commit, `plan_move` fails with
`LineChanged` and the modal toasts + closes. This is the same guard the
CLI uses (`run_move` builds `MoveSource` from the scanned task).

### D6. Command + keymap: `tasks.move`, bound to `M`

New `CommandDef { name: "tasks.move", ... }` in the Tasks-tab
`COMMANDS` slice, bound `M` in the Tasks-tab `KEYMAP`. `M` (shift-m) is
free in the Tasks-tab keymap (bound keys: `k j R l h p P x X t T e c C s`;
lowercase `m` is also free). `M` matches the mnemonic and the Graph tab's
`m` move chord convention without colliding. On a non-task row, `M`
toasts "select a task first" (mirroring `tasks.search.rs:1082`).

## Risks / Trade-offs

- **[Stale `Task` between open and commit]** The user opens the picker,
  the file is edited externally, then the user commits. → Mitigation:
  `expected: Some(task)` makes `plan_move` fail with `LineChanged`; the
  modal toasts and closes, and the graph refresh picks up the new state.
  Same guard the CLI relies on.
- **[Cross-folder wikilink breakage]** `plan_move` moves bytes verbatim
  (the `TODO(plan-003)` in `ops.rs`); wikilinks pointing at the moved
  task's old location may dangle until Obsidian re-indexes. → Mitigation:
  inherited from the CLI; documented in the proposal's Non-Goals. Not
  introduced by this change.
- **[Modal vs. tab dispatch ordering]** The picker must capture keys
  ahead of the tab. → Mitigation: standard `ActiveModal` dispatch
  (modal first, tab second) handles this; the `FuzzyPicker` already works
  inside modals (used by `SectionMove`, `Append`, `Create`).
- **[Keymap drift]** Forgetting the `with_keymap_overlay` line or the
  `commands docs` regen silently breaks `?` help / `ft commands
  check-keymap` / `docs/keybindings.md`. → Mitigation: tasks.md includes
  both as explicit checklist items; `commands docs --check` is a build
  invariant.

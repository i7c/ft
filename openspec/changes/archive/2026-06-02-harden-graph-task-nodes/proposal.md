## Why

The `graph-task-nodes` change landed (commit 5372d3e) with green builds, but a review surfaced several gaps: two spec scenarios were tested as text/syntax rather than as behavior, two scenarios were not tested at all, the implementation locks the DSL contract to Rust's `Debug` format, and a handful of unspec'd extensions and dead code paths slipped in. Fixing these now — before the patterns get copied or other features build on top — keeps the capability honest and the DSL contract stable.

## What Changes

- **Strengthen behavioral tests** for two existing scenarios:
  - "Expand revealing tasks": replace the `edge.kind in {…}` workaround with the spec's `expand where … to.kind in {Note, Directory, Task}` form and assert tasks appear.
  - "tasks-in-tree preset": build a graph with tasks, apply each preset, and assert the resulting walk includes/excludes task nodes — instead of inspecting DSL strings.
- **Add missing scenario tests**:
  - Task whose `source_file` matches no note: task node exists, no `HasTask` edge.
  - Filter operators not yet covered: `!=` and `ends_with` on string task attributes, and `in` (set) on `status`/`priority`/`due`/`scheduled`/`description`.
- **Stabilise the DSL string contract for status and priority** — **BREAKING (internal)**: replace `format!("{:?}", task.status)` and `format!("{:?}", priority)` in `Graph::insert_task_node` with explicit `as_str()` (or `Display`) methods on `Status` and `Priority`. The spelling stays the same (`"Open"`, `"Done"`, `"InProgress"`, `"Cancelled"`, `"High"`, …), but it is no longer coupled to `#[derive(Debug)]`.
- **Remove unspec'd extensions and dead code**:
  - Drop the `Attr::Path` arm on `NodeKind::Task` (`query.rs:1354`) — task nodes are not addressable by `path` in the spec.
  - Drop the `NodeKind::Task` mapping in `output/links.rs` (`LinkRowTarget::Unresolved { raw: t.description }`) — `run_links` is guarded by `unreachable!`, so this branch is unreachable in practice.
  - Drop the `Attr::Tags` arm for `NodeKind::Task` in `node_string_attr` — `eval_cond_on_node` handles tags via a dedicated branch and never falls through to the string path, so the comma-joined value is unreachable.
- **Fix performance footguns**:
  - Rewrite `Graph::insert_hastask_edges` to use the existing `path_index` for O(N) lookup instead of the current O(N×M) nested loop over notes × tasks.
  - Stop calling `Vault::scan()` in `cmd/notes.rs::run_links` and `cmd/notes.rs::run_rename`. Those commands do not need task data; they should pass `&Scan::default()` (or an explicit "no tasks" sentinel) to `Graph::build`. The TUI graph tab keeps the full scan since it renders tasks.
- **Mark `tasks.md` task 2.6 done** in the `graph-task-nodes` change to reflect that all `Graph::build` callers were updated.

## Capabilities

### New Capabilities
- _None._

### Modified Capabilities
- `graph-task-nodes`: tighten existing requirements (behavioural assertions for expand/preset scenarios, additional operator coverage, missing-note edge scenario, unknown-attribute scenario), remove the unspec'd `path` attribute on task nodes, and declare the `status`/`priority` string spellings as a stable contract rather than an incidental `Debug` artefact.

## Impact

- **ft-core/src/task/mod.rs** — add `Status::as_str` and `Priority::as_str` (or `Display`) returning stable strings.
- **ft-core/src/graph/mod.rs** — `insert_task_node` uses `as_str()` for status/priority; `insert_hastask_edges` rewritten against `path_index`.
- **ft-core/src/graph/query.rs** — remove `Attr::Path` arm and `Attr::Tags` arm on `NodeKind::Task` in `node_string_attr`.
- **ft/src/cmd/notes.rs** — `run_links` and `run_rename` pass `&Scan::default()` to `Graph::build`; their `NodeKind::Task` arms remain `unreachable!`.
- **ft/src/output/links.rs** — remove `NodeKind::Task` arms from both `LinkRow` constructors.
- **ft-core/src/graph/tests.rs**, **ft-core/src/graph/query.rs (tests)**, **ft-core/src/graph/preset.rs (tests)**, **ft/src/tui/tabs/graph.rs (tests)** — new and rewritten tests covering the scenarios above.
- **openspec/changes/graph-task-nodes/tasks.md** — mark 2.6 complete.
- No external API or CLI behavior changes. The DSL contract for `status` / `priority` strings is unchanged in spelling but is now backed by explicit methods.

## Context

`graph-task-nodes` (commit 5372d3e) landed the data model, build wiring, DSL surface, TUI rendering, and a built-in preset for task nodes. A post-merge review against the original `specs/graph-task-nodes/spec.md` flagged:

- Two behavioural scenarios were "tested" by asserting DSL string contents or by substituting `edge.kind` filtering for the `to.kind` filtering the spec specifies.
- Two scenarios — "Task with no matching note" and "Unknown attribute on task node" — have no test at all.
- The string contract for `status` and `priority` in the DSL is implicitly defined by `format!("{:?}", …)`. The spelling is correct today but coupled to `#[derive(Debug)]`; a future contributor renaming a variant or moving Status to a tuple variant would break DSL queries silently.
- Three pieces of dead or unspec'd code: `Attr::Path` on task nodes, `LinkRowTarget::Unresolved` Task mapping, and a comma-joined `Attr::Tags` arm in `node_string_attr` that `eval_cond_on_node` never reaches.
- `Graph::insert_hastask_edges` does O(N×M) nested iteration over notes and tasks despite `path_index` already existing.
- `cmd/notes.rs::run_links` and `run_rename` now call `Vault::scan()` even though the rename and link-listing logic never touch tasks.

Nothing here is a new feature — every item is either a test that should have been written, dead code to delete, or an implementation tightening behind an existing spec requirement.

## Goals / Non-Goals

**Goals:**
- Bring the test suite into line with the spec scenarios listed in `specs/graph-task-nodes/spec.md` — behaviourally, not textually.
- Make the `status` / `priority` DSL string contract explicit and stable, decoupled from `Debug`.
- Remove the three unspec'd / unreachable code paths flagged in review.
- Restore the pre-`graph-task-nodes` CLI cost for `notes links` and `notes rename` (no task scan).
- Replace the quadratic `insert_hastask_edges` with a `path_index` lookup.

**Non-Goals:**
- No changes to public CLI flags, output formats, or graph DSL surface — the spelling of `status` / `priority` strings stays exactly the same.
- No changes to the TUI graph tab's behaviour or rendering. It continues to do a full `Vault::scan()` because it renders tasks.
- No new attributes, edge kinds, or presets.
- No changes to `Vault::scan()` itself or to the `Task` model.

## Decisions

### D1: `Status::as_str` and `Priority::as_str` for the DSL contract

**Options considered:**
- A) Add inherent `as_str(self) -> &'static str` methods returning the same spellings that `{:?}` currently produces.
- B) Add `Display` impls returning the same spellings.
- C) Keep `format!("{:?}", …)` and add a comment.

**Decision: A** — explicit `as_str()` methods on both enums, returning `&'static str` (no allocation). The graph builder calls `task.status.as_str().to_string()` and `task.priority.map(|p| p.as_str().to_string())`. `Display` (option B) is fine in principle but would invite use in user-facing output where we already have emoji rendering for `Priority`; keeping the DSL-facing method named `as_str()` separates the concerns. Option C leaves a footgun in place.

Test: a one-liner unit test asserts the exhaustive mapping for both enums.

### D2: Strengthen tests instead of changing the spec

The two behavioural test gaps (`expand-by-to.kind`, `preset-shows-tasks`) are tests that should have been written the first time. They go in the same modules as the existing weak tests, and the weak tests are deleted (not kept alongside) — there is no value in keeping a test that asserts the wrong thing.

The two missing-scenario tests (`no-matching-note`, `unknown-attribute-on-task`) are pure additions. For the "unknown attribute" scenario, we use a name that *is* a valid DSL attribute on other node kinds but maps to `None` on `Task` — the simplest example is `title`. This matches the spirit of the spec scenario (the literal `nonexistent` it lists would fail at parse time, which is a flaw in the original scenario wording we silently correct here).

### D3: Remove `Attr::Path` on task nodes; do not extend the spec

`Attr::Path` for `NodeKind::Task` currently returns `t.source_file.to_string_lossy()`. This is unspec'd and there is no caller relying on it. We remove the arm. If a future use case wants "tasks in this directory", that can be a separate spec change exposing a documented attribute (e.g., `source_file`). Note that the parser already accepts `path` as an attribute name globally; on task nodes it will simply evaluate to `None`, which falls back to "no match" — same behaviour as any unknown attribute on any node kind.

A regression test asserts `node where kind = Task and path = "root.md"` returns zero rows in a vault where tasks live in `root.md`.

### D4: `run_links` and `run_rename` use `Scan::default()`

Both commands currently call `Graph::build(&vault, &vault.scan())`. The rename planner walks incoming link edges and the links command walks outgoing link/embed edges; neither touches task nodes. `unreachable!` arms for `NodeKind::Task` in both commands are kept (they are defensive, not a contract). We pass `&Scan::default()` to `Graph::build` from both call sites and drop the unused scan work.

The TUI graph tab (`on_focus`, `refresh`, the `r` keybind) keeps the full scan because it renders tasks.

### D5: `insert_hastask_edges` uses `path_index`

The new implementation iterates `task_index` (or equivalently, walks task nodes once), looks each task's `source_file` up in `path_index`, and adds an edge when found. This is O(T) instead of O(N×T), and uses already-maintained state. Behaviour is identical: tasks without a matching note still get a node but no edge.

### D6: Drop dead `LinkRowTarget` and `node_string_attr` arms

The `NodeKind::Task` arms in both `LinkRow` constructors in `output/links.rs` are added by 5372d3e but never reached — `run_links` rejects task `id`s with `unreachable!` before either constructor is called. Removing the arms compiles via the `_ => …` catch-all is **not** what we want; we want the arms removed and the match to remain exhaustive — i.e., add `NodeKind::Task(_) => unreachable!(...)` if needed, or remove the arms entirely if the matches are non-exhaustive with a wildcard. The existing pattern in `output/links.rs` uses explicit arms with no wildcard, so we add `NodeKind::Task(_) => unreachable!("task nodes are not link targets")` to stay exhaustive.

The `Attr::Tags` arm in `node_string_attr` for `NodeKind::Task` returns `Some(t.tags.join(","))`. `eval_cond_on_node` handles `Attr::Tags` for `NodeKind::Task` directly and only forwards string attributes (`Status`, `Priority`, `Due`, `Scheduled`, `Description`) to `node_string_attr`. So this arm is unreachable from the evaluator. Other callers of `node_string_attr` (e.g., projection / display) do not request `Attr::Tags`. We delete the arm; if a future caller requests it the function returns `None`, which is consistent with "no string projection for tags".

## Risks / Trade-offs

- **Risk:** Removing `Attr::Path` on tasks is technically a behaviour change for any user who happened to discover and rely on it. → **Mitigation:** the original change just merged; no user could have come to depend on this in the ~hours-old window. The behaviour is also undocumented.
- **Risk:** `as_str()` introduces a slim API for two enums that may invite use elsewhere. → **Mitigation:** documented purpose ("stable DSL spelling"). Not a `Display` impl, so it won't accidentally land in user-facing output.
- **Risk:** The "unknown attribute on task" scenario in the spec is itself slightly broken (uses a literal `nonexistent` attribute that the parser would reject). We test the *spirit* (a valid attribute name that yields no value on tasks). → **Mitigation:** add a note in the test explaining the chosen example and why; if the spec wording is updated later, the test still demonstrates the intent.
- **Trade-off:** `insert_hastask_edges` could also live on `Graph::insert_task_node` (insert the edge as part of inserting the task). We keep the two-phase build (all task nodes first, then all edges) because it matches the existing pattern for note → ghost edge resolution and keeps `insert_task_node` callable from future code paths that might not have notes yet.

## Migration Plan

Not applicable — internal API only. The DSL contract is preserved (same strings), all CLI/TUI behaviour is preserved except for the removed scan in `notes links` / `notes rename` (which is a net positive). No data migration, no config changes.

## Open Questions

- None. Every item is either a clear test, a clear deletion, or a clear refactor with the same observable behaviour.

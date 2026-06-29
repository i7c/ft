## 1. ft-core types

- [x] 1.1 Replace `CyclePolicy` with `VisitPolicy { Dedup, Tree, Allow }` in `ft-core/src/graph/query.rs`; `Default` = `Dedup`. Update doc comments.
- [x] 1.2 Change `WalkOptions`: rename `cycle_policy` → `visit: VisitPolicy`; add `max_nodes: Option<usize>` (default `None`). Keep `Default` + `unlimited()` helper.
- [x] 1.3 Add `enum NodeClosure { Open, Reference, Cycle }` (`Default = Open`); replace `WalkNode.cycle: bool` with `closure: NodeClosure`. (`WalkNode` never derived `Default` — `NoteId` has none — and is only constructed internally by `walk`, so none was added.)

## 2. Walk algorithm

- [x] 2.1 Thread a `&mut HashSet<NoteId> visited` through `walk` / `walk_node`.
- [x] 2.2 Implement `Dedup`: on first reach insert into `visited` and expand; on re-encounter emit a `Reference` leaf with empty children and do not recurse.
- [x] 2.3 Keep `Tree` behavior as the old path-based cycle stop (ancestor stack), emitting `Cycle` on ancestor re-entry; `Allow` performs no detection.
- [x] 2.4 Keep `max_depth` composition: stop descending at the bound under every policy; dedup still applies inside the bounded region.
- [x] 2.5 Enforce `max_nodes` backstop: abort the walk with a recoverable error/signal when the materialized node count exceeds the budget.

## 3. CLI surface (`ft/src/cmd/graph.rs`)

- [x] 3.1 Rename `--cycle-policy`/`CycleArg` to `--visit-policy` with values `dedup` (default), `tree`, `allow`; map to `VisitPolicy`.
- [x] 3.2 Generalize the depth guard: `tree` and `allow` both require `--depth`; `dedup` does not. Update the error message to point at the safe default.
- [x] 3.3 Update the renderer(s) to key off `NodeClosure` (distinct glyph/label for `Reference` vs `Cycle` vs `Open`).

## 4. Tests

- [x] 4.1 Update existing walk unit tests for the renamed enum/field and `..Default::default()` construction.
- [x] 4.2 Add a dense / diamond re-convergence test asserting a node is expanded once and appears as `Reference` under the other parent.
- [x] 4.3 Add an unbounded-walk termination test over a dense fixture (would hang under the old behavior) bounding visited-node count.
- [x] 4.4 Add a `Tree`-mode test asserting shared subtrees are repeated, and an `Allow`-without-depth CLI rejection test.
- [x] 4.5 Refresh `insta` snapshots for `ft graph query` output affected by the new default and marker. (None needed: the only graph-query snapshot, `graph_query_dirs_tree`, is a pure directory tree with no shared descendants, so its output is identical under dedup.)

## 5. Verify invariants

- [x] 5.1 Run all five build invariants (`cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `commands docs --check`).
- [x] 5.2 Manually confirm `ft graph query 'node where path = ""; expand where edge.kind in {directory-contains, note-link};'` on a dense vault returns without `--depth`.

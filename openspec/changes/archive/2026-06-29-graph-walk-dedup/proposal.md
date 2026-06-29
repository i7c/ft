## Why

`ft graph query '<select>; expand where …;'` hangs on real vaults when the
expand set includes dense edge kinds (e.g. `note-link`). The traversal is a
DFS that materializes a tree, and its only termination guard is *path-based*
cycle detection (a node may not appear in its own ancestor chain). That guard
does nothing about **re-convergence** — a node reached again via a different,
non-cyclic path (shared hubs, diamonds) is fully re-expanded every time. The
number of simple paths through a dense graph is exponential, so the walk emits
an astronomically large tree and appears to hang. Passing `--depth 10` only
masks it by capping path length. This makes the marquee graph query unusable on
the very vaults it's meant for.

## What Changes

- Add **node-level dedup** to `GraphQuery::walk`: a global "already-expanded"
  set, so each reachable node's subtree is materialized at most once. This makes
  any unbounded walk `O(V + E)` and guarantees termination regardless of graph
  density.
- On a re-encountered node, emit it as a **reference leaf** — the node is shown
  (so the edge is visible) but not descended into, marked distinctly from a
  true cycle.
- Make dedup the **default** walk behavior. The existing path-tree behavior
  (same subtree repeated under every parent, `tree(1)`-style) becomes an
  explicit opt-in.
- **BREAKING (output):** the default `ft graph query … expand` output changes
  for graphs with shared descendants — a node reachable by multiple paths now
  appears once with its full subtree and as a marker elsewhere, instead of
  being duplicated. The dedup default also subsumes the old cycle case.
- Update the CLI surface (`--cycle-policy` / a visit-policy flag on
  `ft graph query`) and its `--depth` guard to match the new default.

## Capabilities

### New Capabilities
- `graph-walk-traversal`: Defines the termination and shape guarantees of
  `GraphQuery::walk` — node-level dedup as the default, the reference-leaf
  marker on re-encountered nodes, true-cycle handling, the depth bound, and the
  opt-in path-tree mode. Captures the requirement that an unbounded walk over
  any finite graph terminates in `O(V + E)`.

### Modified Capabilities
<!-- No existing capability spec owns walk traversal semantics; the behavior is
     introduced as a new capability above. -->

## Impact

- **Code:** `ft-core/src/graph/query.rs` — `GraphQuery::walk` / `walk_node`
  (thread a `visited` set), `CyclePolicy` / `WalkOptions`, `WalkNode` (new
  marker field). `ft/src/cmd/graph.rs` — policy flag, `--depth` guard, render
  of the new marker.
- **Behavior:** default output of `ft graph query … expand`. The TUI graph tree
  is **not** affected — it does its own lazy, user-driven per-hop expansion via
  `query.expand` (`ft/src/tui/tabs/graph.rs`) and never calls `walk`.
- **Tests:** walk unit tests and `insta` snapshots for `ft graph query` output
  need refreshing; add explicit re-convergence / dense-graph termination tests.
- **No new dependencies.**

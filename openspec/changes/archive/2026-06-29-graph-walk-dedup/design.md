## Context

`GraphQuery::walk` (`ft-core/src/graph/query.rs:1766`) is a recursive DFS that
materializes a `Vec<WalkNode>` tree from the roots returned by `select`,
expanding one hop at a time via `GraphQuery::expand`. Its only termination guard
is path-based cycle detection:

```rust
let is_cycle = matches!(opts.cycle_policy, CyclePolicy::Stop) && ancestors.contains(&id);
```

where `ancestors` is the stack of nodes on the *current* DFS path (pushed at
`:1797`, popped at `:1810`). This catches back-edges (a node in its own ancestor
chain) but not **re-convergence**: a node reached again via a different,
non-cyclic path is expanded again from scratch. Real vaults link densely
(`note-link`), so the number of distinct simple paths is exponential and the
unbounded walk emits an astronomically large tree — the reported "hang."
`--depth 10` caps path length, bounding the blow-up enough to return.

Current types:

```rust
pub enum CyclePolicy { Stop, Allow }                 // default Stop
pub struct WalkOptions { max_depth: Option<usize>, cycle_policy: CyclePolicy }
pub struct WalkNode { id, depth, edge_to_parent, cycle: bool, children }
```

The CLI (`ft/src/cmd/graph.rs`) maps `--cycle-policy {stop,allow}` to
`CyclePolicy` and rejects `allow` without `--depth`. The TUI graph tab does
**not** use `walk`; it lazily expands user-opened nodes via `query.expand` and
is out of scope.

## Goals / Non-Goals

**Goals:**
- An unbounded `walk` over any finite graph terminates in `O(V + E)` by default.
- Each reachable node is expanded at most once; re-encounters become reference
  leaves, marked distinctly from genuine leaves and (where relevant) true cycles.
- Preserve the old `tree(1)`-style repeated-subtree behavior as an explicit,
  bounded opt-in.
- Keep the surface and test ripple contained and well-signposted.

**Non-Goals:**
- Changing the TUI graph tab traversal (separate lazy model, no hang).
- Changing `select`, `expand`, the DSL grammar, or edge semantics.
- Deduping *output rows* in any global cross-root canonical-tree sense beyond
  "expand each node once" (we keep the forest shape rooted at `select`).

## Decisions

### 1. Replace `CyclePolicy` with a three-way `VisitPolicy`

```rust
pub enum VisitPolicy {
    Dedup,     // default: global visited set, reference leaf on re-encounter — O(V+E)
    Tree,      // old `Stop`: path-based cycle stop, repeats shared subtrees
    Allow,     // no detection at all; must be depth-bounded
}
```

`WalkOptions.cycle_policy: CyclePolicy` becomes `visit: VisitPolicy` with
`Dedup` as `Default`. Rationale: the field no longer describes only cycles, and
three behaviors don't fit a two-variant "cycle" enum cleanly. Per CLAUDE.md
("don't add backwards-compat shims"), we rename rather than alias.

*Alternative considered:* add a `Dedup` variant to `CyclePolicy` and keep the
name. Rejected — the name would lie about what the default does, and `Stop`
(path cycle) vs `Dedup` (global) are genuinely different axes.

### 2. Dedup uses a global `visited: HashSet<NoteId>` threaded through the walk

`walk_node` gains a `&mut HashSet<NoteId>` (alongside the existing `ancestors`
stack, used only in `Tree` mode). First reach: insert and expand. Subsequent
reach under `Dedup`: emit a reference leaf, don't recurse. Because we never
re-expand, total work is bounded by `O(V + E)`. The `ancestors` push/pop stays
for `Tree`/`Allow` semantics; under `Dedup` it is unused (or kept only for the
cycle-vs-reference distinction below).

*Alternative considered:* keep path-based detection but add a per-node
"expansion budget" / global node cap. Rejected as the primary mechanism — it
truncates output arbitrarily instead of producing the correct dedup'd subgraph.
We keep a node-count budget only as a defensive backstop (Decision 5).

### 3. `WalkNode.cycle: bool` becomes an enum field

```rust
pub enum NodeClosure { Open, Reference, Cycle }   // Default = Open
pub struct WalkNode { id, depth, edge_to_parent, closure: NodeClosure, children }
```

- `Open` — expanded (children may be empty if it has no matching neighbors).
- `Reference` — already expanded earlier in this walk (dedup); empty children.
- `Cycle` — re-entered via the current ancestor path (only produced by `Tree`).

Under `Dedup`, an ancestor re-encounter is just another already-visited node, so
it is reported as `Reference`; `Cycle` is reserved for `Tree` mode where the
distinction is meaningful. Renderers key off `closure`.

*Alternative considered:* keep `cycle: bool` and add a separate `shared: bool`.
Rejected — two bools admit nonsense combinations and read worse than one enum.
The enum is a documented behavioral surface; the proposal already flags this as
**BREAKING** for output.

### 4. CLI flag becomes `--visit-policy {dedup,tree,allow}` (default `dedup`)

`ft/src/cmd/graph.rs` maps the flag to `VisitPolicy`. The existing guard
generalizes: **both** `tree` and `allow` require `--depth`, because `tree` mode
is also unbounded-unsafe on dense (not just cyclic) graphs — its whole point is
to repeat subtrees. `dedup` needs no bound. Error text names the safe path:
"`--visit-policy {tree,allow}` requires `--depth`; the default `dedup` is
unbounded-safe."

*Alternative considered:* keep `--cycle-policy` and only add the new default.
Rejected for the same naming reason as Decision 1; this is a pre-1.0 CLI surface
and the rename is cheap.

### 5. Defensive node-count budget (backstop)

Add an optional `max_nodes: Option<usize>` to `WalkOptions` (default `None` from
the CLI). If a future caller selects `tree`/`allow` and a bound is exceeded, the
walk stops and surfaces a clear error rather than consuming all memory. This is
belt-and-suspenders behind the depth guard, not the fix.

## Risks / Trade-offs

- **[Output change for shared descendants]** Default output differs: a node
  reachable via multiple paths now shows its subtree once plus reference
  markers. → Documented as BREAKING in the proposal; refresh `ft graph query`
  `insta` snapshots; the new shape is what users actually want for note-link
  webs.
- **[Signature ripple]** `WalkOptions`, `WalkNode`, and `walk_node` change shape,
  touching every test that builds them (CLAUDE.md flags this for core APIs). →
  `WalkOptions`/`WalkNode` keep `Default` so tests use `..Default::default()`;
  `walk`'s public signature (`&self, graph, &WalkOptions`) is unchanged, so only
  `WalkOptions`/`WalkNode` construction sites and `closure` assertions move.
- **[`tree` mode now needs `--depth`]** Users who relied on unbounded `--cycle-policy stop`
  over a pure directory tree must pass `dedup` (identical output for a true tree)
  or a `--depth`. → Acceptable; `dedup` is a strict superset of safe `tree`
  output on acyclic, non-converging graphs.
- **[Memory still `O(V+E)`, not streaming]** Very large vaults still build a full
  in-memory tree. → Out of scope; the budget (Decision 5) caps pathological
  cases, and `O(V+E)` is tractable for realistic vaults.

## Migration Plan

1. Land the `ft-core` type changes (`VisitPolicy`, `NodeClosure`, `visited` set,
   optional budget) with unit tests, including a dense/re-convergence
   termination test and a `Tree`-mode repeat test.
2. Update `ft/src/cmd/graph.rs` flag + guard + renderer for the `closure` enum.
3. Refresh affected `insta` snapshots; run the five build invariants
   (build/test/clippy/fmt + `commands docs --check`).
4. No data migration. Scripts passing `--cycle-policy` must switch to
   `--visit-policy` (`stop`→`tree`, `allow`→`allow`); the default improves
   silently for the common unflagged case.

## Open Questions

_(Both resolved 2026-06-29.)_

- **Flag naming — resolved: `--visit-policy`.** Rename `--cycle-policy` →
  `--visit-policy {dedup,tree,allow}` (default `dedup`); the old flag is removed
  (`stop`→`tree`, `allow`→`allow`). No compat alias, per CLAUDE.md.
- **Marker shape — resolved: keep the `NodeClosure` enum.** `WalkNode.closure:
  NodeClosure { Open, Reference, Cycle }` (Decision 3), keeping the
  reference-vs-cycle distinction for `Tree` mode rather than collapsing to a
  single bool.

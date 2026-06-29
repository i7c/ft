## ADDED Requirements

### Requirement: Unbounded walk terminates on any finite graph

`GraphQuery::walk` SHALL terminate in time and space `O(V + E)` over the
reachable subgraph, for any finite graph, when `max_depth` is `None` (unbounded)
under the default visit policy. The traversal MUST NOT enumerate distinct simple
paths, and MUST NOT re-expand a node that has already been expanded elsewhere in
the same walk.

#### Scenario: Dense graph with shared descendants does not hang

- **WHEN** `node where path = ""; expand where edge.kind in {directory-contains, note-link};` is walked with `max_depth = None` over a vault whose notes link densely (shared hubs and diamonds)
- **THEN** the walk completes, visiting each reachable node a bounded number of times rather than once per distinct path

#### Scenario: True cycle terminates

- **WHEN** a query expands over a subgraph containing a cycle (e.g. `a → b → a`) with `max_depth = None`
- **THEN** the walk terminates and the re-entered node is emitted without descending into it

### Requirement: Node-level dedup is the default visit policy

Under the default visit policy, each reachable node SHALL be expanded at most
once across the entire walk. The first time a node is reached it is expanded
normally; any later encounter (whether via a cycle back-edge or via a distinct
non-cyclic path) SHALL be emitted as a reference leaf — the node is present in
the output so the incoming edge is visible, but its `children` is empty and the
walk does not descend into it.

#### Scenario: Diamond re-convergence emits a reference leaf

- **WHEN** the graph has edges `a → b`, `a → c`, `b → d`, `c → d` and the walk roots at `a` with unbounded depth
- **THEN** `d` is expanded under exactly one of `b` or `c` (whichever sorts first), and under the other `d` appears as a reference leaf with empty children

#### Scenario: First encounter expands fully

- **WHEN** a node is reached for the first time during the walk and is below the depth bound
- **THEN** its matching children are expanded according to the expand block

### Requirement: Re-encountered nodes are marked distinctly

A `WalkNode` SHALL carry a marker that distinguishes a fully-expanded node, a
reference leaf produced by dedup, and (when applicable) a node re-entered via a
true cycle, so that renderers and tests can tell them apart.

#### Scenario: Reference leaf is marked

- **WHEN** the walk emits a node that was already expanded earlier in the walk
- **THEN** that `WalkNode` is flagged as a reference (e.g. a `shared`/`reference` marker) and its `children` is empty

#### Scenario: Fully-expanded node is not marked

- **WHEN** the walk emits a node for the first time and expands it
- **THEN** that `WalkNode` carries no reference or cycle marker

### Requirement: Depth bound composes with dedup

When `max_depth` is `Some(n)`, the walk SHALL stop descending at depth `n`
regardless of visit policy, and dedup SHALL still apply within the bounded
region so that the bounded walk is never larger than the unbounded one.

#### Scenario: Depth zero returns roots only

- **WHEN** the walk runs with `max_depth = Some(0)`
- **THEN** each root is returned with empty `children`

#### Scenario: Bounded walk still dedups

- **WHEN** the walk runs with `max_depth = Some(n)` over a graph with shared descendants
- **THEN** a node already expanded earlier in the walk is emitted as a reference leaf rather than re-expanded, even though depth permits descent

### Requirement: Path-tree mode is an explicit opt-in

The walk SHALL provide an explicit, non-default path-tree visit policy that
reproduces the previous behavior: a node reachable by multiple paths is repeated
with its full subtree under every parent, bounded only by path-based cycle
detection. Selecting this mode with an unbounded depth over a cyclic or dense
graph MUST be bounded by the caller (e.g. via a depth limit).

#### Scenario: Opt-in repeats shared subtrees

- **WHEN** the walk runs in path-tree mode over a graph with a shared descendant `d` reachable via two parents
- **THEN** `d`'s subtree is materialized under both parents

#### Scenario: CLI guards unbounded unsafe modes

- **WHEN** the user requests a visit policy that does not guarantee termination (path-tree / cycle-allow) without `--depth`
- **THEN** the command exits with an error explaining that a depth bound is required

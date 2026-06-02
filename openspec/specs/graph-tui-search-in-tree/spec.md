# graph-tui-search-in-tree Specification

## Purpose
TBD - created by archiving change graph-tui-search-in-tree. Update Purpose after archive.
## Requirements
### Requirement: f key opens a fuzzy picker over the current view's reachable subgraph

When the user presses `f` on the Graph tab with a non-empty tree and no other overlay active, the system SHALL open a centred fuzzy picker whose row set is exactly the nodes reachable from the active view's roots via the active query's `expand` policy.

#### Scenario: Picker opens with reachable nodes
- **WHEN** the active query is `node where kind = Directory and path = ""; expand where edge.kind = directory-contains;` against a vault with `/foo/bar/baz.md`
- **THEN** the picker opens and lists at least one row for `/`, `foo`, `bar`, and `baz`

#### Scenario: f is a no-op when tree is empty
- **WHEN** the active view has no parsed query or zero rows
- **THEN** pressing `f` does not open the picker and leaves view state unchanged

#### Scenario: f is gated behind other overlays
- **WHEN** the create / append / capture / rename / related / preset / move overlay is open
- **THEN** pressing `f` is captured by the active overlay (per existing dispatch precedence) and does not open the search picker

#### Scenario: Picker is per active view
- **WHEN** the picker is open and the user closes it (Esc) and switches to a different view via `Ctrl+PageDown`
- **THEN** the picker state from the first view does not appear in the new view

### Requirement: Picker rows match against leaf display plus breadcrumb

Each picker row SHALL match against a haystack composed of the node's leaf display followed by its breadcrumb path. Typing either the leaf string or any breadcrumb substring SHALL surface the row.

#### Scenario: Match by leaf name
- **WHEN** the picker contains a row for `bar/` reachable via `/ → foo → bar`
- **AND** the user types `bar`
- **THEN** the row is present in the visible results

#### Scenario: Match by breadcrumb substring
- **WHEN** the picker contains the same row for `bar/` reachable via `/ → foo → bar`
- **AND** the user types `foo/bar`
- **THEN** the row is present in the visible results

#### Scenario: Display shows leaf and breadcrumb
- **WHEN** a row is displayed in the picker
- **THEN** the rendered label shows the leaf display followed by a separator and the breadcrumb (ancestor leaf displays joined with `/`)

### Requirement: Selecting a row jumps the cursor to the target node

When the user presses `Enter` on a highlighted picker row, the system SHALL close the picker and place the tree cursor on the target node, materialising every ancestor along the shortest root-to-target path.

#### Scenario: Target under expanded ancestors
- **WHEN** the picker is open with `bar` under `/ → foo → bar` and the user presses Enter on the `bar` row
- **THEN** the picker closes, `/` and `foo` are expanded, and the selected row in the tree is `bar`

#### Scenario: Target is a root
- **WHEN** the user selects a row whose path has length one (the node is itself a root)
- **THEN** no expansions are added and the selected row is that root

#### Scenario: Target is left collapsed
- **WHEN** the target node has its own expandable children
- **THEN** after the jump the target row is shown collapsed (the expansion stops at the target's parent)

#### Scenario: View survives subsequent graph refresh
- **WHEN** the user jumps to a target, then triggers a graph refresh (`Ctrl+R`)
- **THEN** the tree is rebuilt with the same ancestors expanded and the cursor on the same target (because the jump writes to `expanded_paths` and `selected_path`, which survive rebuild)

### Requirement: Shortest path wins for multi-parent targets

When a target node is reachable via multiple paths from the active view's roots under the expand policy, the picker SHALL list one row per node using the shortest of those paths. Ties are resolved deterministically by BFS visit order.

#### Scenario: Single row per node
- **WHEN** the policy is the link graph and a note is reachable via three distinct ancestor chains of lengths 3, 4, and 5
- **THEN** the picker contains exactly one row for that note, and its breadcrumb has length 2 (the length-3 shortest path)

### Requirement: Unreachable nodes are excluded

Nodes the active query cannot reach from any root under the `expand` policy SHALL NOT appear in the picker.

#### Scenario: Note outside directory-contains policy
- **WHEN** the active query expands only `directory-contains` edges and a Task node exists in the graph
- **THEN** the picker does not list the Task node

#### Scenario: No expand block
- **WHEN** the active query has no `expand` block
- **THEN** the picker lists exactly the rows returned by `query.select(graph)` (the roots), each with a path of length one

### Requirement: BFS handles cycles

When the policy-induced subgraph contains cycles, the system SHALL avoid infinite traversal via a visited set and SHALL list each node at most once, at its shortest distance from a root.

#### Scenario: Two-cycle in link graph
- **WHEN** notes A and B link to each other under a link-traversing expand policy with A in the roots
- **THEN** the picker lists A once at depth 0 and B once at depth 1; the picker does not hang

### Requirement: Esc cancels the picker with no state change

When the user presses `Esc` while the picker is open, the system SHALL close the picker and leave `expanded_paths`, `selected_path`, `selected`, and `scroll_offset` unchanged.

#### Scenario: Cancel preserves view
- **WHEN** the picker is open and the user presses Esc
- **THEN** the picker is no longer rendered and the tree's selection and expansion are identical to their pre-open values

### Requirement: f is shown in the help overlay

The `f` keybinding SHALL appear in the Graph tab's `?` help overlay in the Navigation section.

#### Scenario: Help overlay lists f
- **WHEN** the user presses `?` on the Graph tab
- **THEN** the overlay's Navigation section includes an entry for `f` describing the jump-to-node action


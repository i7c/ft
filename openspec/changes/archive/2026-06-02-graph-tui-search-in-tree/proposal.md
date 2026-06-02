## Why

The Graph tab is an "infinite tree" — a view of the link / containment graph that can be followed forever, so we never render the whole graph at once. Once the user has expanded a few levels (or the view is naturally deep, like the directory tree), navigating to a specific node by `j`/`k` and `Enter`/`l` becomes tedious. There's no quick way to say "jump to the node called `bar`" — the user has to remember which ancestor chain leads there and step into each level manually. A fuzzy search over the current view's reachable subgraph closes that gap without changing the user's mental model of the tree.

## What Changes

- Add an `f` keybinding on the Graph tab that opens a centred fuzzy picker over every node reachable from the current view's roots via the active query's `expand` policy.
- The picker matches against a combined string of the leaf display (e.g. `bar/`) and the breadcrumb of ancestor displays (e.g. `foo/bar`), so typing either the leaf name or any portion of the path filters the same way.
- On selection, the system computes the shortest root-to-target path (already known from the BFS that fed the picker), records every ancestor in `expanded_paths`, sets `selected_path` to the full path, and calls the existing `restore_expansion` to materialise the tree with the target visible and the cursor on it.
- The target node is left collapsed (consistent with how `Enter`/`l` first lands the cursor on a row, then expands).
- One row per reachable node — shortest path wins when multiple paths exist (e.g. a note linked from several files under the link-graph policy).
- Nodes the active query can never reach are excluded from the picker — the search is honest about what it can land on.
- `Esc` cancels the picker with no state change. The picker is per active view; switching views or editing the query naturally resets it.
- Help overlay entry for `f` is added to the Navigation section.

## Capabilities

### New Capabilities

- `graph-tui-search-in-tree`: Pressing `f` on the Graph tab opens a fuzzy picker that jumps the cursor to any node reachable under the current query, auto-expanding the ancestors along the shortest path.

### Modified Capabilities

<!-- None — purely additive TUI feature; no engine surface changes. -->

## Impact

- **TUI**: `ft/src/tui/tabs/graph.rs` — new `search_picker: Option<FuzzyPicker<GraphSearchPickerSource>>` field on `GraphTab`, new `GraphSearchPickerSource` (BFS + nucleo ranking), new dispatch branch in `handle_event` and render branch in `render`, plus a help entry. New `f` binding only.
- **ft-core**: no changes. The existing `GraphQuery::select` / `GraphQuery::expand` + `Graph::node` API is sufficient for the BFS.
- **Tests**: a small unit module for the BFS / shortest-path helper; integration tests over the existing `tests/fixtures/dirs` vault; one `TestBackend` snapshot of the picker open against a known fixture.
- **Performance**: BFS at picker-open is O(V + E) over the policy-reachable subgraph. Synchronous; expected sub-100ms on a 50k-node vault under the directory-contains policy. No work on every keystroke — `nucleo_matcher` filters the cached candidate list.

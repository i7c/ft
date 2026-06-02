## Why

The graph tab always builds its tree from the roots specified in the query's `node` block. To inspect a subtree in isolation, the user must manually edit the query to target a specific node by path — cumbersome and error-prone. A single keystroke that re-roots the view on the currently-selected node while preserving the expansion policy makes focused exploration fast and fluid.

## What Changes

- Add a `z` keybinding on the graph tab that re-writes the active view's query to root the tree at the currently-selected node.
- The new `node` block selects exactly the selected node by `kind` and `path`. The existing `expand` block (if any) is preserved verbatim.
- The query text in the input bar is updated to reflect the new query, so the user sees the change and can further edit it.
- Works only for Note and Directory nodes (which have paths). Ghost and Task nodes are a no-op.
- Pressing `z` again on a different node re-roots to that new node — successive presses are intentionally overwriting, not stacking.

## Capabilities

### New Capabilities

- `graph-z-root`: Pressing `z` on the graph tab re-roots the tree on the selected Note or Directory node, preserving the expand policy.

### Modified Capabilities

<!-- None — this is a new UI-only binding with no changes to core types or existing behavior. -->

## Impact

- **TUI**: `ft/src/tui/tabs/graph.rs` — new key handler for `z`, new `rewrite_query_for_root` method on `GraphTab` (or a free function).
- **TUI help**: `?` overlay keymap needs a new entry for `z`.

## Why

On the Graph tab, the `t` (today) and `p` + period-letter periodic-note keybindings currently open the target file in the system `$EDITOR`, jumping the user out of the graph exploration flow. Instead, they should navigate *within* the graph tree to the periodic note — auto-expanding ancestors and placing the cursor on it — so the user stays in the graph without losing context.

## What Changes

- **Modify** `t` (today) and `p` + `d`/`w`/`m`/`q`/`y` (periodic leader) keybindings on the Graph tab: instead of opening the periodic note in `$EDITOR`, resolve the shortest path from the current query's roots to that note within the active view's reachable subgraph and `GraphJumpToNodes` to it.
- **Precondition check**: if the periodic note is not part of the active query's result set (i.e., not reachable via the current `select` + `expand` policy), queue an informational toast and do nothing — the user must adjust their query to include it.
- **Fallback removal**: the existing `run_periodic_open` (which creates the file on disk and opens in `$EDITOR`) is no longer called from the Graph tab's periodic dispatch. The Notes tab retains its existing editor-open behaviour.

## Capabilities

### New Capabilities

- `graph-periodic-navigate`: When a periodic-note keybinding is pressed on the Graph tab and the target note is reachable in the active view's subgraph, the tree navigates to that note by expanding the shortest root-to-target path and placing the cursor on it.

### Modified Capabilities

- `graph-to-journal-jump`: No requirement changes — spacing-related. The `J` binding continues to switch tabs; periodic bindings stay in-graph.

## Impact

- **Affected code**: `ft/src/tui/tabs/graph.rs` (periodic dispatch in `dispatch_command`, new BFS navigation method), `ft/src/tui/modal.rs` (`PeriodicLeader` modal's handler). `ft/src/tui/notes_actions/periodic.rs` may need a variant that returns the resolved path/NoteId without opening the editor.
- **No API or breaking changes**. Existing keybindings are repurposed in the Graph tab only; Notes tab behaviour is unchanged.
- **Dependency**: requires the existing `GraphQuery::select` / `GraphQuery::expand` evaluator and the `jump_to_path` helper already in `GraphTab`.

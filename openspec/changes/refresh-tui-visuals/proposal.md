## Why

The TUI currently uses a generic cyan/yellow/magenta palette that lacks visual warmth and personality. The graph tab's tree area has no frames, making it feel raw compared to other tabs. The graph tab places the search bar at the bottom while other tabs (tasks, notes) place it at the top — an inconsistency that breaks muscle memory. The timeblocks tab defaults to a two-pane side-by-side view, which is unnecessarily wide for many users who only need to see today.

## What Changes

- Replace the current accent palette (cyan, magenta) with a warm orange/red/yellow palette applied consistently across all tabs and UI elements
- Add block frames (borders) around the graph tab's tree area to match the visual framing used by other tabs (timeblocks panes, notes panel)
- Move the graph tab's query input bar from the bottom of the body area to the top, consistent with the tasks tab
- Change the timeblocks tab's default view mode from `Split` (two-pane) to `Single` (full-width single day), keeping `f` as the toggle

## Capabilities

### New Capabilities
- `tui-color-palette`: A warm orange/red/yellow color theme applied across the TUI — tab bar, status bar, content panes, modals, and help overlay
- `tui-graph-frames`: The graph tab's tree viewport renders with a bordered frame matching the convention of other tabs (timeblocks panes, notes idle panel)
- `tui-graph-query-bar-top`: The graph tab's query input bar renders at the top of the body area (above the tree), consistent with the tasks tab's query bar placement
- `tui-timeblocks-default-single`: The timeblocks tab initializes in `ViewMode::Single` showing only today, with `f` toggling to the side-by-side `ViewMode::Split`

### Modified Capabilities
<!-- No existing spec-level behavior changes — only visual style and defaults change -->

## Impact

- **Affected code**: `ft/src/tui/ui.rs` (tab bar, status bar, help overlay colors), `ft/src/tui/tabs/graph.rs` (tree frame, query bar position), `ft/src/tui/tabs/timeblocks/view.rs` (unchanged, render already handles both modes), `ft/src/tui/tabs/timeblocks/mod.rs` (default ViewMode), `ft/src/tui/tabs/notes/view.rs` (idle panel border color), `ft/src/tui/tabs/tasks/search.rs` (query bar border color), modal renderers across `ft/src/tui/`
- **Snapshot tests** in `ft/src/tui/tests.rs` will need update for new colors and layout changes
- No **breaking** changes to keybindings, APIs, CLI, or data formats

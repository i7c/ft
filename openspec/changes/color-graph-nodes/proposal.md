## Why

The graph tab currently renders all rows in a single uniform color (white foreground). With five node types (Note, Directory, Ghost, Task, Paragraph) visible simultaneously in the same tree, users must read the single-character kind prefix (`N`, `D`, `G`, `T`, `P`) to tell types apart. Color-coding each node type makes the tree scannable at a glance and reduces visual cognitive load when working with dense trees.

## What Changes

- Assign a distinct foreground color to each `NodeKind` variant in the graph tab tree renderer.
- The kind character (`kind_char`) and display text are styled with the per-type color in addition to any selection highlighting.
- The selected row's highlight (black-on-white) is preserved; the highlight color is layered with the type color so selected rows remain readable.
- Ghost rows (unresolved links) use a dim/less-saturated color to visually de-emphasize them as "not real files".
- Paragraph rows use a muted tone distinct from Notes so inline paragraph entries don't blend into their parent notes.
- The jump-to-node fuzzy-picker and in-tree search picker row labels inherit type coloring where feasible.

## Capabilities

### New Capabilities

- `graph-node-colors`: Per-node-type foreground coloring in the graph tab tree, with selectable rows preserving their type color against the highlight background. Ghosts and paragraphs get visually de-emphasized tones.

### Modified Capabilities

<!-- No existing spec requirements change. -->

## Impact

- **Affected code**: `ft/src/tui/tabs/graph.rs` — the `render` method, `leaf_display` may return a `Color` alongside the kind char, and the fuzzy picker label formatting for `GraphSearchPickerSource`.
- **APIs**: No core API changes. The color mapping is a TUI-presentation decision that stays in the binary crate.
- **Dependencies**: None new. Uses existing `ratatui::style::Color`.
- **Snapshot tests**: All TUI frame snapshots (`ft/src/tui/tests.rs`) that exercise the graph tab will need regeneration since the visual output changes.

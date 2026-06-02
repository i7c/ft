## Why

The graph currently tracks direct note-to-note links (`Link` / `Embed`) and folder containment (`Contains`), but there is no edge that captures "this note links to notes inside that folder." This makes it impossible to query folder-level cross-referencing patterns — e.g., "which folders does `Areas/finance.md` reach into?" or "what notes outside `Projects/alpha/` link into it?" Adding a `links-into` edge fills this gap with one edge per unique (source-note, target-folder) pair, derived from existing resolved link data.

## What Changes

- Add a new `EdgeKind::LinksInto` variant to the graph core, with a corresponding `"links-into"` label in the DSL, CLI output, and edge-kind string tables.
- During graph build (and `refresh_note`), after link resolution, insert one `LinksInto` edge per unique (source note, target note's parent directory) pair for every resolved `Link` or `Embed` edge. Targets at the vault root point to the root Directory node.
- Unresolved links (ghosts) are excluded. Embeds are included.
- The query DSL automatically supports `edge.kind = "links-into"` (and related operators) through the existing edge-kind registration tables.
- A new built-in graph preset `crosslinks` exposes the new edge type: "show the vault-root tree plus which notes link into which folders."

## Capabilities

### New Capabilities

- `links-into-edge`: A new graph edge type `LinksInto` from Note to Directory, representing "this note links to one or more notes contained in that folder." Created during graph build and refresh, queryable through the existing DSL `edge.kind` filter.

### Modified Capabilities

<!-- None — this is purely additive. Existing edge types and behavior are unchanged. -->

## Impact

- **Core**: `ft-core/src/graph/mod.rs` — new `EdgeKind::LinksInto` variant, new insertion pass in `Graph::build()` and `refresh_note`.
- **Query DSL**: `ft-core/src/graph/query.rs` — register `"links-into"` in `edge_kind_str()`, `check_kind_values()`, and `validate_attr_subject()` (edge-only, like `form`).
- **CLI output**: `ft/src/output/graph.rs` — add `"links-into"` to `edge_kind_label()`.
- **Presets**: `ft-core/src/graph/preset.rs` — add `"crosslinks"` built-in preset.
- **Tests**: `ft-core/src/graph/tests.rs` — verify the edge appears, cardinality (one per folder), root-target behavior, and ghost exclusion.
- **TUI**: No rendering changes needed — the graph tab displays node kinds, not edge kinds. The new edge type is transparently navigable.

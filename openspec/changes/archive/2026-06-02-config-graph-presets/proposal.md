## Why

Graph queries are repetitive — users type the same DSL strings to answer questions like "what links to this note?" or "show me orphans". The task-query side already has `Config::presets` for named task DSL strings, but the graph side has only a single `default_query`. Named graph presets would let users save common queries, reference them by name from the CLI (`--preset`), and populate the TUI's new-view list with one keystroke.

## What Changes

- Add a `presets` field to `GraphCfg` — a `HashMap<String, String>` mapping preset names to graph-DSL strings, mirroring the task-side `Config::presets`.
- Support graph presets in the TOML config under `[graph]` as ` presets = { name = "..." }`.
- Resolution order in TUI: offer graph presets as quick-pick entries when creating a new view.
- Built-in graph presets for common queries (orphans, directory tree, backlinks, etc.).
- User presets shadow built-ins of the same name (matching task-preset convention).

## Capabilities

### New Capabilities

- `graph-presets`: Named graph-query presets stored in config, resolvable by name from CLI and TUI, with built-in defaults and user-override via TOML.

### Modified Capabilities

## Impact

- `ft-core/src/config.rs` — `GraphCfg` gains a `presets` field; `Config::presets` remains task-only.
- `ft-core/src/graph/query/preset.rs` (new) — built-in graph preset table, parallel to `ft-core/src/query/preset.rs`.
- `ft/src/cmd/graph.rs` — `--preset <name>` flag, resolution before DSL parsing.
- `ft/src/tui/tabs/graph.rs` — preset quick-pick on new-view creation.
- Config TOML schema gains `[graph.presets]` section (nested under existing `[graph]`).
- No breaking changes — `default_query` keeps working; presets are additive.


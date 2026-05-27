## Context

`ft` has two DSLs — task and graph — each with its own parser, evaluator, and output pipeline. The task side already has named presets via `Config::presets` (a `HashMap<String, String>` mapping names to task-DSL strings) plus a built-in preset table in `query::preset`. The graph side has only `GraphCfg::default_query` — a single optional string that seeds the TUI's first view. There is no mechanism to name, store, or recall graph queries.

The graph query DSL (`graph::query`) is fully implemented: a recursive-descent parser produces a `GraphQuery` AST (initial selectors + expand policy), `walk()` materializes a subtree, and the CLI/TUI both consume it. The DSL round-trips through `Display` — `parse(format!("{q}")) == Ok(q)` — so stored strings are always valid.

Task-preset resolution lives entirely in the CLI (`ft/src/cmd/tasks.rs`): `--preset <name>` checks user config first, falls back to `builtin()`, and feeds the resulting DSL string to the parser. The TUI task list uses the same resolution path.

## Goals / Non-Goals

**Goals:**
- Add a `presets` map to `GraphCfg` so users can store named graph-DSL strings in TOML under `[graph]`.
- Provide built-in graph presets for common queries (orphans, backlinks, directory tree, etc.).
- Allow `ft graph query --preset <name>` to resolve a preset to a DSL string, matching the task-side `--preset` pattern.
- Surface graph presets in the TUI as quick-pick entries when creating a new view.

**Non-Goals:**
- Merging task and graph preset maps (they hold different DSLs; keeping them separate avoids ambiguity).
- Making `default_query` reference a preset by name (it already takes a raw DSL string; adding indirection is unnecessary complexity).
- Auto-completing preset names in the TUI's query input (that is a separate UX enhancement).
- Supporting preset namespaces or categories (a flat name map is sufficient, matching task presets).

## Decisions

### 1. Store graph presets in `GraphCfg::presets`, not `Config::presets`

`Config::presets` is `HashMap<String, String>` holding task-DSL strings. Reusing it for graph DSL would require runtime disambiguation (parse as task? parse as graph?) and risk name collisions between task and graph presets of the same name. Instead, add a separate `presets: HashMap<String, String>` to `GraphCfg`, making the TOML structure `[graph.presets]`.

**Alternative considered**: A unified `presets` map with a `type` field per entry. Rejected because it complicates the TOML schema and resolution logic for no user-facing benefit — the two DSLs never share a preset.

### 2. Built-in graph presets live in `ft-core/src/graph/query/preset.rs`

A new module parallel to `ft-core/src/query/preset.rs`, with the same API: `builtin(name) -> Option<&'static str>` and `builtin_names() -> &[&str]`. Each built-in string MUST parse cleanly through `graph::query::parse` (enforced by a unit test).

Proposed built-ins:
| Name | DSL |
|------|-----|
| `orphans` | `node where indegree = 0 and kind = Note;` |
| `tree` | `node where kind = Directory and path = ""; expand where edge.kind = directory-contains;` |
| `links` | `node where kind = Note; expand where edge.kind in {link, embed};` |
| `dangling` | `node where kind = Ghost;` |

### 3. CLI resolution mirrors task-side pattern

`--preset <name>` is a new mutually-exclusive source alongside the positional `QUERY`, `--query`, and `--from-file`. Resolution order: user config → built-in table. If the name resolves, the resulting string feeds into the same `parse()` + `walk()` pipeline. If it doesn't resolve, exit with code 2 (unknown preset) — matching the DSL-parse-error convention.

### 4. TUI quick-pick on new-view creation

When the user presses `Ctrl+N` to create a new graph view, instead of starting with a blank query, offer a list of preset names (built-in + user-defined) as quick-pick entries. Selecting one pre-fills the query string; the user can still edit it before applying. This is additive — the current blank-input path remains available.

## Risks / Trade-offs

- **[Preset name collision with future DSL syntax]** → Built-in names are short, stable, and unlikely to conflict. Document the reserved names. User presets shadow built-ins, so users can always override.
- **[Stale preset strings if DSL grammar evolves]** → The unit test that asserts `parse(builtin(name)).is_ok()` catches drift immediately. User presets will get parse errors at resolution time — same failure mode as typing the query manually.
- **[TUI quick-pick adds UI complexity]** → Start with a simple list rendered as a menu overlay, not a fuzzy finder. The list is small (4 built-ins + user presets).
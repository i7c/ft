## Context

The graph tab's `ExpandedView` holds a `query_text: String` (the raw DSL text in the input bar) and a `query: Option<GraphQuery>` (the parsed AST). The tree is derived from `query.select(graph)` for roots and `query.expand(graph, id)` for children. The user edits the query text in input mode and presses Enter to re-parse and rebuild the tree.

The `GraphQuery` canonical serialization (`Display` impl) joins node blocks with `"; "` separators and appends the expand block (if any) with a leading `"; "` before `"expand"` and a trailing `";"`. This format is parseable — it round-trips.

## Goals / Non-Goals

**Goals:**
- `z` re-writes the query text to root on the selected Note or Directory node.
- The `expand` block from the current query is preserved unchanged.
- The updated query text is visible in the input bar.
- Ghost and Task nodes are no-ops (no `path` attribute).
- Help overlay shows the new keybinding.

**Non-Goals:**
- No undo/stack — pressing `z` again on a different node overwrites.
- No interaction with the `expanded_paths` set — the tree is rebuilt fresh from the new roots.
- No core changes — this is purely TUI presentation logic.
- No automatic expansion — the new root starts collapsed, same as any new query.

## Decisions

### Rewrite via canonical serialization + string split

The implementation formats the current `GraphQuery` to its canonical string, locates the boundary between node blocks and the expand block by finding `"; expand"`, and constructs a new query with a single `node where kind = <K> and path = "<P>"` prefix followed by the extracted expand suffix.

**Alternative**: Parse the query, build a new `GraphQuery` AST with a single `NodeSelector`, then serialize it. Rejected: this requires making the condition serialization functions public, and the string-manipulation approach is simpler and trivially correct since canonical serialization is deterministic.

### Kind and path strings

For Note nodes: `kind = "Note"`, path from `NoteData.path.to_string_lossy()`. For Directory nodes: `kind = "Directory"`, path from `DirData.path.to_string_lossy()`. The root directory has path `""`.

Path escaping: double-quote and backslash in paths are escaped. Vault paths are typically alphanumeric with `/`, spaces, and hyphens, so this is rarely needed but handled defensively.

### Key handler placement

`z` is added to the tree-navigation keymap in `handle_event` (alongside `j`, `k`, `Enter`, `h`, etc.), before the outer-tab passthrough. It requires the graph and tree to be non-empty (same guard as other navigation keys).

### No state flag

No `bool` toggle or internal mode — the query text is directly rewritten and re-parsed. This keeps zero internal state complexity and makes the change visible to the user in the input bar.

## Risks / Trade-offs

- **Lossy round-trip for expand-less queries**: The canonical form of `node where kind = Ghost` is `node where kind = Ghost;` (trailing semicolon). When we replace the node block, the trailing semicolon is included. This is fine — both parse identically.
- **Path escaping edge case**: If a vault path contains `\"` or `\\`, our escaped output must round-trip through the DSL parser. The DSL parser handles `\"` and `\\` escape sequences in string literals, so manual escaping with the same rules is correct.

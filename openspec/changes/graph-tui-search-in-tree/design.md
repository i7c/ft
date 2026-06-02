## Context

The Graph tab renders a flat list (`TreeState.rows: Vec<TreeRow>`) derived from a `GraphQuery` and the user's expansion gestures. State on each `ExpandedView` is split between a *spec* (`query`, `expanded_paths: HashSet<Vec<NoteId>>`, `selected_path: Option<Vec<NoteId>>`) and *derived* fields (`tree`, `selected`, `scroll_offset`). The spec/derived split is what lets the tree survive a graph rebuild: `restore_expansion(graph)` re-runs `query.select`, then replays the saved expansion paths shortest-first via `find_row_for_path` + `TreeState::expand_at`, then walks the saved `selected_path` to relocate the cursor.

That same machinery is the leverage point for "jump to node": if we can compute the root-to-target path for any reachable node, writing it onto the spec and calling `restore_expansion` already materialises the tree exactly as we want it. The only new logic is the BFS that produces the path and the picker UI to choose a target.

The existing `FuzzyPicker<S: PickerSource>` widget (`ft/src/tui/widgets/picker.rs`) is the canonical modal UI pattern in this tab — the preset picker (`PresetPickerSource` at `ft/src/tui/tabs/graph.rs:61`) is a near-identical template: build an item list at construction, score with `nucleo_matcher` on each query, return `PickerItem`s.

## Goals / Non-Goals

**Goals:**

- A single keystroke (`f`) opens a fuzzy picker scoped to the active view's reachable subgraph.
- Picker matches against `"<leaf> <breadcrumb>"`, displays `"<leaf>  ·  <breadcrumb>"`.
- Selecting a row jumps the cursor to that node, expanding only the ancestors along the shortest path.
- The picker is honest: it only lists nodes the active query can reach. Unreachable nodes are excluded.
- BFS handles cycles in the link graph via a visited set.
- No changes to `ft-core` — feature is entirely TUI-side.
- Help overlay lists `f`.

**Non-Goals:**

- No vim-style `/foo` + `n`/`N` cycle; one-shot selection only.
- No auto-expand of the target node — cursor lands on it collapsed.
- No persistence across view switches or query edits — picker state is short-lived.
- No incremental / async BFS — synchronous at picker-open. Picker rejected on empty tree.
- No multi-path display — one row per node, shortest path wins.

## Decisions

### BFS over `query.select` + `query.expand`

The picker source's constructor runs a single BFS:

```text
let roots = query.select(graph);
queue ← deque of (NoteId, Vec<NoteId> path)
seed queue with (root, vec![root]) for each root
visited: HashSet<NoteId> seeded with roots
while let Some((id, path)) = queue.pop_front():
    push Candidate { id, path: path.clone(), label, kind_char } to results
    if let Some(children) = query.expand(graph, id):
        for child in children:
            if visited.insert(child):
                let mut p = path.clone(); p.push(child);
                queue.push_back((child, p));
```

Each `Candidate` stores its `path` so selection is O(1) — no second traversal.

**Alternative considered**: lazily compute the path on selection by walking parent pointers. Rejected: the path is needed for the breadcrumb label anyway, so caching it upfront removes a second `HashMap<NoteId, NoteId>` and keeps construction and selection in one pass.

### Reuse `expanded_paths` + `selected_path` + `restore_expansion`

On selection (`path = candidate.path`):

```rust
if path.len() > 1 {
    v.add_expansion_path(path[..path.len() - 1].to_vec()); // ancestors only
}
v.selected_path = Some(path);
v.restore_expansion(graph);
v.scroll_to_selection(visible);
```

`add_expansion_path` already records every prefix, so the `HashSet<Vec<NoteId>>` invariant (closed under prefixes) holds. `restore_expansion` replays expansions shortest-first, so every ancestor's children are present when its own row is searched. `find_row_for_path` resolves the leaf and lands `selected` on it.

**Alternative considered**: directly mutate `TreeState` (call `expand_at` for each ancestor, set `selected` to the resolved leaf row). Rejected: this duplicates `restore_expansion`'s logic and risks drift in invariants (e.g. forgetting to update the `expansion_cache`).

### Label / match strings

For each candidate:

- `leaf` = the same string `TreeState::make_row` uses for `display` (Directory → `name/` or `/`, Note → file stem, Ghost → raw, Task → description, Paragraph → `path:line`).
- `breadcrumb` = `leaf` of each ancestor joined with `/` (e.g. `foo/Areas/finance`).
- `match_haystack` = `format!("{leaf} {breadcrumb}")` so typing `bar`, `foo/bar`, or `finance Areas` all surface the right row.
- `display_label` = `format!("{leaf}  ·  {breadcrumb}")`; rendered with `match_indices` from `nucleo_matcher` only if they fall in the `leaf` portion (highlighting in the breadcrumb adds noise and would require remapping indices through the formatted display — skipped for v1).

**Alternative considered**: matching only the leaf. Rejected per the user's confirmed choice — combined is materially more useful for the directory tree (`foo/bar` is a natural query) without much complexity cost.

### Picker source built at picker-open

The picker source captures `(&Graph, &GraphQuery)` borrows transitively via the `Vec<Candidate>` snapshot. Once built, no further graph access is needed for filtering (nucleo runs over the cached label strings). This is the same lifetime shape as `PresetPickerSource`.

If the graph or query changes while the picker is open — they can't: editing the query requires closing the picker first (the dispatcher orders search-picker capture *before* `input_mode`).

### Key binding: `f`

`f` is currently unbound in the Graph tab keymap. It's short, mnemonic ("find"), and doesn't collide with the existing `/` (edit query), `g`/`G` (top/bottom), `r`/`Ctrl+R` (rename / refresh).

The handler is gated identically to the other modal-opener keys (`m`, `R`, `J`, `c`, `C`, `A`, `Q`): only fires when no other overlay is active. The search-picker capture branch runs ahead of `input_mode` so typing inside the picker doesn't drop into the query bar.

### Empty-tree behaviour

When the tree is empty (no roots — e.g. parse error or an empty-select query), `f` is a no-op. Consistent with `j`/`k`/`Enter` which all early-return on empty tree at the same guard.

### Picker render

Re-uses the same centered-rect + `Clear` overlay pattern as `preset_picker` and `capture_picker`, with a slim header (`" graph search "`) and a footer hint (`"Enter: jump · Esc: cancel"`). 60×60% sizing matches `preset_picker`.

## Risks / Trade-offs

- **[BFS over link-graph queries can enumerate the whole graph]** → Acceptable for the vault sizes ft targets (≤ 100k nodes). Concretely, BFS with a `HashSet<NoteId>` visited set and `query.expand` returning sorted child vectors is O(V + E) of the reachable subgraph. Mitigation if it becomes a problem later: cap candidate count (say 10k) and surface a "search truncated" footer.
- **[`expand` policy with cycles]** → BFS handles this by construction via the visited set. The shortest-path guarantee falls out of BFS on the unweighted directed subgraph.
- **[Path-prefix expansion may surface a lot of rows]** → If the user jumps to a node 8 levels deep, every ancestor expands. That's exactly what the user asked for; if it produces a wall of noise, the existing `h` (collapse / jump to parent) is the recovery.
- **[Highlighting in breadcrumb is suppressed]** → Match indices apply to the leaf only; matches that hit the breadcrumb still rank the row but don't show character highlights in the breadcrumb portion. Trade-off accepted to keep label rendering simple — users still see *which* row matched.
- **[Borrowing graph immutably across the picker's lifetime]** → The picker source owns a `Vec<Candidate>` (no graph borrow held across input events), matching the existing `PresetPickerSource` shape.

## Open Questions

- Should `f` survive a graph refresh (`Ctrl+R`) while the picker is open? Currently no overlay survives a refresh keystroke because the keymap is gated; same here. Acceptable for v1.
- Long-running BFS: do we want a visible progress hint if BFS takes > N ms? Probably no for v1 — measure first.

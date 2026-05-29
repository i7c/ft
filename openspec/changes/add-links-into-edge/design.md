## Context

The graph core (`ft-core/src/graph/mod.rs`) models a heterogeneous graph with four `NodeKind` variants (Note, Ghost, Directory, Task) and four `EdgeKind` variants (Link, Embed, Contains, HasTask). Each edge kind registers into three string-mapping tables that feed the query DSL parser/evaluator, the CLI output formatters, and the TUI rendering.

The graph build pipeline in `Graph::build()` runs sequentially: insert note nodes → insert directory nodes → insert contains edges → insert link edges (resolve + insert) → insert task nodes + has-task edges. The `LinksInto` edge must be inserted _after_ link resolution (so we know which notes resolve to which directories) but before the graph is returned. The same logic must run in `refresh_note` for incremental updates.

Link resolution (`resolve.rs`) already determines the target `NoteId` for every wikilink and markdown link. Ghosts (unresolved) have no directory, so they're excluded. Embeds use the same resolution path and are included.

The root Directory node has `path: PathBuf::new()` (empty). A root-level note like `Index.md` has `parent() == Some(Path::new(""))`, which normalizes to the root directory — so the lookup works naturally.

## Goals / Non-Goals

**Goals:**

- Add `EdgeKind::LinksInto` as a unit variant (no associated data — like `Contains` and `HasTask`).
- Insert one `LinksInto` edge per unique (source Note, target Note's parent Directory) pair for every resolved Link or Embed.
- Register `"links-into"` in all edge-kind string tables so the DSL, CLI, and TUI pick it up.
- Handle `refresh_note` correctly: remove old `LinksInto` edges from the source, recompute from current resolved links.
- Add a `"crosslinks"` built-in graph preset.

**Non-Goals:**

- No reciprocal edge (e.g., `LinkedFrom` from Directory → Note). Querying `incoming(links-into)` on a directory already answers "which notes link into me?".
- No per-link-linkedge duplication — one edge per (src, dir) pair regardless of how many individual links point into that folder.
- No changes to the TUI rendering pipeline (edge kinds aren't surfaced in tree rows).
- No config knobs or user-facing toggles.

## Decisions

### Edge shape: unit variant

`EdgeKind::LinksInto` carries no associated data. There's no `LinkEdge` to store — no source byte range, no line number, no raw text — because the edge is a derived aggregate, not a direct parse artifact. This matches the shape of `Contains` and `HasTask`.

**Alternative considered**: Storing a `Vec<LinkEdge>` on the variant to preserve provenance. Rejected: the individual link edges already exist on the graph (as `Link`/`Embed` edges to the resolved Note nodes). Users who need per-link detail query those edges directly. `LinksInto` answers the folder-level aggregate question.

### Insertion strategy: post-hoc pass over existing edges

A new method `insert_links_into_edges()` runs after all `insert_edges_for()` calls in `Graph::build()`. It iterates over all outgoing Link/Embed edges in the graph, extracts `(source, target_note.parent_dir)`, deduplicates with a `HashSet<(NoteId, NoteId)>`, and inserts one `LinksInto` edge per unique pair.

**Alternative considered**: Inline collection during `insert_edges_for`. Rejected because `resolve_wiki` and `resolve_md` return `NoteId` directly and `insert_edges_for` doesn't know which targets are Notes vs Ghosts until after the edge is inserted. A post-hoc pass over the already-materialized graph is simpler and reads the final resolved state.

### Refresh note: remove + recompute

`refresh_note` currently calls `remove_outgoing_edges` (which removes all outgoing edges from the source, including Link/Embed/Contains) then `insert_edges_for`. We extend the removal to also cover `LinksInto` edges (they're already outgoing from the source, so `remove_outgoing_edges` would already remove them if they were on the graph — wait, actually `remove_outgoing_edges` only removes edges from the source to the target, not edges we add to the source that point to directories… Let's verify).

Looking at `remove_outgoing_edges`: it collects all edges directed out of `src.0`, removes them, and garbage-collects ghost neighbors. Directory nodes are not ghost nodes, so they wouldn't be garbage-collected. `LinksInto` edges point from a Note to a Directory — `remove_outgoing_edges` would remove them too since they're outgoing from `src`. So removal is already handled.

Then we need to re-insert `LinksInto` edges after `insert_edges_for`. A new private method `insert_links_into_for(src)` handles just one source node's set.

**Alternative considered**: Modify `remove_outgoing_edges` to special-case `LinksInto`. Rejected — the existing method already removes _all_ outgoing edges from the source, which is what we want. No change needed there.

### Query DSL registration

Three touch points in `ft-core/src/graph/query.rs`:

1. `edge_kind_str()` — add `EdgeKind::LinksInto => "links-into"`
2. `check_kind_values()` for `Subject::Edge` — add `"links-into"` to the allowed set
3. `validate_attr_subject()` — `LinksInto` has no `LinkEdge`, so `edge.form` returns `None` naturally via `edge_form_str()` → `e.link()` → `None`. No change needed for form validation; the existing logic already handles edge kinds without link data correctly.

### Preset design

The `"crosslinks"` preset follows the `"tree"` preset pattern: a query rooted at the vault-root Directory, expanding via both `directory-contains` and `links-into`:

```
node where kind = Directory and path = ""; expand where edge.kind in {directory-contains, links-into};
```

This shows the folder tree with cross-links from notes into folders they reach.

## Risks / Trade-offs

- **Graph size increase**: One extra edge per unique (note, folder) pair. In a vault with N notes across M folders, worst case is N×M edges if every note links to every folder. Real vaults are sparse — a few edges per note. Mitigation: the edge is a unit variant (no heap allocation), and the HashSet dedup pass is O(outgoing_edges) which is already bounded by the number of parsed links.
- **`refresh_note` correctness**: If a note's links change, old `LinksInto` edges must be removed and new ones inserted atomically. Since `remove_outgoing_edges` already removes all outgoing edges (including `LinksInto`), and `insert_links_into_for` recomputes from the new link set, this is naturally correct.
- **Ghost exclusion transparency**: Users might expect `links-into` edges for unresolved links too (e.g., "this note intends to link into that folder"). This is a deliberate exclusion — ghosts have no directory and the resolution hasn't happened yet. Documented in the spec.

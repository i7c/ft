## Context

`ft` maintains an in-memory heterogeneous `Graph` (petgraph `StableDiGraph`) built on every `vault.scan()` call. The graph already carries `NodeKind::Note`, `Ghost`, `Directory`, and `Task` nodes — all referenced by `NoteId`, a newtype over petgraph's `NodeIndex`. The graph's comment block explicitly anticipates additional node kinds. Link extraction (`graph::parser::extract_links`) already reads every file's content in a parallel pass during `Graph::build`; that content is discarded after edge insertion.

Obsidian vaults in practice are commit-heavy (automated daily commits), so `git blame` data is meaningful and per-line granularity is achievable. The vault already has a `.ft/` config directory; a `.ft/cache/` subdirectory for the blame cache fits naturally.

The graph query DSL (`graph::query`) is hand-rolled recursive-descent and handles node/edge type discrimination via `NodeKind` / `EdgeKind` pattern matching. Extending it for new kinds is additive.

## Goals / Non-Goals

**Goals:**
- Paragraph nodes in the same `Graph`, traversable by the same `incoming` / `outgoing` API
- `ft notes journal <note>`: reverse-chronological feed of paragraphs linking to a note or its Related aliases, with git-blame-derived dates
- `ft notes update-related <note>`: co-occurrence scoring + TUI graph-tab modal that appends selected concepts to the Related section via plan/apply
- Lazy blame cache (`.ft/cache/blame.msgpack`) so repeated journal queries are fast
- Graph query DSL node-kind and edge-kind filters for `Paragraph`, `OwnsParagraph`, `ParagraphLink`

**Non-Goals:**
- Paragraph content text filtering in the DSL (v2)
- Bare-title (non-wikilink) mention matching
- Pre-built persistent graph index (graph remains rebuilt from scratch per session)
- Cross-file "same calendar day" co-occurrence scoring (same-file cross-paragraph is sufficient for v1)

## Decisions

### D1: Paragraph nodes in the same `Graph`, not a separate structure

**Decision**: Add `NodeKind::Paragraph` to the existing `StableDiGraph` alongside notes, tasks, and directories.

**Rationale**: Reuses Obsidian shortest-path link resolution, `NoteId` identity, and all existing traversal API (`incoming`, `outgoing`, `nodes`). The journal query becomes `graph.incoming(note_id).filter(ParagraphLink)` — no separate scan. `NoteId` is already used for all node types; petgraph doesn't enforce homogeneity.

**Alternative considered**: A separate `ParagraphGraph` struct wrapping or augmenting `Graph`. Rejected: duplicates traversal code, can't reuse DSL, requires bridging the two identity spaces.

### D2: Paragraph extraction in the existing parallel parse phase

**Decision**: During `Graph::build`, after `extract_links(&content)`, also call `extract_paragraphs(&content)` in the same parallel closure. Paragraph nodes and edges are inserted in the serial phase alongside link edges.

**Rationale**: File content is already in memory during the parse phase. Adding paragraph extraction adds only CPU time (pure string ops), zero extra I/O. Keeping both extractions in one pass avoids a second file-read loop.

**Alternative considered**: A separate post-build pass that re-reads files. Rejected: doubles I/O, complicates `Graph::build`.

### D3: Paragraph identity keyed on `(rel_path, line_start)`

**Decision**: `paragraph_index: HashMap<(PathBuf, u32), NoteId>` on `Graph`. Paragraphs are identified by their owning note's path and the 1-indexed line number of their first line.

**Rationale**: Line start is stable within a single build and directly addresses `git blame` output (which also works in line coordinates). Paragraph index (ordinal position) shifts on insertions above; line start shifts only if lines above are added/removed, but that's acceptable since the cache is invalidated by HEAD hash anyway.

**Alternative considered**: Byte offset as key. Rejected: `git blame` outputs line numbers, so line-start is the natural join key between the graph and the blame cache.

### D4: Lazy BlameCache, not blame-at-build-time

**Decision**: `git blame --porcelain` is run lazily on first journal query for a given file within a session, results stored in `.ft/cache/blame.msgpack` keyed on `(rel_path_string, HEAD_hash)`.

**Rationale**: Running blame on all vault files at build time (~500 files × ~30ms = ~15s) would make every graph build unusable. Journal queries touch only the files that contain matching paragraphs — typically tens of files. Lazy evaluation + persistent cache gives sub-second repeated queries.

**Cache format**: msgpack via `rmp-serde`. Compact binary, fast ser/de, structurally uniform data (arrays of ints and short strings). Single file avoids per-file cache management.

**Invalidation**: Entry is valid if the stored HEAD hash matches current HEAD. On commit (HEAD advances), all entries for modified files become stale and are recomputed on next query. Unmodified files retain valid cache entries.

**Alternative considered**: SQLite. More structured but adds a heavier dependency and per-row overhead for what is effectively a flat key-value store.

### D5: Alias resolution at query time, not graph time

**Decision**: When building the journal for note N, aliases are resolved at query time by: (1) find the `## Related` heading line range in N's content, (2) filter `outgoing(N, EdgeKind::Link)` by `edge.line in related_range`, (3) traverse `incoming(alias_id, EdgeKind::ParagraphLink)` for each resolved alias.

**Rationale**: No new edge kind needed. The Related section's wiki links already appear as `Link` edges in the graph. Filtering by line range at query time costs O(outgoing(N)) and is trivial for typical note link counts. Avoids a special `RelatedAlias` edge kind that would couple the journal feature into every graph build.

**Alternative considered**: A dedicated `EdgeKind::RelatedAlias` edge inserted during build. Rejected: requires scanning for `## Related` heading during build for every note, adds graph complexity for a query-time concern.

### D6: Scoring formula for Feature 2

**Decision**:
- +3 for each paragraph that contains a `ParagraphLink` to both N (or an alias) and concept C
- +1 for each file where some paragraph links to N and a *different* paragraph links to C

**Rationale**: Same-paragraph co-occurrence is a strong signal of conceptual proximity. Same-file, different-paragraph is a weaker contextual signal. The 3:1 ratio reflects this without requiring calibration data. Limiting to same-file (not same-day cross-file) keeps scoring cheap and deterministic.

### D7: TUI modal on the graph tab, not a standalone tab

**Decision**: Feature 2's interactive picker is a modal overlay on the existing graph tab, triggered by a keybinding when a Note node is selected.

**Rationale**: The graph tab already displays note nodes and supports selection. A modal reuses the tab's note-selection context. Adding a new tab for a relatively narrow operation would clutter the tab bar. The pattern matches existing modal overlays (help `?`, rename modal).

## Risks / Trade-offs

**[Risk] `Graph::build` size growth**: A vault with 500 notes × ~20 paragraphs/note = ~10,000 paragraph nodes. petgraph `StableGraph` stores all nodes in a `Vec`; 10k nodes is negligible (~few MB).
→ Mitigation: No action needed for typical vault sizes. If pathological vaults (100k+ paragraphs) emerge, add a `GraphBuildOptions { include_paragraphs: bool }` flag so callers that don't need journals can opt out.

**[Risk] `refresh_note` complexity**: Today `remove_outgoing_edges` handles ghost GC. Paragraphs are exclusively owned by their note, so refresh must also remove paragraph nodes from `g` and `paragraph_index`.
→ Mitigation: New `remove_paragraph_nodes(src: NoteId)` method mirrors the ghost GC pattern. Unit-tested with a refresh-after-edit fixture.

**[Risk] `Graph::build` signature is widely called**: CLAUDE.md flags this. The paragraph extraction doesn't require a signature change — it runs inside the existing parse phase. `Graph::build(vault, scan)` signature is unchanged.
→ Mitigation: Verified by design: no new parameter needed.

**[Risk] Blame subprocess latency on first query**: If 50 files match on the first journal query and the blame cache is cold, 50 × ~30ms = ~1.5s.
→ Mitigation: Acceptable for a non-interactive CLI command. Show a progress indicator if running in TTY. Cache eliminates latency on subsequent runs.

**[Risk] msgpack cache format evolution**: Adding fields to the cached struct requires a migration or cache invalidation strategy.
→ Mitigation: Cache entries are invalidated by HEAD hash anyway. Format changes just mean a cold cache on next run — not data loss.

## Migration Plan

No persistent state migration needed. The `.ft/cache/` directory is created on first use. The blame cache is self-healing: stale or missing entries are recomputed transparently.

The graph changes are purely additive (new `NodeKind`/`EdgeKind` variants). All existing match arms on `NodeKind`/`EdgeKind` in the codebase will produce exhaustiveness warnings at compile time if not updated — these are caught by `cargo build` before any release.

## Open Questions

- None. All design decisions are resolved.

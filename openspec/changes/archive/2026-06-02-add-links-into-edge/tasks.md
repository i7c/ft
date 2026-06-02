## 1. Core: EdgeKind variant and graph insertion

- [x] 1.1 Add `EdgeKind::LinksInto` unit variant to the enum in `ft-core/src/graph/mod.rs`
- [x] 1.2 Add `insert_links_into_edges(&mut self)` private method to `Graph`: iterates all Note nodes, collects (source, target_note.parent_dir) pairs from existing Link/Embed outgoing edges, deduplicates per source with a `HashSet<NoteId>`, inserts one `LinksInto` edge per unique pair
- [x] 1.3 Call `insert_links_into_edges()` in `Graph::build()` after `insert_hastask_edges()` (after all link edges are inserted)
- [x] 1.4 Add `insert_links_into_for(&mut self, src: NoteId)` private method mirroring the single-source logic for `refresh_note`
- [x] 1.5 Call `insert_links_into_for(src)` in `refresh_note` after `insert_edges_for(src, ...)` — `remove_outgoing_edges` already clears old `LinksInto` edges

## 2. Query DSL registration

- [x] 2.1 Add `EdgeKind::LinksInto => "links-into"` arm to `edge_kind_str()` in `ft-core/src/graph/query.rs`
- [x] 2.2 Add `"links-into"` to the allowed edge-kind set in `check_kind_values()` (Subject::Edge array)
- [x] 2.3 Add a parse round-trip test for the new edge kind in the query parser tests

## 3. CLI output and presets

- [x] 3.1 Add `EdgeKind::LinksInto => "links-into"` arm to `edge_kind_label()` in `ft/src/output/graph.rs`
- [x] 3.2 Add `"crosslinks"` built-in preset to `builtin()` in `ft-core/src/graph/preset.rs` with query: `node where kind = Directory and path = ""; expand where edge.kind in {directory-contains, links-into};`
- [x] 3.3 Add `"crosslinks"` to `builtin_names()` in sorted position

## 4. Core graph tests

- [x] 4.1 Test: note linking to a note in a subdirectory produces a LinksInto edge to that directory
- [x] 4.2 Test: note linking to a root-level note produces a LinksInto edge to the root Directory node
- [x] 4.3 Test: embed link produces a LinksInto edge (e.g., `![[image.png]]` in a subdirectory)
- [x] 4.4 Test: multiple links from one note to notes in the same folder produce exactly one LinksInto edge (deduplication)
- [x] 4.5 Test: links to notes in different folders produce separate LinksInto edges
- [x] 4.6 Test: unresolved (ghost) links produce no LinksInto edges
- [x] 4.7 Test: mix of resolved and unresolved links — resolved produces LinksInto, ghost does not
- [x] 4.8 Test: note linking to a sibling in its own folder still produces a LinksInto edge
- [x] 4.9 Test: `refresh_note` recomputes LinksInto edges correctly (add new, remove stale)

## 5. Integration and build validation

- [x] 5.1 Run `cargo build --release` — confirm compile
- [x] 5.2 Run `cargo test --workspace` — all tests pass
- [x] 5.3 Run `cargo clippy --workspace --tests -- -D warnings` — clean
- [x] 5.4 Run `cargo fmt --check` — clean

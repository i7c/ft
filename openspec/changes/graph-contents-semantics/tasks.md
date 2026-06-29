## 1. Semantics doc + proposal (done)

- [x] 1.1 Write `docs/graph-semantics.md` (canonical prose reference) — committed `23ddaaf`
- [x] 1.2 Create openspec change `graph-contents-semantics` with proposal, design, spec deltas (`graph-model` new; `links-into-edge`, `notes-journal`, `related-updater`, `link-review`, `synth-notes`, `graph-rename-directory` modified)

## 2. Heading extraction + Heading node kind (ft-core)

- [ ] 2.1 Add `NodeKind::Heading(HeadingData)` and `HeadingData { source_file: PathBuf, line: u32, level: u8, text: String }` to `ft-core/src/graph/mod.rs`; derive `PartialEq, Eq, Clone, Debug`
- [ ] 2.2 Add `NodeKey::Heading(PathBuf, u32)` to `NodeKey`; implement `stable_key` + `id_for_key` arms for it
- [ ] 2.3 Add `heading_index: HashMap<(PathBuf, u32), NoteId>` to `Graph`; add `heading_by_loc(&self, path: &Path, line: u32) -> Option<NoteId>` lookup
- [ ] 2.4 Add `EdgeKind::OwnsHeading` variant; update all exhaustive `match` arms across `ft-core` and `ft` (`cargo build` will surface them)
- [ ] 2.5 Extend `Graph::build`'s parallel parse phase to also call `markdown::extract_headings(&content)` (already `LineSkipState`-aware), collecting `(rel_path, headings)` tuples alongside links/paragraphs
- [ ] 2.6 In the serial phase of `Graph::build`, insert heading nodes + `OwnsHeading` edges via the heading-stack algorithm (D6): process headings in document order; for a heading at level `L`, pop every heading with level `>= L`, set parent to new top-of-stack or the note, add `OwnsHeading`, push. Populate `heading_index`
- [ ] 2.7 Write graph unit tests for heading nodes: note with `# A\n## B\n### C` yields `OwnsHeading` edges note→A, A→B, B→C; sibling `## B\n## C` both owned by `A`; heading-in-fenced-code not a node; `heading_by_loc` round-trip; `stable_key`/`id_for_key` round-trip across rebuild

## 3. Nearest-container OwnsParagraph (ft-core)

- [ ] 3.1 Change `insert_paragraph_nodes_for` to assign each paragraph to its nearest container: parent = heading on top of the heading stack at the paragraph's start line, or the note if the stack is empty. Add `OwnsParagraph(parent -> paragraph)`
- [ ] 3.2 Enforce the heading-paragraph ordering invariant (D6): within one note, process headings and paragraphs in ascending `line` order; when a heading and a paragraph share a start line (Fork A2), process the heading first so the paragraph is owned by its own heading
- [ ] 3.3 Add `Graph::note_paragraphs(note_id) -> Vec<NoteId>` (recursive `OwnsHeading` walk ∪ direct `OwnsParagraph` children) and `note_headings(note_id) -> Vec<NoteId>` (direct `OwnsHeading` children) and `all_headings(note_id) -> Vec<NoteId>` (full subtree)
- [ ] 3.4 Update `Graph::refresh_note`: remove the note's heading nodes (and their `OwnsHeading`/`OwnsParagraph`/outgoing link edges) and paragraph nodes, re-insert headings + paragraphs via the same stack/nearest-container rules, purge `heading_index`/`paragraph_index`
- [ ] 3.5 Write unit tests: intro paragraph before first heading is note-owned; paragraph under `# A` is A-owned; paragraph under `# A\n## B` is B-owned; exclusive ownership (exactly one incoming `OwnsParagraph` per paragraph); `note_paragraphs` recurses through nested headings; `note_headings` returns only direct children; `refresh_note` updates heading count and cleans stale `heading_index`

## 4. DSL: Heading kind + owns-heading edge (ft-core, ft)

- [ ] 4.1 Add `Heading` to `node_kind_str` / kind-value validation table in `ft-core/src/graph/query.rs`; add `owns-heading` to the edge-kind value table; update `child_sort_key` with a `Heading` rank (between Directory and Note, per design)
- [ ] 4.2 Add heading `title` support to `node_string_attr` (return `HeadingData.text` for `Attr::Title` on `NodeKind::Heading`); keep `path` returning `None` for Heading (Non-Goal: no `source_file` via `path`)
- [ ] 4.3 Update DSL parse/eval tests: `node where kind = Heading` selects headings; `node where kind = Heading and title = "X"` filters by heading text; `expand where edge.kind = owns-heading` yields sub-headings; `node where kind = Note; expand where edge.kind in {owns-heading, owns-paragraph}` yields a note's headings + heading-less paragraphs
- [ ] 4.4 Regenerate `insta` snapshots in `ft-core/src/graph/tests.rs` and `ft-core/src/graph/query.rs` tests for the new heading-node counts and `owns-heading` edges
- [ ] 4.5 Update `ft/src/output/graph.rs` and `ft/src/tui/tabs/graph.rs` to render `Heading` nodes (label = heading text or `#`-prefixed level); regenerate TUI `TestBackend` snapshots in `ft/src/tui/tests.rs`

## 5. Migrate consumers to note_paragraphs (ft-core, no link changes yet)

- [ ] 5.1 `ft-core/src/journal.rs`: replace `graph.outgoing(note).filter(OwnsParagraph)` paragraph enumeration with `Graph::note_paragraphs`; keep `ParagraphLink` (still wiki-only/data-less at this phase) matching for now
- [ ] 5.2 `ft-core/src/related.rs`: replace the same-file cross-paragraph paragraph enumeration (`outgoing(owner).filter(OwnsParagraph)`) with `Graph::note_paragraphs`
- [ ] 5.3 `ft-core/src/link_review.rs`: replace `graph.outgoing(note_id).filter(OwnsParagraph)` with `Graph::note_paragraphs`
- [ ] 5.4 Run `cargo test --workspace`; fix any consumer tests that assumed flat note→paragraph ownership. Regenerate affected `insta` snapshots (journal/related outputs unchanged in *content* at this phase — only the traversal path changed — but verify)

## 6. Unified link kinds + LinkEdge.is_embed (ft-core)

- [ ] 6.1 Add `is_embed: bool` field to `LinkEdge`; update `EdgeKind::link()` helper and all `LinkEdge` construction sites
- [ ] 6.2 Replace `EdgeKind::Link(LinkEdge)` and `EdgeKind::Embed(LinkEdge)` with `EdgeKind::NoteLink(LinkEdge)`, `EdgeKind::HeadingLink(LinkEdge)`, `EdgeKind::ParagraphLink(LinkEdge)`; remove the old data-less `ParagraphLink` variant. Update all exhaustive `match` arms (`cargo build` surfaces them)
- [ ] 6.3 Update `ft-core/src/graph/parser.rs` `RawLink`: it already carries `is_embed`; ensure `extract_links` output is sufficient for the build to route each occurrence to its container (heading-line vs paragraph-body). Add a `container_line: usize` (or equivalent) to `RawLink` if needed, OR derive the container in the build phase from the heading/paragraph line ranges
- [ ] 6.4 In `Graph::build` serial phase, insert `NoteLink` edges (step 3, per D4) — one per occurrence, targeting note/ghost (anchor ignored for target, D5), carrying full `LinkEdge` with `is_embed`
- [ ] 6.5 In step 5, insert `HeadingLink` edges for occurrences whose line is a heading line, and `ParagraphLink` edges for occurrences whose line falls in a paragraph (including heading lines, per Fork A2 overlap). Both carry full `LinkEdge`. Both support wiki + md forms
- [ ] 6.6 Update `Graph::refresh_note` to remove and re-insert all three link kinds for the refreshed note
- [ ] 6.7 Update ghost GC in `remove_outgoing_edges` / `remove_paragraph_nodes` / heading removal to consider all three link levels (a ghost kept alive by any of NoteLink/HeadingLink/ParagraphLink is not collected)
- [ ] 6.8 Write unit tests: link in paragraph body → NoteLink + ParagraphLink (same `LinkEdge`); link on heading line → NoteLink + HeadingLink + ParagraphLink (all three); `![[Foo]]` → `is_embed = true` on all applicable edges; markdown link `[Foo](foo.md)` in paragraph → ParagraphLink with `form = MdLink`; no `Embed` variant exists; ghost GC across levels

## 7. Anchor resolution → heading targets (ft-core)

- [ ] 7.1 Add `ft-core/src/graph/resolve.rs::resolve_anchor(graph, note_id, anchor_text) -> Option<NoteId>` that finds a heading node owned (transitively) by `note_id` whose normalized `text` equals `anchor_text` (case-insensitive, trim, collapse whitespace, strip trailing `#`s)
- [ ] 7.2 In the `HeadingLink`/`ParagraphLink` insertion (build step 5 + refresh), resolve anchors: if the note target resolves and `resolve_anchor` hits, the edge targets the heading node; else targets the note (or ghost), with `anchor` retained on the `LinkEdge`
- [ ] 7.3 `NoteLink` edges keep targeting note/ghost only (D5), retaining `anchor` as metadata
- [ ] 7.4 Add `Graph::mentions_of(note_id) -> impl Iterator<Item = (NoteId, &LinkEdge)>` yielding incoming NoteLink/HeadingLink/ParagraphLink edges targeting the note OR any of its transitively-owned headings
- [ ] 7.5 Write unit tests: `[[Foo#Bar]]` with heading `Bar` → ParagraphLink targets heading; `[[Foo#Nope]]` → targets note with `anchor = Some("Nope")`; `[[Missing#Bar]]` → targets ghost keyed by `Missing`; `NoteLink` for `[[Foo#Bar]]` targets note (not heading); `mentions_of(foo)` yields both direct-note and heading-targeted edges; heading-match normalization (case, whitespace, trailing `#`s)

## 8. Migrate consumers to unified links + mentions_of (ft-core)

- [ ] 8.1 `ft-core/src/journal.rs`: switch matching from `incoming(target).filter(ParagraphLink)` to `Graph::mentions_of(target)`; collect matched paragraphs from the source-side of those edges. Markdown-form links now count (intended behavior change, D9). Update `matched`-subset computation for multi-target mode to use `mentions_of`
- [ ] 8.2 `ft-core/src/related.rs`: switch matching to `mentions_of(N)` (and per-alias); co-occurrence enumeration uses outgoing `ParagraphLink` (now full-data, wiki+md). Markdown links now contribute to scores (D9). Update alias-set computation to read outgoing `NoteLink` (not the removed `Link`) within the Related section line range
- [ ] 8.3 `ft-core/src/link_review.rs`: the diff→paragraph mapping walks `note_paragraphs`; per-paragraph target enumeration uses outgoing `ParagraphLink` (now includes md links). Callout-skip by line range unchanged
- [ ] 8.4 `ft-core/src/graph/rename.rs`: update `plan_rename` to iterate incoming `NoteLink` edges (per-occurrence, full `byte_range`) for byte-precise rewrites; update the ghost/paragraph/heading skip arms in the linker-node match; verify embed (`is_embed`) links rewrite identically
- [ ] 8.5 Verify `ft-core/src/synth/scaffold.rs` is unaffected: `section_text` derives from `JournalEntry.section_text` ← `ParagraphData.text` (unchanged by Fork A2). Add a regression test that a paragraph beginning at a heading line scaffolds with the heading line included
- [ ] 8.6 Run `cargo test --workspace`; update consumer unit/integration tests for the markdown-link inclusion behavior change. Regenerate `insta` snapshots for journal/related/link-review outputs (feeds grow, scores rise — expected)

## 9. DSL: restructured link edge kinds + edge.embed (ft-core, ft)

- [ ] 9.1 Update the edge-kind value table in `ft-core/src/graph/query.rs`: remove `link` and `embed`; add `note-link`, `heading-link`, `paragraph-link`; keep `directory-contains`, `has-task`, `subtask`, `links-into`, `owns-paragraph`, `owns-heading`
- [ ] 9.2 Add `edge.embed` boolean predicate support (`true`/`false`) to the DSL — new `Attr::Embed` or equivalent; wire into `eval_cond_on_edge` checking `LinkEdge.is_embed`
- [ ] 9.3 Update `edge_form_str` / `edge_kind_str` and the kind-values validation error messages to list the new allowed set
- [ ] 9.4 Update DSL parse/eval tests: `edge.kind = note-link` / `heading-link` / `paragraph-link` work; `edge.embed = true` follows only embed edges; old `edge.kind = link` and `edge.kind = embed` fail with `UnknownKindValue` listing the allowed set
- [ ] 9.5 Regenerate `insta` snapshots in `ft-core/src/graph/query.rs` tests and `ft-core/src/graph/tests.rs` for the restructured edge kinds
- [ ] 9.6 Update `ft/src/output/graph.rs` edge-kind rendering (Tree/Edges/Markdown labels) for the new edge-kind strings; regenerate CLI snapshots in `ft/tests/` if any graph-query snapshots exist

## 10. Preset + docs migration (ft, docs)

- [ ] 10.1 Audit built-in graph presets (`ft-core/src/graph/preset.rs::builtin`) and task presets (`ft-core/src/query/preset.rs`) for `edge.kind = link` / `edge.kind = embed` usage; rewrite to the new value set + `edge.embed`
- [ ] 10.2 Audit fixture-vault / test presets for the same; rewrite
- [ ] 10.3 Update `docs/graph-query-dsl.md` kind-values tables: add `Heading` node kind; replace `link`/`embed` with `note-link`/`heading-link`/`paragraph-link`; document `edge.embed`
- [ ] 10.4 Add a migration note (new `docs/graph-dsl-migration.md` or a section in `docs/migrating-task-queries.md`) showing `edge.kind = link` → `edge.kind in {note-link, heading-link, paragraph-link}` and `edge.kind = embed` → `edge.embed = true`
- [ ] 10.5 Update `docs/architecture.md` "Graph query DSL" / "Profiles and the unified DSL" sections to cross-reference `docs/graph-semantics.md` as the canonical model reference
- [ ] 10.6 Update `docs/graph-semantics.md` if any decision was refined during implementation (keep it the source of truth)

## 11. Build invariants + final verification

- [ ] 11.1 `cargo build --release` — fix all exhaustiveness warnings from the `NodeKind`/`EdgeKind` changes
- [ ] 11.2 `cargo test --workspace` — green; all regenerated snapshots committed
- [ ] 11.3 `cargo clippy --workspace --tests -- -D warnings` — clean
- [ ] 11.4 `cargo fmt --check` — clean (apply `cargo fmt` if needed)
- [ ] 11.5 `cargo run --release -q -- commands docs --check` — keybindings doc in sync (no `CommandDef` changes expected; if any, regenerate `docs/keybindings.md`)
- [ ] 11.6 `rg "EdgeKind::Link|EdgeKind::Embed|edge.kind = link|edge.kind = embed" ft-core/src ft/src` — no residual references to the removed kinds
- [ ] 11.7 `rg "outgoing\(.*\)\.filter.*OwnsParagraph|outgoing\(.*\)\.filter.*EdgeKind::Link"` — no residual flat-ownership / old-link-kind consumer patterns
- [ ] 11.8 Real-vault gated check: `FT_REAL_VAULT_TESTS=1 cargo test --workspace` (run manually if the real vault is available; not required for green)
- [ ] 11.9 Archive the change via the `openspec-archive-change` skill once all tasks are complete and the `paragraph-graph` capability spec is retired in favor of `graph-model`

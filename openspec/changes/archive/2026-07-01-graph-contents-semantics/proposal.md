## Why

The graph models the *contents* of notes today, but inconsistently: links are recorded twice (note-level `Link`/`Embed` with full data, paragraph-level `ParagraphLink` wiki-only and data-less), paragraphs are owned flat (every paragraph hangs directly off its note), headings are entirely unmodeled (parsed for `move-sections` and `## Related` alias resolution but absent from the graph), and `LinkEdge.anchor` is parsed but never consulted. This caps the leverage graph algorithms can provide: "incoming links to this heading," "all paragraphs under this section," and "did this paragraph's markdown link count?" are all unanswerable.

We need clean, internally-consistent contents-graph semantics so that consumers (journal, related-updater, link review, synth, rename, the DSL, the TUI) all read the same model and new graph algorithms compose naturally. `docs/graph-semantics.md` is the canonical prose reference; this change formalizes and implements it.

## What Changes

- **Add `Heading` node kind.** Every ATX heading becomes a `NodeKind::Heading(HeadingData { source_file, line, level, text })` node, identified by `(source_file, line)`. The heading line remains part of the paragraph that begins at that line (Fork A2 — dual-owned content), so existing consumers of `ParagraphData.text` are unaffected.
- **Model heading/paragraph hierarchy via exclusive containment.** New `OwnsHeading` edge (`note→heading` and `heading→heading`, heading-stack algorithm). `OwnsParagraph` becomes nearest-container: a paragraph under a heading is owned by that heading, else by the note (Fork B-exclusive). New `Graph::note_paragraphs` / `note_headings` / `mentions_of` helpers centralize the traversals every consumer needs.
- **Unify link semantics across three levels.** Replace note-level `Link`/`Embed` and the data-less `ParagraphLink` with three full-payload link kinds — `NoteLink`, `HeadingLink`, `ParagraphLink` — each carrying the shared `LinkEdge` (form, is_embed, byte_range, line, raw_text, target_text, anchor, display). **All three support wiki and markdown forms** (today `ParagraphLink` is wiki-only — a behavior change for journal/related, which now count `[Foo](foo.md)` co-occurrences). Embed-ness moves from a separate `EdgeKind::Embed` variant to `LinkEdge.is_embed` data.
- **Implement anchors.** A link with a resolvable anchor (`[[Foo#Bar]]` where `Bar` is a heading in `Foo`) targets the **heading node**; an anchor that doesn't resolve (or no anchor) targets the note; an unresolved note still points at its ghost (anchor dangles as metadata). Ghosts remain keyed by the note target.
- **Update every graph consumer** to the new semantics. Rename rewrites via per-occurrence `LinkEdge.byte_range` (now available at all three levels). Journal and related-updater traverse via `note_paragraphs` + `mentions_of` and now include markdown links. Link review's diff→paragraph mapping gains heading nodes (callout-skip unchanged). Synth is unaffected (`section_text` derives from `ParagraphData.text`, preserved by A2). The DSL and TUI gain the `Heading` kind and restructured link edge kinds.
- **Regenerate the `paragraph-graph` capability spec** into a broader `graph-model` capability spec covering notes + headings + paragraphs + the unified link kinds + anchors. **BREAKING** to the DSL `edge.kind` value set: `link`/`embed` → `note-link`/`heading-link`/`paragraph-link` + `edge.embed` boolean predicate.

## Capabilities

### New Capabilities
- `graph-model`: The contents graph model — node kinds (Note, Heading, Paragraph, Task, Ghost, Directory), the two edge families (exclusive containment: Contains/OwnsHeading/OwnsParagraph/HasTask/Subtask/LinksInto; duplicated reference: NoteLink/HeadingLink/ParagraphLink with shared `LinkEdge` payload and resolution), identity/stable keys, build/refresh invariants, anchor resolution. Supersedes `paragraph-graph`.

### Modified Capabilities
- `links-into-edge`: `LinksInto` is now derived from the unified link kinds (NoteLink/HeadingLink/ParagraphLink) rather than `Link`/`Embed`. The derivation rule (one edge per unique source-note → target-note's-parent-directory pair, resolved targets only, deduped, survives refresh) is unchanged; only the source-edge kind set changes.
- `notes-journal`: Matching now uses the unified `ParagraphLink` (all forms, full payload) plus `mentions_of` (note ∪ its headings) for anchored links. Markdown-form links now count as mentions. `section_text` semantics unchanged (A2). Adds the `note_paragraphs` traversal dependency.
- `related-updater`: `score_related` co-occurrence now uses the unified `ParagraphLink` (all forms) and `note_paragraphs` traversal. Markdown links now contribute to co-occurrence scoring. Scoring weights (+3 same-paragraph, +1 same-file cross-paragraph) unchanged.
- `link-review`: Paragraph walk uses `note_paragraphs`; `ParagraphLink` carries data. Adds heading-node awareness (headings don't directly affect the diff→paragraph mapping but are now in the graph). Callout-skip behavior unchanged.
- `synth-notes`: No requirement changes — `section_text` derives from `ParagraphData.text`, preserved by Fork A2. Listed because the implementation touches the journal→scaffold data path; verify no behavioral drift.
- `graph-rename-directory`: Directory rename's external-reference walk now iterates the unified link kinds (`NoteLink`, and — if rename supports heading/paragraph-sited edits in future — `HeadingLink`/`ParagraphLink`). The byte-precise edit mechanism is unchanged; only the source edge kind set changes.

## Impact

**Code (ft-core):**
- `ft-core/src/graph/mod.rs` — `NodeKind::Heading`, `EdgeKind::{OwnsHeading, NoteLink, HeadingLink, ParagraphLink}` (renamed/restructured), `LinkEdge` gains `is_embed`; `heading_index` side table; `NodeKey::Heading`; build phase reordering (notes → dirs → note links → headings → paragraphs + heading/paragraph links → tasks → LinksInto); `refresh_note` removes/reinserts headings + all link kinds; ghost GC across all three levels; new `note_paragraphs`/`note_headings`/`mentions_of` helpers; `heading_by_loc` lookup.
- `ft-core/src/graph/parser.rs` — `extract_links` tags each `RawLink` with its container (heading-line vs paragraph-body) so the build emits the right edge kind per occurrence. Embed detection recorded on the link, not as a separate edge decision.
- `ft-core/src/graph/resolve.rs` — unchanged resolution rules; new anchor→heading resolution (`resolve_anchor(note, anchor_text) -> Option<NoteId>` heading target).
- `ft-core/src/graph/rename.rs` — iterate `NoteLink` (per-occurrence, full `byte_range`) for rewrites; update ghost/paragraph/heading skip arms.
- `ft-core/src/graph/query.rs` — `node_kind_str` gains `Heading`; `edge_kind_str` set restructured; `node_string_attr` gains heading `title` (heading text); `child_sort_key` gains a Heading rank; update kind-values validation tables and snapshots.
- `ft-core/src/journal.rs`, `ft-core/src/related.rs`, `ft-core/src/link_review.rs` — switch to `note_paragraphs`/`mentions_of`; accept markdown links via the unified `ParagraphLink`.
- `ft-core/src/markdown.rs` — `extract_paragraphs` unchanged (A2); `extract_headings` already provides heading data. No new extraction needed.

**Code (ft binary):**
- `ft/src/output/graph.rs`, `ft/src/tui/tabs/graph.rs` — render `Heading` nodes; restructured edge-kind rendering; `TestBackend` snapshots regenerated.
- `ft/src/cmd/graph.rs`, `ft/src/cmd/review.rs`, `ft/src/cmd/notes.rs` — pass-through; verify no edge-kind string literals break.

**Docs:**
- `docs/graph-semantics.md` — already written (committed `23ddaaf`); the canonical reference.
- `docs/graph-query-dsl.md` — update kind-values tables (add `Heading`; `link`/`embed` → `note-link`/`heading-link`/`paragraph-link` + `edge.embed`).
- `docs/architecture.md` — "Graph query DSL" / "Profiles and the unified DSL" sections cross-reference `graph-semantics.md`.

**Tests / snapshots:**
- `insta` snapshots in `graph/tests.rs`, `graph/query.rs` tests, TUI `TestBackend` frames (`tui/tests.rs`), CLI graph output — all regenerate because node counts change (heading nodes added) and journal/related outputs change (markdown links now count).
- `proptest` round-trips in `markdown.rs` — paragraph boundaries unchanged (A2); verify still green.
- The five build invariants (AGENTS.md): `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `ft commands docs --check`. Keybindings unaffected (no `CommandDef` changes); `docs/keybindings.md` regen not required.

**DSL migration (BREAKING):** Presets/queries using `edge.kind = link` or `edge.kind = embed` require rewrite to `edge.kind in {note-link, heading-link, paragraph-link}` and `edge.embed = true`. A migration note lands in `docs/migrating-task-queries.md` or a new `docs/graph-dsl-migration.md`.

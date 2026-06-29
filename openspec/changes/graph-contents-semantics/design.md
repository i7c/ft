## Context

`ft` maintains an in-memory heterogeneous `Graph` (petgraph `StableDiGraph<NodeKind, EdgeKind>`) built on every `vault.scan()` call. Today it carries six node kinds (`Note`, `Heading`-absent, `Paragraph`, `Task`, `Ghost`, `Directory` — actually five; `Heading` does not exist yet) and eight edge kinds (`Link`, `Embed`, `Contains`, `HasTask`, `Subtask`, `LinksInto`, `OwnsParagraph`, `ParagraphLink`).

The contents-model has accreted inconsistencies (detailed in `docs/graph-semantics.md` and the proposal). The canonical target semantics are now fixed in `docs/graph-semantics.md` (committed `23ddaaf`); this design records *how* to implement them. Two forks were decided in conversation:

- **Fork A = A2.** The heading line belongs to both the heading node (structural) and the paragraph that begins at that line (textual). `ParagraphData.text` is unchanged, preserving every consumer of paragraph text (journal `section_text`, synth callouts, related scoring).
- **Fork B = exclusive nearest-container ownership.** A paragraph under a heading is owned by that heading; a heading-less paragraph (intro before first heading, or any paragraph outside a heading's section) is owned by the note. Not flat-with-added-hierarchy.

Three consumers leverage the graph as a contents model and are the primary blast radius: `journal.rs`, `related.rs`, `link_review.rs`. Two more touch the link edge kinds directly: `rename.rs` (byte-precise edits via `LinkEdge.byte_range`), `synth::scaffold` (consumes `JournalEntry.section_text`, indirectly affected). The DSL (`graph::query`) and the TUI graph tab render every node/edge kind.

Constraints: the plan/apply split for mutations must hold (`write_atomic`, descending-byte-order same-file edits). The unified query DSL with profiles must keep its grammar and `parse(format(q)) == q` canonical round-trip. The five build invariants (AGENTS.md) must stay green. `FT_TODAY` is the seam for "today." Signature changes to widely-called functions (e.g. `Graph::build`) ripple through every test file — minimize them.

Stakeholders: the user (vault owner), who expects journal/related outputs to become *more* complete (markdown links now count) but not to silently reorder in a way that breaks muscle memory; future graph algorithms, which need a consistent model to compose against.

## Goals / Non-Goals

**Goals:**
- First-class `Heading` nodes with `OwnsHeading`/`OwnsParagraph` nearest-container topology.
- Three unified link kinds (`NoteLink`, `HeadingLink`, `ParagraphLink`) sharing one `LinkEdge` payload (with `is_embed`) and identical resolution (wiki + markdown forms, shortest-path rules, ghost keying).
- Anchor resolution: resolvable `[[Foo#Bar]]` targets the heading node `H(Bar)`.
- Every graph consumer migrated to the new model with behavior changes made explicit (markdown-link inclusion in journal/related).
- DSL + TUI render the new model; snapshots regenerated; `paragraph-graph` capability spec superseded by `graph-model`.
- All five build invariants green.

**Non-Goals:**
- Folding `Task` nodes into the heading/paragraph containment tree (`heading → task`). Deferred — interacts with the emoji task model.
- Unifying `notes::extract_sections` (heading-delimited sections, used by `move-sections` / `plan_related_update`) with the heading-node model. Deferred — separate structural concept today; one source of truth is desirable but out of scope.
- Exposing `source_file` via the `path` attribute on `Heading`/`Paragraph` for DSL convenience. Deferred — would change `path includes "…"` semantics for paragraphs.
- Reference-style markdown links (`[text][ref]` + `[ref]: url`). Still out of scope.
- Setext headings (`===`/`---` underlines). Still out of scope.
- Persistent graph index / incremental cross-file refresh. Graph is still rebuilt per session; `refresh_note` is single-file.
- Bare-title (non-wikilink) mention matching. Still out of scope.

## Decisions

### D1: Store all three link levels physically (not derived)

**Decision.** The graph stores `NoteLink`, `HeadingLink`, and `ParagraphLink` edges physically — all three, each per-occurrence, each carrying the full `LinkEdge`. No on-demand derivation.

**Rationale.** The semantics contract (`NoteLink == HeadingLink ⊎ ParagraphLink` as occurrence multisets, per `docs/graph-semantics.md` §"Overlap and the derivation contract") permits derivation, but storing all three is simplest for the consumers: `rename` needs per-occurrence `byte_range` at the note level today and benefits from not recomputing; `journal`/`related`/`link_review` need per-occurrence data at the paragraph level; "incoming links to this heading" (the anchor payoff) needs heading-level edges. Physical storage makes each consumer a direct graph traversal with no derivation step, and the cost is bounded (link count is linear in document size, already the case). Memory is not a measured constraint — the graph is rebuilt per session and fits in memory for realistic vaults.

**Alternatives considered.**
- *Store only `NoteLink` per-occurrence, derive heading/paragraph by routing through the line→container map at query time.* Rejected: `rename`'s rewrite and the anchor→heading query both want the level explicit; derivation-on-read complicates every consumer and re-introduces a "which container does line L belong to" lookup that the build phase already did once.
- *Store only the most specific (heading/paragraph), derive `NoteLink`.* Rejected: `rename` is the hottest writer-path and wants note-level per-occurrence edges directly; deriving would require iterating heading+paragraph edges and deduping, which is exactly the pre-refactor inconsistency we are removing.

**Note on rule 2.** Guiding rule 2 permits derivation; this decision chooses not to exercise that permission. The semantics remain *as if* derivable (the contract holds), which is what rule 2 requires.

### D2: Embed-ness as `LinkEdge.is_embed`, three link kinds (encoding (a))

**Decision.** `EdgeKind` gains three variants — `NoteLink(LinkEdge)`, `HeadingLink(LinkEdge)`, `ParagraphLink(LinkEdge)` — and loses `Link` and `Embed`. `LinkEdge` gains `is_embed: bool`. There is no separate embed edge kind.

**Rationale.** Fewer variants (three, not six); uniform embed treatment across levels; a clean `edge.embed` DSL predicate. Embed-ness is a property of the link *occurrence* (`![[Foo]]` vs `[[Foo]]`), not of the topology — encoding it as data matches the conceptual model.

**Alternatives considered.**
- *Six variants (`NoteLink`/`NoteEmbed`/`HeadingLink`/`HeadingEmbed`/`ParagraphLink`/`ParagraphEmbed`).* Rejected: doubles the variant count, every match arm doubles, and embed-ness is orthogonal to the container level.

**Breaking consequence.** The DSL `edge.kind` value set changes: `link`/`embed` → `note-link`/`heading-link`/`paragraph-link`; a new `edge.embed` boolean predicate replaces `edge.kind = embed`. Presets/queries using the old values require migration (see Migration Plan). This is the only breaking DSL change.

### D3: Anchor resolution targets the heading node

**Decision.** During the build phase, after heading nodes are inserted, each link occurrence with `anchor = Some(a)` is resolved as: if the note target resolves *and* a heading in that note matches `a` (case-insensitive, whitespace-and-trailing-`#`-insensitive, per `markdown::Heading.text`), the edge targets that heading node; otherwise the edge targets the note (or ghost if the note is unresolved), with `anchor` retained as metadata. A new `Graph::mentions_of(note_id)` helper yields all incoming link edges targeting the note *or any of its headings*, at all three levels — this is what `journal`/`related` use to preserve "any `[[Foo…]]` mentions note Foo" semantics.

**Rationale.** Makes "incoming links to this heading" a first-class query — the primary analytical payoff of heading nodes. Anchor metadata is preserved when unresolved so no information is lost. `mentions_of` centralizes the note-mention union so consumers don't each re-derive it.

**Alternatives considered.**
- *Anchors as pure metadata (edge always targets the note).* Rejected: wastes the heading-node opportunity; "links to this section" stays unanswerable, which is the main reason to add heading nodes at all.
- *Distinct `AnchorLink` edge kind.* Rejected: anchors are a target-resolution concern, not a separate topology; a heading-targeted `ParagraphLink` already expresses it.

**Heading-match normalization.** Match `anchor` against heading `text` after: lowercasing, trimming, collapsing internal whitespace, stripping trailing `#`s. This matches `extract_headings`'s `text` normalization. Punctuation-in-heading slug rules (Obsidian's `# Heading!` → `#heading`) are *not* fully replicated in v1 — we match on the normalized text, which covers the common case. Documented as a known limitation; a slug-based matcher is a follow-on.

### D4: Build phase ordering — notes → dirs → note links → headings → paragraphs + heading/paragraph links → tasks → LinksInto

**Decision.** The serial resolution phase of `Graph::build` runs in this order:
1. Insert note nodes (populates `path_index`/`title_index`).
2. Insert directory nodes + `Contains` edges.
3. Insert `NoteLink` edges (resolves against the now-full path/title indexes; needs note nodes only).
4. Insert heading nodes + `OwnsHeading` edges (heading-stack algorithm; populates `heading_index`).
5. Insert paragraph nodes + `OwnsParagraph` edges (nearest-container) + `HeadingLink`/`ParagraphLink` edges (resolves anchors against the now-full heading index).
6. Insert task nodes + `HasTask` + `Subtask`.
7. Derive `LinksInto` from the unified link kinds.

**Rationale.** Resolution dependencies: note links need only notes; heading/paragraph links with anchors need headings inserted first; `LinksInto` derivation needs all link edges. Step 3 before step 4 is required so a `NoteLink` whose anchor resolves to a heading that doesn't exist yet is still a note-level edge (note-level edges don't carry anchor-target resolution — they always target the note or ghost; only heading/paragraph links resolve anchors to heading nodes). Wait — see D5 for the note-level anchor rule.

**Correction (D5 interaction).** Note-level `NoteLink` edges represent "a link anywhere in the note." They target the note (or ghost) and do *not* resolve anchors to headings — the note level is note-granular by definition. Anchor→heading resolution applies only to `HeadingLink` and `ParagraphLink` (the container-specific levels). This keeps `NoteLink`'s target identical to today's `Link`/`Embed` target (note or ghost), so `rename` and backlink queries at the note level are unchanged. `mentions_of` then unions note-level (note target) with heading/paragraph-level (possibly heading target) incoming edges.

### D5: `NoteLink` targets note/ghost only; `HeadingLink`/`ParagraphLink` may target a heading

**Decision.** Anchor→heading resolution applies to `HeadingLink` and `ParagraphLink` only. `NoteLink` edges always target the note (resolved) or ghost (unresolved), ignoring any anchor for target purposes (the anchor is still stored on the `LinkEdge` as metadata).

**Rationale.** The note level answers "does this note link to note Foo" — note-granular. Routing a note-level edge to a heading would conflate the levels and break the "NoteLink == all occurrences" contract (a heading-targeted edge is not a note-targeted edge at the note level). `mentions_of` provides the union for consumers that want "any mention of note Foo."

**Consequence.** `incoming(note)` at the `NoteLink` level = today's `incoming(note)` over `Link`/`Embed`. `incoming(heading)` is new and non-empty only for anchored links. `mentions_of(note)` = `incoming(note)` over all three kinds ∪ `incoming(h)` for every heading `h` owned by the note, over `HeadingLink`/`ParagraphLink`.

### D6: Heading-stack algorithm for `OwnsHeading`/`OwnsParagraph`

**Decision.** During step 4/5, maintain a stack of open headings (each with its level). For a heading `H` at level `L`: pop every heading with level `≥ L`; `H`'s parent is the new top-of-stack (or the note if empty); add `OwnsHeading(parent → H)`; push `H`. For a paragraph `P` starting at line `l`: its parent is the heading on top of the stack at line `l` (or the note if empty); add `OwnsParagraph(parent → P)`.

**Ordering invariant.** Within one note, process headings and paragraphs in ascending `line` order; when a heading and a paragraph share a start line (Fork A2 — the heading line begins a paragraph), process the heading first so the paragraph is owned by its own heading.

**Rationale.** Standard section-stack; matches Obsidian fold semantics; O(n) in heading+paragraph count.

### D7: `Graph::refresh_note` removes/reinserts headings + all link kinds

**Decision.** `refresh_note` extends today's pattern: remove the note's heading nodes (and their `OwnsHeading`/`OwnsParagraph`/`HeadingLink` edges), remove its paragraph nodes (and `OwnsParagraph`/`ParagraphLink`), remove outgoing `NoteLink` edges, GC orphaned ghosts across all three levels, then re-extract and re-insert from the file's current content. The heading-stack and nearest-container rules re-run for that file.

**Rationale.** `refresh_note` is the single-file incremental path; it must produce the same result as a full rebuild for that file's subtree. Ghost GC must consider all three link levels so a ghost kept alive only by a heading-link isn't leaked.

### D8: New `Graph` helpers centralize consumer traversals

**Decision.** Add three public methods:
- `note_paragraphs(note_id) -> Vec<NoteId>` — recursive `OwnsHeading` walk ∪ direct `OwnsParagraph` children. Replaces `outgoing(note).filter(OwnsParagraph)` in `journal`/`related`/`link_review`.
- `note_headings(note_id) -> Vec<NoteId>` — direct `OwnsHeading` children (top-level headings of the note). A recursive variant `all_headings(note_id)` returns the full subtree.
- `mentions_of(note_id) -> impl Iterator<Item = (NoteId, &LinkEdge)>` — incoming link edges at all three levels targeting the note *or* any of its (transitively-owned) headings. Replaces the "incoming ParagraphLink to note" walk in `journal`/`related`.

**Rationale.** Exclusive ownership (Fork B) means consumers can no longer do a flat `outgoing(note).filter(OwnsParagraph)`; the helper makes the correct traversal obvious and testable. `mentions_of` makes the anchor→heading union explicit so consumers preserve note-mention semantics without each re-deriving it.

### D9: Markdown-link inclusion in journal/related is an intended behavior change

**Decision.** `journal` and `related` now count markdown-form links (`[Foo](foo.md)`) as mentions/co-occurrences, because the unified `ParagraphLink` includes them. This is desired (the proposal calls it out) but changes outputs: feeds grow, related scores rise, snapshots shift.

**Rationale.** The pre-refactor `ParagraphLink`-wiki-only behavior was an accidental inconsistency (note-level links were wiki+md, paragraph-level were wiki-only). Unifying is the point. Mitigation: snapshot tests are regenerated with the new (correct) expectations; the behavior change is documented in the proposal and the spec deltas.

### D10: `paragraph-graph` → `graph-model` capability spec

**Decision.** Create `openspec/specs/graph-model/spec.md` as the consolidated contents-graph capability spec. The existing `openspec/specs/paragraph-graph/spec.md` requirements are absorbed (some MODIFIED for the new ownership/link semantics, some REMOVED as superseded). `links-into-edge`, `notes-journal`, `related-updater`, `link-review`, `synth-notes`, `graph-rename-directory` get delta files under the change's `specs/` directory.

**Rationale.** `paragraph-graph`'s Purpose is literally "TBD" and its scope (paragraph nodes + OwnsParagraph + ParagraphLink) is too narrow for the unified model. A new `graph-model` capability is the clean home; `paragraph-graph` is retired by removal in this change's delta. (Physical deletion of `openspec/specs/paragraph-graph/` happens at archive time per openspec convention.)

## Risks / Trade-offs

- **[Risk] Markdown-link inclusion changes journal/related outputs, surprising the user.** → Mitigation: documented as intended in proposal + spec deltas; snapshots regenerated with correct expectations; `docs/graph-semantics.md` consumer-contracts table makes the change explicit. No silent reordering beyond "more entries / higher scores."
- **[Risk] Anchor heading-match normalization diverges from Obsidian's slug rules** (punctuation, CJK). → Mitigation: v1 matches normalized `text` (lowercase, trim, collapse whitespace, strip trailing `#`s); covers the common case; documented as a known limitation with a slug-based matcher as a follow-on. Anchor that doesn't match falls back to note target (no data loss).
- **[Risk] Fork B exclusive ownership breaks the flat `outgoing(note).filter(OwnsParagraph)` assumption in a consumer we miss.** → Mitigation: `rg "OwnsParagraph"` and `rg "outgoing\(.*\)"` across `ft-core` and `ft` to enumerate every call site; the `note_paragraphs` helper is the single replacement. The blast-radius inventory in tasks.md is exhaustive.
- **[Risk] `NoteLink`/`HeadingLink`/`ParagraphLink` rename causes exhaustiveness-match breakage across many files.** → Mitigation: `cargo build` surfaces every match arm; this is mechanical. The DSL `edge.kind` value set change is the only semantic break.
- **[Risk] Build-phase reordering introduces a resolution-order bug** (e.g. a `ParagraphLink` anchor resolved before headings exist). → Mitigation: the ordering in D4 is dependency-ordered; step 4 (headings) precedes step 5 (paragraph links with anchors). Unit tests assert anchor resolution against a multi-heading fixture.
- **[Risk] `refresh_note` ghost GC misses a level** (a ghost kept alive only by a `HeadingLink` is leaked or prematurely dropped). → Mitigation: ghost GC iterates all three link levels; unit test with a ghost whose only incoming edge is a `HeadingLink`.
- **[Trade-off] Storing all three link levels physically uses more memory** than deriving one. → Accepted (D1): bounded by link count, not a measured constraint, and simplifies every consumer.
- **[Trade-off] Fork A2 dual-owns the heading line** (heading node + paragraph `text`). → Accepted: pragmatic, preserves `ParagraphData.text` consumers. Documented in `docs/graph-semantics.md`.
- **[Risk] DSL `edge.kind = link`/`embed` migration breaks user presets.** → Mitigation: migration note in `docs/migrating-task-queries.md` (or new `docs/graph-dsl-migration.md`); old presets fail at parse time with `UnknownKindValue` listing the allowed set (existing behavior). No silent degradation.

## Migration Plan

Phased (matches tasks.md ordering). Each phase keeps the five build invariants green before the next begins.

1. **Semantics doc + proposal** — done (`docs/graph-semantics.md` committed `23ddaaf`; this change's proposal/design/specs).
2. **Heading nodes + hierarchy edges** (no link changes yet): `NodeKind::Heading`, `OwnsHeading`, nearest-container `OwnsParagraph`, `heading_index`, `NodeKey::Heading`, `note_paragraphs`/`note_headings` helpers, build/refresh updates. DSL gains `Heading` kind + `owns-heading` edge kind. Migrate `journal`/`related`/`link_review` to `note_paragraphs`. Regenerate snapshots. Invariants green.
3. **Unified link kinds + anchors**: introduce `NoteLink`/`HeadingLink`/`ParagraphLink` (remove `Link`/`Embed`), `LinkEdge.is_embed`, anchor→heading resolution, `mentions_of` helper. Update `rename` to iterate `NoteLink`. Update DSL `edge.kind` value set + `edge.embed`. Markdown-link inclusion lands in journal/related. Regenerate snapshots. Invariants green.
4. **Consumer verification + docs**: confirm `synth` unaffected (A2), update `docs/graph-query-dsl.md` kind tables, cross-reference `docs/architecture.md`, write DSL migration note. Final invariant sweep.

**Rollback.** Each phase is a commit (or small commit set). Rollback is `git revert`. No on-disk format migration (graph is in-memory; no persistent graph store). `BlameCache` (msgpack) is unaffected. The only user-facing artifact is regenerated snapshots and the DSL value-set change.

**DSL migration for users.** `edge.kind = link` → `edge.kind in {note-link, heading-link, paragraph-link}` (or pick the specific level); `edge.kind = embed` → `edge.embed = true` (optionally ANDed with a `note-link`/etc. filter). Documented in a migration note.

## Open Questions

None blocking. The following are resolved-but-recorded or deferred:

- **Slug-based anchor matching** — deferred (D3 known limitation); v1 uses normalized-text match.
- **`Task` in heading tree** — deferred (Non-Goal).
- **Unify `notes::extract_sections` with heading nodes** — deferred (Non-Goal).
- **`path` attribute on Heading/Paragraph** — deferred (Non-Goal); would change `path includes` semantics.

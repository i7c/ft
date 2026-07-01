## Context

`build_journal` (`ft-core/src/journal.rs`) is the single seam both the CLI (`ft notes journal`) and the TUI Journal tab funnel through; the synth scaffold (`ft-core/src/synth/scaffold.rs`) consumes its `JournalEntry` list as-is. Today a paragraph is included only when `Graph::mentions_of(target)` yields it as a source — i.e. the paragraph (or its owning heading/note) has a direct `ParagraphLink`/`HeadingLink`/`NoteLink` into the target. Because a heading line also begins a paragraph (Fork A2), a heading like `## Thoughts about [[Foo]]` contributes exactly one entry: the paragraph starting at the heading line. Sibling paragraphs in the same section that don't repeat the link are dropped.

The graph already models everything we need: `EdgeKind::HeadingLink` (a link written inside heading text), `OwnsParagraph` (nearest container: heading or note → paragraph), `OwnsHeading` (heading-stack hierarchy), and the `Graph::note_paragraphs(node)` traversal (`OwnsParagraph` + `OwnsHeading` descendants) — all landed by the archived `graph-contents-semantics` change. No new graph primitive is required.

Confirmed design constraints (from product discussion):

- **One entry per paragraph**, always. Expansion adds sibling paragraphs as separate entries; it never merges a section into one entry.
- **Per-paragraph dates** are non-negotiable. Each entry's date = `paragraph_date(blame, P.line_start, P.line_end)` (most-recent commit touching that paragraph's own lines). No shared/section date, even for expanded siblings.
- **Recentness dominates ordering.** The feed stays reverse-chronological by paragraph date; preserving document order across paragraphs of *different* dates is explicitly not a goal (corrected from an earlier draft). Document order is only a same-date tiebreak.
- **`matched` inheritance** in multi-target mode: an expansion-only paragraph's `matched` is the subset of targets its owning-chain headings link to (not the empty set).

## Goals / Non-Goals

**Goals:**
- Include every paragraph in a heading's section when that heading has a `HeadingLink` to a journal target.
- Preserve the one-paragraph-per-entry invariant so the synth flow benefits with no separate change.
- Keep per-paragraph dates and the reverse-chronological feed.
- Attribute expanded paragraphs correctly to their target(s) in multi-target `matched`.
- Add a final `line_start` document-order tiebreak so same-date co-located paragraphs read top-to-bottom without ever overriding recentness.

**Non-Goals:**
- No new graph node/edge kinds or traversal primitive (reuse `note_paragraphs` with a heading root).
- No change to the synth scaffold's ordering rule (it already consumes `entries` in feed order; `synth-notes` "Scaffold content sourcing" requirement is untouched).
- No change to the in-window filter, `BlameCache` semantics, `## Related` alias resolution, or self-exclusion.
- No opt-in/out flag — expansion is always-on for both callers.
- Anchored links *targeting* a heading from elsewhere (`[[Foo#Bar]]` in a body) do not trigger expansion; they remain direct matches via `mentions_of`.

## Decisions

### D1 — Expand via `HeadingLink` sources, not via paragraph owning-chain walks

Two ways to find expanded paragraphs:

- **(a) Target-side:** for each target, `mentions_of(target)` already yields `HeadingLink` sources (headings whose link targets the note). For each such heading `H`, expand `note_paragraphs(H)`.
- **(b) Source-side:** for each paragraph in the vault, walk its owning chain up to the note; if any heading in the chain has a `HeadingLink` to a target, include it.

**Decision: (a).** `mentions_of` is already the matching entry point and already returns `HeadingLink` edges; reusing it keeps a single source of truth for "who links this target" and avoids an O(all-paragraphs) scan. The expansion is a second pass over the `HeadingLink` sources it already yields. `note_paragraphs(H)` then enumerates the section in O(section size).

*Alternative considered:* (b) is conceptually symmetric but scans the whole vault per query and duplicates the "is this heading linked to a target?" predicate. Rejected.

### D2 — Collect `HeadingLink` sources explicitly, separate from `Paragraph` sources

`mentions_of(target)` returns `(src, LinkEdge)` pairs where `src` may be a `Paragraph`, `Heading`, or `Note` node. Today the loop keeps only `Paragraph` sources and silently drops `Heading`/`Note` ones. The change:

1. Keep the existing direct-match pass: `Paragraph` sources → candidate paragraphs (unchanged).
2. Add an expansion pass: `Heading` sources (from the same `mentions_of` results, or a dedicated walk) → for each, `note_paragraphs(H)` → candidate paragraphs. A `Heading` source reached via `mentions_of` is exactly a heading whose `HeadingLink` targets the target or one of its headings.

A paragraph that surfaces in both passes is deduplicated by `NoteId` (HashSet), satisfying the "appears once" requirement. When deduping, **the direct match wins** for `matched` attribution (a paragraph with its own `ParagraphLink` to a target is attributed from that direct edge, not inherited) — handled by computing `matched` per-entry from the paragraph's own outgoing edges first, and only falling back to heading-chain inheritance when that set is empty in multi-target mode.

*Note:* `mentions_of` returns `LinkEdge` (the payload), not the `EdgeKind`. To distinguish `HeadingLink` sources from `NoteLink`/`ParagraphLink` sources at the *source* node, we check `graph.node(src)`: a `NodeKind::Heading` source means the link was on a heading line (the build phase emits `HeadingLink` only for heading-line occurrences). This is robust because a heading node's outgoing link edge to a target is, by construction, a `HeadingLink` (plus the Fork-A2 `ParagraphLink` that overlaps the heading line — but that one's source is the paragraph node, not the heading node).

### D3 — `matched` inheritance via the owning chain

For an expansion-only paragraph `P` in multi-target mode (its own outgoing `ParagraphLink` edges yield no target in `targets`):

1. Find `P`'s nearest `OwnsParagraph` container `C` (a heading, else the note).
2. If `C` is a heading, collect the set of targets reachable from `C` and its `OwnsHeading` ancestors up to the note, via their outgoing `HeadingLink` edges (mapped to note identity through `link_target_note`).
3. Intersect with `targets`, preserving caller order → `matched`.

This reuses `link_target_note` (already used for the existing direct-`matched` computation) and the `OwnsHeading` walk (already used by `link_target_note` itself for anchored links). A helper `heading_chain_targets(graph, paragraph_id) -> HashSet<NoteId>` captures steps 1–2 and is the single new graph-adjacent function (lives in `journal.rs`, not on `Graph`, since it's journal-specific).

Single-target mode is unchanged: `matched == vec![targets[0]]` for every entry regardless of how it was matched.

### D4 — Sort: add `line_start` as the final tiebreak

Current comparator: `b.date.cmp(&a.date).then_with(|| a.source_title.cmp(&b.source_title))`. New comparator appends `.then_with(|| a.line_start.cmp(&b.line_start))`. This is the minimal change: it only breaks ties that were previously arbitrary (insertion order), and it never reorders entries that differ in date or title. Because `line_start` is unique per paragraph within a source note, the sort is now fully deterministic for same-date same-title entries.

### D5 — Blame and `skipped_blame` unchanged

Expansion can surface paragraphs from files that the direct pass didn't touch (a heading links the target, but the heading's sibling paragraphs are in the same file as the heading — so the file is already blamed via the heading-paragraph). In the degenerate case where a file's *only* journal-relevant paragraph was the heading-paragraph (already blamed) the cache is already warm. If a heading links the target but the heading-paragraph itself was *not* a direct match (possible only if the heading line's `ParagraphLink` targeted a heading-node anchor that `link_target_note` maps to the target — i.e. the heading-paragraph *is* a direct match), the file still gets blamed on first expanded-paragraph access via the existing lazy `cache.get` → `blame_file` → `cache.insert` path. No change to `BlameCache` or `skipped_blame` collection; expanded paragraphs that fail blame are recorded in `skipped_blame` exactly like direct ones.

## Risks / Trade-offs

- **[More entries than before for existing vaults]** Existing users with heading-sited target links will see sibling paragraphs appear in the journal/synth that didn't before. This is the intended fix, but it changes snapshot output. → *Mitigation:* regenerate affected `insta` snapshots in `ft-core/src/journal.rs` tests and `ft/tests/notes_journal.rs`; the multi-link integration test gains an expansion case. Behavior change is additive (more entries), not destructive.
- **[Expansion could pull in large sections]** A heading that links many targets, or a very long section, inflates the feed. → *Mitigation:* acceptable — the user opted in by linking from the heading, and the feed is already unbounded by date range. The in-window filter remains available to narrow by commit window. No truncation planned.
- **[`matched` for expansion-only paragraphs is heading-derived, not paragraph-derived]** An expanded paragraph that happens to link a *different* target in its body would, under D2's "direct match wins" rule, be attributed by its own `ParagraphLink` (correct). Only paragraphs with *no* direct target link fall back to heading inheritance. → *Mitigation:* the direct-wins rule is explicit in D2; covered by the "Direct- and expansion-matched paragraph appears once" scenario.
- **[Heading node identified by `NodeKind`, not by edge kind]** D2 relies on `graph.node(src)` being `Heading` to know the source-side link was a `HeadingLink`. If a future change made a non-heading node emit a `HeadingLink`, expansion would misfire. → *Mitigation:* the build phase emits `HeadingLink` only from heading-line occurrences (enforced by `graph-contents-semantics` tests 6.5/6.8), so this invariant is already spec- and test-backed.
- **[No opt-out]** Some users may want the old single-paragraph behavior. → *Trade-off:* accepted per product decision; the heading-link form is unambiguously "this section is about that target," so expansion is the more correct default.

## MODIFIED Requirements

### Requirement: Journal matching via ParagraphLink edges

A paragraph SHALL be included in the journal when **either** of the following holds:

1. **Direct match.** The graph contains a `ParagraphLink` edge (or a `HeadingLink`/`NoteLink` edge, via `Graph::mentions_of`) from that paragraph (or its owning heading/note) to at least one of the resolved targets — the single target plus its aliases in single-target mode, or the set of `--link`-specified targets in multi-target mode.
2. **Heading-section expansion.** A heading in the paragraph's owning chain has a `HeadingLink` edge to at least one of the resolved targets. The "owning chain" of a paragraph is the sequence reached by walking from the paragraph's nearest `OwnsParagraph` container (a heading, else the owning note) up through `OwnsHeading` parents to the note. A heading "has a `HeadingLink` to a target" means the heading node has an outgoing `EdgeKind::HeadingLink` edge whose destination — mapped back to its note-level identity via `Graph::link_target_note` — is in the resolved target set (so a heading linking to an anchored heading of a target note still triggers expansion for that target).

For condition (2), the set of expanded paragraphs for a linking heading `H` SHALL be every paragraph transitively owned by `H`: the direct `OwnsParagraph` children of `H` plus the `OwnsParagraph` children of every `OwnsHeading`-descendant sub-heading of `H` (i.e. `Graph::note_paragraphs(H)`). This is the section spanning `H` up to the next heading of equal-or-higher level, including nested sub-sections.

Matching SHALL use `Graph::mentions_of(target)` for direct matches so that anchored links targeting a heading of the target note count as mentions of that note. Both wikilink and markdown-link forms SHALL produce matches (the unified `ParagraphLink` includes both). No string scanning is performed at query time; the graph SHALL be the sole source of truth for matches. A paragraph reachable via both condition (1) and condition (2) SHALL appear exactly once (deduplicated). Each included paragraph becomes exactly one `JournalEntry`; expansion adds sibling paragraphs as separate entries, not a merged section.

The section-expansion trigger is specifically the `HeadingLink` edge kind — a link written **inside** heading text. Anchored links that *target* a heading from elsewhere (`[[Foo#Bar]]` in a body paragraph) are handled by `mentions_of` under condition (1) and SHALL NOT themselves trigger section expansion.

#### Scenario: Graph edge determines inclusion (single-target)
- **WHEN** a paragraph node has a `ParagraphLink` edge to the target or any alias
- **THEN** that paragraph is included in the journal result

#### Scenario: Graph edge determines inclusion (multi-target)
- **WHEN** a paragraph node has a `ParagraphLink` edge to any of the `--link`-specified targets
- **THEN** that paragraph is included in the journal result

#### Scenario: Anchored link to a heading counts as a mention
- **WHEN** a paragraph contains `[[Foo#Bar]]` resolving to heading `Bar` of note `Foo`, and the journal target is `Foo`
- **THEN** that paragraph is included (the heading-targeted `ParagraphLink` is yielded by `mentions_of(foo)`)

#### Scenario: Markdown link counts as a mention
- **WHEN** a paragraph contains `[Foo](foo.md)` resolving to note `Foo`, and the journal target is `Foo`
- **THEN** that paragraph is included (the unified `ParagraphLink` includes markdown-form links)

#### Scenario: Non-linking paragraph excluded
- **WHEN** a paragraph mentions a target's title as plain text but contains no wikilink or markdown link to it
- **THEN** that paragraph is NOT included (bare-title matching is out of scope)

#### Scenario: Heading link expands to all sibling paragraphs in the section
- **WHEN** note `Daily.md` contains a heading `## Thoughts about [[Foo]]` followed by paragraph A (the heading-paragraph, which carries the link) and paragraphs B and C under the same heading, where neither B nor C contains a link to `Foo`, and the journal target is `Foo`
- **THEN** all three paragraphs A, B, and C are included as separate journal entries, each with its own per-paragraph date

#### Scenario: Expansion includes paragraphs under nested sub-headings
- **WHEN** note `Daily.md` contains `## Thoughts about [[Foo]]` (paragraph A), a `### Sub-point` sub-heading under it (paragraph B), and then a `## Next section` heading (paragraph C, not under `Thoughts`)
- **THEN** paragraphs A and B are included (B is transitively owned by the `Thoughts` heading via `OwnsHeading`), and C is NOT included (it belongs to the next same-or-higher heading)

#### Scenario: Expanded paragraph keeps its own per-paragraph date
- **WHEN** paragraphs A, B, C share a linking heading but their lines were last touched by commits on dates 2026-01-01, 2026-02-01, and 2025-12-01 respectively
- **THEN** each entry's date is its own paragraph's date (2026-02-01 for B, 2026-01-01 for A, 2025-12-01 for C), not a shared section date

#### Scenario: Expanded paragraph matched inherited from the linking heading (multi-target)
- **WHEN** in multi-target mode a paragraph is included only because its owning heading links to target `Foo` (the paragraph itself has no `ParagraphLink` to `Foo`), and the targets are `[Foo, Bar]`
- **THEN** that entry's `matched` field is `vec![Foo]` (the subset of targets its owning-chain headings link to), not the empty vector

#### Scenario: Direct- and expansion-matched paragraph appears once
- **WHEN** a paragraph both has its own `ParagraphLink` to target `Foo` and is owned by a heading that links to `Foo`
- **THEN** the paragraph appears as exactly one journal entry (deduplicated), with `matched` derived from its own direct `ParagraphLink`

#### Scenario: Single-target self-exclusion still drops the target note's own paragraphs
- **WHEN** in single-target mode the target note `Foo` contains a heading `## Notes about [[Foo]]` followed by paragraphs, and the target is `Foo`
- **THEN** those paragraphs are NOT included (single-target self-exclusion by `source_file` applies to expanded paragraphs exactly as to direct-matched ones)

#### Scenario: Heading link to a ghost target expands its section
- **WHEN** a heading `## About [[Phantom]]` exists and `Phantom` is an unresolved ghost target with no backing note, and the journal target is the `Phantom` ghost
- **THEN** the heading's section paragraphs are included (expansion applies to ghost targets via their `HeadingLink` edges just as to note targets)

#### Scenario: Anchored link targeting a heading does not trigger expansion
- **WHEN** a body paragraph in note `X` contains `[[Foo#Bar]]` (targeting heading `Bar` of note `Foo`) and the journal target is `Foo`
- **THEN** the paragraph is included via direct match (condition 1), but the section under `Bar` in note `Foo` is NOT expanded (only a `HeadingLink` *from* a heading in the owning chain triggers expansion)

### Requirement: Journal entries sorted reverse-chronologically
Journal entries SHALL be sorted by their section date (most recent first). Entries with identical dates SHALL be sorted by source note title, ascending, as a stable tiebreaker. Entries sharing both an identical date and an identical source note title SHALL be sorted by `line_start` ascending, so that co-located paragraphs from one source read in top-to-bottom document order. The `line_start` tiebreak SHALL never override a difference in date or source title — paragraph recentness remains the dominant sort signal.

#### Scenario: Reverse-chronological order
- **WHEN** two matching paragraphs have dates 2025-03-01 and 2025-11-14 respectively
- **THEN** the 2025-11-14 entry appears first in the output

#### Scenario: Same-date same-title paragraphs ordered by document position
- **WHEN** three expanded sibling paragraphs A, B, C in one source note all share the same date (committed in one commit) with `line_start` values 5, 9, 13 respectively
- **THEN** they appear in the output in order A, B, C (ascending `line_start`)

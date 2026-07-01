## MODIFIED Requirements

### Requirement: score_related function

`ft_core::related::score_related(graph: &Graph, note_id: NoteId, vault: &Vault) -> Result<Vec<RelatedScore>>` SHALL compute a co-occurrence score for every concept (note or ghost) that appears in the graph alongside the target N. N SHALL be either a `NodeKind::Note` or a `NodeKind::Ghost`. `RelatedScore` SHALL carry: `note_id: NoteId`, `title: String`, `score: u32`, `already_in_related: bool`.

Scoring SHALL use `Graph::mentions_of(N)` (or an alias) to find matching paragraphs, so anchored links to N's headings count as mentions of N. Co-occurring concepts SHALL be enumerated from the matched paragraphs' outgoing `ParagraphLink` edges. Scoring rules:
- **+3** for each `NodeKind::Paragraph` that has `ParagraphLink` edges to both N (or any alias) and C
- **+1** for each vault file where at least one paragraph links to N and at least one *different* paragraph links to C (same-file cross-paragraph co-occurrence)

Both wikilink and markdown-link forms SHALL contribute to scoring (the unified `ParagraphLink` includes both). N itself and N's aliases SHALL be excluded from the scored results. Concepts scoring 0 SHALL be omitted.

**Ghost targets:** when N is a `NodeKind::Ghost`, alias resolution SHALL be skipped (a ghost has no Related section and no backing file to read), so the alias set is empty and `already_in_related` SHALL be `false` for every returned row. The co-occurrence walk SHALL run unchanged against the ghost, since ghosts can be the target of incoming `ParagraphLink` edges. This mirrors `ft_core::journal::build_journal`'s ghost handling.

#### Scenario: Same-paragraph co-occurrence scores 3
- **WHEN** a paragraph node has ParagraphLink edges to both N and concept C
- **THEN** C receives +3 in its score

#### Scenario: Same-file cross-paragraph co-occurrence scores 1
- **WHEN** file F has paragraph P1 linking to N and paragraph P2 (different from P1) linking to C, with P2 not linking to N
- **THEN** C receives +1 from file F

#### Scenario: Same paragraph counts only once per paragraph
- **WHEN** a paragraph has two ParagraphLink edges to C alongside one to N
- **THEN** C still receives only +3 (not +6) for that paragraph

#### Scenario: Markdown link contributes to co-occurrence
- **WHEN** a paragraph contains `[N](n.md)` and `[C](c.md)` (markdown-form links resolving to N and C)
- **THEN** C receives +3 from that paragraph (markdown links count, via the unified ParagraphLink)

#### Scenario: Anchored link to a heading counts as a mention of the note
- **WHEN** a paragraph contains `[[N#Section]]` resolving to heading `Section` of note N, and a `[[C]]` link to C
- **THEN** C receives +3 (the heading-targeted ParagraphLink is yielded by `mentions_of(N)`)

#### Scenario: N excluded from results
- **WHEN** `score_related` is called for note N
- **THEN** N does not appear in the returned `Vec<RelatedScore>`

#### Scenario: Zero-score concepts omitted
- **WHEN** a concept C appears in the vault but never in a paragraph or file that also contains N
- **THEN** C is not present in the returned results

#### Scenario: Ghost target produces scored concepts
- **WHEN** `score_related` is called for a `NodeKind::Ghost` N, and paragraphs link to both N and concept C
- **THEN** C appears in the results with its co-occurrence score, and `already_in_related` is `false` for every returned row

#### Scenario: Ghost target skips alias resolution
- **WHEN** `score_related` is called for a `NodeKind::Ghost` N
- **THEN** no alias set is read (there is no Related section to consult), and no returned row has `already_in_related == true`

### Requirement: already_in_related flag

`RelatedScore.already_in_related` SHALL be `true` if and only if the concept's `NoteId` is among N's alias set (i.e., reachable via outgoing `NoteLink` edges from N within the Related section's line range). The alias-set computation reads N's outgoing `NoteLink` edges (the note-level link kind that replaces the prior `Link`/`Embed`) and filters by line range.

#### Scenario: Concept in Related section marked
- **WHEN** note N's Related section contains `[[Bar]]` and Bar appears in scored results
- **THEN** Bar's `RelatedScore.already_in_related` is `true`

#### Scenario: Concept not in Related section unmarked
- **WHEN** concept C has a non-zero score but is not linked from N's Related section
- **THEN** C's `RelatedScore.already_in_related` is `false`

## MODIFIED Requirements

### Requirement: Journal matching via ParagraphLink edges

A paragraph SHALL be included in the journal if and only if the graph contains a `ParagraphLink` edge (or a `HeadingLink`/`NoteLink` edge, via `Graph::mentions_of`) from that paragraph (or its owning heading/note) to at least one of the resolved targets — the single target plus its aliases in single-target mode, or the set of `--link`-specified targets in multi-target mode. Matching SHALL use `Graph::mentions_of(target)` so that anchored links targeting a heading of the target note count as mentions of that note. Both wikilink and markdown-link forms SHALL produce matches (the unified `ParagraphLink` includes both). No string scanning is performed at query time; the graph SHALL be the sole source of truth for matches.

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

### Requirement: Journal source coverage

Journal source paragraphs SHALL be enumerated via `Graph::note_paragraphs(source_note)` (which returns the note's heading-less paragraphs plus paragraphs owned by any transitively-owned heading), not via a flat `outgoing(note).filter(OwnsParagraph)` walk. The paragraph's `source_file`, `line_start`, `line_end`, and `section_text` SHALL be read from `ParagraphData`, whose `text` still includes any heading line that begins the paragraph (Fork A2 — unchanged from prior behavior).

#### Scenario: Paragraphs under headings are reachable
- **WHEN** a note has a paragraph under a `## Section` heading
- **THEN** `note_paragraphs(note)` returns that paragraph (recursing through the heading's `OwnsParagraph` children)

#### Scenario: section_text preserves heading line
- **WHEN** a paragraph begins at a heading line `## Section` followed by body text
- **THEN** the journal entry's `section_text` begins with `## Section` (the heading line is part of the paragraph text, per Fork A2)

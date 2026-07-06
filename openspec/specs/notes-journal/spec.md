# notes-journal Specification

## Purpose
TBD - created by archiving change related-notes-journal. Update Purpose after archive.
## Requirements
### Requirement: ft notes gather subcommand
`ft notes gather <note>` SHALL be a subcommand under `ft notes`. `<note>` is a fuzzy note selector (same resolution as `ft notes open`) that resolves to a single vault note N. The command SHALL be read-only and SHALL NOT modify any files.

#### Scenario: Invocation with known note
- **WHEN** the user runs `ft notes gather "Foo"`
- **THEN** the command resolves note `Foo.md`, builds the journal, and prints the result to stdout

#### Scenario: Ambiguous note name exits with error
- **WHEN** the note selector matches more than one note
- **THEN** the command exits with a non-zero code and a human-readable error listing the candidates

#### Scenario: Unknown note exits with error
- **WHEN** the note selector matches no note in the vault
- **THEN** the command exits with a non-zero code

### Requirement: Journal alias resolution via Related section
In single-target mode (one positional `<note>` argument or exactly one `--link` resolving to a note), the command SHALL identify aliases for the target N by: (1) locating the `## Related` heading in N's content, (2) finding the line range of that section (up to the next equal-or-higher heading or end of file), (3) filtering N's outgoing `EdgeKind::Link` edges to those whose `line` falls within the Related section's range, (4) collecting the `NoteId` targets of those edges. The journal search SHALL cover N and all resolved aliases. In multi-target mode (more than one `--link`), Related-aliases resolution SHALL be skipped — the user's selection is taken as-is.

#### Scenario: Related section aliases included (single-target)
- **WHEN** note N has a Related section containing `[[Bar]]` and `[[Baz]]` and the command is invoked as `ft notes gather "N"`
- **THEN** the journal includes paragraphs that link to Bar or Baz as well as paragraphs that link to N

#### Scenario: Note with no Related section
- **WHEN** note N has no `## Related` heading
- **THEN** the journal searches for mentions of N only (no aliases)

#### Scenario: Related section with prose links
- **WHEN** the Related section contains `The [[Bar]] system is the main dependency`
- **THEN** `Bar` is included as an alias

#### Scenario: Multi-target skips alias resolution
- **WHEN** the command is invoked as `ft notes gather --link "[[Foo]]" --link "[[Bar]]"`
- **THEN** any `## Related` section in `Foo` or `Bar` is NOT consulted; only `Foo` and `Bar` are used as targets

### Requirement: Journal source coverage

Journal source paragraphs SHALL be enumerated via `Graph::note_paragraphs(source_note)` (which returns the note's heading-less paragraphs plus paragraphs owned by any transitively-owned heading), not via a flat `outgoing(note).filter(OwnsParagraph)` walk. The paragraph's `source_file`, `line_start`, `line_end`, and `section_text` SHALL be read from `ParagraphData`, whose `text` still includes any heading line that begins the paragraph (Fork A2 — unchanged from prior behavior).

#### Scenario: Paragraphs under headings are reachable
- **WHEN** a note has a paragraph under a `## Section` heading
- **THEN** `note_paragraphs(note)` returns that paragraph (recursing through the heading's `OwnsParagraph` children)

#### Scenario: section_text preserves heading line
- **WHEN** a paragraph begins at a heading line `## Section` followed by body text
- **THEN** the journal entry's `section_text` begins with `## Section` (the heading line is part of the paragraph text, per Fork A2)

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

### Requirement: Journal default (table) output
The default output SHALL display each journal entry as: a date line (`YYYY-MM-DD  <Source Note Title>`), a visual separator, and the paragraph text, followed by a blank line between entries. Output SHALL use vault-relative paths for any path references and SHALL respect `--no-color` / `NO_COLOR` / non-TTY auto-disable for ANSI styling.

#### Scenario: Table output format
- **WHEN** `ft notes gather "Foo"` is run in a TTY with color
- **THEN** stdout contains date, source title, separator, and paragraph text for each entry

#### Scenario: No-color mode
- **WHEN** `NO_COLOR=1 ft notes gather "Foo"` is run
- **THEN** output contains no ANSI escape sequences

### Requirement: Journal JSON output
With `--json`, the command SHALL emit a JSON array where each element has fields: `date` (ISO 8601 date string), `source_title` (string), `source_path` (vault-relative string), `section` (string, the paragraph text), and `matched` (array of target display title strings; one element in single-target mode, one or more in multi-target mode).

#### Scenario: JSON output structure
- **WHEN** `ft notes gather "Foo" --json` is run and two entries match
- **THEN** stdout is a valid JSON array with two objects, each containing `date`, `source_title`, `source_path`, `section`, and `matched` fields

#### Scenario: Multi-target JSON shows matched subset
- **WHEN** `ft notes gather --link "[[Foo]]" --link "[[Bar]]" --json` is run and one entry's paragraph contains only `[[Foo]]`
- **THEN** that entry's `matched` array is `["Foo"]`

### Requirement: Multi-link invocation mode
`ft notes gather` SHALL accept one or more `--link <wikilink>` flags as an alternative to the positional `<note>` selector. `--link` SHALL be repeatable. The positional `<note>` argument and `--link` SHALL be mutually exclusive. When `--link` is used, each value SHALL be either a raw `[[Wikilink]]` token (with or without surrounding brackets) or a bare name; resolution SHALL use the same Obsidian shortest-path rules as the existing graph link resolver. Unresolvable link values SHALL fall through to ghost targets (which produce a valid but possibly empty journal contribution).

#### Scenario: Single --link invocation
- **WHEN** the user runs `ft notes gather --link "[[Foo]]"`
- **THEN** the command resolves `Foo` to a note or ghost and builds a multi-target journal with that one target

#### Scenario: Multiple --link invocation
- **WHEN** the user runs `ft notes gather --link "[[Foo]]" --link "[[Bar]]"`
- **THEN** the command builds the multi-target journal across both targets and prints the merged, sorted result

#### Scenario: Positional and --link are mutually exclusive
- **WHEN** the user runs `ft notes gather "Foo" --link "[[Bar]]"`
- **THEN** the command exits with a non-zero code and a clear "mutually exclusive" error

#### Scenario: Ghost link still works
- **WHEN** the user runs `ft notes gather --link "[[NonExistent]]"`
- **THEN** the command builds a journal for the ghost target (paragraphs linking to `NonExistent`); empty result is not an error

### Requirement: In-window filter flag
With `--in-window` plus either `--since <duration>` or `--range <X>..<Y>`, the journal output SHALL be restricted to entries whose `(source_file, line_start..=line_end)` overlaps an added-line recorded by the link-review engine for the same window. Without `--in-window` (or without a window flag), the journal SHALL include all-time matches (existing default behavior preserved).

#### Scenario: Window restricts entries
- **WHEN** `ft notes gather --link "[[Foo]]" --since 7d --in-window` is run
- **THEN** only entries whose paragraph lines overlap added-lines in the last 7 days are included

#### Scenario: Without --in-window, window is ignored for filtering
- **WHEN** `ft notes gather --link "[[Foo]]" --since 7d` is run (no `--in-window`)
- **THEN** all-time matching paragraphs are returned (the window flag is ignored for filtering)

#### Scenario: --in-window requires a window
- **WHEN** `ft notes gather --link "[[Foo]]" --in-window` is run without `--since` or `--range`
- **THEN** the command exits with a non-zero code and an error stating `--in-window` requires a window

### Requirement: Matched-targets field per entry
In multi-target mode, each journal entry SHALL carry a `matched` field listing the subset of selected targets that the entry's paragraph links to. In single-target mode, `matched` SHALL contain the one target. The default text output SHALL append a `matched: X, Y` indicator after the date line when `matched.len() > 1`. The JSON output SHALL include a `matched` array of target display titles.

#### Scenario: Multi-target entry shows matched badge
- **WHEN** a paragraph contains both `[[Foo]]` and `[[Bar]]` and both are selected
- **THEN** that entry's date line is followed by `matched: Foo, Bar`

#### Scenario: Single-match entry does not show badge
- **WHEN** a paragraph contains `[[Foo]]` only and both `[[Foo]]` and `[[Bar]]` are selected
- **THEN** that entry's output has no `matched:` line

#### Scenario: JSON matched array
- **WHEN** `ft notes gather --link "[[Foo]]" --link "[[Bar]]" --json` is run
- **THEN** each JSON object includes a `matched` array containing the subset of `["Foo", "Bar"]` that the entry's paragraph linked to

### Requirement: Cited badge in journal text output
`ft notes gather` SHALL annotate each entry whose citation state is
not `Uncited` with a badge line: `cited: <note stem>` for exact
citations, `cited*: <note stem>` for stale ones; when multiple synth
notes cite the entry, the first (path-sorted) is shown followed by
`+N`. Uncited entries render unchanged.

#### Scenario: Cited entry shows badge
- **WHEN** a journal entry's paragraph is pinned byte-identically in
  `Synthesis/foo.md`
- **THEN** the entry renders a `cited: foo` badge line

#### Scenario: Stale entry shows starred badge
- **WHEN** the paragraph was edited after being pinned
- **THEN** the entry renders `cited*: foo`

### Requirement: cited_in in journal JSON
`ft notes gather --json` entries SHALL gain a `cited_in` array of
`{note, stale}` objects (vault-relative note path, boolean), empty
when uncited. Existing fields SHALL be unchanged.

#### Scenario: JSON carries citation state
- **WHEN** `--json` is used on a feed with one cited and one uncited
  entry
- **THEN** the cited entry has a non-empty `cited_in` and the uncited
  entry has `cited_in: []`

### Requirement: --uncited filter on journal
`ft notes gather --uncited` SHALL keep only entries whose state is
not `Cited` (stale entries are kept). It SHALL compose with existing
flags (`--link`, `--since`/`--range`, `--in-window`, `--json`).

#### Scenario: Filter drops exact citations only
- **WHEN** a feed contains a cited, a stale, and an uncited entry and
  `--uncited` is passed
- **THEN** the stale and uncited entries remain and the cited entry is
  dropped

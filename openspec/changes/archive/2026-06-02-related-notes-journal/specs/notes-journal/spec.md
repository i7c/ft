## ADDED Requirements

### Requirement: ft notes journal subcommand
`ft notes journal <note>` SHALL be a subcommand under `ft notes`. `<note>` is a fuzzy note selector (same resolution as `ft notes open`) that resolves to a single vault note N. The command SHALL be read-only and SHALL NOT modify any files.

#### Scenario: Invocation with known note
- **WHEN** the user runs `ft notes journal "Foo"`
- **THEN** the command resolves note `Foo.md`, builds the journal, and prints the result to stdout

#### Scenario: Ambiguous note name exits with error
- **WHEN** the note selector matches more than one note
- **THEN** the command exits with a non-zero code and a human-readable error listing the candidates

#### Scenario: Unknown note exits with error
- **WHEN** the note selector matches no note in the vault
- **THEN** the command exits with a non-zero code

### Requirement: Journal alias resolution via Related section
Before searching, the command SHALL identify aliases for N by: (1) locating the `## Related` heading in N's content, (2) finding the line range of that section (up to the next equal-or-higher heading or end of file), (3) filtering N's outgoing `EdgeKind::Link` edges to those whose `line` falls within the Related section's range, (4) collecting the `NoteId` targets of those edges. The journal search SHALL cover N and all resolved aliases.

#### Scenario: Related section aliases included
- **WHEN** note N has a Related section containing `[[Bar]]` and `[[Baz]]`
- **THEN** the journal includes paragraphs that link to Bar or Baz as well as paragraphs that link to N

#### Scenario: Note with no Related section
- **WHEN** note N has no `## Related` heading
- **THEN** the journal searches for mentions of N only (no aliases)

#### Scenario: Related section with prose links
- **WHEN** the Related section contains `The [[Bar]] system is the main dependency`
- **THEN** `Bar` is included as an alias

### Requirement: Journal source coverage
The journal SHALL search across all markdown notes in the vault, including daily notes and notes in any directory. The note N itself SHALL be excluded from results.

#### Scenario: Daily notes included
- **WHEN** a daily note `2025-03-14.md` contains a paragraph linking to N
- **THEN** that paragraph appears in the journal

#### Scenario: Note N excluded from its own journal
- **WHEN** note N's own content contains paragraphs that link to N (self-links)
- **THEN** those paragraphs do NOT appear in the journal

### Requirement: Journal matching via ParagraphLink edges
A paragraph SHALL be included in the journal if and only if the graph contains a `ParagraphLink` edge from that paragraph node to N or any of N's aliases. No string scanning is performed at query time; the graph SHALL be the sole source of truth for matches.

#### Scenario: Graph edge determines inclusion
- **WHEN** a paragraph node has a `ParagraphLink` edge to N
- **THEN** that paragraph is included in the journal result

#### Scenario: Non-linking paragraph excluded
- **WHEN** a paragraph mentions N's title as plain text but contains no `[[N]]` wiki link
- **THEN** that paragraph is NOT included (bare-title matching is out of scope)

### Requirement: Journal entries sorted reverse-chronologically
Journal entries SHALL be sorted by their section date (most recent first). Entries with identical dates SHALL be sorted by source note title, ascending, as a stable tiebreaker.

#### Scenario: Reverse-chronological order
- **WHEN** two matching paragraphs have dates 2025-03-01 and 2025-11-14 respectively
- **THEN** the 2025-11-14 entry appears first in the output

### Requirement: Journal default (table) output
The default output SHALL display each journal entry as: a date line (`YYYY-MM-DD  <Source Note Title>`), a visual separator, and the paragraph text, followed by a blank line between entries. Output SHALL use vault-relative paths for any path references and SHALL respect `--no-color` / `NO_COLOR` / non-TTY auto-disable for ANSI styling.

#### Scenario: Table output format
- **WHEN** `ft notes journal "Foo"` is run in a TTY with color
- **THEN** stdout contains date, source title, separator, and paragraph text for each entry

#### Scenario: No-color mode
- **WHEN** `NO_COLOR=1 ft notes journal "Foo"` is run
- **THEN** output contains no ANSI escape sequences

### Requirement: Journal JSON output
With `--json`, the command SHALL emit a JSON array where each element has fields: `date` (ISO 8601 date string), `source_title` (string), `source_path` (vault-relative string), `section` (string, the paragraph text).

#### Scenario: JSON output structure
- **WHEN** `ft notes journal "Foo" --json` is run and two entries match
- **THEN** stdout is a valid JSON array with two objects, each containing `date`, `source_title`, `source_path`, and `section` fields

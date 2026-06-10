# notes-journal Specification

## Purpose
TBD - created by archiving change related-notes-journal. Update Purpose after archive.
## Requirements
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
In single-target mode (one positional `<note>` argument or exactly one `--link` resolving to a note), the command SHALL identify aliases for the target N by: (1) locating the `## Related` heading in N's content, (2) finding the line range of that section (up to the next equal-or-higher heading or end of file), (3) filtering N's outgoing `EdgeKind::Link` edges to those whose `line` falls within the Related section's range, (4) collecting the `NoteId` targets of those edges. The journal search SHALL cover N and all resolved aliases. In multi-target mode (more than one `--link`), Related-aliases resolution SHALL be skipped — the user's selection is taken as-is.

#### Scenario: Related section aliases included (single-target)
- **WHEN** note N has a Related section containing `[[Bar]]` and `[[Baz]]` and the command is invoked as `ft notes journal "N"`
- **THEN** the journal includes paragraphs that link to Bar or Baz as well as paragraphs that link to N

#### Scenario: Note with no Related section
- **WHEN** note N has no `## Related` heading
- **THEN** the journal searches for mentions of N only (no aliases)

#### Scenario: Related section with prose links
- **WHEN** the Related section contains `The [[Bar]] system is the main dependency`
- **THEN** `Bar` is included as an alias

#### Scenario: Multi-target skips alias resolution
- **WHEN** the command is invoked as `ft notes journal --link "[[Foo]]" --link "[[Bar]]"`
- **THEN** any `## Related` section in `Foo` or `Bar` is NOT consulted; only `Foo` and `Bar` are used as targets

### Requirement: Journal source coverage
The journal SHALL search across all markdown notes in the vault, including daily notes and notes in any directory. In single-target mode, the note N itself SHALL be excluded from results. In multi-target mode, no single self-exclusion applies; entries from notes that happen to be selected as targets ARE included.

#### Scenario: Daily notes included
- **WHEN** a daily note `2025-03-14.md` contains a paragraph linking to a selected target
- **THEN** that paragraph appears in the journal

#### Scenario: Note N excluded from its own journal (single-target)
- **WHEN** the command is invoked as `ft notes journal "N"` and N's own content contains paragraphs that link to N
- **THEN** those paragraphs do NOT appear in the journal

#### Scenario: Multi-target does not self-exclude
- **WHEN** the command is invoked as `ft notes journal --link "[[Foo]]" --link "[[Bar]]"` and `Foo` contains a paragraph linking to `Bar`
- **THEN** that paragraph IS included in the journal (no self-exclusion in multi-target mode)

### Requirement: Journal matching via ParagraphLink edges
A paragraph SHALL be included in the journal if and only if the graph contains a `ParagraphLink` edge from that paragraph node to at least one of the resolved targets (the single target plus its aliases in single-target mode, or the set of `--link`-specified targets in multi-target mode). No string scanning is performed at query time; the graph SHALL be the sole source of truth for matches.

#### Scenario: Graph edge determines inclusion (single-target)
- **WHEN** a paragraph node has a `ParagraphLink` edge to the target or any alias
- **THEN** that paragraph is included in the journal result

#### Scenario: Graph edge determines inclusion (multi-target)
- **WHEN** a paragraph node has a `ParagraphLink` edge to any of the `--link`-specified targets
- **THEN** that paragraph is included in the journal result

#### Scenario: Non-linking paragraph excluded
- **WHEN** a paragraph mentions a target's title as plain text but contains no `[[wikilink]]` to it
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
With `--json`, the command SHALL emit a JSON array where each element has fields: `date` (ISO 8601 date string), `source_title` (string), `source_path` (vault-relative string), `section` (string, the paragraph text), and `matched` (array of target display title strings; one element in single-target mode, one or more in multi-target mode).

#### Scenario: JSON output structure
- **WHEN** `ft notes journal "Foo" --json` is run and two entries match
- **THEN** stdout is a valid JSON array with two objects, each containing `date`, `source_title`, `source_path`, `section`, and `matched` fields

#### Scenario: Multi-target JSON shows matched subset
- **WHEN** `ft notes journal --link "[[Foo]]" --link "[[Bar]]" --json` is run and one entry's paragraph contains only `[[Foo]]`
- **THEN** that entry's `matched` array is `["Foo"]`

### Requirement: Multi-link invocation mode
`ft notes journal` SHALL accept one or more `--link <wikilink>` flags as an alternative to the positional `<note>` selector. `--link` SHALL be repeatable. The positional `<note>` argument and `--link` SHALL be mutually exclusive. When `--link` is used, each value SHALL be either a raw `[[Wikilink]]` token (with or without surrounding brackets) or a bare name; resolution SHALL use the same Obsidian shortest-path rules as the existing graph link resolver. Unresolvable link values SHALL fall through to ghost targets (which produce a valid but possibly empty journal contribution).

#### Scenario: Single --link invocation
- **WHEN** the user runs `ft notes journal --link "[[Foo]]"`
- **THEN** the command resolves `Foo` to a note or ghost and builds a multi-target journal with that one target

#### Scenario: Multiple --link invocation
- **WHEN** the user runs `ft notes journal --link "[[Foo]]" --link "[[Bar]]"`
- **THEN** the command builds the multi-target journal across both targets and prints the merged, sorted result

#### Scenario: Positional and --link are mutually exclusive
- **WHEN** the user runs `ft notes journal "Foo" --link "[[Bar]]"`
- **THEN** the command exits with a non-zero code and a clear "mutually exclusive" error

#### Scenario: Ghost link still works
- **WHEN** the user runs `ft notes journal --link "[[NonExistent]]"`
- **THEN** the command builds a journal for the ghost target (paragraphs linking to `NonExistent`); empty result is not an error

### Requirement: In-window filter flag
With `--in-window` plus either `--since <duration>` or `--range <X>..<Y>`, the journal output SHALL be restricted to entries whose `(source_file, line_start..=line_end)` overlaps an added-line recorded by the link-review engine for the same window. Without `--in-window` (or without a window flag), the journal SHALL include all-time matches (existing default behavior preserved).

#### Scenario: Window restricts entries
- **WHEN** `ft notes journal --link "[[Foo]]" --since 7d --in-window` is run
- **THEN** only entries whose paragraph lines overlap added-lines in the last 7 days are included

#### Scenario: Without --in-window, window is ignored for filtering
- **WHEN** `ft notes journal --link "[[Foo]]" --since 7d` is run (no `--in-window`)
- **THEN** all-time matching paragraphs are returned (the window flag is ignored for filtering)

#### Scenario: --in-window requires a window
- **WHEN** `ft notes journal --link "[[Foo]]" --in-window` is run without `--since` or `--range`
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
- **WHEN** `ft notes journal --link "[[Foo]]" --link "[[Bar]]" --json` is run
- **THEN** each JSON object includes a `matched` array containing the subset of `["Foo", "Bar"]` that the entry's paragraph linked to


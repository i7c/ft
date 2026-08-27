# notes-history

## MODIFIED Requirements

### Requirement: build_history core feed

`ft_core::history::build_history` SHALL produce a whole-vault, paragraph-granular,
recency-ordered feed of paragraphs edited within a time window, without requiring
any link target. It SHALL enumerate paragraph nodes from the graph
(`Graph::nodes` filtered to `NodeKind::Paragraph`), reading each paragraph's
`source_file`, `line_start`, `line_end`, and text from `ParagraphData` — the same
node data the paragraph-graph consumers read, so paragraph and owning-heading
structure is reused, not re-parsed.

#### Scenario: Feed needs no target

- **WHEN** `build_history` is called with a window and a vault containing edited paragraphs
- **THEN** it returns entries for those paragraphs with no note/link target supplied

### Requirement: Edit-window inclusion filter

A paragraph SHALL be included in the history feed if and only if its line range
`line_start..=line_end` overlaps at least one line added or changed within the
resolved window, as reported by the link-review engine's added-lines map for that
same window. The window SHALL be resolvable from either a `--since <duration>`
(e.g. `7d`, `24h`, `2w`, `1m`) or a `--range <X>..<Y>` commit range, via the
shared window resolver (`ft_core::pulse::WindowRange`).

#### Scenario: Edited paragraph included

- **WHEN** a paragraph's lines overlap a line added within the window
- **THEN** that paragraph appears in the feed

#### Scenario: Unedited paragraph excluded

- **WHEN** a paragraph's lines do not overlap any line added within the window
- **THEN** that paragraph does NOT appear, even if its file changed elsewhere in the window

### Requirement: Recency ordering matches the journal

History entries SHALL each carry a blame date computed as the most recent commit
touching any line in the paragraph (via `blame_cache`'s `paragraph_date`), and
SHALL be sorted by that date descending, then by source note title ascending, then
by `line_start` ascending. The `line_start`
tiebreak SHALL never override a date or title difference.

#### Scenario: Reverse-chronological order

- **WHEN** two edited paragraphs have blame dates 2026-06-20 and 2026-07-01
- **THEN** the 2026-07-01 entry appears first

#### Scenario: Same-date same-title ordered by document position

- **WHEN** two edited paragraphs in one source note share a blame date with `line_start` 4 and 12
- **THEN** they appear in ascending `line_start` order

### Requirement: History output formats

The default (table) output SHALL render each entry as a date line
(`YYYY-MM-DD  <Source Note Title>`), a separator, and the paragraph text, with a
blank line between entries — using the shared paragraph-feed renderer
(`ft_core::output::feed`). Paths SHALL be
vault-relative. ANSI styling SHALL auto-disable under `--no-color` / `NO_COLOR` /
non-TTY. With `--json`, the command SHALL emit a JSON array whose elements have
`date`, `source_title`, `source_path`, and `section` fields.

#### Scenario: Table output

- **WHEN** `ft notes recent` runs in a TTY with color
- **THEN** stdout shows date, source title, separator, and paragraph text per entry

#### Scenario: No-color mode

- **WHEN** `NO_COLOR=1 ft notes recent` runs
- **THEN** output contains no ANSI escape sequences

#### Scenario: JSON output structure

- **WHEN** `ft notes recent --json` runs with two entries
- **THEN** stdout is a valid JSON array of two objects, each with `date`, `source_title`, `source_path`, and `section`

### Requirement: Cited badge in history text output

`ft notes recent` SHALL annotate entries with the citation-index badge grammar
(`cited: <note stem>` / `cited*: <note stem>`,
first citing note plus `+N` overflow), with uncited entries unchanged.

#### Scenario: History entry shows badge

- **WHEN** a paragraph edited in the window is pinned in a synth note
- **THEN** its history entry renders the `cited:` badge line

### Requirement: cited_in in history JSON

`ft notes recent --json` entries SHALL include an additive
`cited_in` array of `{note, stale}` objects, derived from the citation index.

#### Scenario: cited_in reflects the citation index

- **WHEN** a paragraph edited in the window is pinned in a synth note
- **THEN** its JSON entry carries `cited_in` with that note and the correct staleness flag

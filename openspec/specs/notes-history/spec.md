# notes-history Specification

## Purpose
TBD - created by archiving change notes-history-tab. Update Purpose after archive.
## Requirements
### Requirement: build_history core feed
`ft_core::history::build_history` SHALL produce a whole-vault, paragraph-granular,
recency-ordered feed of paragraphs edited within a time window, without requiring
any link target. It SHALL enumerate paragraph nodes from the graph
(`Graph::nodes` filtered to `NodeKind::Paragraph`), reading each paragraph's
`source_file`, `line_start`, `line_end`, and text from `ParagraphData` — the same
node data the journal reads, so paragraph and owning-heading structure is reused,
not re-parsed. `build_journal` and its semantics SHALL be left unchanged; this is a
sibling builder.

#### Scenario: Feed needs no target
- **WHEN** `build_history` is called with a window and a vault containing edited paragraphs
- **THEN** it returns entries for those paragraphs with no note/link target supplied

#### Scenario: Journal is untouched
- **WHEN** this change is applied
- **THEN** `build_journal`'s signature and behavior are unchanged (verified by its existing tests still passing)

### Requirement: Edit-window inclusion filter
A paragraph SHALL be included in the history feed if and only if its line range
`line_start..=line_end` overlaps at least one line added or changed within the
resolved window, as reported by the link-review engine's added-lines map for that
same window. The window SHALL be resolvable from either a `--since <duration>`
(e.g. `7d`, `24h`, `2w`, `1m`) or a `--range <X>..<Y>` commit range, exactly as
`ft notes journal` resolves its window arguments.

#### Scenario: Edited paragraph included
- **WHEN** a paragraph's lines overlap a line added within the window
- **THEN** that paragraph appears in the feed

#### Scenario: Unedited paragraph excluded
- **WHEN** a paragraph's lines do not overlap any line added within the window
- **THEN** that paragraph does NOT appear, even if its file changed elsewhere in the window

### Requirement: Default window of 7 days
When neither `--since` nor `--range` is supplied, the history feed SHALL default to
a `7d` window. The feed SHALL always be windowed — there is no all-time mode.

#### Scenario: No window flag defaults to 7d
- **WHEN** `ft notes history` is run with no `--since` or `--range`
- **THEN** the feed contains paragraphs edited within the last 7 days

#### Scenario: Explicit window overrides the default
- **WHEN** `ft notes history --since 2w` is run
- **THEN** the feed uses the 2-week window rather than the 7-day default

### Requirement: Recency ordering matches the journal
History entries SHALL each carry a blame date computed as the most recent commit
touching any line in the paragraph (via `blame_cache`'s `paragraph_date`), and
SHALL be sorted by that date descending, then by source note title ascending, then
by `line_start` ascending — identical to the journal's sort. The `line_start`
tiebreak SHALL never override a date or title difference.

#### Scenario: Reverse-chronological order
- **WHEN** two edited paragraphs have blame dates 2026-06-20 and 2026-07-01
- **THEN** the 2026-07-01 entry appears first

#### Scenario: Same-date same-title ordered by document position
- **WHEN** two edited paragraphs in one source note share a blame date with `line_start` 4 and 12
- **THEN** they appear in ascending `line_start` order

### Requirement: Synth notes excluded by default
Paragraphs whose source note carries the `ft-synth: true` frontmatter marker SHALL
be excluded from the history feed by default, so the synth flow does not feed
itself. An opt-in flag (`--include-synth`) SHALL include them. Periodic/daily notes
SHALL be included by default (they are not excluded).

#### Scenario: Synth note excluded by default
- **WHEN** an edited paragraph lives in a note with `ft-synth: true` and `--include-synth` is not passed
- **THEN** that paragraph does NOT appear in the feed

#### Scenario: --include-synth surfaces synth notes
- **WHEN** the same feed is requested with `--include-synth`
- **THEN** the synth-note paragraph appears

#### Scenario: Periodic notes are included
- **WHEN** an edited paragraph lives in a daily/periodic note
- **THEN** that paragraph appears in the feed by default

### Requirement: File-prefilter performance contract
`build_history` SHALL blame only files that were touched within the window
(determined from the link-review added-lines map / git log for that window), not
every file in the vault. Files with no window edits SHALL NOT be blamed.

#### Scenario: Untouched files are not blamed
- **WHEN** the vault has 100 notes but only 3 were touched within the window
- **THEN** at most the 3 touched files are blamed (untouched files trigger no `git blame`)

### Requirement: ft notes history subcommand
`ft notes history` SHALL be a read-only subcommand under `ft notes` that runs
`build_history` and prints the feed. It SHALL accept `--since <duration>` and
`--range <X>..<Y>` (mutually exclusive, defaulting to `7d`), `--include-synth`,
`--json`, and `--no-color`. It SHALL require the vault to be inside a git
repository, erroring clearly otherwise. It SHALL NOT modify any files.

#### Scenario: Default invocation
- **WHEN** the user runs `ft notes history` inside a git-backed vault
- **THEN** it prints the last-7-days feed to stdout and exits successfully

#### Scenario: Mutually exclusive window flags
- **WHEN** the user runs `ft notes history --since 7d --range A..B`
- **THEN** the command exits non-zero with a mutual-exclusion error

#### Scenario: Non-git vault errors
- **WHEN** the vault is not inside a git repository
- **THEN** the command exits non-zero with an error stating git history is required

### Requirement: History output formats
The default (table) output SHALL render each entry as a date line
(`YYYY-MM-DD  <Source Note Title>`), a separator, and the paragraph text, with a
blank line between entries — reusing the journal's renderer. Paths SHALL be
vault-relative. ANSI styling SHALL auto-disable under `--no-color` / `NO_COLOR` /
non-TTY. With `--json`, the command SHALL emit a JSON array whose elements have
`date`, `source_title`, `source_path`, and `section` fields.

#### Scenario: Table output
- **WHEN** `ft notes history` runs in a TTY with color
- **THEN** stdout shows date, source title, separator, and paragraph text per entry

#### Scenario: No-color mode
- **WHEN** `NO_COLOR=1 ft notes history` runs
- **THEN** output contains no ANSI escape sequences

#### Scenario: JSON output structure
- **WHEN** `ft notes history --json` runs with two entries
- **THEN** stdout is a valid JSON array of two objects, each with `date`, `source_title`, `source_path`, and `section`

### Requirement: Cited badge in history text output
`ft notes history` SHALL annotate entries with the same badge grammar
as `ft notes journal`: `cited: <note stem>` / `cited*: <note stem>`,
first citing note plus `+N` overflow, uncited entries unchanged.

#### Scenario: History entry shows badge
- **WHEN** a paragraph edited in the window is pinned in a synth note
- **THEN** its history entry renders the `cited:` badge line

### Requirement: cited_in in history JSON
`ft notes history --json` entries SHALL gain the same additive
`cited_in` array of `{note, stale}` objects as the journal.

#### Scenario: JSON parity with journal
- **WHEN** the same paragraph appears in both journal and history JSON
- **THEN** both report identical `cited_in` contents

### Requirement: --uncited filter on history
`ft notes history --uncited` SHALL keep only entries whose state is
not `Cited` (stale kept), composing with the existing window flags.

#### Scenario: Incremental sweep
- **WHEN** the user runs `ft notes history --since 7d --uncited`
- **THEN** only paragraphs from the window not yet pinned
  byte-identically in any synth note are listed

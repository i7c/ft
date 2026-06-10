## ADDED Requirements

### Requirement: ft review subcommand
`ft review` SHALL be a new top-level subcommand that prints a frequency-ranked list of `[[wikilinks]]` newly mentioned in a time window. It SHALL accept either `--since <duration>` (e.g. `--since 7d`, `--since 24h`) or `--range <X>..<Y>` (two git refs). The two flags SHALL be mutually exclusive. The command SHALL be read-only and SHALL NOT modify any files.

#### Scenario: Invocation with date duration
- **WHEN** the user runs `ft review --since 7d` in a vault that is a git repo with commits in the last 7 days
- **THEN** the command exits 0 and prints a table of links from the window

#### Scenario: Invocation with commit range
- **WHEN** the user runs `ft review --range main~10..HEAD`
- **THEN** the command exits 0 and prints a table of links from that commit range

#### Scenario: Mutually exclusive flags
- **WHEN** the user passes both `--since 7d` and `--range X..Y`
- **THEN** the command exits with a non-zero code and a clear "mutually exclusive" error

#### Scenario: Vault is not a git repo
- **WHEN** the user runs `ft review` in a directory without git
- **THEN** the command exits with a non-zero code and an error naming the missing git repo

### Requirement: Link extraction via git-log scan
The command SHALL invoke `git log -p <X>..<Y>` (or the date-equivalent commit range) over the vault, parse the unified diff, and extract every `[[wikilink]]` token that appears on an added line (lines starting with `+` but not `+++` file markers). Tokens inside fenced code blocks SHALL be ignored using the same line-skip logic as the existing markdown parser.

#### Scenario: Wikilink added on a new line is counted
- **WHEN** a commit in the window adds a line `... mentions [[Foo]] in passing`
- **THEN** `[[Foo]]` is counted

#### Scenario: Wikilink removed in window does not count
- **WHEN** a commit in the window removes a line containing `[[Foo]]` but no other commit adds it
- **THEN** `[[Foo]]` is NOT counted

#### Scenario: Wikilink in fenced code block ignored
- **WHEN** an added line is inside a fenced code block (``` or ~~~) at the post-commit state of its file
- **THEN** any `[[wikilink]]` on that line is NOT counted

### Requirement: Paragraph-level frequency dedup
For each `[[Link]]` occurrence on an added line, the command SHALL map the line to its containing paragraph using the HEAD-state paragraph index of the post-commit path. It SHALL count distinct `(link, paragraph)` pairs. If the line cannot be mapped to a current paragraph (file deleted or paragraph rewritten such that no current paragraph contains the original added line), the command SHALL fall back to a synthetic key of `(path, original-added-line)` so the link is still counted but does not dedup against current paragraphs.

#### Scenario: Same link twice in one paragraph counts once
- **WHEN** an added paragraph contains `[[Foo]] and again [[Foo]]`
- **THEN** the count for `[[Foo]]` from this paragraph is 1

#### Scenario: Same link in two paragraphs of one note counts twice
- **WHEN** two separate paragraphs in one note each contain a `[[Foo]]` mention added in the window
- **THEN** the count for `[[Foo]]` is 2

#### Scenario: Same link across multiple notes accumulates
- **WHEN** notes A, B, and C each contribute one paragraph with `[[Foo]]` in the window
- **THEN** the count for `[[Foo]]` is 3

#### Scenario: Source file deleted after window
- **WHEN** a commit in the window added `[[Foo]]` to a file that has since been deleted
- **THEN** `[[Foo]]` is still counted (via the synthetic-key fallback)

### Requirement: Frequency ranking and ghost marking
Output SHALL list links by descending count, with ascending alphabetical tiebreak on the link target. Each row SHALL show `(<count>) [[<target>]]`. Links whose target does not resolve to an existing note in the current vault state (ghosts) SHALL be suffixed with `?` (e.g. `(3) [[Foo]]?`).

#### Scenario: Sort by descending count
- **WHEN** counts are `[[Foo]] = 3`, `[[Bar]] = 5`, `[[Baz]] = 1`
- **THEN** output order is `[[Bar]]`, `[[Foo]]`, `[[Baz]]`

#### Scenario: Alphabetical tiebreak
- **WHEN** counts are `[[Bar]] = 2` and `[[Foo]] = 2`
- **THEN** output order is `[[Bar]]`, `[[Foo]]`

#### Scenario: Ghost target marked
- **WHEN** the link `[[NonExistent]]` does not resolve to any note in the vault
- **THEN** its row displays `(<count>) [[NonExistent]]?`

### Requirement: Path-prefix exclude filter
The command SHALL exclude `[[wikilink]]` occurrences from files whose vault-relative path starts with any of the prefixes listed in the new `synth.exclude_prefixes` config field. The default value SHALL exclude the configured periodic-notes folder when set.

#### Scenario: Excluded prefix dropped
- **WHEN** `synth.exclude_prefixes = ["Periodic/"]` and a commit added `[[Foo]]` only inside `Periodic/2025-03-14.md`
- **THEN** `[[Foo]]` is NOT counted

#### Scenario: Non-excluded path counts
- **WHEN** the same `[[Foo]]` is added in `Notes/foo.md` (not under any excluded prefix)
- **THEN** `[[Foo]]` is counted

### Requirement: Synth-note callout exclusion
The command SHALL skip `[[wikilink]]` occurrences whose position in the post-commit file falls inside a `> [!ft-source] ...` callout block in a note that is marked `ft-synth: true` in its frontmatter. Wikilinks in the prose of a synth note (outside any `[!ft-source]` callout) SHALL still be counted.

#### Scenario: Wikilink inside protected section ignored
- **WHEN** an added line `> mentions [[Foo]] verbatim` falls inside a `[!ft-source]` callout in `Synthesis/topic.md` (frontmatter has `ft-synth: true`)
- **THEN** `[[Foo]]` from that occurrence is NOT counted

#### Scenario: Wikilink in synth-note prose counted
- **WHEN** an added line in a synth note is between two `[!ft-source]` callouts and contains `[[Bar]]`
- **THEN** `[[Bar]]` IS counted

### Requirement: Default table output
The default output SHALL display the link-review as a table with one row per link, format `(<count>) [[<target>]]` (with `?` suffix for ghosts), one row per line. Output SHALL respect `--no-color` / `NO_COLOR` / non-TTY auto-disable for ANSI styling. Vault-relative paths SHALL be used in any path-bearing diagnostics.

#### Scenario: Table output format
- **WHEN** `ft review --since 7d` is run in a TTY
- **THEN** stdout contains one row per link in the prescribed format

#### Scenario: No-color mode
- **WHEN** `NO_COLOR=1 ft review --since 7d` is run
- **THEN** output contains no ANSI escape sequences

### Requirement: JSON output mode
With `--json`, the command SHALL emit a JSON array where each element has fields: `count` (integer), `target` (string, e.g. `"Foo"`), `is_ghost` (boolean), `source_paths` (array of vault-relative path strings — the paths whose paragraphs contributed).

#### Scenario: JSON structure
- **WHEN** `ft review --since 7d --json` is run and two links are produced
- **THEN** stdout is valid JSON: an array of exactly two objects, each with the four named fields

### Requirement: Empty-window handling
When no commits in the window add any countable wikilinks, the command SHALL exit 0 and print a clear "no new links in window" message (text mode) or an empty JSON array (`--json`).

#### Scenario: No commits in window
- **WHEN** `ft review --since 1m` is run in a vault with no recent commits
- **THEN** the command exits 0 and prints a "no new links" message

#### Scenario: No new wikilinks in window
- **WHEN** commits exist in the window but none added a wikilink
- **THEN** the command exits 0 and the table is empty (or `--json` emits `[]`)

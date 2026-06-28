## ADDED Requirements

### Requirement: ft notes related subcommand
`ft notes related <note>` SHALL be a read-only subcommand under `ft notes`. `<note>` SHALL be resolved via the shared note-or-ghost resolver (same `[[]]`-aware path as `ft notes journal`): exact vault-relative path → title lookup → fuzzy match → `[[Phantom]]` / bare-name ghost fallback. The command SHALL resolve to either a `NodeKind::Note` or a `NodeKind::Ghost`, call `ft_core::related::score_related`, and print the result. It SHALL NOT modify any files and SHALL NOT require a git repository (scoring is pure graph).

The command SHALL accept a single positional NOTE argument. A `--link` form SHALL NOT be supported (unlike `ft notes journal`, multi-source related scoring is out of scope; `<note>` may itself be a `[[Ghost]]` to target a phantom).

#### Scenario: Note target prints scored concepts
- **WHEN** the user runs `ft notes related "Foo"` and Foo.md exists and co-occurs with concepts Bar (score 3) and Baz (score 1)
- **THEN** stdout lists Bar and Baz with their scores, sorted descending by score

#### Scenario: Ghost (phantom) target prints scored concepts
- **WHEN** the user runs `ft notes related "[[Phantom]]"` where Phantom has no backing file but is linked from paragraphs that also link to Bar
- **THEN** stdout lists Bar with its score; the command succeeds without error

#### Scenario: No git repository required
- **WHEN** `ft notes related "Foo"` is run in a vault that is not inside a git repository
- **THEN** the command succeeds and prints scored concepts (unlike `ft notes journal`, no git/blame dependency)

#### Scenario: Read-only — no files modified
- **WHEN** `ft notes related "Foo"` is run
- **THEN** no note file in the vault is modified (the command is purely read-only)

#### Scenario: Empty result exits non-zero by default
- **WHEN** `ft notes related "Foo"` is run and no concepts co-occur with Foo
- **THEN** the command exits with a non-zero code (parity with `ft notes backlinks`/`links`)

#### Scenario: --allow-empty succeeds on empty result
- **WHEN** `ft notes related "Foo" --allow-empty` is run and no concepts co-occur with Foo
- **THEN** the command exits with code 0

#### Scenario: Unknown note exits with error
- **WHEN** the `<note>` argument resolves to neither a note nor a ghost
- **THEN** the command exits with a non-zero code and a human-readable error

### Requirement: ft notes related output formats
`ft notes related` SHALL support `--format table|json|ndjson|markdown` (default `table`), `--no-color`, and honor `NO_COLOR` / non-TTY auto-disable for ANSI styling — parity with `ft notes backlinks`/`links` and `ft notes journal`.

Each row SHALL carry: `title` (the concept's filename stem or ghost raw target), `score` (the aggregate co-occurrence score), `already_in_related` (boolean), and the candidate's path. The candidate path SHALL be serialized as `Resolved { path }` for notes and `Unresolved { raw }` for ghosts (matching `LinkRowTarget` from `ft notes links`).

The `already_in_related` rows (concepts already declared in the target's `## Related` section) SHALL be included in the output, marked, so the printed list matches what the TUI Related modal shows.

#### Scenario: Default table output
- **WHEN** `ft notes related "Foo"` is run in a TTY with color
- **THEN** stdout contains a table with columns for the concepts and their scores, including already-in-related rows marked distinctly

#### Scenario: JSON output structure
- **WHEN** `ft notes related "Foo" --format json` is run and two concepts match
- **THEN** stdout is a valid JSON array with two objects, each containing `title`, `score`, `already_in_related`, and a path field (`{kind:"resolved", path}` or `{kind:"unresolved", raw}`)

#### Scenario: NDJSON output structure
- **WHEN** `ft notes related "Foo" --format ndjson` is run
- **THEN** stdout is one JSON object per line, each with `title`, `score`, `already_in_related`, and the path field

#### Scenario: Markdown output
- **WHEN** `ft notes related "Foo" --format markdown` is run
- **THEN** stdout is markdown listing the concepts and scores (parity with `ft notes links --format markdown`)

#### Scenario: No-color mode
- **WHEN** `NO_COLOR=1 ft notes related "Foo"` is run
- **THEN** output contains no ANSI escape sequences

#### Scenario: Non-TTY auto-disables color
- **WHEN** `ft notes related "Foo"` is run with stdout redirected (non-TTY) and no `--no-color`
- **THEN** output contains no ANSI escape sequences

### Requirement: already-in-related rows included in CLI output
`ft notes related` SHALL include concepts with `already_in_related == true` in its output (they are not filtered out), so the printed list is a faithful read of the same scored data the TUI Related modal displays. This holds for all four output formats.

#### Scenario: Already-in-related concept appears marked
- **WHEN** `ft notes related "N"` is run and N's `## Related` section already declares `[[Alias]]` which also co-occurs with N
- **THEN** Alias appears in the output with `already_in_related` set true (JSON) / marked in the table

#### Scenario: Ghost target has no already-in-related rows
- **WHEN** `ft notes related "[[Phantom]]"` is run
- **THEN** no row has `already_in_related == true` (a ghost has no Related section to read aliases from)

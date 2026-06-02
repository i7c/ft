## MODIFIED Requirements

### Requirement: `ft notes` namespace contains only single-note operations

The `ft notes` namespace SHALL contain only subcommands that operate on a single note. The complete subcommand set SHALL be: `open`, `move-section`, `create`, `today`, `periodic`, `rename`, `mv`, `update-related`, `append`, and the new `find`. The subcommands `backlinks`, `links`, and `journal` SHALL NOT be present under `ft notes`.

#### Scenario: notes subcommand set
- **WHEN** the user runs `ft notes --help`
- **THEN** the listed subcommands are exactly `open`, `move-section`, `create`, `today`, `periodic`, `rename`, `mv`, `update-related`, `append`, `find` — no more, no fewer

#### Scenario: removed subcommands error
- **WHEN** the user runs `ft notes backlinks foo`
- **THEN** the CLI exits with a non-zero code and names `ft graph backlinks` as the replacement (see `cli-namespace-graph`)

### Requirement: `ft notes find` replaces `ft find`

The `ft find` top-level subcommand SHALL be removed. The same fuzzy-find functionality SHALL be reachable as `ft notes find <query>` with the same args (`--format table|json|ndjson|markdown`, `--limit`, heading-anchor syntax `text#heading`, etc.).

#### Scenario: ft notes find works
- **WHEN** the user runs `ft notes find meeting`
- **THEN** the CLI prints the same output that `ft find meeting` produced before this change

#### Scenario: ft notes find with heading anchor
- **WHEN** the user runs `ft notes find 'meeting#Action items'`
- **THEN** the CLI prints heading-anchored matches identically to the pre-move behaviour

#### Scenario: ft find produces moved error
- **WHEN** the user runs `ft find foo`
- **THEN** the CLI exits with code 2 and stderr contains `'ft find' has moved to 'ft notes find'`

### Requirement: `ft notes open` keeps its current surface

The `ft notes open <query>` subcommand SHALL retain its current behaviour: same fuzzy syntax as `ft notes find`, opens the top hit in `$EDITOR` (or via `obsidian://open` with `--obsidian`).

#### Scenario: open unchanged
- **WHEN** the user runs `ft notes open finance`
- **THEN** the top fuzzy hit is opened in `$EDITOR`, identical to pre-change behaviour

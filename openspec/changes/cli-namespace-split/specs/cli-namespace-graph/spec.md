## ADDED Requirements

### Requirement: `ft graph` exposes `backlinks`, `links`, `journal` subcommands

The `ft graph` namespace SHALL expose, in addition to the existing `query` subcommand, three new subcommands: `backlinks <note>`, `links <note>`, `journal <note>`. Each SHALL have the same args, output formats, and behaviour as the previously-located `ft notes backlinks`, `ft notes links`, `ft notes journal` commands.

#### Scenario: backlinks command
- **WHEN** the user runs `ft graph backlinks finance`
- **THEN** the command prints the same output that `ft notes backlinks finance` produced before the move, with identical exit codes and `--format table|json|ndjson|markdown` support

#### Scenario: links command
- **WHEN** the user runs `ft graph links Journal/2026-05-15.md`
- **THEN** the command prints outgoing links (including unresolved ghosts), identical to the pre-move behaviour

#### Scenario: journal command
- **WHEN** the user runs `ft graph journal Areas/finance.md`
- **THEN** the command prints the reverse-chronological paragraph-mention feed with date support from `BlameCache`, identical to the pre-move behaviour

### Requirement: Old `ft notes backlinks|links|journal` paths produce a clear error

When the user invokes `ft notes backlinks`, `ft notes links`, or `ft notes journal`, the CLI SHALL exit non-zero with an error naming the new path under `ft graph`.

#### Scenario: backlinks moved error
- **WHEN** the user runs `ft notes backlinks foo`
- **THEN** the CLI exits with code 2 and stderr contains `'ft notes backlinks' has moved to 'ft graph backlinks'`

#### Scenario: links moved error
- **WHEN** the user runs `ft notes links foo`
- **THEN** the CLI exits with code 2 and stderr contains `'ft notes links' has moved to 'ft graph links'`

#### Scenario: journal moved error
- **WHEN** the user runs `ft notes journal foo`
- **THEN** the CLI exits with code 2 and stderr contains `'ft notes journal' has moved to 'ft graph journal'`

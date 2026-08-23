# notes-journal

## MODIFIED Requirements

### Requirement: ft notes gather subcommand

`ft notes gather <note>` SHALL be a subcommand under `ft notes` that is DEPRECATED: it SHALL be hidden from help (`ft notes --help` SHALL NOT list it), and each invocation SHALL print a single deprecation warning on stderr pointing at `ft notes search` as the successor. The command SHALL remain fully functional and read-only while deprecated — `<note>` is a fuzzy note selector (same resolution as `ft notes open`) that resolves to a single vault note N, and the command SHALL NOT modify any files. The same deprecation applies to `ft notes gather --link ...` and the `ft notes journal` alias. The `ft notes search` command SHALL NOT be hidden.

#### Scenario: Invocation with known note
- **WHEN** the user runs `ft notes gather "Foo"` in a deprecated build
- **THEN** the command resolves note `Foo.md`, prints the deprecation warning on stderr, builds the journal, and prints the result to stdout

#### Scenario: Hidden from help
- **WHEN** the user runs `ft notes --help`
- **THEN** `gather` and `journal` are NOT listed, and `search` IS listed

#### Scenario: Ambiguous note name exits with error
- **WHEN** the note selector matches more than one note
- **THEN** the command exits with a non-zero code and a human-readable error listing the candidates

#### Scenario: Unknown note exits with error
- **WHEN** the note selector matches no note in the vault
- **THEN** the command exits with a non-zero code

## ADDED Requirements

### Requirement: Recent remains unaffected

`ft notes recent` SHALL NOT be deprecated, hidden, or changed by the gather deprecation. Its citation badges, `--uncited` filter, and blame-based ordering SHALL continue to work as specified.

#### Scenario: Recent still listed and functional
- **WHEN** the user runs `ft notes recent --since 7d` and `ft notes --help` in a deprecated build
- **THEN** the command prints the recency feed without a deprecation warning, and `recent` is listed in help

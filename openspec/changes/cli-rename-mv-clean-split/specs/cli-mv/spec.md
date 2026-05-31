## ADDED Requirements

### Requirement: ft notes mv moves one or more sources to a target directory

The system SHALL provide `ft notes mv <SOURCES>... <TARGET>` where each source is a vault-relative path (note or directory) and target is a vault-relative path to an existing directory. The command SHALL move every note (directly specified or discovered via directory expansion) to the target directory, update all vault-wide references, and print a summary.

#### Scenario: Move a single note to a directory

- **WHEN** `ft notes mv foo.md archive/` is run and `foo.md` and `archive/` exist
- **THEN** `foo.md` is renamed to `archive/foo.md`, all references to `foo` are updated, and "moved 1 note(s) to archive/" is printed

#### Scenario: Move multiple notes to a directory

- **WHEN** `ft notes mv a.md b.md target/` is run with both notes existing and `target/` existing
- **THEN** `a.md` → `target/a.md`, `b.md` → `target/b.md`, all references updated, "moved 2 note(s) to target/" printed

#### Scenario: Move a directory

- **WHEN** `ft notes mv projects/old/ archive/` is run, `projects/old/` contains `projects/old/a.md` and `projects/old/b.md`, and `archive/` exists
- **THEN** `a.md` → `archive/old/a.md`, `b.md` → `archive/old/b.md`, all references updated, and the old `projects/old/` directory is removed if empty

#### Scenario: Move mixed notes and directories

- **WHEN** `ft notes mv alpha.md projects/beta/ target/` is run
- **THEN** `alpha.md` → `target/alpha.md` and all files under `projects/beta/` are moved to `target/beta/...`, all references updated

### Requirement: mv sources are vault-relative paths only

The system SHALL resolve each source argument as a vault-relative path. `.md` extension is optional for notes (auto-appended when the bare path doesn't exist but `<path>.md` does). Directories are resolved via graph `DirData` nodes. Title resolution, fuzzy matching, and `[[Ghost]]` syntax SHALL NOT be supported.

#### Scenario: Source with .md extension

- **WHEN** `ft notes mv foo.md target/` is run
- **THEN** `foo.md` is resolved as a note at the vault root

#### Scenario: Source without .md extension

- **WHEN** `ft notes mv foo target/` is run and `foo.md` exists but `foo` (no extension) does not
- **THEN** `.md` is auto-appended, resolving to `foo.md`

#### Scenario: Source is a directory path

- **WHEN** `ft notes mv projects/old/ target/` is run and `projects/old/` has notes under it
- **THEN** the directory is expanded to its contained notes via the graph

#### Scenario: Source does not exist

- **WHEN** `ft notes mv nonexistent.md target/` is run and no file or directory matches
- **THEN** the command exits 2 with "source not found: nonexistent.md"

### Requirement: mv target must exist as a directory

The system SHALL error if the target path does not resolve to an existing directory on disk. The target SHALL NOT be auto-created.

#### Scenario: Target directory exists

- **WHEN** `ft notes mv foo.md archive/` is run and `archive/` exists
- **THEN** the move proceeds

#### Scenario: Target is a file, not a directory

- **WHEN** `ft notes mv foo.md existing.md` is run and `existing.md` is a file
- **THEN** the command exits 2 with "target is not a directory: existing.md"

#### Scenario: Target does not exist

- **WHEN** `ft notes mv foo.md newdir/` is run and `newdir/` does not exist
- **THEN** the command exits 2 with "target directory not found: newdir/"

### Requirement: mv requires at least one source and one target

The system SHALL error with a usage message if fewer than 2 positional arguments are provided.

#### Scenario: Missing arguments

- **WHEN** `ft notes mv` is run with no arguments
- **THEN** the command exits 2 with a usage message

#### Scenario: Only one argument

- **WHEN** `ft notes mv foo.md` is run
- **THEN** the command exits 2 with a usage message (need a target)

### Requirement: mv supports --dry-run

The system SHALL print the plan summary and exit without modifying any files when `--dry-run` is passed.

#### Scenario: Dry-run prints plan

- **WHEN** `ft notes mv foo.md bar.md archive/ --dry-run` is run
- **THEN** the plan is printed showing source → target mappings and link counts; no files are modified

### Requirement: mv reports link updates

The system SHALL print the number of link rewrites and affected files after a successful move.

#### Scenario: Output format

- **WHEN** `ft notes mv foo.md archive/` completes successfully
- **THEN** output includes "moved 1 note(s) to archive/" and "updated N link(s) in M file(s)"

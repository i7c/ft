# cli-rename Specification

## Purpose
TBD - created by archiving change cli-rename-mv-clean-split. Update Purpose after archive.
## Requirements
### Requirement: rename rejects NEW values containing /

The system SHALL reject `<NEW>` values that contain `/` with exit code 2 and an error message directing the user to `ft notes mv`.

#### Scenario: Path in NEW is rejected

- **WHEN** `ft notes rename foo archive/foo.md` is run (or any `<NEW>` containing `/`)
- **THEN** exit code 2, stderr contains "use `ft notes mv` to change directories"

### Requirement: rename accepts bare filename stems

The system SHALL accept `<NEW>` as a bare filename stem (no `/`). `.md` is appended automatically when missing. The note is renamed in its current directory. All existing behavior for ghost renaming, `--dry-run`, and resolving `<NOTE>` by title/fuzzy/path/`[[Phantom]]` is preserved.

#### Scenario: Bare name rename

- **WHEN** `ft notes rename foo bar` is run and `foo` resolves to `foo.md`
- **THEN** `foo.md` is renamed to `bar.md` in the same directory, all references updated

#### Scenario: Rename with explicit .md

- **WHEN** `ft notes rename foo bar.md` is run
- **THEN** `foo.md` is renamed to `bar.md` in the same directory (`.md` not doubled)

#### Scenario: Rename ghost

- **WHEN** `ft notes rename "[[Phantom]]" Real` is run
- **THEN** all `[[Phantom]]` references are rewritten to `[[Real]]`; no file is created

#### Scenario: Dry-run still works

- **WHEN** `ft notes rename foo bar --dry-run` is run
- **THEN** the plan is printed; no files modified

<!-- Migration note: this change split the previous `rename` behaviour.
     Paths-in-NEW (which moved a note to a different directory) are no
     longer accepted by `rename` — use `ft notes mv <note-path> <target-dir>/`
     instead. Example: `ft notes rename foo archive/foo.md` →
     `ft notes mv foo.md archive/`. The "rename rejects NEW values
     containing /" requirement above is the canonical enforcement. -->


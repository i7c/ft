# notes-quote

## Purpose

`ft notes quote` — a read-only plumbing command that emits the
canonical protected-section callout (`> [!ft-source] ...`) for an
arbitrary line range of a vault file, using exactly the same pinning
mechanics as `ft notes synth scaffold`/`grow`. It is the stable CLI
contract for scripts and external tools (notably `ft.nvim`) that want
to pin source paragraphs without going through the gather/journal
scaffold flow.

## ADDED Requirements

### Requirement: ft notes quote command surface

`ft notes quote <FILE> --lines A-B` SHALL emit a single protected
section for the given vault file and line range. `<FILE>` SHALL be a
vault-relative path (absolute paths SHALL be accepted and relativized
for the callout header; the `.md` extension SHALL NOT be auto-appended).
`--lines A-B` SHALL be required, 1-indexed inclusive, with `A` and `B`
positive integers and `A <= B`. The command SHALL be read-only: it
SHALL NOT write, create, or modify any file, SHALL NOT launch an
editor, and SHALL NOT prompt. The exit code SHALL be 0 on success and
1 on any error, with a human-readable message on stderr.

#### Scenario: Emit a callout for a clean range
- **WHEN** `notes/foo.md` contains `First line.\nSecond line.` at
  HEAD, the working tree is clean for that file, and the user runs
  `ft notes quote notes/foo.md --lines 1-2`
- **THEN** stdout is exactly one line `> [!ft-source] "notes/foo.md"
  L1-2 @<sha7> #<hash6>` followed by `> First line.` and `> Second
  line.` on subsequent lines, ending with a single trailing newline,
  and the exit code is 0

#### Scenario: Absolute path is relativized
- **WHEN** the user runs `ft notes quote /vault/path/notes/foo.md
  --lines 1-1`
- **THEN** the callout header contains the vault-relative path
  `notes/foo.md`

#### Scenario: Read-only guarantee
- **WHEN** the command runs successfully in a vault
- **THEN** no file in the vault is created, modified, or deleted, and
  no editor is launched

### Requirement: Source-file prerequisites

The command SHALL fail before emitting a callout when the source file
is missing, unreadable, or not safely pinnable to HEAD. The file SHALL
exist and be readable as UTF-8 text. The file SHALL be committed at
HEAD and unmodified in the working tree — i.e. it SHALL NOT appear in
`git status` as modified, staged, deleted, conflicted, or untracked.
Other dirty files in the repository SHALL NOT block the command. On
failure the error SHALL name the file; a dirty file SHALL additionally
report that a clean, committed source is required.

#### Scenario: Missing file errors
- **WHEN** the user runs `ft notes quote does-not-exist.md --lines
  1-1`
- **THEN** the command exits 1 with an error naming
  `does-not-exist.md`, and nothing is printed to stdout

#### Scenario: Uncommitted edit blocks
- **WHEN** `notes/foo.md` has an uncommitted edit in the working tree
  and the user runs `ft notes quote notes/foo.md --lines 1-1`
- **THEN** the command exits 1 with an error naming `notes/foo.md` and
  stating the source must be committed and unmodified, and nothing is
  printed to stdout

#### Scenario: Untracked file blocks
- **WHEN** `notes/new.md` is untracked (never committed) and the user
  runs `ft notes quote notes/new.md --lines 1-1`
- **THEN** the command exits 1, because an untracked file has no HEAD
  version to pin against

#### Scenario: Unrelated dirty files do not block
- **WHEN** `notes/other.md` is dirty but `notes/foo.md` is clean, and
  the user runs `ft notes quote notes/foo.md --lines 1-1`
- **THEN** the command succeeds and emits the callout

### Requirement: Line-range validation

The line range SHALL be validated against the file's actual line count
before emission. A file whose content ends with a newline SHALL NOT
count that trailing newline as an extra line (a file containing
`a\nb\n` SHALL have 2 lines). Ranges where `A < 1`, `A > B`, or `B`
exceeds the file's line count SHALL fail with an error that names the
file and states the actual number of lines.

#### Scenario: Range within bounds
- **WHEN** a file has 10 lines and the user runs `ft notes quote
  <file> --lines 3-5`
- **THEN** the callout body is the verbatim text of lines 3, 4, and 5
  joined with `\n`, with no trailing newline in the body

#### Scenario: Range past the last line errors
- **WHEN** a file has 10 lines and the user runs `ft notes quote
  <file> --lines 9-11`
- **THEN** the command exits 1 with an error stating the file has 10
  lines

#### Scenario: Trailing newline is not a line
- **WHEN** a file contains exactly `a\nb\n` (2 lines) and the user
  runs `ft notes quote <file> --lines 1-2`
- **THEN** the callout body is `a\nb`, and the range is accepted

### Requirement: Pin construction identical to scaffold

The emitted section SHALL pin the same tokens the scaffold planner
produces: the commit SHALL be the current HEAD of the enclosing
repository shortened to 7 hex chars; the content hash SHALL be the
first 6 hex chars of the blake3 digest of the body text (lines joined
with `\n`, no trailing newline); the header SHALL match the canonical
grammar `> [!ft-source] "<vault-rel-path>" L<a>-<b> @<sha7> #<hash6>`
and the body SHALL be `> `-prefixed. A section emitted by this command
SHALL verify `ok` via `ft notes synth verify` against the same file
and range at the same commit.

#### Scenario: Round-trips through verify
- **WHEN** the command emits a section for a clean range and that
  output is placed in a synth note
- **THEN** `ft notes synth verify` reports the section `ok`

#### Scenario: Same mechanics as scaffold
- **WHEN** the command emits a section for a range and `ft notes synth
  scaffold` pins the same range from the same file at the same HEAD
- **THEN** both outputs have identical path, line range, commit SHA,
  content hash, and body

# notes-export Specification

## Purpose

`ft notes export` — a read-only plumbing command that renders a vault
note (or an original-file line range of it) as clean, portable
CommonMark, stripping the vault-specific structure: the frontmatter
block, `[!ft-source]` callout headers (body lines survive as
blockquotes), and `[[wikilinks]]` (converted to plain text / CommonMark
images). It is the inverse of `ft notes quote` (which wraps a raw range
*into* an ft pin) and the stable CLI contract for pasting, publishing,
and external tools. The stripping rules live behind an extensible
target seam; `commonmark` is the v1 target, with `plaintext` and
`slack` planned.

## Requirements

### Requirement: ft notes export command surface

`ft notes export <FILE> [--lines A-B] [--format TARGET]` SHALL print
the vault-stripped content of the given vault file to stdout. `<FILE>`
SHALL be a vault-relative path (absolute paths SHALL be accepted and
relativized; the `.md` extension SHALL NOT be auto-appended). `--lines`
SHALL be optional and 1-indexed inclusive with the short alias `-l`;
when absent the whole file SHALL be exported. `--format` SHALL accept
`commonmark` and default to it; any other value SHALL be rejected.
The command SHALL be read-only: it SHALL NOT write, create, or modify
any file, SHALL NOT launch an editor, SHALL NOT prompt, and SHALL NOT
require or consult git. The output SHALL be printed to stdout with a
single trailing newline; an empty export SHALL print no bytes. The
exit code SHALL be 0 on success and 1 on any error, with a
human-readable message on stderr.

#### Scenario: Export a whole note
- **WHEN** `notes/foo.md` contains a frontmatter block followed by a
  body and the user runs `ft notes export notes/foo.md`
- **THEN** stdout is the body with vault elements stripped, ending
  with a single trailing newline, and the exit code is 0

#### Scenario: Short flag alias
- **WHEN** the user runs `ft notes export notes/foo.md -l 2-4`
- **THEN** the output is byte-identical to running the same command
  with `--lines 2-4`, and the exit code is 0

#### Scenario: Format flag defaults and validates
- **WHEN** the user runs `ft notes export notes/foo.md` or
  `ft notes export notes/foo.md --format commonmark`
- **THEN** the output is identical, and the exit code is 0
- **WHEN** the user runs `ft notes export notes/foo.md --format
  plaintext`
- **THEN** the command fails with an error and prints nothing to
  stdout

#### Scenario: Absolute path is relativized
- **WHEN** the user runs `ft notes export /vault/path/notes/foo.md`
- **THEN** the command succeeds, and any error messages name the
  vault-relative path `notes/foo.md`

#### Scenario: Read-only guarantee
- **WHEN** the command runs successfully in a vault
- **THEN** no file in the vault is created, modified, or deleted, and
  no editor is launched

#### Scenario: Missing file errors
- **WHEN** the user runs `ft notes export does-not-exist.md`
- **THEN** the command exits 1 with an error naming
  `does-not-exist.md`, and nothing is printed to stdout

#### Scenario: No md auto-append
- **WHEN** only `notes/foo.md` exists and the user runs
  `ft notes export notes/foo`
- **THEN** the command exits 1 with an error naming `notes/foo`

### Requirement: Frontmatter stripping and line-range clamping

The leading frontmatter block (the `---` … `---` block at the very top of the file) SHALL be entirely excluded from the export, including both fences. `--lines` numbers SHALL refer to the original file's lines. The effective range start SHALL be clamped to the first line after the frontmatter closing fence, so any range that includes frontmatter lines SHALL have those lines dropped, and a range whose lines all fall at or before the closing fence SHALL produce empty output (exit 0, no bytes). A file without a frontmatter block SHALL have no clamp (line 1 is the first body line). Stripping the frontmatter SHALL NOT add or remove any other line: the output SHALL contain exactly the selected original-file lines, transformed per the other requirements.

#### Scenario: Range starts after the frontmatter
- **WHEN** `notes/foo.md` has frontmatter on lines 1-5, line 6 is
  `First body line.`, line 7 is `Second body line.`, and the user runs
  `ft notes export notes/foo.md --lines 6-7`
- **THEN** stdout is `First body line.\nSecond body line.\n`

#### Scenario: Whole-file export drops frontmatter
- **WHEN** `notes/foo.md` has frontmatter on lines 1-5 followed by a
  body and the user runs `ft notes export notes/foo.md` (no range)
- **THEN** stdout contains the body only, and no frontmatter line
  appears in the output

#### Scenario: Mixed range clamps to the body
- **WHEN** `notes/foo.md` has frontmatter on lines 1-5 and the user
  runs `ft notes export notes/foo.md --lines 1-7`
- **THEN** stdout is exactly the transformed lines 6 and 7

#### Scenario: Range fully inside frontmatter is empty
- **WHEN** `notes/foo.md` has frontmatter on lines 1-5 and the user
  runs `ft notes export notes/foo.md --lines 1-3`
- **THEN** stdout is empty, no bytes are printed, and the exit code
  is 0

#### Scenario: Blank line after frontmatter is respected
- **WHEN** `notes/foo.md` has frontmatter on lines 1-5, line 6 is
  blank, and the user runs `ft notes export notes/foo.md --lines 6-7`
- **THEN** stdout begins with the blank line followed by line 7 —
  the blank line is not removed

#### Scenario: No frontmatter means no clamp
- **WHEN** `notes/foo.md` has no frontmatter block and the user runs
  `ft notes export notes/foo.md --lines 1-2`
- **THEN** stdout is the first two lines of the file

### Requirement: ft-source callout conversion

Every line matching the canonical `[!ft-source]` header grammar (`> [!ft-source] "<path>" L<a>-<b> @<sha> #<hash>`) SHALL be dropped from the export. The `>`-prefixed body lines of a callout SHALL be kept verbatim, including their `>` prefixes — they are valid CommonMark blockquotes. A line that resembles a callout header but does not match the canonical grammar (e.g. a missing token) SHALL be kept verbatim. Callout handling SHALL be per-line, so a range that starts or ends inside a callout SHALL still yield valid CommonMark output (body lines inside the range survive as blockquotes; header lines inside the range drop).

#### Scenario: Callout becomes a blockquote
- **WHEN** `notes/foo.md` contains a canonical callout with a
  two-line body and the user exports the whole file
- **THEN** the header line `> [!ft-source] "…" L…-… @… #…` is absent
  from the output, and both `> body` lines appear verbatim

#### Scenario: Malformed header is preserved
- **WHEN** `notes/foo.md` contains `> [!ft-source] "notes/foo.md"`
  (missing the line range, SHA, and hash)
- **THEN** that line appears verbatim in the output

#### Scenario: Partial range through a callout
- **WHEN** a callout header is at line 10 with body lines 11-13, and
  the user exports `--lines 11-13`
- **THEN** stdout is lines 11-13 as `> `-prefixed blockquote lines
  with no header line, and the output is valid CommonMark

### Requirement: Wikilink and embed conversion

In the exported content, wikilinks SHALL be converted to plain text
and Obsidian embeds SHALL be converted to CommonMark images according
to this table:

| Source | Output |
|---|---|
| `[[T]]` | `T` |
| `[[T\|D]]` | `D` |
| `[[T#A]]` | `T` |
| `[[T#A\|D]]` | `D` |
| `[[#A]]` | `#A` |
| `[[#A\|D]]` | `D` |
| `![[T]]` | `![T](href)` |
| `![[T\|D]]` | `![D](href)` |
| `![[T#A]]` | `![T](href)` |
| `![[T#A\|D]]` | `![D](href)` |

For embeds, `href` SHALL be `T`, wrapped in angle brackets `<…>` when
it contains whitespace. The conversion SHALL apply to every
non-dropped line, including blockquote lines and the kept body lines
of ft-source callouts. It SHALL NOT apply inside inline code spans
(backtick runs) or fenced code blocks. Markdown links `[x](y)` and
Markdown images `![alt](src)` SHALL be preserved verbatim and never
reinterpreted. A wikilink whose target is the note itself SHALL be
treated like any other link. Constructs that are not real links —
`[[]]`, `[[\|D]]` (no target and no anchor), and an unterminated `[[`
— SHALL be left verbatim.

#### Scenario: Wikilink becomes plain text
- **WHEN** the body contains `see [[Some other file]]`
- **THEN** the output contains `see some other file`

#### Scenario: Alias uses display text
- **WHEN** the body contains `[[Foo|Bar]]` and `[[Foo#H|Baz]]`
- **THEN** the output contains `Bar` and `Baz`

#### Scenario: Anchor is dropped on cross-file links
- **WHEN** the body contains `[[Foo#Heading]]`
- **THEN** the output contains `Foo`

#### Scenario: Same-file anchor strips brackets
- **WHEN** the body contains `[[#Heading]]`
- **THEN** the output contains `#Heading`

#### Scenario: Embed becomes a CommonMark image
- **WHEN** the body contains `![[image.png]]` and `![[image.png|alt
  text]]`
- **THEN** the output contains `![image.png](image.png)` and
  `![alt text](image.png)`

#### Scenario: Embed href with whitespace is angle-bracketed
- **WHEN** the body contains `![[my image.png]]`
- **THEN** the output contains `![my image.png](<my image.png>)`

#### Scenario: Code spans and fences are untouched
- **WHEN** the body contains `` `[[Foo]]` `` and a fenced block
  containing `[[Bar]]`
- **THEN** both `[[Foo]]` and `[[Bar]]` appear verbatim in the output

#### Scenario: Conversion inside blockquotes
- **WHEN** the body contains `> [!note] Title` followed by
  `> See [[Foo]]`
- **THEN** the output contains `> See Foo` — the callout block is
  preserved as a blockquote and its wikilink is converted

#### Scenario: Markdown links are preserved
- **WHEN** the body contains `[text](foo.md)` and `![alt](img.png)`
- **THEN** both appear verbatim in the output

#### Scenario: Multiple links on one line
- **WHEN** the body contains `[[A]] and [[B|bee]] and ![[c.png]]`
- **THEN** the output contains `A and bee and ![c.png](c.png)`

### Requirement: Pass-through of other content

All content other than the listed stripped/converted elements SHALL
pass through verbatim: ATX headings, paragraphs, task list items
(including any task emoji metadata such as priority and date emoji),
non-ft Obsidian callouts (`> [!note]`, `> [!warning]`), blockquotes,
fenced and indented code blocks, lists, tables, and horizontal rules.

#### Scenario: Task lines preserved with emoji
- **WHEN** the body contains `- [ ] ⏫ 📅 2026-08-05 Finish the report`
- **THEN** the line appears verbatim in the output

#### Scenario: Non-ft callouts preserved
- **WHEN** the body contains `> [!warning] This is important`
- **THEN** the line appears verbatim in the output

#### Scenario: Heading converts only its wikilink
- **WHEN** the body contains `# [[Foo]]`
- **THEN** the output contains `# Foo`

### Requirement: Line-range validation

`--lines A-B` SHALL require positive integers for `A` and `B` with
`A <= B`; any violation SHALL fail with a parse error and print
nothing to stdout. `B` SHALL NOT exceed the file's raw line count
(a trailing newline is not an extra line: a file containing `a\nb\n`
SHALL have 2 lines). A `B` beyond the file's line count SHALL fail
with an error naming the file and stating the actual number of lines.

#### Scenario: Valid range within bounds
- **WHEN** a file has 10 lines and the user runs `ft notes export
  <file> --lines 3-5`
- **THEN** the output is the transformed lines 3, 4, and 5

#### Scenario: Range past the last line errors
- **WHEN** a file has 10 lines and the user runs `ft notes export
  <file> --lines 9-11`
- **THEN** the command exits 1 with an error naming the file and
  stating it has 10 lines, and nothing is printed to stdout

#### Scenario: Trailing newline is not a line
- **WHEN** a file contains exactly `a\nb\n` and the user runs
  `ft notes export <file> --lines 1-2`
- **THEN** the export succeeds and includes both lines

#### Scenario: Invalid range specifications fail
- **WHEN** the user passes `--lines` values `1`, `a-b`, `0-1`,
  `2-1`, `1-0`, or `-1-2`
- **THEN** the command fails with a parse error and prints nothing
  to stdout

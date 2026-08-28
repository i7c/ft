## ADDED Requirements

### Requirement: Soft-break resolution (--unwrap)

The export driver SHALL resolve hard-wrapped source lines into single
logical lines when soft-break resolution is enabled: lines separated
only by a bare `\n` within the same block (a CommonMark soft break,
which renders as a space) SHALL join with a single space. Soft-break
resolution SHALL be enabled by default for the `slack` target and
disabled by default for the `commonmark` target. `--unwrap` SHALL
enable it and `--no-unwrap` SHALL disable it for either target;
passing both SHALL fail with an error and print nothing to stdout.
The join SHALL apply only within a block, never across a block
boundary:

- Consecutive paragraph lines SHALL join into one line.
- An indented line that is not a list-item marker following an open
  list item SHALL join into that item's first line, preserving the
  item marker, the target's indentation normalization, and all other
  target transforms.
- Consecutive blockquote lines SHALL join (`> a` / `> b` → `> a b`,
  the first line's `>` prefixes winning), except that a line SHALL NOT
  join into a callout-title line (`> [!type] Title` keeps its body on
  a separate line), and a blockquote line whose content is empty
  (`>`) SHALL be a boundary that never joins.

The join SHALL NOT cross: blank lines, list-item marker lines, ATX
headings, thematic rules, code lines (fenced or indented), or a line
ending in a CommonMark hard break (a lone trailing `\` or two or more
spaces). Code lines SHALL pass through verbatim and SHALL NOT join.
A dropped line (an ft-source callout header) SHALL act as a boundary:
it flushes any open logical line and contributes nothing. A `--lines`
range whose first selected line is a continuation SHALL begin a fresh
logical line (no join across the range boundary).

#### Scenario: Wrapped paragraph joins (slack default)
- **WHEN** the body contains a paragraph split across two lines
  (`first line` / `second line`, no blank between) and the format is
  `slack`
- **THEN** the output contains a single line `first line second line`

#### Scenario: Wrapped list item joins
- **WHEN** the body contains `- line items that are long` followed by
  an indented continuation `  and thus are broken` and the format is
  `slack`
- **THEN** the output contains a single bullet line
  `- line items that are long and thus are broken`

#### Scenario: Nested items do not join
- **WHEN** the body contains `- a` / `  - b` and the format is `slack`
- **THEN** the output contains two bullet lines (`- a` and a nested
  `    - b`) — a marker line always starts a new item

#### Scenario: Blank lines separate paragraphs
- **WHEN** the body contains two wrapped paragraphs separated by a
  blank line and the format is `slack`
- **THEN** the output contains the two joined paragraphs on separate
  lines with the blank line between them

#### Scenario: Hard breaks are preserved
- **WHEN** a paragraph line ends with a lone trailing `\` or two or
  more spaces and the format is `slack`
- **THEN** the following line does not join into it — both lines
  appear separately

#### Scenario: Code blocks are never joined
- **WHEN** a fenced or indented code block contains consecutive lines
  and the format is `slack`
- **THEN** the lines pass through verbatim, each on its own output
  line

#### Scenario: Callout title does not absorb its body
- **WHEN** the body contains `> [!note] Title` followed by `> body`
  and the format is `slack`
- **THEN** the output contains `> Title` and `> body` on separate
  lines

#### Scenario: Quoted paragraph joins
- **WHEN** the body contains `> quoted line one` followed by
  `> quoted line two` (no callout marker) and the format is `slack`
- **THEN** the output contains a single line `> quoted line one
  quoted line two`

#### Scenario: Headings do not absorb the following paragraph
- **WHEN** the body contains `# H` followed by a wrapped paragraph
  and the format is `slack`
- **THEN** the output contains `*H*` on its own line, and the
  paragraph is joined on the following line

#### Scenario: Commonmark stays verbatim by default
- **WHEN** the body contains a wrapped paragraph and the format is
  `commonmark`
- **THEN** the output is byte-identical to the source lines

#### Scenario: Unwrap is opt-in for commonmark
- **WHEN** the body contains a wrapped paragraph and the command is
  `ft notes export notes/foo.md --unwrap`
- **THEN** the output contains the paragraph joined on one line

#### Scenario: No-unwrap restores verbatim slack output
- **WHEN** the body contains a wrapped paragraph and the command is
  `ft notes export notes/foo.md --format slack --no-unwrap`
- **THEN** the output keeps the source line breaks

#### Scenario: Both flags are rejected
- **WHEN** the user runs `ft notes export notes/foo.md --unwrap
  --no-unwrap`
- **THEN** the command fails with an error and prints nothing to
  stdout

#### Scenario: Mid-block range starts fresh
- **WHEN** the user exports `--lines` a range whose first selected
  line is a paragraph continuation and the format is `slack`
- **THEN** the first line appears as its own logical line, not joined
  to anything before the range

## MODIFIED Requirements

### Requirement: ft notes export command surface

`ft notes export <FILE> [--lines A-B] [--format TARGET]` SHALL print
the vault-stripped content of the given vault file to stdout, and
`--unwrap` / `--no-unwrap` SHALL be accepted as optional flags. `<FILE>` SHALL be a vault-relative path
(absolute paths SHALL be accepted and relativized; the `.md`
extension SHALL NOT be auto-appended). `--lines` SHALL be optional and
1-indexed inclusive with the short alias `-l`; when absent the whole
file SHALL be exported. `--format` SHALL accept `commonmark` and
`slack`, and SHALL default to `commonmark`; any other value SHALL be
rejected. `--unwrap` and `--no-unwrap` SHALL be optional and mutually
exclusive, controlling soft-break resolution (see Soft-break
resolution); when absent the target's default SHALL apply. The command
SHALL be read-only: it SHALL NOT write, create, or modify any file,
SHALL NOT launch an editor, SHALL NOT prompt, and SHALL NOT require or
consult git. The output SHALL be printed to stdout with a single
trailing newline; an empty export SHALL print no bytes. The exit code
SHALL be 0 on success and 1 on any error, with a human-readable
message on stderr.

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

#### Scenario: Slack format is accepted
- **WHEN** the user runs `ft notes export notes/foo.md --format slack`
- **THEN** the command succeeds and prints the note's content
  transformed per the Slack target rules

#### Scenario: Unwrap flags default per target
- **WHEN** the user runs `ft notes export notes/foo.md --format slack`
  with no unwrap flag, and `ft notes export notes/foo.md` with no
  unwrap flag
- **THEN** the slack export applies soft-break resolution by default
  and the commonmark export does not

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

### Requirement: Pass-through of other content

The `commonmark` target SHALL pass through verbatim all content other
than the listed stripped/converted elements: ATX headings, paragraphs,
task list items (including any task emoji metadata such as priority and
date emoji), non-ft Obsidian callouts (`> [!note]`, `> [!warning]`),
blockquotes, fenced and indented code blocks, lists, tables, markdown
links and images, and horizontal rules. The `slack` target SHALL pass
through the same content after applying the Slack target rules, and
SHALL pass through `&`, `<`, and `>` characters raw (no HTML-entity
escaping), tables and horizontal rules as literal text. Under
soft-break resolution (see Soft-break resolution), the pass-through
SHALL join hard-wrapped lines into single logical lines, preserving
every character except inter-line whitespace; the `commonmark` target
SHALL join only when `--unwrap` is given.

#### Scenario: Task lines preserved with emoji (commonmark target)
- **WHEN** the body contains `- [ ] ⏫ 📅 2026-08-05 Finish the report`
  and the format is `commonmark`
- **THEN** the line appears verbatim in the output

#### Scenario: Non-ft callouts preserved (commonmark target)
- **WHEN** the body contains `> [!warning] This is important` and the
  format is `commonmark`
- **THEN** the line appears verbatim in the output

#### Scenario: Heading converts only its wikilink (commonmark target)
- **WHEN** the body contains `# [[Foo]]` and the format is
  `commonmark`
- **THEN** the output contains `# Foo`

#### Scenario: Ampersand is not escaped (slack target)
- **WHEN** the body contains `AT&T and <value> and a > b` and the
  format is `slack`
- **THEN** the line appears verbatim in the output

### Requirement: Slack mrkdwn target

The `slack` target SHALL transform content for Slack's mrkdwn dialect
in addition to the shared vault stripping. ATX headings (`#` through
`######`) SHALL render as bold text with the level discarded: `# H` →
`*H*`. Emphasis SHALL convert to Slack's dialect: `**bold**` → `*bold*`,
`*italic*` → `_italic_`, `_italic_` → `_italic_` (unchanged),
`~~strike~~` → `~strike~`, and `***both***` → `*_both_*`. Delimiters
SHALL be recognized per CommonMark flanking rules so that `snake_case`
and `2 * 3` remain literal, and SHALL NOT be recognized inside inline
code spans or fenced code blocks. Markdown links SHALL convert per the
target table:

| Source | Output |
|---|---|
| `[text](https://url)` | `<https://url\|text>` |
| `[text](mailto:…)` | `<mailto:…\|text>` |
| `[text](internal.md)` | `text` (display text only) |
| `![alt](https://x/img.png)` | `https://x/img.png` (bare URL) |
| `![alt](local.png)` | `alt` (alt text only) |

Markdown link titles (`[x](y "title")`) SHALL be discarded. Callout
marker tokens SHALL be stripped from blockquote lines: a `[!word]`
token that starts the content after the `>` prefixes SHALL be removed,
keeping the rest of the line (`> [!note] Title` → `> Title`); this
applies to every callout type, including malformed `[!ft-source]`
lines (canonical `[!ft-source]` header lines SHALL be dropped entirely,
as in the `commonmark` target). Task list items SHALL drop the checkbox
but keep the bullet character, indentation, and emoji metadata:
`- [ ] ⏫ 📅 2026-08-05 Finish` → `- ⏫ 📅 2026-08-05 Finish`; `[x]`
(done) checkboxes SHALL drop the same way. The checkbox drop SHALL
apply at any nesting depth — the checkbox SHALL be recognized after any
amount of leading whitespace, not only the 0-3 spaces of a top-level
item — so a nested `    - [ ] foo` SHALL export as a nested bullet
with the checkbox removed. Fenced code blocks SHALL normalize to
Slack's syntax: opening-fence language tags SHALL be stripped
(```` ```rust ```` → ```` ``` ````) and `~~~`-delimited fences SHALL
convert their open and close delimiter lines to ` ``` `; content inside
fences SHALL pass through verbatim. Blockquotes SHALL keep their `>`
prefixes, and inline code SHALL pass through. Bulleted and numbered
list items SHALL pass through with leading indentation normalized to
Slack's 4-space-per-level rule: a depth-`n` item SHALL have exactly
`4n` leading spaces, with depth 0 (the top level) unindented. Marker
kinds `-`, `*`, `+`, and ordered `N.` (digits followed by a period)
SHALL be treated alike. The nesting level SHALL be derived from the
item's indentation relative to the preceding list items: an item
indented deeper than its predecessor SHALL nest one level deeper, an
item at the same indentation SHALL stay at the same level, and an item
indented less SHALL move up to the matching level. A source list
indented 2 spaces per level SHALL therefore be re-indented to 4
(`- foo` / `  - bar` / `    - lol` / `- baz` SHALL export as `- foo` /
`    - bar` / `        - lol` / `- baz`), and an item whose source
indentation already matches its level times 4 SHALL be unchanged
(normalization SHALL be idempotent). List-item lines inside fenced or
indented code blocks SHALL NOT be re-indented. A non-list, non-blank
content line with no leading whitespace between list items SHALL reset
the nesting — the list SHALL be considered interrupted — so a following
indented item SHALL start a new top-level list. Under soft-break
resolution (the default for this target, see Soft-break resolution),
continuation lines of multi-line items SHALL join into the item's
logical line rather than appear as separate indented lines; lists
nested inside blockquote lines (`> - foo`) SHALL keep their source
indentation, and their continuation lines SHALL follow the blockquote
join rule. With `--no-unwrap`, lines that are not list items SHALL
keep their source indentation and continuation lines of multi-line
items SHALL appear as separate lines. The target SHALL NOT escape `&`,
`<`, or `>` (no HTML-entity conversion), and SHALL NOT produce tables,
images, or horizontal rules. The `commonmark` target SHALL be
unaffected by the list rules: its list output SHALL remain
byte-identical.

#### Scenario: Heading becomes bold
- **WHEN** the body contains `# Title` and `## Subtitle` and the
  format is `slack`
- **THEN** the output contains `*Title*` and `*Subtitle*`

#### Scenario: Bold and italic convert to Slack dialect
- **WHEN** the body contains `**bold** and *italic* and _also_ and
  ~~gone~~ and ***both***`
- **THEN** the output contains `*bold* and _italic_ and _also_ and
  ~gone~ and *_both_*`

#### Scenario: Code spans keep content literal
- **WHEN** the body contains `` `**not bold**` `` and the format is
  `slack`
- **THEN** the code span appears verbatim in the output

#### Scenario: Flanking rules keep prose literal
- **WHEN** the body contains `snake_case` and `2 * 3` and the format
  is `slack`
- **THEN** both appear verbatim in the output

#### Scenario: Markdown link becomes Slack link
- **WHEN** the body contains `see [docs](https://docs.example.com/x)`
  and the format is `slack`
- **THEN** the output contains `see <https://docs.example.com/x|docs>`

#### Scenario: Internal markdown link loses the link
- **WHEN** the body contains `see [other note](notes/other.md)` and
  the format is `slack`
- **THEN** the output contains `see other note`

#### Scenario: Remote image becomes a bare URL
- **WHEN** the body contains `![diagram](https://ex.com/img.png)` and
  the format is `slack`
- **THEN** the output contains the bare URL `https://ex.com/img.png`

#### Scenario: Local image becomes alt text
- **WHEN** the body contains `![screenshot](local.png)` and the
  format is `slack`
- **THEN** the output contains `screenshot`

#### Scenario: Callout marker stripped from blockquote
- **WHEN** the body contains `> [!note] Keep this` and
  `> > [!warning] nested` and the format is `slack`
- **THEN** the output contains `> Keep this` and `> > nested`

#### Scenario: Task checkbox dropped, emoji kept
- **WHEN** the body contains `- [ ] ⏫ 📅 2026-08-05 Finish` and
  `  - [x] done` and the format is `slack`
- **THEN** the output contains `- ⏫ 📅 2026-08-05 Finish` and
  `    - done` (the nested item is re-indented to 4 spaces and its
  checkbox is dropped)

#### Scenario: Nested task checkbox dropped at any depth
- **WHEN** the body contains `- parent` followed by
  `    - [x] done` and the format is `slack`
- **THEN** the output contains `- parent` and `    - done` — the
  checkbox is dropped even though the item is indented 4 spaces

#### Scenario: Two-space sub-items re-indented to four
- **WHEN** the body contains
  `- foo` / `  - bar` / `    - lol` / `- baz` and the format is `slack`
- **THEN** the output is
  `- foo` / `    - bar` / `        - lol` / `- baz` — each nesting
  level gets exactly 4 spaces

#### Scenario: Deep nesting scales by level
- **WHEN** the body contains `- a` / `  - b` / `    - c` /
  `      - d` and the format is `slack`
- **THEN** the output is `- a` / `    - b` / `        - c` /
  `            - d` (0 / 4 / 8 / 12 spaces)

#### Scenario: All marker kinds normalized
- **WHEN** the body contains `- one` / `  * two` / `    + three` /
  `      1. four` and the format is `slack`
- **THEN** the output is `- one` / `    * two` / `        + three` /
  `            1. four`

#### Scenario: Already-correct four-space sources unchanged
- **WHEN** the body contains `- foo` / `    - bar` / `        - lol`
  and the format is `slack`
- **THEN** the output is byte-identical to the input

#### Scenario: List-looking lines inside code are not re-indented
- **WHEN** the body contains a fenced block with a `  - item` line
  inside it, and an indented code block containing a `- item` line,
  and the format is `slack`
- **THEN** both `- item` lines appear inside their code blocks exactly
  as written (indentation untouched)

#### Scenario: List interrupted by a heading resets nesting
- **WHEN** the body contains `- a` / `# Heading` / `  - b` and the
  format is `slack`
- **THEN** the output contains `*Heading*` and `- b` unindented — the
  heading ends the list, so the indented item starts a new top-level
  list

#### Scenario: Fence language tag stripped
- **WHEN** the body contains a fenced block opening with ` ```rust `
  and the format is `slack`
- **THEN** the opening delimiter line appears as ` ``` ` and the
  content inside the fence is unchanged

#### Scenario: Tilde fence converted to backticks
- **WHEN** the body contains a code block delimited by `~~~` lines and
  the format is `slack`
- **THEN** both delimiter lines appear as ` ``` ` and the content
  inside is unchanged

#### Scenario: Fence content is not transformed
- **WHEN** a fenced block contains `**bold**` and `# heading` and the
  format is `slack`
- **THEN** those lines appear verbatim inside the fence

#### Scenario: Code inside a fence does not close the block
- **WHEN** a fenced block's content includes a line starting with
  ` ```js ` (backticks plus text) and the format is `slack`
- **THEN** the block still opens and closes exactly once — the content
  line is not treated as an opening delimiter

#### Scenario: Commonmark list output unchanged
- **WHEN** the body contains a 2-space-indented nested list and the
  format is `commonmark`
- **THEN** the list lines appear byte-identical to the source

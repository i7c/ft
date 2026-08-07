## MODIFIED Requirements

### Requirement: ft notes export command surface

`ft notes export <FILE> [--lines A-B] [--format TARGET]` SHALL print
the vault-stripped content of the given vault file to stdout. `<FILE>`
SHALL be a vault-relative path (absolute paths SHALL be accepted and
relativized; the `.md` extension SHALL NOT be auto-appended). `--lines`
SHALL be optional and 1-indexed inclusive with the short alias `-l`;
when absent the whole file SHALL be exported. `--format` SHALL accept
`commonmark` and `slack`, and SHALL default to `commonmark`; any other
value SHALL be rejected. The command SHALL be read-only: it SHALL NOT
write, create, or modify any file, SHALL NOT launch an editor, SHALL
NOT prompt, and SHALL NOT require or consult git. The output SHALL be
printed to stdout with a single trailing newline; an empty export
SHALL print no bytes. The exit code SHALL be 0 on success and 1 on any
error, with a human-readable message on stderr.

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

### Requirement: Wikilink and embed conversion

In the exported content, wikilinks SHALL be converted to plain text in
every target according to this table (shared across targets):

| Source | Output |
|---|---|
| `[[T]]` | `T` |
| `[[T\|D]]` | `D` |
| `[[T#A]]` | `T` |
| `[[T#A\|D]]` | `D` |
| `[[#A]]` | `#A` |
| `[[#A\|D]]` | `D` |

Obsidian embeds SHALL convert per target: the `commonmark` target SHALL
render `![[T]]` as `![T](href)`, `![[T|D]]` as `![D](href)`, and
`![[T#A]]` / `![[T#A|D]]` as `![T](href)` / `![D](href)`, where `href`
SHALL be `T`, wrapped in angle brackets `<…>` when it contains
whitespace. The `slack` target SHALL render embeds as plain text: the
display text when present, otherwise the trimmed target; anchors are
dropped. Markdown links `[x](y)` and Markdown images `![alt](src)`
SHALL be preserved verbatim by the `commonmark` target and never
reinterpreted; the `slack` target SHALL convert them per the Slack
target rules. The conversion SHALL apply to every non-dropped line,
including blockquote lines and the kept body lines of ft-source
callouts. It SHALL NOT apply inside inline code spans (backtick runs)
or fenced code blocks. A wikilink whose target is the note itself SHALL
be treated like any other link. Constructs that are not real links —
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

#### Scenario: Embed becomes a CommonMark image (commonmark target)
- **WHEN** the body contains `![[image.png]]` and `![[image.png|alt
  text]]` and the format is `commonmark`
- **THEN** the output contains `![image.png](image.png)` and
  `![alt text](image.png)`

#### Scenario: Embed becomes plain text (slack target)
- **WHEN** the body contains `![[image.png]]` and `![[image.png|alt
  text]]` and the format is `slack`
- **THEN** the output contains `image.png` and `alt text`

#### Scenario: Code spans and fences are untouched
- **WHEN** the body contains `` `[[Foo]]` `` and a fenced block
  containing `[[Bar]]`
- **THEN** both `[[Foo]]` and `[[Bar]]` appear verbatim in the output

#### Scenario: Conversion inside blockquotes
- **WHEN** the body contains `> [!note] Title` followed by
  `> See [[Foo]]`
- **THEN** the `commonmark` output contains `> See Foo` — the callout
  block is preserved as a blockquote and its wikilink is converted

#### Scenario: Markdown links are preserved (commonmark target)
- **WHEN** the body contains `[text](foo.md)` and `![alt](img.png)`
  and the format is `commonmark`
- **THEN** both appear verbatim in the output

#### Scenario: Multiple links on one line
- **WHEN** the body contains `[[A]] and [[B|bee]] and ![[c.png]]`
- **THEN** the output contains `A and bee` followed by the embed
  rendered per the active target

### Requirement: Pass-through of other content

The `commonmark` target SHALL pass through verbatim all content other
than the listed stripped/converted elements: ATX headings, paragraphs,
task list items (including any task emoji metadata such as priority and
date emoji), non-ft Obsidian callouts (`> [!note]`, `> [!warning]`),
blockquotes, fenced and indented code blocks, lists, tables, markdown
links and images, and horizontal rules. The `slack` target SHALL pass
through the same content after applying the Slack target rules, and
SHALL pass through `&`, `<`, and `>` characters raw (no HTML-entity
escaping), tables and horizontal rules as literal text.

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

## ADDED Requirements

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
(done) checkboxes SHALL drop the same way. Fenced code blocks SHALL
normalize to Slack's syntax: opening-fence language tags SHALL be
stripped (```` ```rust ```` → ```` ``` ````) and `~~~`-delimited fences
SHALL convert their open and close delimiter lines to ` ``` `; content
inside fences SHALL pass through verbatim. Blockquotes SHALL keep their
`>` prefixes, bulleted/numbered lists SHALL pass through, and inline
code SHALL pass through. The target SHALL NOT escape `&`, `<`, or `>`
(no HTML-entity conversion), and SHALL NOT produce tables, images, or
horizontal rules.

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
  `  - done`

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

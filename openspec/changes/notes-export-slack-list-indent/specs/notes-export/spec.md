## MODIFIED Requirements

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
indented item SHALL start a new top-level list. Lines that are not
list items SHALL keep their source indentation: continuation lines of
multi-line items and lists nested inside blockquote lines
(`> - foo`) are out of scope for normalization. The target SHALL NOT
escape `&`, `<`, or `>` (no HTML-entity conversion), and SHALL NOT
produce tables, images, or horizontal rules. The `commonmark` target
SHALL be unaffected by the list rules: its list output SHALL remain
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

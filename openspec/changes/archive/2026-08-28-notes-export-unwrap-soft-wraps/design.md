## Context

`ft notes export` is a read-only, line-oriented pipeline:
`export_content` (ft-core/src/export.rs) walks the body lines, tracks
code-fence state (`LineSkipState`) and list nesting
(`ListDepthTracker`), builds a per-line `LineContext`, and calls
`ExportTarget::transform_line` — one output line per source line,
byte-for-byte except the per-line transforms. That model is why
hard-wrapped source exports badly: a soft break (a bare `\n` inside a
paragraph or list item, which CommonMark renders as a space) becomes a
real newline in the output, and Slack — which renders every newline as
a line break — shows shattered paragraphs and orphaned bullet
continuations.

The seam precedent this change extends: the Slack list re-indent work
added `list_depth` to `LineContext` and a `ListDepthTracker` to
`markdown.rs`, consumed only by the Slack target. Target-specific
policy lives on the `ExportTarget` impl; target-independent structure
lives in the driver / `markdown.rs`.

## Goals / Non-Goals

**Goals:**

- `ft notes export --format slack` output of hard-wrapped notes is
  directly pastable into Slack: paragraphs flow as one line per
  paragraph, wrapped list items as one clean bullet, wrapped quote
  paragraphs as one quoted line.
- The join matches CommonMark soft-break rendering: a soft break
  becomes a space; a hard break (blank line, trailing `\` or two+
  spaces, block boundary) stays a break.
- CommonMark output is byte-identical by default; the join is an
  opt-in (`--unwrap`) there.
- The seam stays target-parametric: new targets get the join by opting
  into a trait method; the driver stays target-agnostic.

**Non-Goals:**

- Re-wrapping to a column width (Slack wraps long lines itself).
- Lazy continuation (unindented text after a list item is a new
  paragraph, matching `ListDepthTracker`'s reset rule).
- Setext headings (`Title` / `===`) — already out of scope in
  `markdown.rs`.
- Cleaning hard-break markers (`\`, trailing spaces) out of Slack
  output — markers are kept verbatim.
- Any change to the other export surfaces (`quote`, `--lines`
  semantics, frontmatter clamp).

## Decisions

### D1: The join is a driver-level pass after per-line transforms

The join runs inside `export_content`, replacing the plain
`out_lines.push(t)` with an accumulator state machine: a `pending`
logical line (text + source block kind + flags) that either absorbs the
next line or flushes. It runs **after** `transform_line` (so the
emitted text is the transformed line) but classifies the **source**
line (so the join decision sees the real structure).

Why not join transformed lines only? The Slack target rewrites `# H` →
`*H*` and `- [ ] x` → `- x`; a join over output text alone cannot tell
a bolded heading from a bolded sentence, so `*H*` could wrongly absorb
the following paragraph. Why not join source lines before transforms?
That would change the `LineContext`/fence/list semantics the per-line
transforms rely on, and couples the join to every future transform.

### D2: A source-line `BlockKind` classification + merge table

The driver classifies each non-code line as one of `Blank`, `Heading`,
`ListItem`, `Blockquote`, `Paragraph`, `Break` (thematic rule), using
existing `markdown.rs` primitives (`is_list_item_marker`,
`leading_ws`, `parse_atx`, `is_blockquote_line`, `is_rule_separator`
— the last becomes `pub(crate)`). A continuation line may join the
pending line only when:

| prev kind | cur kind | join? |
|---|---|---|
| Paragraph | Paragraph | yes (soft break) |
| ListItem | Paragraph | yes iff cur has leading indent (wrapped item text) |
| Blockquote | Blockquote | yes, unless prev was a callout title or either side is an empty `>` line |
| anything else | anything else | no |

Every other line kind — Blank, Code, Heading, Break, ListItem,
Blockquote-after-Paragraph, Paragraph-after-anything-else — starts a
new logical line (flushing pending first). A dropped line (transform →
`None`, e.g. an ft-source header) is a boundary: it flushes pending,
contributes nothing, and resets the state.

Joining appends the continuation's content with a single space: strip
the continuation's leading whitespace (and `>` prefixes for
blockquotes — the first line's prefix structure wins), trim trailing
whitespace off the pending text.

### D3: Policy on the trait, override on the CLI

`ExportTarget` gains `fn unwrap_soft_wraps(&self) -> bool { false }`;
`SlackExport` overrides to `true`. `export_content` gains a
`unwrap: Option<bool>` parameter (the CLI passes the flag resolution;
`None` falls back to the target's policy) — the single production
caller and the unit tests update mechanically. The CLI adds
`--unwrap` / `--no-unwrap`, mutually exclusive (the `--has-due` /
`--no-due` precedent), resolved as flag → target default.

Alternatives considered: always join for Slack with no flag (rejected —
scripts may want the old verbatim output, and an escape hatch costs
nothing); a third `--format` value (rejected — flag proliferation, and
join is orthogonal to the mrkdwn conversion); join default on for
CommonMark too (rejected — wrapped source is idiomatic for markdown
receivers; the pain is Slack-specific).

### D4: Callout titles never absorb their body

A blockquote whose content starts with a `[!type]` marker (the
`strip_callout_marker` pattern, factored into a shared predicate) is
flagged on the pending line; a Blockquote continuation never joins
*into* a flagged title. This preserves the title/body split in the
export and — load-bearing for the existing test suite — keeps
`> Keep me` / `> see Baz` and `> Keep this` / `> > nested` outputs
byte-identical to today.

### D5: Hard breaks never join

A source line ending in a lone trailing `\` or two or more spaces is a
CommonMark hard break. The break prevents the *next* line from joining;
the markers are kept verbatim (v1 — Slack cleanup is target-specific
and out of scope). Blank lines, empty `>` lines, and code lines are
handled the same way: flush and emit separately.

### D6: Mid-list / mid-block range fragments start fresh

The state machine starts empty for every export, so a `--lines` range
that begins mid-paragraph or mid-item exports its first line as a
standalone logical line — the same convention `ListDepthTracker`
already uses for indented range fragments.

## Risks / Trade-offs

- [Slack default output changes for wrapped content] → that is the
  point of the change; `--no-unwrap` restores verbatim output; the
  docs and the spec delta state the default.
- [Lazy continuation mis-split: unindented text after a list item is
  treated as a new paragraph] → consistent with the existing
  `ListDepthTracker` reset rule and with real-world Obsidian style
  (continuations are indented); documented as a limitation.
- [Setext heading underline `===` could join with its title line] →
  setext is already out of scope for the codebase's heading parsing;
  `---`-style underlines are caught by the `Break` kind.
- [Poetry-style paragraphs (intentional single newlines without blank
  lines) flatten] → that is exactly CommonMark soft-break semantics —
  the author's intent is a space; real breaks (blank lines, `\`, `  `)
  survive.
- [Empty `>` spacer lines inside quotes stop the join] → conservative:
  they pass through verbatim and never merge, so no structure is
  invented.
- [Join-state complexity in the driver] → kept to one small
  accumulator + one pure merge-table function, unit-tested
  exhaustively; dropped lines, code, and blanks are handled by the same
  code path as today's push.

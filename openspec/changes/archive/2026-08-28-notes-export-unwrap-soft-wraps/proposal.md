## Why

`ft notes export` passes hard-wrapped source lines through verbatim: a
paragraph wrapped at ~80 columns, or a list item whose continuation
lines are indented under the marker, exports as multiple lines. That is
correct for CommonMark receivers (a single `\n` is a *soft break* that
renders as a space), but Slack's mrkdwn renders **every** newline as a
real line break — so pasting an export into Slack shows a paragraph
shattered into fragments and bullet items whose continuations appear as
orphaned indented text, which the user must hand-repair before the
message is presentable.

## What Changes

- **Soft-break resolution ("unwrap") pass in the export driver** — a
  cross-line join that runs *after* the per-line target transforms and
  merges lines that are soft-break continuations, matching what a
  CommonMark renderer would produce: consecutive paragraph lines join
  with a space; an indented non-marker line under an open list item
  joins into that item's first line (so a wrapped `- item` becomes one
  clean bullet); consecutive blockquote lines join (`> a` / `> b` →
  `> a b`).
  - **Real breaks survive**: blank lines, list-item marker lines,
    headings, thematic rules, code blocks, and CommonMark hard breaks
    (trailing `\` or two+ spaces) never join.
  - **Callout titles are protected**: a `> [!note] Title` line never
    absorbs its body — the title/body split survives (this also keeps
    today's output for `> Keep me` / `> see Baz` byte-identical).
  - **Code is protected**: fence and indented-code lines pass through
    verbatim and are never joined.
  - The join decision uses the **source line's block kind** (headings
    must not absorb following paragraphs even though the Slack target
    rewrites them to `*H*`), so it runs on source classification, not
    on transformed text.
- **Per-target default + CLI override** — `ExportTarget` gains an
  `unwrap_soft_wraps()` policy (default off); `SlackExport` overrides
  to on. `ft notes export` gains `--unwrap` / `--no-unwrap` (mutually
  exclusive, like `--has-due`/`--no-due`) to override the target
  default. Slack joins by default (the fix); CommonMark stays verbatim
  by default (`--unwrap` opts in — wrapped source is idiomatic when the
  receiver is a markdown tool). **Behavior change**: `--format slack`
  output of wrapped content differs from today (that is the point);
  `--no-unwrap` restores the old output. CommonMark output is
  byte-identical unless `--unwrap` is given.
- **Documented limitations** (out of scope): lazy continuation
  (unindented text after a list item is treated as a new paragraph, not
  a continuation — consistent with the existing list-depth tracker's
  reset rule); setext headings; hard-break marker cleanup in Slack
  output (a literal trailing `\` stays).

## Capabilities

### New Capabilities

None — this is a behavior change to an existing capability.

### Modified Capabilities

- `notes-export`: the `slack` target gains soft-break resolution
  (hard-wrapped paragraphs, list items, and blockquotes export as
  single logical lines) behind a per-target default with a `--unwrap` /
  `--no-unwrap` override; the `commonmark` target gains the same
  join as an opt-in and stays byte-identical by default. The
  "Pass-through of other content" and "Slack mrkdwn target"
  requirements change from "continuation lines keep their source
  indentation" to "soft-break continuations join into the logical
  line".

## Impact

- `ft-core/src/export.rs` — `BlockKind` source-line classifier, the
  join pass in `export_content`, a new `ExportTarget::unwrap_soft_wraps()`
  hook (mirrors the `list_depth` seam precedent), and unit tests.
- `ft-core/src/markdown.rs` — `is_rule_separator` becomes `pub(crate)`
  for the classifier (no behavior change).
- `ft/src/cmd/export.rs` — `--unwrap` / `--no-unwrap` flags, help text,
  effective-policy resolution (flag → target default).
- Spec delta: `openspec/specs/notes-export/spec.md`.
- Docs: `docs/guide/notes.md` (Slack export section), README.
- Tests: unit tests in `ft-core/src/export.rs`; integration tests in
  `ft/tests/notes_export.rs` (wrapped-content fixture, flag matrix,
  existing slack expectations unchanged).

## Context

`ft notes export` renders a vault note (or original-file line range) as
clean, portable markdown via the `ExportTarget` seam in
`ft-core/src/export.rs`: the driver (`export_content`) handles
frontmatter clamping, range slicing, and per-line markdown structure
(fences, indented code) via `LineSkipState` in `ft-core/src/markdown.rs`,
surfaced to targets through `LineContext` (`in_code`, `opened_fence`,
`fence_char`). `SlackExport::transform_line` runs an inline scanner then
structural rewrites (`[!type]` marker strip, `- [ ]` checkbox drop,
`# H` → `*H*`), and passes list lines through with indentation
verbatim.

Slack's mrkdwn requires **4 spaces of indentation per nesting level**
for a bullet to render as a sub-item; CommonMark/Obsidian accept 2
(indeed, CommonMark treats 4-space-indented lines after a blank line as
*indented code*, which is why vault notes use 2-space indents). So
`  - bar` under `- foo` renders flat (or as a broken list) in Slack.
The user confirmed the mapping is *level-based*, not a
ceil-to-multiple-of-4 of the source indent: their example re-indents a
4-source-space item to 8 (two levels). Decisions confirmed with the
user: all marker kinds normalize (`-`/`*`/`+` and ordered `N.`);
continuation lines and blockquote-nested lists stay verbatim; and the
adjacent `drop_task_checkbox` bug (checkbox recognized only within the
first 3 leading spaces, so nested task items keep their dead `[ ]`) is
fixed in the same change.

## Goals / Non-Goals

**Goals:**
- Slack-export list items re-indent to exactly `4n` leading spaces for
  depth `n` (top level unindented), idempotent on 4-space sources.
- All marker kinds (`-`/`*`/`+`, ordered `N.`) normalized alike.
- Code protected: list-marker lines inside fenced or indented code
  blocks never re-indent.
- List interruption honored: an unindented non-list content line
  resets nesting (CommonMark rule), so a following indented item
  starts a new top-level list.
- Nested task items drop their checkbox at any depth (bug fix).
- `--format commonmark` output byte-identical to today; CLI surface
  untouched.

**Non-Goals:**
- **Continuation lines / multi-line items.** Only list-marker lines are
  re-indented; indented non-marker lines (item continuations) keep
  source indentation (documented; Slack's multi-line-item rendering is
  unreliable anyway).
- **Lists inside blockquote lines** (`> - foo`). Left verbatim —
  automatically excluded by the marker pattern (lines start with `>`).
- **Slack's own list-rendering quirks** (blank lines between items,
  whether `*`/`+` bullets render, ordered-list behavior) — out of our
  control; normalization targets the indentation rule only.
- **No CommonMark list parser.** The tracker is a width-stack heuristic,
  not a spec-complete block parser (same spirit as the existing
  "no full CommonMark inline parser" non-goal in the slack target).

## Decisions

### D1. Driver-computed list depth on `LineContext` (mirrors `opened_fence`)

The driver maintains a small `ListDepthTracker` (in `ft-core/src/markdown.rs`,
next to `LineSkipState`; `pub(crate)`) advanced once per line when the
line is not code. For a list-item line it returns the nesting depth;
`LineContext` gains `list_depth: Option<usize>` (default `None`), and
`SlackExport::transform_line` re-indents a depth-`Some(d)` line by
replacing its leading whitespace with `" ".repeat(4 * d)` as the final
step (after structural rewrites, so checkbox-dropped task lines are
re-indented cleanly). `CommonMarkExport` ignores the field; targets
stay stateless `&'static` impls.

*Alternatives rejected:*
- **`postprocess(&self, text)` hook on `ExportTarget`** — a second pass
  over the joined output that must re-derive fence state to protect
  code blocks. The codebase explicitly avoids re-deriving structure
  ("read it here rather than re-deriving fence state"); `opened_fence`
  was added to `LineContext` for exactly this reason, and list depth is
  the same category of target-independent markdown structure.
- **Stateful `SlackExport`** (interior mutability / non-`&'static`
  target) — breaks the stateless-target pattern.

### D2. Depth derivation: source-indent width stack

The tracker keeps a stack of source indent widths, one per open level
(columns; tabs advance to the next multiple of 4, the same rule as
`starts_with_indent`). For a list-item line with leading width `w`:

- stack empty → push `w`, depth 0 (a fragment starting mid-list is
  treated as a new top-level list);
- `w > top` → push `w`, depth = new stack height − 1 (nest one level);
- `w == top` → depth = height − 1 (same level);
- `w < top` → pop while `top > w`; then `w == top` (same level),
  `w > top` (between levels → push, nest), or empty (push `w`, depth 0).

This derives *levels* from relative indentation, which is what makes
the user's 4-source-space two-level item map to 8 target spaces, and
makes the rule idempotent on already-4-space sources (walk of `- foo` /
`    - bar` / `        - lol` yields depths 0/1/2 → indents 0/4/8).

### D3. Code protection and the reset rule

The driver only advances the tracker on lines where
`LineSkipState::last_was_code()` is false, so fence delimiters,
in-fence content, and indented-code lines never feed the stack — a
`- item` inside a fence keeps its indentation (the re-indent step also
only fires for depth-`Some` lines, and code lines carry `None`).
Separately, a non-list, non-blank line with **zero** leading whitespace
resets the stack (`clear()`): per CommonMark such a line (heading,
paragraph, blockquote) interrupts a list, so a following indented item
belongs to a new list and must be depth 0 (`- a` / `# H` / `  - b` →
`- b` unindented). Blank lines do **not** reset — loose lists continue.
A code block between items leaves the stack intact (simplification:
the next item still resolves by width, which covers the common case).

### D4. List-item detection and the checkbox fix

A line is a list item when, after its leading whitespace, it starts
with `-`/`*`/`+` or `\d+\.` followed by whitespace (space or tab). The
tracker's detection lives in `markdown.rs`; `drop_task_checkbox` keeps
its own shape logic (it also requires the `[ ]`/`[x]` token) but its
leading-whitespace scan is widened from "up to 3 spaces" to "any
leading spaces and tabs", so a nested `    - [ ] foo` drops its
checkbox. The re-indent step defensively re-checks the transformed line
still carries a list marker before rewriting (transforms preserve
markers today; the guard is cheap insurance against mangling).

## Risks / Trade-offs

- **Width-stack heuristic vs. true CommonMark** → pathological
  indentation (items whose width matches no prior level) snaps to a
  nearest-level decision and may differ from CommonMark's reading.
  Mitigation: the rule only rewrites list-marker lines; the worst case
  is a bullet one level shallower/deeper in Slack, never lost content
  or corrupted code blocks (code is protected by D3).
- **Unindented content line between items forces a reset** → a loose
  list whose items are separated by an unindented paragraph (rare in
  practice; CommonMark actually *interrupts* the list there too, so the
  reset matches the source's structure).
- **Slack's own quirks (blank lines, `*`/`+`, ordered lists) may still
  render imperfectly** → out of scope (Non-Goals); no worse than today
  for those cases, strictly better for the common `-` sub-list case.
- **Multi-line items keep source indentation** → continuations may
  render oddly in Slack; documented limitation, no regression (today
  they are verbatim too).
- **`drop_task_checkbox` widening** → only affects the slack target
  (the function is called exclusively from `structural_rewrites`), and
  the widened scan still requires the full `- marker + [ ]/[x] + space`
  shape, so non-task lines are untouched.

## Migration Plan

Read-only command; no data migration. The slack-export output changes
by design; `commonmark` output is byte-identical. Rollback is a revert
of the implementation commit — no state to undo.

## Open Questions

None — the four scope questions (marker coverage, continuation lines,
blockquote-nested lists, checkbox fix) were confirmed with the user.

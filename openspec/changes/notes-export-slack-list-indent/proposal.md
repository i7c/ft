## Why

Slack's mrkdwn requires **4 spaces of indentation per nesting level**
for a bullet to render as a sub-item; CommonMark (and Obsidian) accept
2. `ft notes export --format slack` currently passes list indentation
through verbatim, so vault notes that use 2-space-indented sub-items
(common, since Obsidian follows CommonMark) render as flat text or a
broken list when pasted into Slack.

## What Changes

- **Slack target normalizes list indentation to 4 spaces per level** —
  a depth-`n` list item line gets `4n` leading spaces. The mapping is
  *level-based* (tracked by nesting depth, not a ceil-to-multiple-of-4
  of the source indent): `- foo` / `  - bar` / `    - lol` / `- baz`
  exports as `- foo` / `    - bar` / `        - lol` / `- baz`. The
  rule is idempotent on already-4-space sources.
  - Applies to **all marker kinds** the slack target already treats as
    lists: `-` / `*` / `+` bullets and ordered `1.`-style items (same
    rule, one mechanism).
  - **Code is protected**: list-marker lines inside fenced or indented
    code blocks are never re-indented (they are content, not lists).
  - **List interruption is honored**: an unindented non-list content
    line (heading, paragraph, blockquote) between list items resets
    the nesting, matching CommonMark's list-interruption rule.
  - Out of scope (documented limitations): continuation lines inside a
    multi-line item keep their source indentation (only marker lines
    are re-indented); lists nested inside blockquote lines
    (`> - foo`) stay verbatim.
- **Nested task checkboxes now drop in the slack target** — fixes an
  adjacent bug: `drop_task_checkbox` only inspected the first 3
  leading spaces, so a task item nested at 4+ spaces (`    - [ ] foo`)
  kept its dead `[ ]` checkbox. It now scans all leading whitespace,
  so nested task lines export as clean bullets like their top-level
  siblings.
- **`commonmark` output is byte-identical** to today — the list-depth
  tracking is structure the driver computes; only the slack target
  consumes it.

## Capabilities

### New Capabilities

None — this is a behavior change to an existing capability.

### Modified Capabilities

- `notes-export`: the slack target's list handling. The
  "Pass-through of other content" and "Slack mrkdwn target"
  requirements change from "bulleted/numbered lists SHALL pass
  through" (indentation verbatim) to "SHALL pass through with
  indentation normalized to 4 spaces per nesting level", with the
  marker coverage, code-protection, and interruption rules above, plus
  the nested task-checkbox drop.

## Impact

- `ft-core/src/export.rs` — `LineContext` gains a `list_depth` field
  (mirroring the `opened_fence` precedent); the driver advances a
  list-depth tracker; `SlackExport::transform_line` re-indents list
  items; `drop_task_checkbox` leading-whitespace cap fixed.
- `ft-core/src/markdown.rs` — list-item detection / depth tracker
  (shared markdown-structure helpers, next to `LineSkipState`).
- `ft/src/cmd/export.rs` — no changes (CLI surface untouched).
- Spec delta: `openspec/specs/notes-export/spec.md`.
- Docs: `docs/guide/notes.md` (Slack export section).
- Tests: unit tests in `ft-core/src/export.rs`, integration tests in
  `ft/tests/notes_export.rs` (existing slack expectations with nested
  lists update).

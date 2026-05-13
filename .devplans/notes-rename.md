---
id: 007
name: notes-rename
title: Notes: rename headings on section-move insert
status: finished
created: 2026-05-13
updated: 2026-05-13
---

# Notes: rename headings on section-move insert

## Goal
Extend the Notes section-move flow (plan 003) so the compose step can
rename each pending heading at the destination, independently of (and in
addition to) the existing level shift. Example: picking `## A` and `## B`
from source, the compose view lets the user commit them as `# A` and
`# C` — the `B → C` rename happens on the fly, before the write.

The rename targets only the **top** heading of each picked section.
Nested headings in the section body are untouched (their text isn't
addressable from compose; only their level cascades). A CLI surface is
explicitly out of scope for v1 — TUI only — since the friction this
feature removes is a TUI flow, not a script.

## Motivation and Context
With plan 003 shipped, the section-move flow can drag content into a
target file and reshape its level, but it can't reshape its title. The
real friction case is when a section's heading is meaningful only in the
source context — e.g. a daily-note heading reading `Meeting notes` that
should land in a project note as `2026-05-12 sync`, or a clipboard of
sibling sections that need disambiguating titles at the destination.
Today the user has to commit the move, then open the target in `$EDITOR`
and rename by hand. Doing this at compose time is two keystrokes and
zero context switch.

The compose state machine already has per-row focus, per-row level
shift, a freshness check at commit, and a `Vec<ComposeRow>` layout that
makes per-row data trivial to attach. The rename slot in cleanly
without touching disjoint validation, freshness, or `write_pair`.

## Acceptance Criteria

### Library — `ft_core::notes`

- [x] `SectionPick` gains an optional `new_text: Option<String>` field.
      `None` means "keep the source text" (current behavior). `Some(s)`
      means "render the destination top heading with text `s`". Existing
      callers default to `None`; the public struct stays a plain data
      type (no builder).
- [x] `move_sections` applies the rename after `shift_section_level` and
      before insertion. The first heading line of the shifted body is
      re-rendered with the new text, preserving its (already shifted)
      level, leading whitespace, and trailing newline. Internal helper
      `rerender_heading_line` is generalized to take both a new level
      and an optional new text, or a sibling helper is added — author's
      call at implementation time, no public surface change beyond
      `SectionPick`.
- [x] Rename validation in `move_sections`:
      - Empty text (after trimming) → `Err(Error::Notes(...))`.
      - Text containing `\n` or `\r` → `Err(Error::Notes(...))`.
      - Leading/trailing whitespace is trimmed before write.
      - No uniqueness check across picks or against existing target
        headings — Markdown allows duplicates and so do we.
- [x] Unit tests for the new path:
      - Rename only (no level shift): top heading text changes; nested
        headings unchanged; body content unchanged.
      - Rename + shift down: rename applies after the cascade; nested
        headings shift correctly; top heading carries both new level
        and new text.
      - Rename + shift up: same as above in the other direction.
      - Empty rename rejected; whitespace-only rename rejected.
      - Multi-line rename (`"a\nb"`) rejected.
      - Two picks renamed to the same text in one call → succeeds (no
        uniqueness check).
      - Rename preserves any leading indentation on the heading line
        (lines like `  ## A` keep the two-space prefix).

### TUI — Notes tab compose step

- [x] `ComposeRow::Pending` gains `rename: Option<String>` (None means
      "use source text"). New rows start with `None`.
- [x] New keybinding on the focused Pending row in `Composing`: `r`
      opens an inline rename buffer. The buffer is a transient sub-mode
      of `Composing` (not a new top-level `SectionMoveState` variant) —
      modelled as `editing: Option<RenameBuffer>` on the `Composing`
      state, where `RenameBuffer { row_idx: usize, buf: EditBuffer }`
      reusing the existing `ft/src/tui/widgets/edit_buffer.rs`
      (`EditBuffer::from(...)` for pre-fill; `insert`, `backspace`,
      `delete`, `left`, `right`, `home`, `end`, `delete_word_backward`
      for the editing primitives). The buffer is pre-filled with the
      row's current effective text (the override if one is already set,
      otherwise the clipboard item's `source_text`).
- [x] While the rename buffer is open, the only handled keys are:
      printable chars (`EditBuffer::insert`), `Backspace`
      (`EditBuffer::backspace`), `Delete` (`EditBuffer::delete`),
      `←`/`→` (cursor within the buffer — *not* the row-level
      level-shift), `Home`/`End`, `Ctrl+W` (`delete_word_backward`),
      `Enter` (commit), `Esc` (discard). All other compose keys (`r`,
      `j`/`k`, `Shift+↑`, `Tab`, etc.) are ignored while editing.
      Global keys (tab switch, quit) still work — same precedence as
      other modal inputs in the codebase.
- [x] `Enter` in the buffer:
      - Trims whitespace, then validates non-empty and no newline.
      - On invalid input, raises a toast (`rename cannot be empty` or
        `rename cannot contain newlines`) and leaves the buffer open
        so the user can fix it.
      - On valid input, stores the trimmed text into
        `ComposeRow::Pending.rename` and closes the buffer.
- [x] `Esc` in the buffer discards changes and closes the buffer; the
      row's existing `rename` (if any) is preserved.
- [x] Visual: while a row has a rename override, the compose view
      renders the override (with a small marker like `→ new title`)
      instead of the source title, on the same row. While the rename
      buffer is open, an inline edit field appears under (or visually
      replacing) the focused row, with a left-edge marker so the user
      knows they're typing.
- [x] `commit_move` threads each Pending row's `rename` into the
      corresponding `SectionPick.new_text`. Freshness check unchanged —
      it keys on `(source_line, source_text, level)`, none of which the
      rename touches.
- [x] Reordering (`Shift+↑/↓`) and level-shifting (`←/→`) on a renamed
      row preserve the rename. Moving a Pending row across Anchors does
      not affect its override.
- [x] Step-4 keymap footer is updated to include `r rename`. The `?`
      help overlay (idle state) is *not* updated — the overlay only
      lists idle-state bindings per plan 003.

### Testing

- [x] Unit tests in `ft_core::notes` per the library section above.
- [x] TUI behavior tests in `tui::tests`:
      - `r` on a focused Pending opens the rename buffer pre-filled
        with the source text.
      - `Enter` with a valid new text closes the buffer and the row
        renders the override.
      - `Enter` with empty/whitespace-only input keeps the buffer open
        and toasts.
      - `Esc` discards the buffer and the row reverts to its prior
        state (override preserved if one existed).
      - While the buffer is open, `r` and `Shift+↑` are no-ops (event
        consumed by the buffer, no state change).
      - `Shift+↑` after renaming reorders the row and keeps the
        override.
      - `←/→` after renaming shifts the level and keeps the override.
      - Commit with one renamed Pending writes the renamed heading to
        the target file on disk (TempDir vault).
- [x] Snapshot tests:
      - `notes_move_compose_renamed.snap` — compose view with one
        Pending row showing a rename override.
      - `notes_move_compose_renaming.snap` — compose view with the
        rename buffer open and a partial new title typed.
- [x] End-to-end test extending the existing four-step flow harness:
      pick two sibling H2 sections, rename one of them in compose,
      commit, and assert both target and source files match the
      expected post-move content (full string compare). The other
      Pending is left un-renamed to prove the `None` path still works.

## Technical Notes

- **Rename happens after the cascade.** In `move_sections`, the order
  is: `shift_section_level(...)` to produce the shifted body, then
  rewrite *only* the first heading line of that body with the new text.
  The shift cascade is a no-op on body text, so the order is
  commutative on content but the contract is cleaner this way:
  validation (cascade overflow) runs before the text rewrite.
- **Top heading only.** The user request is per-pick, addressing only
  the section's top heading. Nested heading renames would require
  another UI for traversing the body — out of scope. Document this in
  the `move_sections` doc-comment.
- **Empty / multi-line rejection at the library boundary.** The TUI
  also validates before storing into the buffer, but the library is
  the source of truth — a future CLI surface would inherit the same
  guarantees without re-validating.
- **State shape.** A nested `editing: Option<RenameBuffer>` on
  `Composing` is preferred over a new top-level `SectionMoveState`
  variant: the rename is conceptually a sub-mode of compose, not a
  new step. `handle_compose_key` gets an early return at the top
  (`if let Some(buf) = editing { return handle_rename_buffer(...) }`)
  so the existing compose key dispatch stays intact.
- **Marker glyph.** Reuse the `→` glyph for the override marker; it's
  already in the project's terminal palette via other TUI surfaces
  (verify against `tui::ui` before settling). If `→` causes width
  issues with the existing layout math, fall back to a literal
  `(renamed)` suffix — implementation-time call, no plan re-spec.
- **No CLI surface in this plan.** A future `ft notes move-section
  --rename SOURCE=DEST` is straightforward (the library already takes
  `new_text`), but out of scope here. If users start asking for it,
  spin a small follow-up plan; the library work in this plan covers
  it.

## Future (explicitly out of scope for this plan)

- CLI `--rename` flag for `ft notes move-section`.
- Renaming nested headings within a moved section.
- Editing the section *body* at compose time.
- Bulk rename templates (e.g. prefixing every pending with a date).

## Sessions

### Session 1 · 2026-05-13 · done
**Goal:** Library rename support + TUI compose rename mode (single session)
**Outcome:** Shipped. `SectionPick.new_text` + rename validation in
`move_sections` (10 unit tests). TUI: `ComposeRow::Pending.rename`,
`RenameBuffer` sub-mode of `Composing`, `r` opens an inline `EditBuffer`
pre-filled with the row's effective text, `Enter` validates +
commits, `Esc` discards, `Ctrl+W` word-delete. Compose view renders
the override marker (`→ new title`, yellow bold) on the row and an
inline edit line above the footer; footer keymap flips to
`Enter commit rename · Esc discard` while editing. Threading into
`SectionPick.new_text` is via `rename.clone()` at commit time;
freshness check unchanged. 18 compose-step TUI tests (incl. 2 new
snapshots `notes_move_compose_renamed_80x24` / `renaming_80x24` + an
e2e two-H2 pick with one rename); `notes_move_compose_80x24`
re-accepted to pick up the new `r rename` footer key. All
acceptance criteria checked off below. Workspace test suite (~600
tests across 14 binaries) + clippy + fmt all green.

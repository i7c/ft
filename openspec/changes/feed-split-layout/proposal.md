## Why

Three of the TUI's paragraph/feed tabs have layout problems that hurt
readability:

- The **Pulse** tab renders its `[[link]]` rows into a single
  `Paragraph` with no viewport follow — the cursor can move below the
  fold but the view never scrolls to show it, so links past the first
  screen are effectively unreachable.
- The **Recent** and **Gather** tabs render every entry as a tall block
  (header band + up to two badge lines + the full wrapped paragraph + a
  blank separator). With more than a few entries the feed becomes a long
  wall of prose where individual entries are hard to scan and
  distinguish — the very thing an email-client-style split (compact
  one-line list on top, paragraph preview on the bottom) exists to fix.

Both paragraph tabs share the same entry shape (`RecentEntry` ≈
`GatherEntry` minus `matched`), so the split layout can be one shared
widget.

## What Changes

- **Pulse scroll-follow.** The Pulse tab's viewport SHALL auto-follow the
  cursor and render a right-edge scrollbar on overflow, using the
  existing shared `render_scroll_list` widget so its look matches every
  other scrollable list in the TUI. Cursor movement, multi-select, window
  adjust, and handoff behaviour are otherwise unchanged.
- **Split layout for Recent and Gather.** Both paragraph tabs SHALL move
  from "every entry is a tall block" to a two-pane split:
  - **List pane (top):** a stable-height (default 10 rows, clamped to the
    entry count) one-line-per-entry list. Each row shows `{date} {title}`
    plus a compact inline citation badge (`cited` / `cited*` / `in note`
    / `missing`, omitted when there is nothing to show). Multi-select
    marker (`●`) and cursor highlight render as in other lists.
  - **Preview pane (bottom):** a header line, visually distinct (different
    colors + a separating rule), showing the selected entry's **title,
    date, line range, `matched:` badge (Gather multi-source only), and
    the full citation detail** — for cited entries, *which* note(s)
    cite it; for `cited*` (stale) entries, that staleness is surfaced.
    Below the header, the selected entry's wrapped paragraph body. The
    preview pane does **not** scroll independently: paragraphs longer
    than the pane are visibly cut off (the user opens the paragraph in
    `$EDITOR` via `Enter` to read the whole thing).
- **Shared split widget.** A new `ft/src/tui/widgets/feed_split.rs` SHALL
  render the list/preview split given (a) a compact one-line row per
  entry, (b) a preview-header builder, (c) a preview-body builder, (d)
  `selected` + the multi-select set. Both tabs feed it; per-tab badge
  construction stays local to each tab. Pulse is *not* converted to the
  split (it has no paragraph body to preview); it only gains
  scroll-follow.
- **Multi-select and synth flows unchanged.** `Space` toggles selection
  on the cursor row exactly as today; send-to-synth (`s` / `S` / `n`),
  move-section (`m`), open-in-editor (`Enter`), reload (`R`), and all
  filters (`w`, `u`) behave identically — they refer to the marked set,
  and the preview always shows the cursor row.
- **Gather's Sources strip stays.** Gather's existing 2-row Sources strip
  (loaded sources / window / filters / context note) stays **above**
  the split, so Gather's layout becomes
  `Sources strip (2) / list pane / preview pane`. Recent has no strip;
  its layout is `list pane / preview pane`.
- **Empty/loading/error states** stay as full-pane messages (no split is
  drawn when there is nothing to preview).

This is a **visual/layout redesign**, not a behavioural change to the
synth, move, or handoff flows. No commands or keymap *chords* are added
or removed; `docs/keybindings.md` stays in sync (regenerated only if a
help-section description changes).

## Capabilities

### New Capabilities
- `tui-feed-split`: the shared list/preview split widget and the
  rendering contract both paragraph tabs use (stable-height compact list
  on top, single-entry preview pane on the bottom with a distinct header
  rule, no independent preview scroll).

### Modified Capabilities
- `history-tui-tab`: the Recent tab's entry rendering requirement
  changes from the tall-block layout to the split layout (compact list +
  preview pane), with the citation detail moving from per-row badges into
  the preview header.
- `journal-tui-tab`: the Gather tab's entry rendering requirement changes
  the same way (split layout, citation + `matched:` detail in the preview
  header), with the Sources strip preserved above the split.
- `synthesis-review-tui-tab`: the Pulse tab's link-list rendering gains a
  viewport-follow + scrollbar requirement so the cursor stays visible past
  the first screen.

## Impact

- **New code**: `ft/src/tui/widgets/feed_split.rs` (shared split
  widget); registered in `ft/src/tui/widgets/mod.rs`.
- **Modified code**:
  - `ft/src/tui/tabs/recent.rs` — `render_history` replaced with a
    split-layout render that builds compact rows + a preview header/body
    and hands them to the new widget.
  - `ft/src/tui/tabs/gather.rs` — `render_gather` body replaced the
    same way; `render_sources_strip` and the Sources-strip layout are
    preserved above the split. `citation_badge_line` and the
    `inline_markdown_spans` / `wrap_line` / `pad_to_width` helpers stay
    (reused by the split).
  - `ft/src/tui/tabs/pulse.rs` — `render` body switched to
    `render_scroll_list` for scroll-follow + scrollbar; cursor/selection
    state unchanged.
- **Tests / snapshots**: the existing insta snapshots for these tabs
  (`journal_entry_blocks_80x24.snap`, the `journal_tab_entry_blocks_*`
  and `journal_tab_selected_entry_body_always_visible` tests, the
  `history_tab_renders_recent_feed` assertions, and any Pulse snapshot)
  will be re-recorded. New snapshots cover the split layout (selected
  entry's header + cut-off body) and Pulse scroll-follow. The
  `journal_tab_selected_entry_body_always_visible` *intent* (the selected
  entry's body is visible) is preserved by the preview pane.
- **No breaking changes** to commands, keymaps, CLI, or data formats.
  No new dependencies.

## Why

The `?` keymap-help overlay renders every section top-to-bottom into a
single `Paragraph` with no scroll state. On tabs with many bindings
(Graph has ~35 bindings across multiple groups, plus the global
section) the content overflows the popup and the bottom rows are
silently clipped — there is no way to see them and no way to scroll.
The current workaround is a 95%-height popup, which still overflows on
short terminals and collides with the status bar. This makes a
load-bearing discovery surface (`?` is how users learn the keymap)
unreliable exactly when it's most needed (a tab the user is unfamiliar
with).

## What Changes

- The `?` help overlay gains vertical scrolling when its content
  exceeds the popup height.
- New keys are active **only while `mode == Help`** (mode-local, not
  `CommandDef`s, matching the existing `GitLeader`/`SyncConflict`
  precedent): `j`/`↓` line down, `k`/`↑` line up, `PageDown`/`Space`
  page down, `PageUp`/`b` page up, `g` home, `G` end.
- `Esc`/`?`/`q` continue to dismiss the overlay (unchanged).
- A scrollbar renders on the popup's right edge on overflow, matching
  the styling of `widgets::scroll_list` so all scrollable surfaces in
  the TUI look identical.
- The overlay footer hint teaches the scroll keys.
- Scroll offset resets to `0` every time the overlay is opened.
- The overlay's content (which rows render, the source-of-truth
  keymap/registry walk) is unchanged — only overflow handling and
  interaction change.

## Capabilities

### New Capabilities

- `tui-help-overlay`: The `?` keymap-help overlay's rendering and
  interaction — popup sizing, content composition (global + active
  tab/modal sections), vertical scrolling on overflow, scrollbar
  affordance, and the mode-local key set active while the overlay is
  open.

### Modified Capabilities

<!-- None. The `tui-keymaps` requirement "`?` overlay and `docs/keybindings.md`
generated from keymaps" governs *which rows* render (keymap/registry
walk); this change does not touch row generation, only overflow
handling. No spec-level requirement there changes. -->

## Impact

- **`ft/src/tui/app.rs`**: new `help_scroll: Cell<usize>` (and a
  `help_view_height: Cell<u16>` to give page-step key handling a real
  viewport height) on `App`; reset-on-entry in the `"app.help"` command
  arm and the `enter_help` test helper; new scroll key handling in the
  `Mode::Help` branch of `handle_event`; pass scroll state through in
  `draw`'s `Mode::Help` arm.
- **`ft/src/tui/ui.rs`**: `render_help_overlay` signature gains a
  scroll parameter; computes `max_scroll` from `block.inner(popup)`,
  clamps, renders `Paragraph::scroll((offset, 0))` into the text area,
  draws a `Scrollbar` + `ScrollbarState` on the right edge on overflow;
  footer hint updated.
- **`ft/src/tui/tests.rs`**: new snapshot + behavioral tests for scroll
  on the Graph tab; updated existing `help_overlay_*` snapshots where
  the footer hint byte-shifts. `enter_help` continues to reset scroll.
- **No `CommandDef`/`KeyMap`/registry changes**: scroll keys are
  mode-local, so `ft commands docs --check` and `docs/keybindings.md`
  are unaffected. The footer hint is their documentation, mirroring how
  the git leader documents its own `s`.
- **No dependency changes**: uses `ratatui` `Paragraph::scroll`,
  `Scrollbar`, `ScrollbarState` already in the dependency tree (same
  APIs used by `tabs/journal.rs` and `widgets/scroll_list.rs`).

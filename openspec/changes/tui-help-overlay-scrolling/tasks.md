## 1. App state

- [x] 1.1 Add `help_scroll: Cell<usize>` and `help_view_height: Cell<u16>` fields to `App` in `ft/src/tui/app.rs`, initialised to `0` in `with_tabs`.
- [x] 1.2 Reset `help_scroll` to `0` on every entry into `Mode::Help`: grep `self.mode = Mode::Help` and add `self.help_scroll.set(0)` at each hit (at minimum the `"app.help"` command arm and the `enter_help` test helper).

## 2. Renderer

- [x] 2.1 Change `ft/src/tui/ui.rs::render_help_overlay` signature to accept `scroll: &mut usize` (and write back the clamped value).
- [x] 2.2 In `render_help_overlay`: draw the `Block`, compute `inner = block.inner(popup)`, `max_scroll = lines.len().saturating_sub(inner.height as usize)`, clamp `*scroll`.
- [x] 2.3 When `lines.len() > inner.height`: reserve the rightmost column for the scrollbar track, render `Paragraph::new(lines).scroll((*scroll as u16, 0))` into the narrowed text area, then render a `Scrollbar` + `ScrollbarState::new(lines.len()).viewport_content_length(inner.height).position(*scroll)` into the rightmost column with thumb/track styling copied from `widgets/scroll_list.rs`.
- [x] 2.4 When content fits: render the `Paragraph` into the full `inner` with no scrollbar (no scroll offset applied beyond the clamp).
- [x] 2.5 Update the footer hint line to name scroll + dismiss keys (e.g. `↑/↓ or j/k scroll · PgUp/PgDn · g/G · ?/Esc/q close`).

## 3. Wiring (draw + key handling)

- [x] 3.1 In `App::draw`'s `Mode::Help` arm (`ft/src/tui/app.rs`), pass `&mut self.help_scroll` (via `Cell::get`/set helper or a local `let mut s = self.help_scroll.get(); … self.help_scroll.set(s);`) and, after render, store the popup's inner height into `help_view_height`.
- [x] 3.2 Have `render_help_overlay` write the inner height back through a new `&mut u16` out-param (or return it) so `draw` can store it in `help_view_height`.
- [x] 3.3 In `App::handle_event`'s `if self.mode == Mode::Help` branch, add scroll-key handling **before** the dismiss check: `j`/`↓` +1, `k`/`↑` -1, `PageDown`/`Space` +view_height, `PageUp`/`b` -view_height, `g` → 0, `G` → saturating max (use a large value; the render clamp bounds it). Clamp each to `0..` via `saturating_sub`; keep `Esc`/`?`/`q` as dismiss.
- [x] 3.4 Confirm the modal-keymap branch of the `Mode::Help` arm (`active_modal` Some) still flows through `render_help_overlay` so modal help scrolls too — no extra work expected, just verify.

## 4. Tests

- [x] 4.1 Add `help_overlay_scrolls_on_graph_tab`: enter help on the Graph tab at 80×24, snapshot the top frame (`help_overlay_graph_scrolled_top_80x24`) asserting a scrollbar is present and the footer hint names scroll keys.
- [x] 4.2 In the same test, press `j` several times and snapshot the scrolled frame (`help_overlay_graph_scrolled_down_80x24`), asserting an initially-hidden row is now visible and an initially-visible top row is gone.
- [x] 4.3 Add `help_overlay_page_keys`: open help on Graph, press `PageDown`, assert the offset advanced by the view height (compare visible windows); press `PageUp`, assert it returned to the top.
- [x] 4.4 Add `help_overlay_scroll_clamps_at_bottom`: open help on Graph, press `G`, assert the last binding row is visible and no panic; press `j` again, assert offset unchanged (clamped).
- [x] 4.5 Add `help_overlay_reopen_resets_scroll`: open help, scroll down with `j`, close with `?`, reopen, assert the top row is visible again (offset reset to 0).
- [x] 4.6 Update the existing `help_overlay_*` snapshots (`help_overlay_80x24`, `help_overlay_over_tasks_80x24`, `notes_help_overlay_80x24`, `timeblocks_help_overlay_80x24`, `help_overlay_with_keymap_override_80x24`) via `insta review` — only the footer hint line should differ.

## 5. Build invariants

- [x] 5.1 `cargo build --release`
- [x] 5.2 `cargo test --workspace`
- [x] 5.3 `cargo clippy --workspace --tests -- -D warnings`
- [x] 5.4 `cargo fmt --check`
- [x] 5.5 `cargo run --release -q -- commands docs --check` — confirm `docs/keybindings.md` is unaffected (scroll keys are mode-local, not `CommandDef`s).

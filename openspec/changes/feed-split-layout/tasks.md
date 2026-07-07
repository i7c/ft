## 1. Shared split widget

- [ ] 1.1 Create `ft/src/tui/widgets/feed_split.rs` with `render_feed_split(frame, area, list_rows, selected, multi_selected, preview_header, preview_body)` that splits `area` into `[list (min(LIST_DEFAULT=10, entries.len(), area.height))][preview]`, renders the list via `render_scroll_list` (cursor-follow + scrollbar), draws a separating rule under the preview header, and renders header+body into the preview pane (no independent scroll)
- [ ] 1.2 Register `feed_split` in `ft/src/tui/widgets/mod.rs`
- [ ] 1.3 Unit-test the split geometry: list height clamps to `entries.len()` when small, stays stable across cursor moves, preview gets the remainder; empty `list_rows` is a caller contract (widget assumes non-empty)

## 2. Pulse: scroll-follow

- [ ] 2.1 In `ft/src/tui/tabs/pulse.rs::render`, replace the hand-rolled `Paragraph::new(lines)` with `render_scroll_list`, passing `ListItem`s built from the same `(count) [[target]]?` rows and `selected = Some(self.cursor)`; keep multi-select `[*]` as a row prefix
- [ ] 2.2 Add/refresh an insta snapshot for Pulse showing cursor reachable past the first screen (overflow > viewport) with scrollbar rendered
- [ ] 2.3 Keep the existing empty/loading/error states as full-pane messages (no split)

## 3. Recent: split layout

- [ ] 3.1 In `ft/src/tui/tabs/recent.rs`, replace `render_history`'s tall-block body with a split render: build compact list rows (`{date} {title}` + inline `citation_badge_line` compact form, `●` multi-select marker, `pad_to_width`-truncated) and a preview header (title · date · line range · citation detail with note stems + staleness) + wrapped body (`wrap_line` + `inline_markdown_spans`) for the selected entry, then call `render_feed_split`
- [ ] 3.2 Preserve the empty/loading/error full-pane states before calling the widget (no split drawn when feed empty)
- [ ] 3.3 Keep `scroll_offset` field or remove it if the split widget owns list scrolling now; keep `selected`/`entry_selected` state unchanged (multi-select + synth/move/open/reload/filter flows untouched)
- [ ] 3.4 Re-record the `history_tab_renders_recent_feed` snapshot and any other affected Recent snapshots; add a new snapshot showing the selected entry's preview header + cut-off body

## 4. Gather: split layout (Sources strip preserved)

- [ ] 4.1 In `ft/src/tui/tabs/gather.rs`, replace `render_gather`'s tall-block body with a split render feeding `render_feed_split`, keeping `render_sources_strip` (2 rows) above the split; compact rows + preview header (title · date · line range · `matched:` for multi-source · citation detail) + wrapped body for the selected entry
- [ ] 4.2 Reuse `citation_badge_line` for the compact list badge and derive the full citation detail (citing note stems, staleness, `in note`/`missing` for context-note mode) for the preview header from `CitationState` directly
- [ ] 4.3 Keep `entry_matched_titles`, `context_note`, `in_window_only`, `uncited_only` flows and the synth-send overlay rendering unchanged; preserve empty/loading/error full-pane states
- [ ] 4.4 Re-record `journal_entry_blocks_80x24.snap` and the `journal_tab_selected_entry_body_always_visible` / `journal_tab_entry_blocks_layout` snapshots; preserve the *intent* of `journal_tab_selected_entry_body_always_visible` (selected entry's body visible in the preview pane)

## 5. Verification

- [ ] 5.1 `cargo build --release`
- [ ] 5.2 `cargo test --workspace`
- [ ] 5.3 `cargo clippy --workspace --tests -- -D warnings`
- [ ] 5.4 `cargo fmt --check`
- [ ] 5.5 `cargo run --release -q -- commands docs --check` (regenerate `docs/keybindings.md` only if a help-section description changed)

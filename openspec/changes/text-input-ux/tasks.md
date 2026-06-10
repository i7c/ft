## 0. Migrate graph query bar onto `EditBuffer`

- [x] 0.1 Replace `query_text: String` + `input_cursor: usize` with a single `query_buf: EditBuffer` field on `ExpandedView` in `ft/src/tui/tabs/graph.rs`. (Simpler than the design's `QueryBarState` wrapper since there are no other co-located fields to bundle; the parsed `query: Option<GraphQuery>` already lives as a sibling.)
- [x] 0.2 Add a `set_query_text(s: impl AsRef<str>)` helper on `ExpandedView` and switch every seeding site (preset apply, default seed, `z` rewrite) to use it
- [x] 0.3 Update read sites (renderer, `query_snippet`, `apply_query`) to read `v.query_buf.text` / `v.query_buf.cursor`. The renderer uses the char cursor directly as a column offset (correct for ASCII; acceptable for multi-byte single-cell chars).
- [x] 0.4 Rewrite `QueryBar::handle_event` (`ft/src/tui/modal.rs`): keep `Esc → Closed`, `Enter → fire GraphApplyQueryBar + Closed`; forward **all** other keys via `AppRequest::GraphQueryBarKey` (no modifier filter)
- [x] 0.5 Rewrite `GraphTab::graph_query_bar_key`: delegate to the buffer's existing methods (`insert`/`backspace`/`delete`/`left`/`right`/`home`/`end`). §2 will replace this body with a single `EDIT_KEYMAP` dispatch once the keymap exists.
- [x] 0.6 Integration test `graph_tab_query_bar_basic_editing_preserved_after_migration` in `ft/src/tui/tests.rs`: open query bar, type, exercise Home/End/Left/Right/Backspace/Delete, assert the rendered query line matches
- [x] 0.7 `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check` all clean

## 1. EditBuffer operations + kill ring

- [x] 1.1 Add `kill_ring: Option<String>` field to `EditBuffer`
- [x] 1.2 The existing `home`/`end` methods already do "cursor → 0 / char count". The §2 keymap will map `edit.move-line-start` → `home()` etc.; no rename needed for 18 existing callers.
- [x] 1.3 Implement `move_word_back`, `move_word_forward` using the unified `[A-Za-z0-9_]` boundary rule
- [x] 1.4 Implement `kill_to_end`, `kill_to_start` via a private `kill_range(start_char, end_char)` helper that translates char indices to byte offsets, mutates `text`, and updates `kill_ring`
- [x] 1.5 Implement `kill_word_back`, `kill_word_forward` on top of `kill_range`
- [x] 1.6 **Behavior change**: rework `delete_word_backward` to delegate to `kill_word_back` — same `[A-Za-z0-9_]` boundary, now also populates the kill ring so `Ctrl+Y` can recover the loss. Existing call sites (18 of them) keep working; their tests still pass because every existing case (whitespace-separated words) produces identical output under both rules.
- [x] 1.7 Implement `yank`: insert `kill_ring.clone()` at cursor (no-op if `None` or empty); ring is not cleared
- [x] 1.8 Implement `transpose_chars` matching Emacs semantics (mid-line: swap (cur-1, cur), cursor += 1; at end: swap last two; at start: no-op)
- [x] 1.9 24 unit tests covering each op + edge cases (cursor at 0, at end, empty buffer, ASCII vs multi-byte chars, punctuation under the new word rule)
- [x] 1.10 (incidental) Add `#[allow(clippy::large_enum_variant)]` to `CreateStep` (`ft/src/tui/notes_actions/create.rs`) — adding 24 bytes to `EditBuffer` for the kill ring pushed the variant-size delta past clippy's default threshold; same allow pattern as `ActiveModal` (`modal.rs:197`) since this enum is single-slot at the App level
- [x] 1.11 (incidental) Module-level `#[allow(dead_code)]` on `ft/src/tui/widgets/edit_buffer.rs` while new methods await §2 wiring

## 2. Edit-buffer keymap

- [x] 2.1 `EDIT_COMMANDS` defines 14 `edit.*` commands in `ft/src/tui/widgets/edit_keymap.rs` (`edit.move-line-start`, `edit.move-line-end`, `edit.move-char-back/forward`, `edit.move-word-back/forward`, `edit.kill-to-end/start`, `edit.kill-word-back/forward`, `edit.yank`, `edit.transpose-chars`, `edit.delete-char-back/forward`). `edit.complete` / `edit.dismiss-popup` land in §4 with the popup.
- [x] 2.2 `EDIT_KEYMAP` binds every chord from design.md plus `Home`/`End`/`Left`/`Right`/`Backspace`/`Delete` (the buffer now owns these too — host modals previously special-cased them)
- [x] 2.3 `EditBuffer::handle_event(key) -> EditOutcome`: normalize chord, look up in `EDIT_KEYMAP`, dispatch via `dispatch_edit_command`; fall back to char-insert for printable chars; otherwise `NotHandled` so unbound `Ctrl+R` / `Alt+R` etc. fall through to the host
- [x] 2.4 `CommandScope::Widget(&'static str)` variant added in `command.rs`; `as_str` returns `widget/{w}`; `parse_scope` recognises `widget/edit-buffer`; `cmd/commands.rs` `scope_filter_matches` + `scope_matches` updated; `ft commands list --scope widget/edit-buffer` returns all 14 commands
- [x] 2.5 14 unit tests in `edit_keymap.rs` exercise each binding + the fall-through and printable-char paths; 2 integration tests in `tests.rs` (`graph_tab_query_bar_ctrl_a_e_k_work_after_keymap_wired`, `graph_tab_query_bar_alt_bindings_work`) verify the chords reach the buffer end-to-end through the graph query bar
- [x] 2.6 Drop `#![allow(dead_code)]` on `edit_buffer.rs` — the new methods are now reachable via `dispatch_edit_command`
- [x] 2.7 Wire the new dispatch into the graph query bar — `graph_query_bar_key` is now a one-liner delegating to `buf.handle_event(key)`

## 3. `CompletionProvider` trait + items

- [x] 3.1 `ft/src/tui/widgets/completion.rs` ships `CompletionProvider` trait (with `Debug` supertrait), `CompletionContext<'a>` (text + `cursor_byte`), `CompletionTrigger` (Manual / OnInput), `CompletionItem` (label/insert_text/replace_span/kind/description), `CompletionKind` (Attribute/Operator/Value/Keyword/Path/Tag/Other + 1-char `glyph()`), `TriggerSet`
- [x] 3.2 `TriggerSet` exposes `manual()`, `printable()`, `on_chars(...)` constructors plus a `matches(trigger, ch)` method that handles the manual-only short-circuit
- [x] 3.3 `CompletionItem.replace_span: Option<Range<usize>>` (byte range). `None` => `EditBuffer::current_word_byte_range()` is used (uniform `[A-Za-z0-9_]` word rule shared with `delete_word_backward`)
- [x] 3.4 `StubProvider` in `#[cfg(test)] pub(crate) mod tests` returns a configurable fixed list, used by buffer-side tests in §5

## 4. `CompletionPopup` widget

- [x] 4.1 `CompletionPopup { items, selected, scroll_offset }` plus `MAX_VISIBLE_ITEMS = 8`
- [x] 4.2 `compute_area(host, cursor, max_label_width)` positions above the cursor if the cursor is in the bottom half of `host`, below otherwise; clamps to host bounds
- [x] 4.3 `render(frame, area)` draws a bordered list, each row prefixed by the kind glyph in dim style; the selected row uses `Modifier::REVERSED` on the primary color
- [x] 4.4 `handle_event(key)` consumes `Up`/`Ctrl+P` / `Down`/`Ctrl+N` (navigate, wrapping), accepts on `Tab`/`Enter` (returns `Accepted(item)`), dismisses on `Esc`, otherwise `NotHandled` so printable chars fall through
- [x] 4.5 8 unit tests cover navigation wrap, refresh+clamp, accept, dismiss, fall-through, and positioning above/below

## 5. EditBuffer ↔ popup integration

- [x] 5.1 `EditBuffer.completion: Option<CompletionState>` bundles `provider + popup` so the buffer holds one Option, not two. Manual `Clone` impl drops the provider on clone (trait objects aren't `Clone`-able). `set_completion`/`take_completion`/`popup_is_open` helpers.
- [x] 5.2 After a printable char-insert, `maybe_query_completion(OnInput, Some(c))` calls `provider.complete(ctx)`; empty result closes the popup, non-empty either opens (`new`) or refreshes (`refresh`). Kill/yank re-queries are deferred — no concrete provider yet to take advantage of them.
- [x] 5.3 `handle_event` routes the key to `popup.handle_event` first when the popup is open. `Accepted(item)` triggers `apply_completion`, which substitutes `item.replace_span` (or the current-word range) with `item.insert_text` and places the cursor immediately after. `Dismissed` and `Consumed` keep state coherent.
- [x] 5.4 7 buffer-side tests cover: provider attaches and gets called on char insert; Tab accepts the selected item; Down navigates then Tab accepts; Esc dismisses without buffer mutation; popup-open blocks unbound keys from falling through (precedence sanity); provider returning empty closes popup; buffer without provider behaves identically to baseline
- [x] 5.5 (covered by §5.4 `popup_open_blocks_unbound_keys_from_falling_through`; §6 ties the precedence to the actual modal driver in production code)
- [x] 5.6 (incidental) Add `#[allow(clippy::large_enum_variant)]` to `CaptureResult` (`notes_actions/capture.rs`) and `timeblocks::Mode` — adding the completion slot pushed both enums past the variant-size delta threshold. Same pattern as `CreateStep` in §1.
- [x] 5.7 (incidental) `#![allow(dead_code)]` on `completion.rs` while concrete providers await; per-method `#[allow]` on `set_completion`/`take_completion`/`popup_is_open` for the same reason

## 6. Modal driver integration

- [ ] 6.1 Confirm the dispatch order works: `ActiveModal.handle_event` calls the modal's `handle_event`, which (if it owns an `EditBuffer`) calls the buffer's `handle_event`, which (if popup is open) calls the popup's `handle_event` first
- [ ] 6.2 Document this precedence in `docs/architecture.md` (the modal driver section)
- [ ] 6.3 Test: query bar modal with popup open, press Esc → popup closes (not the modal); press Esc again → modal closes

## 7. Mount sites

- [ ] 7.1 Audit every `EditBuffer` mount site and confirm new bindings work: graph query bar (post-§0), `FuzzyPicker` input (`ft/src/tui/widgets/picker.rs:88`), `GraphRenameState.buffer` (`ft/src/tui/tabs/graph.rs:985`), `AppendState.buf`, `CaptureVarPromptState.buf`, `CreateSubdirState.buf`, `Mode::Quickline/EditDesc/Form` fields in timeblocks, tasks search `edit_state` + `Quickline.input`, journal entry `buf`
- [ ] 7.2 Integration test in `ft/src/tui/tests.rs` per site: open the input, press `Ctrl+A`, type, press `Ctrl+E`, press `Ctrl+K`, snapshot the buffer state. Query-bar test goes first since it's the headline feature.

## 8. Docs

- [ ] 8.1 Update `docs/commands.md` (created by `commands-and-keymaps`) with the "Edit buffer commands" section
- [ ] 8.2 Update `docs/keybindings.md` regeneration to include the edit-buffer chords; verify the CI freshness check still passes
- [ ] 8.3 Add a "macOS terminal notes" subsection covering `Opt+Left/Right` interop

## 9. Build validation

- [ ] 9.1 `cargo build --release` — clean
- [ ] 9.2 `cargo test --workspace` — all tests pass
- [ ] 9.3 `cargo clippy --workspace --tests -- -D warnings` — clean
- [ ] 9.4 `cargo fmt --check` — clean
- [ ] 9.5 `ft completions docs --check` — clean

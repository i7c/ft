## 0. Migrate graph query bar onto `EditBuffer`

- [ ] 0.1 Introduce `QueryBarState { buf: EditBuffer, … }` on `View` in `ft/src/tui/tabs/graph.rs`; remove `query_text: String` and `input_cursor: usize`
- [ ] 0.2 Update every seeding site (`v.query_text = …; v.input_cursor = …`) — around lines 2388, 2574, 2687 — to `v.query.buf = EditBuffer::from(...)`
- [ ] 0.3 Update every read site (renderer, `query_snippet`, `rewrite_query_for_root`, `apply_query`) to use `v.query.buf.text` and `v.query.buf.cursor` (char count); convert to byte offset where needed via `text.char_indices().nth(cursor)`
- [ ] 0.4 Rewrite `QueryBar::handle_event` (`ft/src/tui/modal.rs:968`): keep `Esc → Closed`, `Enter → fire GraphApplyQueryBar + Closed`; forward **all** other keys via `AppRequest::GraphQueryBarKey` (no modifier filter)
- [ ] 0.5 Rewrite `GraphTab::graph_query_bar_key` (`ft/src/tui/tabs/graph.rs:2727`) to delegate to `v.query.buf.handle_event(...)` (no hand-rolled `match` over `KeyCode`)
- [ ] 0.6 Snapshot test: open query bar, type characters, arrow / Home / End / Backspace — baseline behaviour preserved (no regression before any new bindings are wired)
- [ ] 0.7 `cargo build --release` + `cargo test --workspace` green after migration, before §1 starts

## 1. EditBuffer operations + kill ring

- [ ] 1.1 Add `kill_ring: Option<String>` field to `EditBuffer` (the buffer stores `text: String`; kill ring matches)
- [ ] 1.2 Implement `move_line_start`, `move_line_end` (set cursor to 0 / char count of `text`)
- [ ] 1.3 Implement `move_word_back`, `move_word_forward` using the unified `[A-Za-z0-9_]` boundary rule
- [ ] 1.4 Implement `kill_to_end`, `kill_to_start`: extract the range as `String`, replace it in `text`, store in kill ring, reposition cursor (in char-index space, translating to byte ranges via `char_indices`)
- [ ] 1.5 Implement `kill_word_back`, `kill_word_forward`: same shape as kill_to_* but bounded by word boundaries
- [ ] 1.6 **Behavior change**: rework existing `delete_word_backward` to use `[A-Za-z0-9_]` (today: whitespace-bounded). Update existing tests; add a regression test showing the new boundary against punctuation (`foo.bar` → two kills)
- [ ] 1.7 Implement `yank`: insert `kill_ring.clone()` at cursor (no-op if `None`); does not clear the ring
- [ ] 1.8 Implement `transpose_chars`: swap chars at and before cursor
- [ ] 1.9 Unit tests for each operation, including edge cases (cursor at 0, cursor at end, empty buffer, ASCII vs multi-byte chars)

## 2. Edit-buffer keymap

- [ ] 2.1 Define `edit.*` commands in `ft/src/tui/widgets/edit_keymap.rs`: `edit.move-line-start`, `edit.move-line-end`, `edit.move-char-back`, `edit.move-char-forward`, `edit.move-word-back`, `edit.move-word-forward`, `edit.kill-to-end`, `edit.kill-to-start`, `edit.kill-word-back`, `edit.kill-word-forward`, `edit.yank`, `edit.transpose-chars`, `edit.delete-char-back`, `edit.delete-char-forward`, `edit.complete`, `edit.dismiss-popup`
- [ ] 2.2 Build static `EDIT_KEYMAP: KeyMap`: bind every chord listed in design.md to the corresponding command
- [ ] 2.3 `EditBuffer::handle_event` becomes: lookup chord in `EDIT_KEYMAP`, dispatch to `dispatch_edit_command` if found; otherwise treat as raw char insert
- [ ] 2.4 Add a `CommandScope::Widget(&'static str)` variant in `ft/src/tui/command.rs`; update `CommandScope::as_str` (`widget/{w}`), the registry filter logic, and any `match` arms over `CommandScope`. Register `edit.*` under `CommandScope::Widget("edit-buffer")`; verify it appears in `ft commands list`
- [ ] 2.5 Tests: each chord in each mount site (query bar, picker input, rename modal, quickline) produces the expected buffer state

## 3. `CompletionProvider` trait + items

- [ ] 3.1 Create `ft/src/tui/widgets/completion.rs` with `CompletionProvider` trait, `CompletionContext`, `CompletionTrigger`, `CompletionItem`, `CompletionKind`, `TriggerSet`
- [ ] 3.2 `TriggerSet` supports: printable chars, specific chars (e.g. `.`, `:`), manual-only (only fires on explicit Tab-to-complete)
- [ ] 3.3 `CompletionItem.replace_span`: byte range to replace; default behaviour (None) is "replace current word"
- [ ] 3.4 `StubCompletionProvider` in `#[cfg(test)] mod tests` returns a fixed Vec when asked, used by tests in this change

## 4. `CompletionPopup` widget

- [ ] 4.1 `CompletionPopup` struct: `items: Vec<CompletionItem>, selected: usize, scroll_offset: usize`
- [ ] 4.2 Positioning logic: compute popup rect based on cursor row vs host area (above/below), clamp to bounds, max 8 visible items
- [ ] 4.3 Render with item label + kind glyph (e.g., `A` for attribute, `O` for operator); optional dim description below
- [ ] 4.4 Key handling: `Up`/`Ctrl+P` selection up, `Down`/`Ctrl+N` selection down, `Tab`/`Enter` accept (consume; return `Accepted(item)`), `Esc` dismiss
- [ ] 4.5 Unit + snapshot tests for the popup against the stub provider

## 5. EditBuffer ↔ popup integration

- [ ] 5.1 Add `completion: Option<Box<dyn CompletionProvider>>` and `popup: Option<CompletionPopup>` fields to `EditBuffer`
- [ ] 5.2 On every mutating input event (char insert, delete, kill, yank), if `completion` is `Some` and the event matches `trigger_on()`, call `provider.complete(ctx)`; open / refresh / close the popup based on the returned items
- [ ] 5.3 On dispatch: route the event to `popup.handle_event` first if `popup` is `Some`; on `Accepted(item)`, apply `item.replace_span` (or current-word default) + `item.insert_text` to the buffer, close the popup; on `Dismissed`, close the popup
- [ ] 5.4 Tests: with stub provider, typing triggers the popup; Tab accepts an item; the buffer reflects the chosen `insert_text` at the correct span
- [ ] 5.5 Tests: a popup opened on a buffer mounted inside a modal does NOT leak keys to the modal until the popup closes

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

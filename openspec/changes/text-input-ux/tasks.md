## 1. EditBuffer operations + kill ring

- [ ] 1.1 Add `kill_ring: Option<Vec<char>>` field to `EditBuffer`
- [ ] 1.2 Implement `move_line_start`, `move_line_end` (set cursor to 0 / `text.len()`)
- [ ] 1.3 Implement `move_word_back`, `move_word_forward` using the `[A-Za-z0-9_]` boundary rule
- [ ] 1.4 Implement `kill_to_end`, `kill_to_start`: drain the range, store in kill ring, reposition cursor
- [ ] 1.5 Implement `kill_word_back`, `kill_word_forward`: same as kill_to_* but bounded by word boundaries
- [ ] 1.6 Implement `yank`: insert `kill_ring.clone()` at cursor (no-op if `None`)
- [ ] 1.7 Implement `transpose_chars`: swap chars at and before cursor
- [ ] 1.8 Unit tests for each operation, including edge cases (cursor at 0, cursor at end, empty buffer, ASCII vs multi-byte chars)

## 2. Edit-buffer keymap

- [ ] 2.1 Define `edit.*` commands in `ft/src/tui/widgets/edit_keymap.rs`: `edit.move-line-start`, `edit.move-line-end`, `edit.move-char-back`, `edit.move-char-forward`, `edit.move-word-back`, `edit.move-word-forward`, `edit.kill-to-end`, `edit.kill-to-start`, `edit.kill-word-back`, `edit.kill-word-forward`, `edit.yank`, `edit.transpose-chars`, `edit.delete-char-back`, `edit.delete-char-forward`, `edit.complete`, `edit.dismiss-popup`
- [ ] 2.2 Build static `EDIT_KEYMAP: KeyMap`: bind every chord listed in design.md to the corresponding command
- [ ] 2.3 `EditBuffer::handle_event` becomes: lookup chord in `EDIT_KEYMAP`, dispatch to `dispatch_edit_command` if found; otherwise treat as raw char insert
- [ ] 2.4 Register `edit.*` commands in the central registry (under scope `EditBuffer`); they appear in `ft commands list`
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

- [ ] 7.1 Audit every site that uses `EditBuffer` (`query bar`, `FuzzyPicker` internal, `GraphRenameState`, quickline, capture var prompt) and confirm the new bindings work in each
- [ ] 7.2 Integration test in `ft/src/tui/tests.rs` per site: open the input, press `Ctrl+A`, type, press `Ctrl+E`, press `Ctrl+K`, snapshot the buffer state

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

## Why

Every text input in ft — the Graph tab query bar, the fuzzy pickers, the rename modal, the quickline task entry, the capture var prompts — uses the shared `EditBuffer` widget (`ft/src/tui/widgets/edit_buffer.rs`). The widget supports basic cursor movement and character entry, but readline-style bindings every terminal user expects (`Ctrl+A` line start, `Ctrl+E` line end, `Ctrl+K` kill-to-end, word jumps via `Opt+Left`/`Opt+Right` or `Alt+B`/`Alt+F`) are missing. The friction is small per keystroke but compounds — you can feel it most when editing a long graph DSL query, where `Ctrl+A` to jump back to fix a typo isn't there.

The query DSL is also the place where autocompletion would help most: attribute names (`kind`, `path`, `status`, …), operators (`in`, `includes`, `starts_with`), and enum values (`Note`, `Task`, `Open`, `Done`) all benefit from a popup that surfaces valid completions as the user types. This change lays the *foundation* for that: a `CompletionProvider` trait, a popup widget rendered at cursor position, and the hook points in `EditBuffer` to query a provider on input. No concrete providers ship in this change — DSL completion is its own follow-up — but everything is in place so adding one is "implement a 200-line provider," not "first reshape the widget."

This change sequences after `commands-and-keymaps` (so edit-buffer bindings are expressed as commands) and before `unify-query-dsls` (so the new DSL surface area lands on the improved editing experience).

## What Changes

### Edit-buffer enhancements

- Extend `EditBuffer` with: `move_line_start`, `move_line_end`, `move_word_back`, `move_word_forward`, `kill_to_end`, `kill_to_start`, `kill_word_back`, `kill_word_forward`, `yank` (paste from kill ring), `transpose_chars`.
- Add a one-slot kill ring on `EditBuffer` (the last kill replaces the previous; the next `Ctrl+Y` yanks it).
- Word boundary definition matches `Atom::is_word_char` (alphanumeric + `_`). Configurable later; not now.

### Edit-buffer keymap

- Define a new `EDIT_KEYMAP` (per the model from `commands-and-keymaps`) with bindings for every operation above. Scope: `EditBuffer` — applies wherever the widget is mounted (query bar modal, picker input, rename modal, quickline, capture var prompt).
- Default bindings (overrideable by future user config): `Ctrl+A` start, `Ctrl+E` end, `Ctrl+K` kill-to-end, `Ctrl+U` kill-to-start, `Ctrl+W` kill-word-back (already partially supported), `Alt+B`/`Alt+Left`/`Opt+Left` word-back, `Alt+F`/`Alt+Right`/`Opt+Right` word-forward, `Alt+D` kill-word-forward, `Ctrl+T` transpose, `Ctrl+Y` yank.
- Bindings are expressed as `edit.*` commands: `edit.move-line-start`, `edit.move-word-forward`, `edit.kill-to-end`, etc. Registered in the central command registry; `?` overlay and `docs/keybindings.md` document them.

### Autocompletion scaffolding

- New `CompletionProvider` trait: `fn complete(&mut self, ctx: &CompletionContext) -> Vec<CompletionItem>` where `CompletionContext` carries the buffer text, cursor position, and a provider-specific reason (e.g., "word at cursor", "after keyword `where`").
- New `CompletionPopup` widget: a slim list rendered near the cursor (above if the cursor is in the bottom half of the area, below otherwise), styled like the fuzzy picker but vertical-only and scoped to the active input.
- `EditBuffer` gains an optional `completion: Option<Box<dyn CompletionProvider>>` slot. When set, the buffer's `handle_event` queries the provider on each input mutation and renders the popup if items are returned.
- Selecting a completion (`Tab` or `Enter`-in-popup) inserts the chosen text at the buffer's "completion span" (provider-defined: from the start of the current token to the cursor) and dismisses the popup. `Esc` dismisses without insertion.
- The popup integrates with the modal driver from `extract-modal-driver`: it's a sub-modal of whatever modal hosts the edit buffer. Dispatch precedence is `popup → host modal → tab → global`.

### Zero concrete providers in this change

- The trait, popup widget, and integration hooks are all wired and tested with a fixture `StubCompletionProvider` returning canned items.
- The graph DSL completion provider, file-path provider, tag provider, etc. are explicitly out of scope. They are follow-up changes against this scaffold.

## Capabilities

### New Capabilities

- `tui-edit-buffer-keymap`: An `EDIT_KEYMAP` of readline-style bindings (`Ctrl+A`/`E`/`K`/`U`/`W`/`T`/`Y`, `Alt+B`/`F`/`D`, `Opt+Left`/`Right`) that applies wherever the shared `EditBuffer` widget is mounted. Commands `edit.*` registered in the central command registry.
- `tui-autocomplete-scaffold`: A `CompletionProvider` trait, a `CompletionPopup` widget, and `EditBuffer` integration that queries the provider on input and renders the popup. The widget participates in the modal driver's dispatch precedence as a sub-modal of its host.

### Modified Capabilities

- `tui-edit-buffer`: Extended with `move_line_start`, `move_line_end`, `move_word_back`, `move_word_forward`, `kill_to_end`, `kill_to_start`, `kill_word_back`, `kill_word_forward`, `yank`, `transpose_chars` operations, plus a one-slot kill ring and optional `CompletionProvider` slot.

## Impact

- **Modified**: `ft/src/tui/widgets/edit_buffer.rs` (~150 net new lines for new ops, kill ring, completion slot, popup integration).
- **New**: `ft/src/tui/widgets/completion.rs` (≈ 250 lines: `CompletionProvider` trait, `CompletionItem`, `CompletionContext`, `CompletionPopup` widget).
- **New**: `ft/src/tui/widgets/edit_keymap.rs` (the static `EDIT_KEYMAP` definition + `edit.*` command set).
- **Modified**: `ft/src/tui/modal.rs` (the `ActiveModal` enum gains nothing; the popup is a *sub-modal* dispatched by the host edit-buffer-bearing modal — no top-level variant needed).
- **Tests**: New unit tests for each edit-buffer operation, integration tests proving readline bindings work in every text input site, snapshot test of the completion popup against the stub provider.
- **Docs**: `docs/commands.md` gains an "Edit buffer commands" section; `docs/keybindings.md` is regenerated to include them.
- All four build invariants stay green.
- **Cross-platform note**: On macOS terminals (iTerm, Terminal.app, WezTerm, Ghostty), `Opt+Left/Right` emits `Alt+Left/Right` to crossterm by default. The keymap binds both forms so the user-facing experience matches regardless of terminal config. Documented in `docs/keybindings.md`.

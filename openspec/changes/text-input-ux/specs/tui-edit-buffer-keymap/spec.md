## ADDED Requirements

### Requirement: `EDIT_KEYMAP` binds readline chords to `edit.*` commands

A static `EDIT_KEYMAP: KeyMap` SHALL define the default chord-to-command bindings for the shared `EditBuffer` widget. The map SHALL bind: `Ctrl+A` → `edit.move-line-start`, `Ctrl+E` → `edit.move-line-end`, `Ctrl+B`/`Left` → `edit.move-char-back`, `Ctrl+F`/`Right` → `edit.move-char-forward`, `Alt+B`/`Alt+Left` → `edit.move-word-back`, `Alt+F`/`Alt+Right` → `edit.move-word-forward`, `Ctrl+K` → `edit.kill-to-end`, `Ctrl+U` → `edit.kill-to-start`, `Ctrl+W` → `edit.kill-word-back`, `Alt+D` → `edit.kill-word-forward`, `Ctrl+Y` → `edit.yank`, `Ctrl+T` → `edit.transpose-chars`, `Ctrl+H`/`Backspace` → `edit.delete-char-back`, `Ctrl+D`/`Delete` → `edit.delete-char-forward`. `Tab` → `edit.complete` and `Esc` → `edit.dismiss-popup` SHALL apply only when a completion popup is open.

#### Scenario: `Ctrl+A` jumps cursor to line start
- **WHEN** an `EditBuffer` contains `hello world` with the cursor at position 11 and receives a `Ctrl+A` key event
- **THEN** `EDIT_KEYMAP` resolves the chord to `edit.move-line-start`, the buffer dispatches it, and the cursor moves to position 0

#### Scenario: `Alt+B` jumps cursor one word back
- **WHEN** an `EditBuffer` contains `foo bar baz` with the cursor at position 11 and receives an `Alt+B` key event
- **THEN** the cursor moves to position 8 (start of `baz`)

### Requirement: `edit.*` commands are scoped to a new `CommandScope::Widget("edit-buffer")` variant

A new `CommandScope::Widget(&'static str)` variant SHALL be added to the command-scope enum. Every `edit.*` command SHALL be registered with `scope: CommandScope::Widget("edit-buffer")`. The scope SHALL render in user output (`?` overlay, `ft commands list --scope`, generated docs) as `widget/edit-buffer`.

#### Scenario: `ft commands list --scope widget/edit-buffer` lists every edit-buffer command
- **WHEN** the user runs `ft commands list --scope widget/edit-buffer`
- **THEN** every `edit.*` command appears in the output and no commands from other scopes appear

#### Scenario: `?` overlay groups edit-buffer commands under their widget scope
- **WHEN** the user opens `?` while a modal that hosts an `EditBuffer` is active
- **THEN** the overlay renders an "Edit buffer" section listing the chords from `EDIT_KEYMAP`

### Requirement: `EDIT_KEYMAP` applies wherever the shared widget is mounted

Every mount site of the shared `EditBuffer` widget (graph query bar, fuzzy picker input, rename modal, quickline, capture var prompt, timeblocks form fields, journal entry input) SHALL receive the same `EDIT_KEYMAP` behavior without per-site wiring. The host modal SHALL forward unhandled key events to the buffer's dispatch path without filtering on modifiers.

#### Scenario: Graph query bar accepts `Ctrl+A` after migration
- **WHEN** the graph query bar is open (`/`), the buffer contains `node where path includes "ops/" and tag = ` with the cursor at the end, and the user presses `Ctrl+A`
- **THEN** the cursor moves to position 0 (without closing the modal, without falling through to a tab binding)

#### Scenario: Picker input accepts `Alt+B`
- **WHEN** any fuzzy picker is open with text typed into its input and the user presses `Alt+B`
- **THEN** the picker's input cursor moves one word back; the picker's selection does not change

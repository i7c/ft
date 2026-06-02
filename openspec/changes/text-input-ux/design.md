## Context

The existing `EditBuffer` (`ft/src/tui/widgets/edit_buffer.rs`) is a single-line `Vec<char>` with a `cursor: usize` index. It supports char insertion, deletion (`Backspace`, `Delete`), cursor arrows, and (partially) `Ctrl+W` word-back delete. Beyond that, every text-input site has its own local behaviour, and none of the standard readline conventions are wired up. The DSL query bar is the most painful site because queries are long and editing them feels mechanical (arrow-key your way back, character-by-character).

Three things have to land together for the experience to actually improve:

1. The buffer needs the right operations (word jumps, line jumps, kill operations).
2. The keymap needs to actually invoke them. With the commands-and-keymaps change in flight, this is now a uniform `EDIT_KEYMAP` of `edit.*` commands.
3. Completion has to be designable now even if it's not implementable yet — otherwise the next change has to retrofit the popup and the dispatch ordering. Doing the scaffold cleanly the first time costs less than adding it later.

## Goals / Non-Goals

**Goals:**

- Readline-style bindings (`Ctrl+A`/`E`/`K`/`U`/`W`/`T`/`Y`, `Alt+B`/`F`/`D`, `Opt+Left`/`Right`) work in every site that uses `EditBuffer`.
- `Opt+Left`/`Opt+Right` on macOS terminals work without per-terminal config (we bind both Opt and Alt forms).
- A one-slot kill ring lets `Ctrl+Y` paste back the last killed region.
- `CompletionProvider` trait + `CompletionPopup` widget land with a stub provider exercising the dispatch path.
- The completion popup integrates with the modal driver's precedence rules.
- The new `edit.*` commands are first-class in the registry — `?` overlay shows them, `docs/keybindings.md` documents them, `ft commands list` lists them.

**Non-Goals:**

- No concrete completion providers (graph DSL completion, file paths, tags) — separate change.
- No multi-line input — `EditBuffer` stays single-line.
- No vim-style modal editing.
- No multi-slot kill ring (one slot only — sufficient for the use case).
- No clipboard integration (yank is internal to the kill ring; nothing reaches the OS clipboard).
- No undo/redo on the buffer (separate, larger change).

## Decisions

### One kill-ring slot, replaced on each kill

```rust
struct EditBuffer {
    text: Vec<char>,
    cursor: usize,
    kill_ring: Option<Vec<char>>,
    completion: Option<Box<dyn CompletionProvider>>,
    popup: Option<CompletionPopup>,
}
```

Every kill operation (`Ctrl+K`, `Ctrl+U`, `Ctrl+W`, `Alt+D`) replaces the previous `kill_ring` with the killed text. `Ctrl+Y` inserts the kill ring at the cursor.

**Alternative considered: multi-slot ring with `Alt+Y` cycling.** Rejected for v1 — readline's `Alt+Y` is rarely-discovered and not worth the bookkeeping.

### Word boundaries

A word is a maximal run of `[A-Za-z0-9_]`. `move_word_forward` from inside or just before a word moves to one past its end; `move_word_back` from inside or just after a word moves to its start. Whitespace runs are skipped.

This matches the existing `Ctrl+W` semantics that the buffer partially supports today (verified by reading `delete_prev_word`). Extending to forward-word ops is symmetric.

### Bindings table

| Chord | Command | Action |
| --- | --- | --- |
| `Ctrl+A` | `edit.move-line-start` | cursor to start |
| `Ctrl+E` | `edit.move-line-end` | cursor to end |
| `Ctrl+B`, `Left` | `edit.move-char-back` | cursor one char back |
| `Ctrl+F`, `Right` | `edit.move-char-forward` | cursor one char forward |
| `Alt+B`, `Opt+Left` | `edit.move-word-back` | cursor one word back |
| `Alt+F`, `Opt+Right` | `edit.move-word-forward` | cursor one word forward |
| `Ctrl+K` | `edit.kill-to-end` | delete cursor → end, save to kill ring |
| `Ctrl+U` | `edit.kill-to-start` | delete start → cursor, save to kill ring |
| `Ctrl+W` | `edit.kill-word-back` | delete word before cursor, save to kill ring |
| `Alt+D` | `edit.kill-word-forward` | delete word after cursor, save to kill ring |
| `Ctrl+Y` | `edit.yank` | insert kill ring at cursor |
| `Ctrl+T` | `edit.transpose-chars` | swap chars at and before cursor |
| `Ctrl+H`, `Backspace` | `edit.delete-char-back` | delete char before cursor |
| `Ctrl+D`, `Delete` | `edit.delete-char-forward` | delete char at cursor |
| `Tab` | `edit.complete` | accept selected completion (only when popup open) |
| `Esc` | `edit.dismiss-popup` | close completion popup (only when popup open) |

All chords are registered in the central registry as `edit.*` commands.

**Alternative considered: only bind `Opt+Left/Right`, not `Alt+B/F`.** Rejected — Alt forms are the readline standard and work on Linux + crossterm out of the box, and binding both is cheap.

### Completion as a sub-modal

The completion popup is not a top-level `ActiveModal` variant. It's a sub-modal owned by `EditBuffer`. Dispatch precedence:

```text
event arrives
  ├─ active_modal active?
  │   ├─ yes → modal hosts an EditBuffer with popup open?
  │   │       ├─ yes → popup.handle_event first
  │   │       │       ├─ Consumed/Closed/Insert → no fallthrough
  │   │       │       └─ NotHandled → fall through to host modal
  │   │       └─ no  → host modal handles normally
  │   └─ no  → tab + global as usual
```

The popup's keymap is small — `Up`/`Down`/`Ctrl+P`/`Ctrl+N` to navigate, `Tab`/`Enter` to accept, `Esc` to dismiss. The host modal sees keys only when the popup returns `NotHandled` (printable characters, line edits — popup stays open and re-queries).

**Alternative considered: popup is a sibling modal at the App level.** Rejected — the popup must follow the cursor of the host modal's edit buffer. Coupling them tightly avoids cross-modal coordination logic.

### `CompletionProvider` trait

```rust
pub struct CompletionContext<'a> {
    pub text: &'a str,
    pub cursor_byte: usize,
    pub trigger: CompletionTrigger,    // Manual | OnInput
}

pub enum CompletionTrigger { Manual, OnInput }

pub struct CompletionItem {
    pub label: String,
    pub insert_text: String,
    pub replace_span: Option<Range<usize>>,  // byte range; default = current word
    pub kind: CompletionKind,                // Attribute, Operator, Value, Keyword, Path, …
    pub description: Option<String>,
}

pub trait CompletionProvider {
    fn complete(&mut self, ctx: &CompletionContext) -> Vec<CompletionItem>;
    fn trigger_on(&self) -> TriggerSet;  // chars/contexts that auto-fire the popup
}
```

The provider is queried by `EditBuffer` on each input event that matches the provider's `trigger_on()`. The popup is opened with the returned items. Items are ordered by the provider; the popup does no re-ranking (providers control relevance).

**Alternative considered: provider returns ranked candidates and popup re-ranks.** Rejected — providers know their domain (graph DSL knows attribute vs. value context); re-ranking would discard that knowledge.

### Cross-platform Opt vs Alt

On Linux terminals, `Opt`-equivalent keys typically arrive as `KeyModifiers::ALT`. On macOS terminals, `Opt+Left/Right` is configured per terminal — most modern terminals (iTerm, WezTerm, Ghostty, Kitty) emit `Alt+Left/Right` by default; Terminal.app sometimes emits escape-prefixed sequences interpreted as `Esc`+`Left`. Crossterm normalizes the common cases.

The keymap binds both `Alt+B`/`Alt+F`/`Alt+D` and the cursor variants `Alt+Left`/`Alt+Right`. If a user's terminal sends something different, the existing `crossterm` event stream surfaces it as-is and we document the alternative in `docs/keybindings.md` with terminal-specific notes.

### Stub provider for testing

`tests/fixtures/StubCompletionProvider` returns a fixed list of items when asked. Used by:

- A unit test that an attached provider gets called on each input mutation.
- A snapshot test of the popup overlay rendered against the stub.
- An integration test that selecting a stub item rewrites the buffer correctly.

This proves the dispatch and rendering paths work end-to-end without needing a real provider.

## Risks / Trade-offs

- **[Terminal key variance for `Opt`/`Alt`]** → Mitigated by binding both `Alt+B/F/D` and `Opt+Left/Right` forms; documenting per-terminal notes. Users on niche terminals can override via the future user keymap config.
- **[Single-slot kill ring loses earlier kills]** → Acceptable for v1. Multi-slot is a small additive change later.
- **[`Ctrl+H` and `Backspace` collide on some terminals]** → Bind both to the same command; existing behaviour. No regression.
- **[Completion popup adds rendering cost on every keystroke]** → Provider is queried only when its `trigger_on()` matches the event. The popup re-renders on each frame anyway. No measurable cost.
- **[Popup positioning near screen edges]** → Above-if-bottom-half-of-area, below otherwise. Clamp to screen bounds. Test cases: popup at row 0, popup at last row, popup at narrow terminal.
- **[`Tab` is used as a global tab-cycle key]** → Inside a text input with a popup open, `Tab` is consumed by `edit.complete`. When the popup is closed, `Tab` falls through to `app.next-tab`. This is the existing dispatch precedence and matches user intuition (Tab inside an input == completion).

## Open Questions

- Should `edit.yank` cycle through history if pressed repeatedly (à la Emacs `Alt+Y`)? **Leaning:** no for v1 — out of scope until a multi-slot ring lands.
- Should the popup show item descriptions in a side panel or below each item? **Leaning:** inline below the label, like a fuzzy picker. Match the established visual language.
- Where should the provider's "context detection" live? E.g., "I'm right after `where`, suggest attributes." **Leaning:** in the provider itself — it sees the text and cursor. The trait stays simple.

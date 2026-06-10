## Context

The existing `EditBuffer` (`ft/src/tui/widgets/edit_buffer.rs`) is a single-line `String` with `cursor: usize` as a *character* count. It supports char insertion, deletion (`Backspace`, `Delete`), cursor arrows, and (partially) `Ctrl+W` whitespace-bounded word-back delete. Beyond that, every text-input site has its own local behaviour, and none of the standard readline conventions are wired up. The DSL query bar is the most painful site because queries are long and editing them feels mechanical (arrow-key your way back, character-by-character).

Three blockers, not two:

0. **The DSL query bar doesn't actually use `EditBuffer` today.** `View.query_text: String` + `View.input_cursor: usize` (byte cursor) on `GraphTab`, with the `QueryBar` modal forwarding only an allowlist of plain-modifier keys (`Char | Backspace | Delete | Left | Right | Home | End`, `NONE | SHIFT`). Even with new bindings on the widget, the query bar would never see `Ctrl+A`/`Ctrl+E`/`Alt+B`. Migrating the query bar onto `EditBuffer` is step zero.
1. The buffer needs the right operations (word jumps, line jumps, kill operations).
2. The keymap needs to actually invoke them. With the commands-and-keymaps change archived, this is now a uniform `EDIT_KEYMAP` of `edit.*` commands.
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

### Migrate the graph query bar onto `EditBuffer`

The query bar's per-view state changes from:

```rust
struct View {
    query_text: String,
    input_cursor: usize, // byte offset
    // …
}
```

to:

```rust
struct View {
    query: QueryBarState,
    // …
}

struct QueryBarState {
    buf: EditBuffer,
    // existing fields like parsed query, snippet, etc. stay
}
```

Touchpoints:

- `QueryBar::handle_event` (`ft/src/tui/modal.rs:968`) stops the hardcoded `match (key.code, key.modifiers)` filter. `Esc` → `Closed`, `Enter` → fire `GraphApplyQueryBar` then `Closed`; **everything else** forwards via `AppRequest::GraphQueryBarKey { view_id, key }` regardless of modifier. The buffer's keymap decides what to do with `Ctrl+A`, `Alt+B`, plain `Char`, etc.
- `GraphTab::graph_query_bar_key` (`ft/src/tui/tabs/graph.rs:2727`) becomes a one-liner: route the key through `v.query.buf.handle_event(...)`.
- Read sites that touch `v.query_text` / `v.input_cursor` (the seeding paths around lines 2388, 2574, 2687, the rendering code, the `query_snippet` helper) switch to `v.query.buf.text` and `v.query.buf.cursor`. The byte-vs-char offset distinction matters at render time — the existing renderer uses byte offsets, so we adapt at the render boundary (char offset → byte via `text.char_indices().nth(cursor)`).

The migration is mechanical but touches enough call sites to deserve its own task block (§0 below).

**Alternative considered: skip migration; document the gap.** Rejected — the headline value prop of this change is "DSL queries become editable." Without the migration the change ships a feature the user can't reach.

### Add `CommandScope::Widget(&'static str)`

The existing scope enum (`ft/src/tui/command.rs:87`) is `Global | Tab(&str) | Modal(&str)`. The `edit.*` set belongs to none of those — it lives on a widget that any modal or tab might mount. Add a fourth variant:

```rust
pub enum CommandScope {
    Global,
    Tab(&'static str),
    Modal(&'static str),
    Widget(&'static str), // new
}
```

Ripples: `CommandScope::as_str` gains a `format!("widget/{w}")` arm; `ft commands list --scope widget/edit-buffer` filters by it; the `?` overlay grouping treats widget scopes like modal scopes for display order; the docs generator emits a "Widget commands" section.

**Alternative considered: register under `Modal("edit-buffer")`.** Rejected — `EditBuffer` is plainly not a modal (the modal driver doesn't know about it), and shoehorning would mislead future readers grepping for modal handling.

### One kill-ring slot, replaced on each kill

The existing `EditBuffer` stores `text: String` with `cursor: usize` as a *char* count (not a byte offset) — see `ft/src/tui/widgets/edit_buffer.rs:11`. The new fields slot in alongside; the kill ring stores a `String` so it round-trips back through `insert` unchanged.

```rust
struct EditBuffer {
    text: String,
    cursor: usize,             // character count, not byte offset
    kill_ring: Option<String>,
    completion: Option<Box<dyn CompletionProvider>>,
    popup: Option<CompletionPopup>,
}
```

Every kill operation (`Ctrl+K`, `Ctrl+U`, `Ctrl+W`, `Alt+D`) replaces the previous `kill_ring` with the killed text. `Ctrl+Y` inserts the kill ring at the cursor.

Word ops work in char-index space against `self.text.chars().collect::<Vec<_>>()` (or an on-the-fly iterator) and translate to byte ranges via `char_indices` only when calling `String::replace_range` — same shape as the current `delete_word_backward`.

**Alternative considered: multi-slot ring with `Alt+Y` cycling.** Rejected for v1 — readline's `Alt+Y` is rarely-discovered and not worth the bookkeeping.

### Word boundaries

A word is a maximal run of `[A-Za-z0-9_]`. `move_word_forward` from inside or just before a word moves to one past its end; `move_word_back` from inside or just after a word moves to its start. Non-word runs (whitespace *and* punctuation) are skipped.

**This is a behavior change for the existing `Ctrl+W`.** Today `delete_word_backward` is `unix-word-rubout` (whitespace-bounded — `foo.bar.baz` is one kill). After this change, the same input is three kills. We chose the unified rule because:

- It matches Emacs/readline `Alt+B`/`Alt+F`/`Alt+D` and is what users typing DSL queries (`priority=High`, `path includes "ops/"`) actually want — character-class words, not whitespace runs.
- Carrying two definitions of "word" inside one widget invites the next reader to ask which rule applies where.

Release notes in `docs/keybindings.md` call the change out so users who relied on the old behavior aren't surprised.

**Alternative considered: keep `Ctrl+W` whitespace-bounded, use `[A-Za-z0-9_]` only for `Alt+B/F/D`.** Rejected — two rules is the worst of both worlds.

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

**Char-vs-byte adapter.** `EditBuffer.cursor` is a *character* count; `CompletionContext.cursor_byte` and `CompletionItem.replace_span` are *byte* offsets. The buffer converts when building the context (`self.text.char_indices().nth(self.cursor).map(|(b, _)| b).unwrap_or(self.text.len())`) and when applying an accepted item's `replace_span` (a byte range arrives, becomes a `String::replace_range` directly, and the cursor is updated by counting chars from 0 to the new byte position). Providers always see and emit byte offsets so they can index into `text` without a conversion table.

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

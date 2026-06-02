## Why

`ft notes journal <note>` (shipped in `related-notes-journal`) surfaces a reverse-chronological feed of paragraph mentions for a note, but only as one-shot CLI output. The most common workflow — picking a note, skimming its history, opening an entry to edit — currently means dropping out of `ft` for the editor and back to the shell for the next query. Bringing the journal into the TUI closes that loop: pick interactively, scroll the feed, jump from a paragraph straight into `$EDITOR`, and bounce there from the graph tab without retyping a note selector.

## What Changes

- New `Journal` tab in the TUI, slotted after the existing Graph tab (`App::new`).
- The tab reuses the existing `FuzzyPicker<VaultFilePickerSource>` for note selection.
- Loaded journal entries persist across tab switches; `R` reloads, `c` clears back to the picker.
- `Enter` on an entry raises an editor-open request that jumps to the paragraph's `line_start` in the source note.
- New `Tab::queue_journal_for(path)` trait hook (default no-op), mirroring the existing `queue_related_modal` plumbing.
- Graph tab gains a `J` keybinding that, when a `NodeKind::Note` row is selected, queues that note's path on the Journal tab and switches focus to it.
- `BlameCache` is loaded once per Journal-tab session and reused; saved after each successful `build_journal` call (best-effort).

## Capabilities

### New Capabilities

- `journal-tui-tab`: The Journal tab itself — state, picker integration, rendering, navigation, and the `Enter`/`R`/`c` keymap.
- `graph-to-journal-jump`: The graph tab's `J` keybinding plus the App-level tab-switch mechanism that forwards the selected note's path to the Journal tab.

### Modified Capabilities

## Impact

- **`ft/src/tui/tabs/journal.rs`** (new): Journal tab implementation and its `Tab` impl.
- **`ft/src/tui/tabs/mod.rs`**: register the new tab module.
- **`ft/src/tui/app.rs`**: include `JournalTab` in the `App::new` tab vector.
- **`ft/src/tui/tab.rs`**: add `queue_journal_for(&Path)` default-no-op method to the `Tab` trait.
- **`ft/src/tui/tabs/graph.rs`**: `J` keybinding when a Note row is selected; raises a request that the App services by switching tabs and forwarding the path.
- **`ft/src/tui/help.rs`** (no API change): each tab contributes its own keymap via `help_sections`.
- **`ft/src/tui/tests.rs`**: new `TestBackend` snapshot tests for the Journal tab states and the cross-tab jump.
- No new dependencies; no `ft-core` changes (the data path already exists in `ft_core::journal` + `ft_core::blame_cache`).
- All four build invariants must stay green.

## Why

The Journal tab today silently holds one or many sources (loaded via `/`, `Shift+J` from the Graph tab, or a Review-tab handoff) but offers no visual confirmation beyond a single-line title bar, no way to remove or add individual sources, and no path to grow the source set from a Graph-tab multi-selection. As the Journal tab becomes the central "what does my vault think about this set of topics" surface, the source set needs to be a first-class, editable object — not an invisible side effect of how the user got there.

## What Changes

- Add an always-visible **Sources strip** at the top of the Journal tab. The strip renders in both empty and loaded states, lists every current source (Note path or `<raw> (ghost)`), surfaces the window if one is attached, and truncates with `… +N more` when the set exceeds the visible width.
- Add a **Sources Manager modal** that lists every current source with row navigation, `d` to remove the focused row, `c` to clear all, and `a` to open an inner fuzzy picker. The picker results SHALL include both real notes and ghost names from the current graph.
- **BREAKING (UX-only)**: Pressing `/` on the Journal tab opens the Sources Manager (landed on the add-source picker) instead of the legacy single-source fuzzy picker overlay. Selecting a single note no longer replaces the entire source set as a side effect; the user adds it explicitly via the manager.
- Add a new Graph-tab command `graph.add-to-journal-sources` (bound to `Shift+A`) that takes the active view's `multi_selected` set (or the cursor row if empty), converts each entry to a `JournalTarget` (Note or Ghost), and hands them to the Journal tab.
- Add an **Append-or-Replace confirmation prompt** raised on the Journal tab whenever an external add-sources request arrives (initially: from the Graph-tab `Shift+A`). The prompt offers `[a] append` (union with current sources) or `[r] replace` (discard current set). The existing Review→Journal `Enter` handoff continues to use the replace-style `JournalForMulti` path; only the new Graph append flow surfaces this prompt.
- Update the journal-tab build pipeline so the source set is a single mutable `Vec<JournalTarget>` consulted on every rebuild — `load_for` / `load_for_multi` collapse into one `rebuild_journal` that always reads from this slot.

## Capabilities

### New Capabilities
- `journal-sources-manager`: An always-visible sources strip plus a Sources Manager modal that lets the user view, clear, add, and remove individual Journal-tab sources, with ghost-aware fuzzy picking.
- `graph-to-journal-sources`: A Graph-tab keybinding that hands the current multi-selection (or cursor row) to the Journal tab as additional sources via an Append/Replace prompt.

### Modified Capabilities
- `journal-tui-tab`: Replaces the "Note selection via fuzzy picker" requirement with sources-strip + manager-modal behavior. Adds the append-or-replace prompt requirement. Existing entry-navigation, in-window filter, send-to-synth, and multi-target queue requirements stay intact.
- `graph-to-journal-jump`: Adds the multi-select-to-sources requirement alongside the existing single-target `Shift+J` jump (which is preserved unchanged).

## Impact

- **Affected code (TUI-only — no `ft-core` changes)**:
  - `ft/src/tui/tabs/journal.rs` — sources strip rendering; collapse `load_for` / `load_for_multi` into one source-set-backed `rebuild`; consume new AddSources hook; host the append/replace prompt; remove the per-tab `picker` field in favour of the modal.
  - `ft/src/tui/tabs/graph.rs` — new `graph.add-to-journal-sources` command, default keymap binding (`Shift+A`), help section entry.
  - `ft/src/tui/tab.rs` — new `AppRequest::JournalAddSources { targets, default_mode }` variant and matching `Tab::queue_journal_add_sources` hook with default no-op.
  - `ft/src/tui/modal.rs` (and `app.rs` dispatch) — new `ActiveModal::JournalSources` and `ActiveModal::JournalAppendOrReplace` variants installed via the modal driver.
  - `ft/src/tui/widgets/picker.rs` — new `GhostAwarePickerSource` (or composed source) that surfaces ghost names alongside real-note hits from the current graph snapshot.
- **Tests**:
  - New `TestBackend` snapshot tests for the strip (empty state, single source, multi-source with window, truncated).
  - New snapshot tests for the Sources Manager and the Append/Replace prompt.
  - Integration test in `ft/tests/` for the Graph→Journal append flow (multi_select two notes, `Shift+A`, choose append, assert sources strip reflects the union).
- **No breaking changes** to keybindings outside the Journal tab itself, CLI surface, on-disk formats, or APIs.
- **One Journal-tab UX breakage**: `/` no longer opens a one-shot note picker that replaces the source set. Documented in the modified spec and reflected in the help overlay.

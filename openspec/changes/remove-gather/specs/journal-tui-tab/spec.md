# journal-tui-tab

## REMOVED Requirements

### Requirement: Gather tab registration
**Reason**: The Gather tab is deleted; the Search tab (in the default slot since the add-paragraph-search change) covers paragraph resurfacing.
**Migration**: Use the Search tab for mention-driven paragraph lookup; the `[tui] show_gather` config flag is removed.

### Requirement: Empty-state picker prompt
**Reason**: The Gather tab is deleted.
**Migration**: Use the Search tab's query input (`/`) to start a paragraph search.

### Requirement: Note selection via fuzzy picker
**Reason**: The Gather tab is deleted; the note picker surface moved to Search/Notes.
**Migration**: Use the Search tab (query `[[Note]]` for a link-target search) or the Notes tab picker.

### Requirement: BlameCache reuse across loads
**Reason**: The Gather tab is deleted; the blame cache remains a `recent`/`synth --sort date` concern.
**Migration**: No user action — blame caching for `ft notes recent` and `ft synth scaffold --sort date` is unchanged.

### Requirement: Entries persist across tab switches
**Reason**: The Gather tab is deleted.
**Migration**: Use the Search tab; results re-derive from the shared snapshot on focus.

### Requirement: Reload (`R`) re-runs build_journal
**Reason**: The Gather tab is deleted.
**Migration**: Use the Search tab's `R` to re-run the current query.

### Requirement: Clear (`c`) returns to picker prompt
**Reason**: The Gather tab is deleted.
**Migration**: Use the Search tab's `c` to clear the query and results.

### Requirement: Entry navigation
**Reason**: The Gather tab is deleted.
**Migration**: Search and Recent tabs keep the same cursor chords (`j`/`k`, `Ctrl+D`/`Ctrl+U`, `g`/`G`).

### Requirement: Enter opens source in editor
**Reason**: The Gather tab is deleted.
**Migration**: `Enter` on the Search and Recent tabs opens the source note at the paragraph line, unchanged.

### Requirement: Help overlay lists Journal keybindings
**Reason**: The Gather tab is deleted; its `?` overlay section is gone with it.
**Migration**: The `?` overlay on Search/Recent lists their keymaps.

### Requirement: queue_journal_for hook on Tab trait
**Reason**: The Gather tab and its `queue_gather_for` hook are deleted; cross-tab jumps now route to the Search tab.
**Migration**: Cross-tab mention jumps use `AppRequest::SearchWithQuery` (see graph-to-search-jump).

### Requirement: Multi-target mode queueing
**Reason**: The Gather tab is deleted; multi-target handoffs now lower to any-mode search clauses.
**Migration**: Pulse and Graph handoffs open the Search tab prefilled with `[[…]]` clauses in any-mode.

### Requirement: Matched-targets badge rendering
**Reason**: The Gather tab is deleted.
**Migration**: Search result rows label matched clauses; no multi-target badge surface remains.

### Requirement: In-window-only toggle
**Reason**: The Gather tab is deleted; search has no window concept (`--sort date` covers recency).
**Migration**: Use `ft notes search --sort date` or `ft notes recent --since <duration>` for windowed views.

### Requirement: Entry multi-select
**Reason**: The Gather tab is deleted.
**Migration**: `Space` multi-select is unchanged on the Search and Recent tabs.

### Requirement: Send-to-synth action
**Reason**: The Gather tab is deleted; the shared send-to-synth flow lives on the Search and Recent tabs.
**Migration**: `s` (append to existing) and `S` (create new) on the Search and Recent tabs; the `n` new-only watermark flow is removed with the tab.

### Requirement: Help overlay covers new bindings
**Reason**: The Gather tab is deleted.
**Migration**: Search/Recent `?` overlays list `s`/`S`/`Space`.

### Requirement: Citation badge on journal rows
**Reason**: The Gather tab is deleted; citation badges remain on the Recent tab and `ft notes recent`.
**Migration**: Citation state is visible via `ft notes recent` badges and `--uncited`.

### Requirement: journal.toggle-uncited command
**Reason**: The gather command/keymap statics are deleted.
**Migration**: `ft notes recent --uncited` retains the uncited filter; the Recent tab keeps its own uncited toggle.

### Requirement: Note-context badges
**Reason**: The Gather tab is deleted; the `o` context-note flow and `parse_synth_targets` are removed with it.
**Migration**: Work a topic toward a synth note via Search → `s`/`S`; dedup-on-append is unchanged.

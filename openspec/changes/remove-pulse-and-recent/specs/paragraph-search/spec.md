## MODIFIED Requirements

### Requirement: Search TUI tab

The TUI SHALL register a `Search` tab in the default tab lineup with a `<TAB>_COMMANDS`/`<TAB>_KEYMAP` pair, a keymap overlay, a `help_sections()` entry, and a `TestBackend` snapshot. The tab SHALL show an inline input line and a live results list. The results SHALL re-query synchronously on every input change against the snapshot's search index (an `Arc<SearchIndex>` rebuilt on graph generation change; the tab SHALL re-derive on `on_graph_ready` / `on_focus`, never by scanning the vault itself). A status bar SHALL render the live parse: term count, AND/ANY, and sort mode. Keys: `a` SHALL toggle all/any; `o` SHALL cycle sort relevance ↔ date; `Space` SHALL toggle multi-select on the focused row; `Enter` SHALL open the source note in `$EDITOR` at the paragraph's line; `s` SHALL append selected (or all) results to an existing synth note; `S` SHALL create a new synth note from them; `R` SHALL re-run the query. Send-to-synth SHALL reuse the shared synth-send machinery.

#### Scenario: Live results update per keystroke

- **WHEN** the user types `eig` and then `eigen` in the input line
- **THEN** the results list updates after each keystroke without any explicit reload

#### Scenario: All/any toggle

- **WHEN** the user presses `a` on a two-term query
- **THEN** the status bar flips AND ↔ ANY and the result set updates accordingly

#### Scenario: Sort toggle

- **WHEN** the user presses `o`
- **THEN** the status bar shows the new sort mode and the result order updates

## REMOVED Requirements

### Requirement: Pulse handoff to Search
**Reason**: The Pulse tab is deleted, so it can no longer hand off rows. The
`AppRequest::SearchWithQuery` mechanism it used survives for the Graph tab's
`J` / `Ctrl+J` handoffs and is specified by `graph-to-search-jump`.
**Migration**: Type the query on the Search tab directly, or use `J` /
`Ctrl+J` from the Graph tab to seed it from a note's links.

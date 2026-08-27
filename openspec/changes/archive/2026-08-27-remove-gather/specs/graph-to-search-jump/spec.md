# graph-to-search-jump Specification

## Purpose
Graph-tab cross-tab handoffs that route mention lookups to the Search tab,
replacing the old jump to the removed Gather tab. `J` searches the selected
note's mentions; `Ctrl+J` searches the mentions of a multi-selection (or the
cursor row) in any-mode — preserving the old multi-target OR semantics.

## ADDED Requirements

### Requirement: Graph tab `J` keybinding opens the Search tab
When the graph tab is focused and the currently-selected row is a NodeKind::Note or NodeKind::Ghost node, pressing `J` SHALL raise AppRequest::SearchWithQuery with the node's display title lowered to a single `[[…]]` clause (e.g. `[[Foo]]`) in AND mode (any = false), switching the App's active tab to the Search tab and running the query. The command SHALL be named `graph.search-mentions`.

#### Scenario: Jump from a Note row
- **WHEN** the user has the graph tab focused with a Note row selected and presses `J`
- **THEN** focus switches to the Search tab, the query box shows `[[<title>]]`, and the results list the paragraphs mentioning that note

#### Scenario: Jump from a Ghost row
- **WHEN** the user presses `J` on a Ghost row (unresolved link target)
- **THEN** focus switches to the Search tab with `[[<raw target>]]` as the query, covering paragraphs that mention the ghost

#### Scenario: Jump from a non-Note row produces a toast
- **WHEN** the user presses `J` with a Directory, Task, or Paragraph row selected
- **THEN** focus does NOT switch and a "select a Note or Ghost row to open its mentions" toast is queued

#### Scenario: Jump with empty selection produces a toast
- **WHEN** the user presses `J` with no row selected (empty tree)
- **THEN** focus does NOT switch and an informational toast is queued

### Requirement: Graph tab `Ctrl+J` multi handoff to Search
When the graph tab is focused, `Ctrl+J` SHALL raise AppRequest::SearchWithQuery with the selected rows (or the cursor row when nothing is multi-selected) lowered to `[[…]]` clauses in any-mode (any = true), switching to the Search tab. Only Note and Ghost rows contribute clauses. The command SHALL be named `graph.search-mentions-multi`.

#### Scenario: Multi-selection handed off
- **WHEN** the user multi-selects three Note rows and presses `Ctrl+J`
- **THEN** focus switches to the Search tab with `[[A]] [[B]] [[C]]` in any-mode, listing paragraphs mentioning any of the three

#### Scenario: Cursor-row fallback
- **WHEN** no rows are multi-selected and the user presses `Ctrl+J` on a Note row
- **THEN** the Search tab opens with that one note as a single any-mode clause

#### Scenario: No selectable rows produces a toast
- **WHEN** the selection (or cursor row) contains no Note or Ghost rows
- **THEN** focus does NOT switch and a "no Note or Ghost rows selected" toast is queued

### Requirement: Help overlay on graph tab lists the handoff keys
The graph tab's `help_sections()` SHALL list `J` ("open Search for the selected note") and `Ctrl+J` ("search selected (or cursor) rows' mentions") in its cross-tab section so the bindings are discoverable via the `?` overlay.

#### Scenario: Overlay shows the search handoffs
- **WHEN** the user opens the `?` overlay on the graph tab
- **THEN** it contains rows mentioning `J` and `Ctrl+J` with the Search handoff wording

## ADDED Requirements

### Requirement: Graph tab `N` searches near the selected note

Pressing `N` on the graph tab with a `NodeKind::Note` row selected SHALL raise
`AppRequest::SearchWithQuery` with the query `near:[[<title>]] ` (trailing space)
in AND mode, switch to the Search tab, and leave the query
editor active with the cursor at the end, so the next keystrokes type the search
terms. The command SHALL be named `graph.search-near`. On a Ghost, Directory,
Task, Paragraph, or Heading row, or with no selection, focus SHALL NOT switch and
a "select a note to search near it" toast SHALL be queued. The graph tab's
`help_sections()` SHALL list `N`.

#### Scenario: Search near a note

- **WHEN** the user selects the Note row `Caching` and presses `N`, then types `invalidation`
- **THEN** the Search tab shows the query `near:[[Caching]] invalidation` and lists paragraphs matching `invalidation`, those in or near `Caching.md` boosted

#### Scenario: Non-note row produces a toast

- **WHEN** the user presses `N` on a Ghost or Directory row
- **THEN** focus does NOT switch and the "select a note to search near it" toast is queued

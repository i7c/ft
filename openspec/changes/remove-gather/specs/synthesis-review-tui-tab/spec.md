# synthesis-review-tui-tab

## MODIFIED Requirements

### Requirement: Handoff to Search tab on enter

Pressing `<enter>` on the Pulse tab SHALL lower the currently selected link
targets to `[[…]]` search clauses and raise `AppRequest::SearchWithQuery` with
`any: true`, switching focus to the Search tab and running the query. If no
rows are selected when `<enter>` is pressed, the link under the cursor SHALL be
used as the sole clause. The command SHALL be named `pulse.handoff-to-search`
(previously `pulse.handoff-to-gather`; behavior already routed to Search). No
window range is passed — the Search tab has no in-window toggle.

#### Scenario: Selected rows handed off

- **WHEN** the user selects three links and presses `<enter>`
- **THEN** focus switches to the Search tab prefilled with `[[A]] [[B]] [[C]]` in any-mode and the results list paragraphs mentioning any of the three

#### Scenario: No selection falls back to cursor

- **WHEN** no rows are selected and the user presses `<enter>` on a row
- **THEN** the Search tab opens with that one link as a single any-mode clause

#### Scenario: Ghost targets become link clauses

- **WHEN** a selected row is a ghost target
- **THEN** it is lowered to a `[[<raw target>]]` clause like any note, and the search covers paragraphs mentioning the ghost

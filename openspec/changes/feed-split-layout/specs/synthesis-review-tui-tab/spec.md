# synthesis-review-tui-tab — delta

## MODIFIED Requirements

### Requirement: Link list rendering
The body SHALL list one link per row in the format `(<count>) [[<target>]]`, with `?` suffix on ghosts, sorted by count descending and alphabetically ascending on ties. Selected entries SHALL be visually distinguished (e.g., prefix marker or color). The list viewport SHALL auto-scroll to keep the cursor row visible when the cursor moves below the fold, and SHALL render a right-edge scrollbar when the rows overflow the viewport (reusing the codebase's shared `render_scroll_list` widget so its look matches every other scrollable list in the TUI).

#### Scenario: Display format
- **WHEN** the review yields `[[Foo]]=3 (ghost)` and `[[Bar]]=2`
- **THEN** the list shows `(3) [[Foo]]?` on the first row and `(2) [[Bar]]` on the second

#### Scenario: Selection visual
- **WHEN** the user selects `(3) [[Foo]]?`
- **THEN** the row is visually marked as selected

#### Scenario: Cursor stays visible past the fold
- **WHEN** the review yields more rows than the viewport fits and the user moves the cursor below the first screen
- **THEN** the viewport scrolls so the cursor row remains visible

#### Scenario: Scrollbar on overflow
- **WHEN** the rows overflow the viewport
- **THEN** a scrollbar is rendered on the right edge of the list

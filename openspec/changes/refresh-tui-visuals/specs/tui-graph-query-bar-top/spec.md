## ADDED Requirements

### Requirement: Graph query bar renders at the top of the body area
The graph tab's query input bar SHALL render at the top of the body area, above the view-tab strip and the tree viewport. The layout order SHALL be `[query_input(1), view_strip(1), tree(Min(1))]`.

#### Scenario: Query bar is the first element in the body layout
- **WHEN** `GraphTab::render` splits the body area
- **THEN** the first vertical constraint is the query input bar (height 1), followed by the view-tab strip (height 1), followed by the tree (minimum 1)

#### Scenario: Query bar shows current query text when not editing
- **WHEN** the graph tab renders and the query bar is NOT in editing mode
- **THEN** the query bar at the top shows `> <query text>` in the palette's dim color

#### Scenario: Query bar enters editing mode at top
- **WHEN** the user presses `/` in the graph tab
- **THEN** the query bar at the top enters editing mode with the cursor positioned after `> `, using the palette's primary accent color

#### Scenario: Cursor position is correct with query bar at top
- **WHEN** the query bar is in editing mode at the top of the body
- **THEN** the cursor position is set to the correct column within the top-row input area, matching the prompt width + input cursor offset

### Requirement: View-tab strip renders below the query bar
The view-tab strip SHALL render immediately below the query input bar, displaying view labels (`1: <snippet> 2: <snippet> ...`).

#### Scenario: View-tab strip is between query bar and tree
- **WHEN** the graph tab renders with multiple views
- **THEN** the view-tab strip row appears after the query input bar row and before the tree viewport

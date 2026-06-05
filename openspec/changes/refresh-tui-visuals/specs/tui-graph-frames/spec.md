## ADDED Requirements

### Requirement: Graph tree viewport has a bordered frame
The graph tab's tree area SHALL render with a `Block::default().borders(Borders::ALL)` frame wrapping the tree list, matching the visual convention of timeblocks panes and the notes idle panel.

#### Scenario: Tree area renders with all borders
- **WHEN** `GraphTab::render` renders the tree area
- **THEN** the tree viewport has top, bottom, left, and right border lines

#### Scenario: Frame title shows the active view label
- **WHEN** the graph tab is rendering with one or more views
- **THEN** the tree frame's title displays the active view's query snippet (same text shown in the view-tab strip)

#### Scenario: Frame border color uses the primary accent
- **WHEN** the graph tab renders
- **THEN** the tree frame's border style foreground color is the palette's primary accent (orange)

#### Scenario: Frame is present in every render state
- **WHEN** the graph tab is rendering with any active view (including empty state with the "press / to edit query" hint)
- **THEN** the tree frame is present

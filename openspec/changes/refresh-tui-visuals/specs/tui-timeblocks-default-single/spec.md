## ADDED Requirements

### Requirement: Timeblocks tab initializes in Single-day view
The timeblocks tab SHALL initialize with `ViewMode::Single` showing only the focused day (today) in full-width. The `f` key SHALL toggle between `ViewMode::Single` and `ViewMode::Split`.

#### Scenario: New TimeblocksTab starts in Single view
- **WHEN** `TimeblocksTab::new()` or `TimeblocksTab::default()` is called
- **THEN** `tab.view` is `ViewMode::Single`

#### Scenario: `f` toggles from Single to Split
- **WHEN** the timeblocks tab is in `ViewMode::Single` and `f` is pressed
- **THEN** `tab.view` transitions to `ViewMode::Split` and both today and tomorrow panes are visible side-by-side

#### Scenario: `f` toggles from Split to Single
- **WHEN** the timeblocks tab is in `ViewMode::Split` and `f` is pressed
- **THEN** `tab.view` transitions to `ViewMode::Single` and only the focused day is shown full-width

#### Scenario: Sidebar reflects view mode in Single
- **WHEN** the timeblocks tab is in `ViewMode::Single`
- **THEN** the sidebar displays "view: single (f)" in the palette's dim color

#### Scenario: Sidebar reflects view mode in Split
- **WHEN** the timeblocks tab is in `ViewMode::Split`
- **THEN** the sidebar displays "view: split" in the palette's dim color

### Requirement: `h` and `l` swap focused day in Single view
In `ViewMode::Single`, the `h` and `l` keys SHALL swap which day is shown on screen by changing `tab.focus` between `Pane::Today` and `Pane::Tomorrow`.

#### Scenario: `l` advances from Today to Tomorrow in Single view
- **WHEN** the timeblocks tab is in `ViewMode::Single` with focus on `Pane::Today` and `l` is pressed
- **THEN** `tab.focus` becomes `Pane::Tomorrow` and tomorrow's blocks are shown full-width

#### Scenario: `h` returns from Tomorrow to Today in Single view
- **WHEN** the timeblocks tab is in `ViewMode::Single` with focus on `Pane::Tomorrow` and `h` is pressed
- **THEN** `tab.focus` becomes `Pane::Today` and today's blocks are shown full-width

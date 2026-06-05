## ADDED Requirements

### Requirement: Warm color palette is defined in a central module
The TUI SHALL use a warm orange/red/yellow color palette defined in a single module `ft/src/tui/palette.rs` with semantic color name constants. Every render site SHALL reference these constants instead of inlining raw `Color` values.

#### Scenario: Palette module exports semantic color constants
- **WHEN** another module imports `crate::tui::palette`
- **THEN** it has access to named constants for primary accent (orange), secondary accent (gold/yellow), tertiary accent (warm red), dim (warm gray), and background (warm dark)

#### Scenario: Palette is used in tab bar rendering
- **WHEN** `render_tab_bar` renders the selected tab highlight
- **THEN** the highlight style uses the palette's primary accent color instead of `Color::Cyan`

#### Scenario: Palette is used in status bar rendering
- **WHEN** `render_status_bar` renders the left, center, and right cells
- **THEN** vault/tab labels use the palette's primary accent, mode label uses palette's tertiary accent, and in-flight indicator uses palette's primary accent

#### Scenario: Palette is used in help overlay
- **WHEN** `render_help_overlay` renders section headers and key labels
- **THEN** section headers use the palette's primary accent and key labels use the palette's secondary accent

#### Scenario: Palette is used in graph tab rendering
- **WHEN** `GraphTab::render` renders the view-tab strip, tree items, and input bar
- **THEN** active view label uses palette's primary accent, selected row uses palette's primary accent background with palette text color, and query bar prompt uses palette's primary accent

#### Scenario: Palette is used in tasks tab rendering
- **WHEN** `SearchView::render` renders the query bar and task list
- **THEN** the query bar border uses palette's primary accent, selected task uses palette's primary accent background, and overdue tasks use palette's tertiary accent

#### Scenario: Palette is used in timeblocks tab rendering
- **WHEN** `render_sidebar`, `render_pane`, and `render_form_modal` render
- **THEN** sidebar border uses palette dim, focused pane border uses palette's primary accent, and clock display uses palette's primary accent

#### Scenario: Palette is used in notes tab rendering
- **WHEN** `render_idle_body` renders the notes panel
- **THEN** the border uses palette dim, the title uses palette's primary accent, and key labels use palette's secondary accent

### Requirement: Success and error colors remain distinct
Success toast messages SHALL render in green (`Color::Green`) and error messages SHALL render in red (`Color::Red`), distinct from the warm palette to preserve clear semantic signaling.

#### Scenario: Success toast renders in green
- **WHEN** a success toast is displayed in the status bar
- **THEN** the toast text is rendered with `Color::Green` and bold modifier

#### Scenario: Error toast renders in red
- **WHEN** an error toast or parse error text is rendered
- **THEN** the text is rendered with `Color::Red`

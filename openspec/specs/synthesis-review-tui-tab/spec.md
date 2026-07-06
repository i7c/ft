# synthesis-review-tui-tab Specification

## Purpose
TBD - created by archiving change add-synthesis-flow. Update Purpose after archive.
## Requirements
### Requirement: Pulse tab registration
The TUI SHALL register a new top-level tab titled `Review`, slotted after the existing `Journal` tab in `App::new`. The tab SHALL implement the `Tab` trait alongside the existing tabs.

#### Scenario: Tab appears in the tab strip
- **WHEN** the TUI starts
- **THEN** the tab strip lists `Graph`, `Tasks`, `Notes`, `Timeblocks`, `Journal`, `Review` (in that order)

#### Scenario: Tab can receive focus
- **WHEN** the user presses the digit key for the Pulse tab's position
- **THEN** focus switches to the Pulse tab and `on_focus` runs

### Requirement: Default window on first focus
On first focus, the Pulse tab SHALL compute and display the link-review over a default window equivalent to `--since 7d`. The window range SHALL be shown in the tab header so the user always knows what period is being reviewed.

#### Scenario: Default window shown
- **WHEN** the Pulse tab is focused for the first time
- **THEN** the tab header shows the date range covered (e.g. `2026-06-01 .. 2026-06-08`) and the body lists links from that range

### Requirement: Link list rendering
The body SHALL list one link per row in the format `(<count>) [[<target>]]`, with `?` suffix on ghosts, sorted by count descending and alphabetically ascending on ties. Selected entries SHALL be visually distinguished (e.g., prefix marker or color).

#### Scenario: Display format
- **WHEN** the review yields `[[Foo]]=3 (ghost)` and `[[Bar]]=2`
- **THEN** the list shows `(3) [[Foo]]?` on the first row and `(2) [[Bar]]` on the second

#### Scenario: Selection visual
- **WHEN** the user selects `(3) [[Foo]]?`
- **THEN** the row is visually marked as selected

### Requirement: Multi-select via space
Pressing `<space>` on the focused row SHALL toggle that row's selection. The selection SHALL persist across cursor movement within the tab.

#### Scenario: Toggle selection
- **WHEN** the user navigates to a row and presses `<space>`
- **THEN** the row's selection state flips

#### Scenario: Selection persists on cursor move
- **WHEN** the user selects three rows and then moves the cursor
- **THEN** all three rows remain selected

### Requirement: Window adjustment
The Pulse tab SHALL allow adjusting the window from within the tab. At least one keybinding SHALL be provided for "narrower window" and "wider window" (exact bindings TBD during implementation but documented in `Tab::help_sections`). When the window changes, the review SHALL re-run and the list and header SHALL update.

#### Scenario: Widen window
- **WHEN** the user presses the "wider window" key
- **THEN** the window expands (e.g., from 7d to 14d) and the list re-runs

#### Scenario: Narrow window
- **WHEN** the user presses the "narrower window" key
- **THEN** the window shrinks and the list re-runs

### Requirement: Handoff to Gather tab on enter
Pressing `<enter>` on the Pulse tab SHALL queue the currently selected link targets to the Gather tab and switch focus to it. If no rows are selected when `<enter>` is pressed, the link under the cursor SHALL be used as the sole target. The window range in effect at handoff SHALL also be passed so the Gather tab can offer its in-window-only toggle.

#### Scenario: Selected rows handed off
- **WHEN** the user selects three links and presses `<enter>`
- **THEN** focus switches to the Gather tab and it loads the multi-source journal for those three link targets

#### Scenario: No selection falls back to cursor
- **WHEN** no rows are selected and the user presses `<enter>` on a row
- **THEN** the Gather tab loads the multi-source journal for that one link's target

#### Scenario: Window range passed along
- **WHEN** the Pulse tab's window is the last 14 days at handoff
- **THEN** the Gather tab's in-window-only toggle uses that same window

### Requirement: Help overlay via Tab::help_sections
The Pulse tab SHALL override `Tab::help_sections()` so the `?` overlay shows its keymap: navigation, `<space>` to select, `<enter>` to hand off, window-adjustment keys, and any others.

#### Scenario: Help overlay shows Review keys
- **WHEN** the Pulse tab is focused and the user presses `?`
- **THEN** the overlay includes a section for the Pulse tab listing all its bindings

### Requirement: Background loading via mpsc
The Pulse tab's link-review computation SHALL run on a background worker thread following the existing single-threaded + mpsc producer pattern. The worker SHALL post a `BgEvent` back to the main loop on completion; in-flight state SHALL live in a typed `RefCell<Option<...>>` slot on `App`. There SHALL be no async runtime and no `Mutex<AppState>`.

#### Scenario: UI remains responsive during load
- **WHEN** the tab is focused and the link-review is computing
- **THEN** the rest of the UI remains responsive (other tabs render normally, keys are not blocked)

#### Scenario: Result posted on completion
- **WHEN** the worker finishes computing the review
- **THEN** the main loop receives the result via the mpsc channel and the body updates

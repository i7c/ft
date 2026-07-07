# tui-feed-split — delta

## ADDED Requirements

### Requirement: Split layout widget
The TUI SHALL provide a shared `feed_split` widget that renders a
paragraph feed as a two-pane split: a compact one-line-per-entry **list
pane** on top and a single-entry **preview pane** on the bottom. The
widget SHALL take, per entry, (a) a compact one-line list row, (b) a
preview-header builder, and (c) a preview-body builder, plus the
`selected` cursor index and the multi-select set. The list pane SHALL
auto-scroll the cursor into view (reusing the codebase's shared
`render_scroll_list` semantics) and render a right-edge scrollbar on
overflow.

#### Scenario: List follows the cursor
- **WHEN** the feed has more entries than the list pane fits and the
  user moves the cursor below the fold
- **THEN** the list pane scrolls so the cursor row stays visible

#### Scenario: Scrollbar on overflow
- **WHEN** the feed overflows the list pane's height
- **THEN** a scrollbar is rendered on the right edge of the list pane

### Requirement: Stable-height list pane
The list pane SHALL occupy a stable height (default 10 rows), clamped
down to the entry count when fewer entries exist and clamped up only by
the available tab area. The preview pane SHALL take the remaining
height of the tab body. The split proportions SHALL NOT change as the
cursor moves, so the preview pane's size stays stable while browsing.

#### Scenario: Few entries shrink the list
- **WHEN** the feed has 3 entries and the list default is 10
- **THEN** the list pane is 3 rows tall and the preview pane takes the
  remainder

#### Scenario: Cursor movement keeps pane heights stable
- **WHEN** the user moves the cursor from entry 1 to entry 50 in a long
  feed
- **THEN** the list pane and preview pane heights do not change between
  renders

### Requirement: Compact list row
Each list row SHALL show `{date} {source_title}` plus, when present, a
compact inline citation badge. The citation badge SHALL be one of
`cited` (with note stem), `cited*` (stale, with note stem), `in note`,
or `missing`, omitted entirely when the entry is uncited in global
mode. A multi-select marker (`●`) SHALL prefix selected rows. The row
SHALL NOT wrap; over-long rows SHALL be truncated to the list pane
width.

#### Scenario: Row shows date, title, and citation
- **WHEN** an entry dated 2026-07-05 in note `Daily` is cited by synth
  note `Syn`
- **THEN** the list row reads `2026-07-05 Daily cited: Syn` (or similar
  compact form) on a single line

#### Scenario: Uncited global entry has no badge
- **WHEN** an entry is not cited in any synth note and no context note
  is active
- **THEN** its list row shows only `{date} {title}` with no citation
  text

### Requirement: Preview pane header
The preview pane SHALL render a header, visually distinct from the
body (different colors and a separating rule line below it), showing
the selected entry's **title, date, line range, and citation detail**.
For cited entries the header SHALL name the citing note(s); for stale
(`cited*`) entries the header SHALL surface the staleness. For Gather
multi-source entries the header SHALL additionally include the
`matched:` badge (target titles, comma-separated). The header content
SHALL reflect the cursor row, which is also the row the preview body
shows.

#### Scenario: Header distinguishes from body
- **WHEN** the preview pane renders
- **THEN** a header line with a distinct color and a separating rule
  below it appears above the paragraph body

#### Scenario: Cited entry names the citing note
- **WHEN** the selected entry is cited exactly by synth note `Syn`
- **THEN** the preview header names `Syn` (and any other citing notes)

#### Scenario: Stale citation is surfaced
- **WHEN** the selected entry's citation is stale
- **THEN** the preview header shows the staleness distinctly from a
  fresh citation

### Requirement: Preview pane body is non-scrolling
The preview pane SHALL render the selected entry's wrapped paragraph
body below the header. The preview pane SHALL NOT scroll
independently; when the paragraph is longer than the preview pane's
remaining height, the body SHALL be visibly cut off (the user opens the
paragraph in `$EDITOR` via `Enter` to read it in full). Moving the
cursor SHALL re-center the preview on the newly selected entry.

#### Scenario: Long paragraph is cut off
- **WHEN** the selected entry's paragraph is longer than the preview
  pane's body height
- **THEN** the body renders up to the pane height and the remainder is
  not shown, with no scroll affordance in the preview pane

#### Scenario: Cursor move re-centers the preview
- **WHEN** the user moves the cursor to a different entry
- **THEN** the preview pane shows the newly selected entry's header and
  body from the top

### Requirement: Empty / loading / error states bypass the split
The tab SHALL render a full-pane message and SHALL NOT draw the
list/preview split when the feed is empty, still loading, or in an
error state. This keeps the empty and error states legible regardless
of split proportions.

#### Scenario: Empty feed shows full-pane message
- **WHEN** the feed is empty
- **THEN** a single message fills the tab body and no list or preview
  pane is drawn

#### Scenario: Error shows full-pane banner
- **WHEN** a load error is present
- **THEN** the error banner fills the tab body and no split is drawn

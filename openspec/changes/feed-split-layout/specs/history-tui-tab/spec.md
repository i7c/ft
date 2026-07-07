# history-tui-tab — delta

## MODIFIED Requirements

### Requirement: Windowed feed rendering
The Recent tab SHALL render the `build_recent` feed for a window that defaults to
`7d`, showing each paragraph entry's date and source title, ordered
reverse-chronologically, in a two-pane split (compact one-line-per-entry
list on top, single-entry paragraph preview on the bottom) as defined by
the `tui-feed-split` capability. It SHALL reuse a `BlameCache` for the session and hold the
current window so the feed can be recomputed on reload.

#### Scenario: Feed renders on focus
- **WHEN** the Recent tab is focused in a git-backed vault
- **THEN** it displays the last-7-days paragraph feed as a list/preview split

#### Scenario: Empty window state
- **WHEN** no paragraphs were edited within the window
- **THEN** the tab shows an empty-state message rather than a blank body

#### Scenario: Reload recomputes the feed
- **WHEN** the user triggers the tab's reload action after committing an edit
- **THEN** `build_recent` is re-run and newly edited paragraphs appear

### Requirement: Citation badge on history rows
Recent tab list rows SHALL render a compact inline citation badge
(`cited` / `cited*` / nothing in global mode), read from the shared
snapshot's citation index via `TabCtx::snapshot`. The full citation
detail (which note(s) cite the entry; staleness for `cited*`) SHALL be
shown in the preview pane header for the selected entry, not on every
row.

#### Scenario: Badge visible in the list
- **WHEN** the Recent tab shows a window containing a paragraph
  pinned in a synth note
- **THEN** that list row carries the compact `cited` marker

#### Scenario: Citation detail in the preview header
- **WHEN** the cursor is on a `cited*` (stale) entry cited by synth note `Syn`
- **THEN** the preview pane header names `Syn` and surfaces the staleness

# journal-tui-tab — delta

## MODIFIED Requirements

### Requirement: Matched-targets badge rendering
When rendering a multi-target journal (loaded via `queued_targets`), entries whose `matched` field contains more than one target SHALL display a `matched: X, Y` badge. The badge SHALL list target display titles (not raw `[[wikilink]]` syntax), comma-separated, in selection order. Entries with `matched.len() == 1` SHALL NOT display the badge. In the split layout, the `matched:` badge SHALL be shown in the preview pane header for the selected entry rather than as a per-row sub-line.

#### Scenario: Badge shown for multi-match entry
- **WHEN** the cursor is on an entry whose paragraph links to both `Foo` and `Bar` (both selected)
- **THEN** the preview pane header shows `matched: Foo, Bar`

#### Scenario: No badge for single-match entry
- **WHEN** the cursor is on an entry whose paragraph links to only `Foo` (with `Foo` and `Bar` selected)
- **THEN** no `matched:` badge appears in the preview header

### Requirement: Citation badge on journal rows
Gather tab list rows SHALL render a compact inline citation badge
from the shared snapshot's citation index: a `cited` marker (with note
stem) for exact citations, a visually distinct `cited*` marker for
stale ones, nothing for uncited. The full citation detail (which
note(s) cite the entry; staleness for `cited*`) SHALL be shown in the
preview pane header for the selected entry. Badge data SHALL come from
`TabCtx::snapshot` — the tab performs no scanning of its own.

#### Scenario: Rows re-badge on snapshot refresh
- **WHEN** the user appends an entry to a synth note and the graph
  refresh completes
- **THEN** that entry's list row shows the compact `cited` marker
  without reloading the tab

### Requirement: Note-context badges
List rows SHALL re-scope to the in-play target synth note (the
append-to-existing (`s`) picker flow, or a journal opened from a
synth note's `ft-synth-targets`):
`in note` when the entry is already pinned there (per the
filter_missing rule), `missing` otherwise, with a status-line
indicator naming the target note. The preview pane header SHALL show
the same note-local badge for the selected entry. Leaving the flow SHALL restore
global badges.

#### Scenario: Working a journal toward a note
- **WHEN** the user enters the `s` flow and picks synth note N
- **THEN** every list row shows `in note` or `missing` relative to N,
  matching exactly what plan-time dedup would drop or keep

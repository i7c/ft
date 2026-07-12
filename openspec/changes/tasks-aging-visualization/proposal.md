## Why

The Tasks tab surfaces urgency (due date) but not staleness. A task that's been
open for 30 days reads the same as one created yesterday, so languishing work is
invisible at a glance. Meanwhile every task created *through ft* gets no `created`
date at all (`created: None` at all four create sites), so even once aging ships,
ft-created tasks would never age — the feature would silently cover only
imported/Obsidian-native tasks. Fixing `created` is a prerequisite, not a side
task: aging visualization is only meaningful if newly created tasks actually
carry the date it derives from.

## What Changes

- Stamp `created: Some(today)` on every task created through ft — the two TUI
  `search.rs` create paths (quickline + popup), the graph-tab create path
  (`ft/src/tui/tabs/graph/tasks.rs`), and the CLI `ft tasks add` path
  (`ft/src/cmd/tasks.rs:554`). Four call sites today pass `created: None`.
- Add an **age badge** column to the Tasks SearchView row. The badge renders the
  task's age (days since `created`) as text and shades its own background grey,
  with the shade determined by fixed absolute age bands. Older tasks get darker
  grey. Tasks with no `created` date render no badge (neutral / blank cell).
- Bands (absolute, calibrated to `today` via the existing `dates::today()` /
  `FT_TODAY` seam):
  - `0–3 days` → lightest grey
  - `4–10 days` → medium grey
  - `11–30 days` → dark grey
  - `>30 days` → darkest grey
  - `None` (no created date) → no background, blank cell
- The age badge is a self-contained fixed-width span carrying its own `.bg()`.
  It does **not** tint the whole row, so it doesn't conflict with the existing
  warm-brown selected-row background or the `DIM` modifier on done/cancelled
  rows. On a selected row the badge keeps its grey shade (age remains visible
  while inspecting).

## Capabilities

### New Capabilities
- `task-aging`: age-band computation (created date → age band → grey shade) and
  the Tasks-view rendering of an age badge column.

### Modified Capabilities
- `tui-commands`: no requirement changes — aging is render-only and introduces
  no new command or keymap. (Listed here only to record that it was considered
  and intentionally excluded; no spec delta file is produced.)

## Impact

- **`ft-core`**: new module for age-band computation (pure function:
  `created: Option<NaiveDate>, today: NaiveDate -> AgeBand`). Lives in core so
  the CLI can reuse it later and so it's unit-testable in isolation. No changes
  to `Task` model, parser, or ops layer.
- **`ft` TUI**: `task_line` in `ft/src/tui/tabs/tasks/search.rs` gains an age
  span; fixed column budget grows from 34 to ~39 chars. `palette.rs` gains 4
  grey-shade constants. Two TUI create sites set `created: Some(today)`.
- **`ft` CLI**: `ft tasks add` sets `created: Some(today)` (`ft/src/cmd/tasks.rs`).
- **TUI graph tab**: create path sets `created: Some(today)`
  (`ft/src/tui/tabs/graph/tasks.rs`).
- **Tests**: `insta` snapshots for the Tasks view will change (new column) and
  must be regenerated. New unit tests for the band function; new snapshot(s)
  covering each band + the no-date case. Existing create tests that assert the
  written line will need to expect a `➕ <today>` segment.
- **No schema/format changes**: the emoji format already serializes `created`
  via ➕; no parser or round-trip changes.

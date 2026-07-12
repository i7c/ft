# task-aging Specification

## Purpose
Task staleness visualization for the Tasks TUI: stamping `created` on
every create path, absolute age-band classification (Fresh / Aging /
Stale / Rotten / Unknown), and a span-scoped grey age badge column in the
Tasks SearchView. Created by archiving change tasks-aging-visualization.

## Requirements
### Requirement: Every newly created task SHALL be stamped with a `created` date

Every task created through ft — via the TUI quickline, TUI edit popup, TUI graph-tab create, or the `ft tasks add` CLI — SHALL have its `created` field set to the current date (`ft_core::dates::today()`). Call sites SHALL NOT pass `created: None`.

#### Scenario: TUI quickline create stamps created
- **WHEN** the user submits a new task via the quickline (`c`) in the Tasks SearchView
- **THEN** the written task line contains a `➕ <today>` segment where `<today>` is the value of `ft_core::dates::today()` (overridable via `FT_TODAY`)

#### Scenario: TUI edit-popup create stamps created
- **WHEN** the user submits the new-blank-form edit popup (`C`) in the Tasks SearchView
- **THEN** the written task line contains a `➕ <today>` segment

#### Scenario: TUI graph-tab create stamps created
- **WHEN** the user creates a task from the graph tab (`ft/src/tui/tabs/graph/tasks.rs`)
- **THEN** the written task line contains a `➕ <today>` segment

#### Scenario: CLI tasks add stamps created
- **WHEN** the user runs `ft tasks add "do a thing"`
- **THEN** the written task line contains a `➕ <today>` segment, regardless of whether `--start` / `--due` / `--scheduled` were supplied

#### Scenario: Created date is the only date defaulted
- **WHEN** a task is created with no explicit `--start` / `--due` / `--scheduled`
- **THEN** only `created` is set; the other date fields remain `None`

### Requirement: Age SHALL be classified into absolute bands

The system SHALL classify a task's age (days from `created` to `today`, inclusive of `today`) into one of four fixed absolute bands using the thresholds below. The classification SHALL be a pure function of `(created: Option<NaiveDate>, today: NaiveDate)` and SHALL NOT depend on the ages of other tasks in any view.

- `Fresh`: 0–3 days
- `Aging`: 4–10 days
- `Stale`: 11–30 days
- `Rotten`: more than 30 days
- `Unknown`: `created` is `None`

#### Scenario: Fresh band
- **WHEN** `created` is 2 days before `today`
- **THEN** the age band is `Fresh`

#### Scenario: Aging band
- **WHEN** `created` is 7 days before `today`
- **THEN** the age band is `Aging`

#### Scenario: Stale band
- **WHEN** `created` is 20 days before `today`
- **THEN** the age band is `Stale`

#### Scenario: Rotten band
- **WHEN** `created` is 45 days before `today`
- **THEN** the age band is `Rotten`

#### Scenario: Boundary — 3 days is Fresh, 4 days is Aging
- **WHEN** `created` is exactly 3 days before `today`
- **THEN** the band is `Fresh`
- **WHEN** `created` is exactly 4 days before `today`
- **THEN** the band is `Aging`

#### Scenario: Boundary — 30 days is Stale, 31 is Rotten
- **WHEN** `created` is exactly 30 days before `today`
- **THEN** the band is `Stale`
- **WHEN** `created` is exactly 31 days before `today`
- **THEN** the band is `Rotten`

#### Scenario: Created today is Fresh
- **WHEN** `created` equals `today`
- **THEN** the band is `Fresh`

#### Scenario: Unknown band when no created date
- **WHEN** `created` is `None`
- **THEN** the age band is `Unknown`

#### Scenario: Classification is independent of cohort
- **WHEN** the same task with `created` 20 days before `today` is rendered in two different views (or two different filtered cohorts)
- **THEN** both classify it as `Stale`

### Requirement: The Tasks SearchView SHALL render an age badge column

Each task row in the Tasks SearchView SHALL render a fixed-width age badge span. The badge's text SHALL be the age in days (e.g. `4d`, `30d`, `45d`), and the badge's background color SHALL be a grey shade determined by the task's age band. The badge SHALL be a self-contained span that carries its own background and SHALL NOT tint the rest of the row.

#### Scenario: Badge text shows days
- **WHEN** a task with `created` 4 days before `today` is rendered
- **THEN** the age badge cell contains the text `4d`

#### Scenario: Badge background maps to band
- **WHEN** a task in the `Fresh` band is rendered
- **THEN** the age badge span's background is the lightest grey shade
- **WHEN** a task in the `Rotten` band is rendered
- **THEN** the age badge span's background is the darkest grey shade

#### Scenario: No created date renders a blank badge
- **WHEN** a task with `created = None` is rendered
- **THEN** the age badge cell renders with no background and blank/empty content, occupying the same fixed width as a populated badge so column alignment is preserved

#### Scenario: Badge keeps its shade on the selected row
- **WHEN** a task row is the cursor-selected row (warm-brown row background)
- **THEN** the age badge span retains its grey background shade; the brown selection background applies to the other spans but not the age badge

#### Scenario: Done/cancelled rows still show the age badge
- **WHEN** a task with `Status::Done` or `Status::Cancelled` and a known `created` date is rendered
- **THEN** the age badge still renders with its grey shade (the row-level `DIM` modifier does not suppress the badge background)

### Requirement: Grey shades SHALL be named palette constants

The four grey background shades (one per non-`Unknown` band) SHALL be defined as named constants in `ft/src/tui/palette.rs` rather than inlined at render sites, so future theme work remains a one-file change.

#### Scenario: Render sites reference palette constants
- **WHEN** the age badge span background is set in `task_line`
- **THEN** it references a `palette::` constant (e.g. `palette::AGE_FRESH`), not an inline `Color::Rgb(...)`

# periodic-notes Specification

## Purpose

Periodic notes — daily, weekly, monthly, quarterly, and yearly — are
configured per period under `[periodic_notes.<period>]` with `path` and
`format` strftime patterns plus an optional template. This capability defines
how a target date becomes a note's on-disk path and template context: it is
always anchored to the **start of the period** (the configured week start for
weekly, the 1st for monthly, the first day of the quarter, January 1 for
yearly), so exactly one note exists per period and opening it on any day of
that period resolves the same file. The week start is configurable
(`week_start`, default Monday). The same anchored resolution backs
`ft notes periodic`/`today`, the Notes- and Graph-tab keybindings, and the
`@weekly`/`@monthly`/`@quarterly`/`@yearly` query sigils.

## Requirements
### Requirement: Periodic note resolution anchors to the period start

The system SHALL compute the on-disk path and template-context date of a
periodic note from the **start of the period** containing the target date, not
from the target date itself. The representative date SHALL be:

- Daily → the target date.
- Weekly → the configured week start (Monday by default) of the target date's
  week.
- Monthly → the first day of the target date's month.
- Quarterly → the first day of the target date's calendar quarter.
- Yearly → January 1 of the target date's year.

This anchoring SHALL apply to `ft notes periodic`, `ft notes today`, the
Notes-tab and Graph-tab periodic keybindings, and the
`@weekly`/`@monthly`/`@quarterly`/`@yearly` query sigils.

#### Scenario: Weekly note opened mid-week resolves to the week's Monday
- **WHEN** `FT_TODAY=2026-05-13` (a Wednesday) and `[periodic_notes.weekly]` has `format = "%Y-%m-%d"`
- **THEN** `ft notes periodic weekly --no-open` creates or opens `journal/<year>/2026-05-11.md` (the Monday), not `2026-05-13.md`

#### Scenario: Reopening on a later day opens the same weekly note
- **WHEN** the weekly note for the week of `2026-05-11` already exists and `FT_TODAY=2026-05-17` (the Sunday)
- **THEN** `ft notes periodic weekly --no-open` reports `Opened` for the `2026-05-11` file and does not create a second file

#### Scenario: Monthly note resolves to the first of the month
- **WHEN** `FT_TODAY=2026-05-14` and `[periodic_notes.monthly]` has `format = "%Y-%m-%d"`
- **THEN** the resolved path is `journal/<year>/2026-05-01.md`

#### Scenario: Quarterly note resolves to the first day of the quarter
- **WHEN** `FT_TODAY=2026-05-14` and `[periodic_notes.quarterly]` has `format = "%Y-%m-%d"`
- **THEN** the resolved path is `journal/<year>/2026-04-01.md`

#### Scenario: Yearly note resolves to January 1
- **WHEN** `FT_TODAY=2026-05-14` and `[periodic_notes.yearly]` has `format = "%Y-%m-%d"`
- **THEN** the resolved path is `journal/<year>/2026-01-01.md`

#### Scenario: Daily note is unchanged
- **WHEN** `FT_TODAY=2026-05-14` and `[periodic_notes.daily]` has `format = "%Y-%m-%d"`
- **THEN** the resolved path is `journal/<year>/2026-05-14.md`

#### Scenario: Week-number weekly format is unchanged
- **WHEN** `FT_TODAY=2026-05-14` and `[periodic_notes.weekly]` has `format = "%G-W%V"`
- **THEN** the resolved path is `journal/<year>/2026-W20.md`, the same as before anchoring

### Requirement: Week start is configurable

The weekly period SHALL anchor to Monday by default. A `week_start` key on a
`[periodic_notes.<period>]` block with the value `"monday"` or `"sunday"` SHALL
select the first day of the week for the weekly period. An unknown value SHALL
be a configuration error. `week_start` on a non-weekly period SHALL have no
effect.

#### Scenario: Default week start is Monday
- **WHEN** `[periodic_notes.weekly]` omits `week_start` and `FT_TODAY=2026-05-13`
- **THEN** the resolved date is Monday `2026-05-11`

#### Scenario: Sunday week start
- **WHEN** `[periodic_notes.weekly]` has `week_start = "sunday"` and `FT_TODAY=2026-05-13`
- **THEN** the resolved date is Sunday `2026-05-10`

#### Scenario: Invalid week start is rejected
- **WHEN** `[periodic_notes.weekly]` has `week_start = "tuesday"`
- **THEN** config loading fails with an error naming the invalid value

### Requirement: Offset applies before anchoring

A target date SHALL be shifted by an offset (`--offset` or a sigil's signed
offset) first and anchored to the period start second, so an offset always lands
on the start of the intended period.

#### Scenario: Weekly offset minus one lands on last week's Monday
- **WHEN** `FT_TODAY=2026-05-13` and `ft notes periodic weekly --offset -1` runs
- **THEN** the resolved date is Monday `2026-05-04`

#### Scenario: Monthly offset clamps then anchors
- **WHEN** `--date 2026-01-31` and `--offset 1` run for a monthly note
- **THEN** the resolved date is `2026-02-01`

### Requirement: Template context uses the anchored date

When a periodic note is created from a template, the system SHALL render the
template's `today` variable as the note's anchored period-start date, so
periodic templates render deterministically regardless of the day they are
opened.

#### Scenario: Weekly template renders the week's Monday
- **WHEN** a weekly note for the week of `2026-05-11` is created from a template containing `{{ today | date(format="%Y-%m-%d") }}` on `FT_TODAY=2026-05-13`
- **THEN** the rendered body contains `2026-05-11`

#### Scenario: Offset note renders its own period
- **WHEN** `ft notes periodic weekly --offset -1` creates a note from that template on `FT_TODAY=2026-05-13`
- **THEN** the template's `today` is `2026-05-04` (last week's Monday), not `2026-05-13`


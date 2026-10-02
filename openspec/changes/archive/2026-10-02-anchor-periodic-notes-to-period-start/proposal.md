## Why

Periodic notes are resolved against the **raw target date**: `resolve_periodic_path`
(`ft-core/src/periodic.rs`) formats the configured `path`/`format` against
whatever date the caller passes (today, `--date`, or an offset of it). There is
no step that moves the date to the start of the period. Daily notes are
unaffected (a day is its own start), but a weekly note opened on Wednesday
resolves its format against Wednesday: with a day-bearing format such as
`format = "%Y-%m-%d"` it creates `2026-05-13.md` instead of the week's
`2026-05-11.md`, and every day of the week creates a different file. Week-number
formats (`%G-W%V`) mask the bug because the rendered name happens to be stable
within the week; the raw date still leaks into the template context
(`{{ today }}`), so a weekly template's frontmatter/tags vary by open day.

The vault already contains the evidence: `areas/business/weekly/2022-10-01.md`
is a Saturday, and `areas/business/clients/…/Decentrafly Week 2024-03-01.md` a
Friday — weekly notes named after the day they were first opened rather than
the week's Monday.

This affects every surface that resolves a periodic note: `ft notes periodic`,
`ft notes today`, the Notes-tab and Graph-tab `t` / `p` chords, and the
`@weekly`/`@monthly`/`@quarterly`/`@yearly` query sigils.

## What Changes

- **Anchor periodic note resolution to the start of the period.** Add
  `Period::start_of(date, week_start)`: Monday (or the configured week start)
  for weekly, the 1st for monthly, the first day of the quarter for quarterly,
  Jan 1 for yearly, and the date itself for daily. Thread the `Period` into the
  core resolution functions so both the on-disk path and the template context
  are computed from the anchored date, not the raw target date.
- **Configurable week start.** New `week_start = "monday" | "sunday"` key on a
  `[periodic_notes.<period>]` block (default `monday`); only the weekly period
  consults it.
- **Offset ordering.** The target date is `--offset`-shifted first, then
  anchored, so `weekly --offset -1` resolves to the previous week's Monday.
- **Template context.** A created periodic note's template is rendered with
  `today` set to the anchored period date, so `{{ today | date }}` in a weekly
  template is the week's Monday. The now-redundant separate `today` argument is
  dropped from `create_or_get_periodic_path` and `Vault::ensure_target`.
- **Sigils inherit the anchoring.** `@weekly` etc. use the shared resolver, so
  with a day-bearing format they expand to the week's Monday path.

## Capabilities

### New Capabilities

- `periodic-notes`: period-start anchoring of periodic-note path and template
  resolution across the CLI, the TUI, and the query sigils; configurable week
  start.

### Modified Capabilities

- `query-sigil-interpolation`: period sigils resolve against the period's
  representative (start-of-period) date rather than the raw today.

## Impact

- **Affected code**: `ft-core/src/periodic.rs` (new `Period::start_of`;
  `resolve_periodic_path` / `create_or_get_periodic_path` take `period`, anchor,
  and use the anchored date for both path and template), `ft-core/src/config.rs`
  (`WeekStart` enum + `PeriodicPeriod.week_start`), `ft-core/src/vault.rs`
  (`resolve_target` / `ensure_target` pass `Period::Daily`; `today` param
  dropped), `ft-core/src/query/interpolate.rs` (pass the sigil's `Period`),
  `ft/src/cmd/notes.rs`, `ft/src/cmd/timeblocks.rs`,
  `ft/src/tui/notes_actions/periodic.rs`, `ft/src/tui/tabs/graph/mod.rs`,
  `ft/src/tui/tabs/timeblocks/mod.rs`, plus the `ensure_target` callers in
  `ft/src/cmd/tasks.rs`, `ft/src/tui/tabs/graph/tasks.rs`, and
  `ft/src/tui/tabs/tasks/search.rs`.
- **Breaking (ft-core internal API)**: the signatures of
  `resolve_periodic_path`, `create_or_get_periodic_path`, and
  `Vault::ensure_target` change. Only this workspace consumes them; ft.nvim is
  unaffected because the CLI surface is unchanged.
- **Struct-literal ripple**: adding `PeriodicPeriod.week_start` touches ~16
  `PeriodicPeriod { … }` literals (12 in `periodic.rs` tests, 3 in
  `interpolate.rs` tests, 1 in `config.rs` tests); `Vault::ensure_target` has
  10 call sites.
- **Behavior change**: a periodic note created for a non-today date (`--date`,
  `--offset`, the timeblocks "tomorrow" pane) now renders its template with that
  period's start date as `today` instead of the invocation day. Daily `%Y-%m-%d`
  paths are unchanged.
- **Docs**: `docs/config.md` (`[periodic_notes.*]`, `week_start`, anchoring of
  `path`/`format`/template), `docs/guide/notes.md` (offset ordering),
  `docs/graph-query-dsl.md` (sigil anchoring).
- **Tests**: `Period::start_of` unit tests; config parse tests; CLI integration
  (`weekly` + `%Y-%m-%d` at `FT_TODAY=2026-05-13` → `2026-05-11.md`); sigil
  interpolation tests with a day-bearing weekly format; TUI periodic open/nav
  tests.

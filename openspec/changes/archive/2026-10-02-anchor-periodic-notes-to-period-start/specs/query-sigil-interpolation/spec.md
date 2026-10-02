## MODIFIED Requirements

### Requirement: Period sigils resolve to the vault-relative periodic-note path

The `@daily`, `@weekly`, `@monthly`, `@quarterly`, and `@yearly` sigils SHALL
each expand to the double-quoted **vault-relative** path of the corresponding
periodic note, computed via `periodic::resolve_periodic_path` with the
vault-root prefix stripped. The path SHALL be resolved from the **start of the
period** containing the resolved date (Monday or the configured week start for
`@weekly`, the 1st for `@monthly`, the first day of the quarter for
`@quarterly`, January 1 for `@yearly`; `@daily` is unchanged), matching the
anchor used by the periodic-note CLI and TUI. The vault-relative form SHALL
match what the DSL `path` attribute compares against (e.g.
`journal/2026/2026-05-11.md`).

#### Scenario: @daily expands to the vault-relative daily path
- **WHEN** `interpolate("path = @daily", ctx)` is called with `ctx.today = 2026-07-29`, `ctx.vault_root = /vault`, and `[periodic_notes.daily]` with `path = "journal/%Y"`, `format = "%Y-%m-%d"`
- **THEN** it returns `path = "journal/2026/2026-07-29.md"`

#### Scenario: @weekly expands using the weekly format
- **WHEN** `interpolate("path = @weekly", ctx)` is called with `ctx.today = 2026-05-14` and `[periodic_notes.weekly]` with `format = "%G-W%V"`
- **THEN** it returns `path = "journal/2026/2026-W20.md"` (ISO-week form, vault-relative)

#### Scenario: @weekly with a day-bearing format anchors to Monday
- **WHEN** `interpolate("path = @weekly", ctx)` is called with `ctx.today = 2026-05-14` (a Thursday) and `[periodic_notes.weekly]` with `format = "%Y-%m-%d"`
- **THEN** it returns `path = "journal/2026/2026-05-11.md"` (the Monday of that week, vault-relative)

#### Scenario: @monthly with a day-bearing format anchors to the 1st
- **WHEN** `interpolate("path = @monthly", ctx)` is called with `ctx.today = 2026-05-14` and `[periodic_notes.monthly]` with `format = "%Y-%m-%d"`
- **THEN** it returns `path = "journal/2026/2026-05-01.md"` (the first of the month, vault-relative)

#### Scenario: @weekly offset anchors to the offset week's Monday
- **WHEN** `interpolate("path = @weekly-2", ctx)` is called with `ctx.today = 2026-05-14` and `[periodic_notes.weekly]` with `format = "%Y-%m-%d"`
- **THEN** it returns `path = "journal/2026/2026-04-27.md"` (the Monday two weeks before)

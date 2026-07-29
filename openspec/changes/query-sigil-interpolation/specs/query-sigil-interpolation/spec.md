# query-sigil-interpolation Specification

## Purpose

A pre-parse expansion layer for the unified graph/task query DSL that
turns `@`-sigil placeholders (`@today`, `@daily`, `@weekly`, `@monthly`,
`@quarterly`, `@yearly`, each with an optional signed integer offset)
into ordinary DSL string literals, resolved against `dates::today()` and
the vault's `[periodic_notes.<period>]` config. The predicate grammar,
AST, and evaluator are unchanged; the parser only ever sees normal
quoted strings. This makes date/path-derived filters expressible without
typing an ISO date, and — critically — makes them usable inside stored
presets, which previously had to bake the date in literally.

## Requirements

### Requirement: Sigil interpolation runs before DSL parsing

The system SHALL provide an interpolation pass that transforms a query
source string into an equivalent DSL string in which every recognized
`@`-sigil placeholder is replaced by a double-quoted DSL string literal.
The output of interpolation SHALL be a valid input to
`graph::query::parse_with` whenever the input was otherwise valid.
Interpolation SHALL be applied at every `parse_with` call site that has
access to the vault (CLI `tasks`/`graph` query and preset paths, TUI
Tasks search, TUI Graph view-apply and preset-picker apply), before the
parser runs. Strings without any `@`-sigil SHALL pass through unchanged.

#### Scenario: No sigils pass through verbatim
- **WHEN** `interpolate("status = Open and due < today", ctx)` is called
- **THEN** it returns `"status = Open and due < today"` unchanged

#### Scenario: Sigil outside a string literal is expanded
- **WHEN** `interpolate("path includes @daily", ctx)` is called with `ctx.today = 2026-07-29` and a `[periodic_notes.daily]` config resolving to `journal/2026/2026-07-29.md`
- **THEN** it returns `path includes "journal/2026/2026-07-29.md"` (a double-quoted DSL string literal)

#### Scenario: Sigil inside a DSL string literal is left untouched
- **WHEN** `interpolate("title includes \"me@daily\"", ctx)` is called
- **THEN** it returns `title includes "me@daily"` unchanged — the `@daily` inside the quoted string is not expanded

### Requirement: `@today` resolves to the raw ISO date string

The `@today` sigil SHALL expand to the double-quoted ISO `YYYY-MM-DD`
string for the resolved "today" (honoring `FT_TODAY`). It SHALL NOT
require any `[periodic_notes]` configuration.

#### Scenario: @today expands to today's ISO date
- **WHEN** `interpolate("path includes @today", ctx)` is called with `ctx.today = 2026-07-29`
- **THEN** it returns `path includes "2026-07-29"`

#### Scenario: @today honors FT_TODAY
- **WHEN** `interpolate("path includes @today", ctx)` is called with `ctx.today = 2025-01-02` (because `FT_TODAY=2025-01-02`)
- **THEN** it returns `path includes "2025-01-02"`

### Requirement: Period sigils resolve to the vault-relative periodic-note path

The `@daily`, `@weekly`, `@monthly`, `@quarterly`, and `@yearly` sigils
SHALL each expand to the double-quoted **vault-relative** path of the
corresponding periodic note for the resolved "today", computed via
`periodic::resolve_periodic_path` with the vault-root prefix stripped.
The vault-relative form SHALL match what the DSL `path` attribute
compares against (e.g. `journal/2026/2026-07-29.md`).

#### Scenario: @daily expands to the vault-relative daily path
- **WHEN** `interpolate("path = @daily", ctx)` is called with `ctx.today = 2026-07-29`, `ctx.vault_root = /vault`, and `[periodic_notes.daily]` with `path = "journal/%Y"`, `format = "%Y-%m-%d"`
- **THEN** it returns `path = "journal/2026/2026-07-29.md"`

#### Scenario: @weekly expands using the weekly format
- **WHEN** `interpolate("path = @weekly", ctx)` is called with `ctx.today = 2026-05-14` and `[periodic_notes.weekly]` with `format = "%G-W%V"`
- **THEN** it returns `path = "journal/2026/2026-W20.md"` (ISO-week form, vault-relative)

### Requirement: Signed integer offsets shift by the period's own units

Each sigil (`@today` and the period sigils) SHALL accept an optional
trailing signed integer offset of the form `[+-]\d+` (e.g. `@daily-1`,
`@today+7`, `@weekly-2`). The offset SHALL shift the resolved date by
the period's own units via `periodic::Period::offset_date` — days for
`@today`/`@daily`, ×7 days for `@weekly`, calendar months for
`@monthly`/`@quarterly`/`@yearly` — before resolving the path/date
string. Month-end clamping follows `Period::offset_date`'s existing
semantics.

#### Scenario: @daily-1 is yesterday's daily path
- **WHEN** `interpolate("path = @daily-1", ctx)` is called with `ctx.today = 2026-07-29` and a daily config resolving to `journal/2026/2026-07-29.md` for today
- **THEN** it returns `path = "journal/2026/2026-07-28.md"`

#### Scenario: @today+7 is seven days from today
- **WHEN** `interpolate("path includes @today+7", ctx)` is called with `ctx.today = 2026-07-29`
- **THEN** it returns `path includes "2026-08-05"`

#### Scenario: @weekly-2 shifts two weeks
- **WHEN** `interpolate("path = @weekly-2", ctx)` is called with `ctx.today = 2026-05-14` and a weekly config with `format = "%G-W%V"`
- **THEN** it returns `path = "journal/2026/2026-W18.md"`

#### Scenario: @monthly+1 clamps month-end
- **WHEN** `interpolate("path = @monthly+1", ctx)` is called with `ctx.today = 2026-01-31` and a monthly config with `format = "%Y-%m"`
- **THEN** it returns `path = "2026-02"` (Jan 31 + 1 month clamps to Feb 28's month)

### Requirement: Unknown sigils and missing periodic config are hard errors

An `@` followed by ASCII letters that is not one of the recognized
sigil names SHALL produce an `UnknownSigil` error naming the offending
text and the valid set. A period sigil (`@daily`/`@weekly`/…) SHALL
produce a `MissingPeriodicConfig` error when the corresponding
`[periodic_notes.<period>]` block is not configured. An offset that is
not a valid signed integer SHALL produce an `InvalidOffset` error. These
errors SHALL surface through the same UX as DSL parse errors — CLI exit
code 2 where parse errors already yield 2, and the inline query-error
string in the TUI search box.

#### Scenario: Misspelled sigil errors
- **WHEN** `interpolate("path includes @datly", ctx)` is called
- **THEN** it returns an `UnknownSigil` error whose message contains `@datly` and lists the valid sigil names

#### Scenario: Period sigil without config errors
- **WHEN** `interpolate("path = @daily", ctx)` is called and `ctx.periodic.daily` is `None`
- **THEN** it returns a `MissingPeriodicConfig` error whose message names `daily` and references `[periodic_notes.daily]`

#### Scenario: Unknown sigil surfaces as CLI exit code 2
- **WHEN** the user runs `ft tasks list 'path includes @datly'` against a vault
- **THEN** the command exits with code 2 and prints an error naming the unknown sigil

#### Scenario: Missing periodic config surfaces as CLI exit code 2
- **WHEN** the user runs `ft tasks list 'path = @daily'` against a vault with no `[periodic_notes.daily]` block
- **THEN** the command exits with code 2 and prints an error naming the missing `[periodic_notes.daily]` config

### Requirement: Interpolation is idempotent and total on sigil-free input

Interpolation SHALL be idempotent: applying it to its own output SHALL
produce the same string. Strings containing no `@`-sigil outside a
quoted literal SHALL pass through byte-for-byte unchanged, so existing
queries and existing presets are unaffected in behavior.

#### Scenario: Idempotent on expanded output
- **WHEN** `out = interpolate(src, ctx)` succeeds and `interpolate(&out, ctx)` is called again
- **THEN** the second call returns `out` unchanged

#### Scenario: Existing preset without sigils is unchanged
- **WHEN** a stored preset `overdue = "(status in {Open, InProgress}) and due < today"` is resolved and interpolated
- **THEN** the interpolated string equals the stored string exactly

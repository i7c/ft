## Why

Filtering tasks/notes by the current daily note requires typing the ISO date
in a quoted string literal — `path includes "2026-07-29"` — which is awkward
(remember the date, get the format and quotes right) and **cannot be stored
as a preset** because the date is baked into the string and goes stale the
next day. There is no way to express "the path of today's daily note" (or
this week's, or yesterday's) in a reusable, dynamic form. The DSL's date
keywords (`today`, `tomorrow`, `+3d`) only work on date-typed attributes
(`due`, `scheduled`, …), not on the string-typed `path`/`title` where the
daily note's identity actually lives.

## What Changes

- Introduce a **sigil interpolation layer** that runs *before* the DSL
  parser, expanding `@`-prefixed placeholders into ordinary DSL string
  literals. The predicate grammar itself is unchanged — the parser sees
  only normal `"…"` strings.
- New placeholders, resolved against `dates::today()` and the vault's
  `[periodic_notes.<period>]` config:
  - `@today` → the raw ISO date string for today (e.g. `2026-07-29`).
    Requires no periodic config; useful for ISO-filename vaults and as a
    general "today as a string" value.
  - `@daily` / `@weekly` / `@monthly` / `@quarterly` / `@yearly` → the
    vault-relative path of the corresponding periodic note for today
    (e.g. `journal/2026/2026-07-29.md`). Requires the matching
    `[periodic_notes.<period>]` block; a missing block is a clear error.
  - Optional signed integer offset on any placeholder: `@daily-1`,
    `@today+7`, `@weekly-2`. Offset units are the period's own units
    (days for `@daily`/`@today`, weeks for `@weekly`, …), reusing
    `Period::offset_date`.
- Interpolation is applied to (a) the raw query string supplied on the
  CLI / in the TUI search box, and (b) the **resolved preset string**
  before parsing — so presets become dynamic. Example:
  ```toml
  [tasks.presets]
  daily-open = "path includes @daily and status in {Open, InProgress}"
  ```
- Interpolation runs at every existing `parse_with` call site that has
  access to `&Vault` + `dates::today()` (CLI `tasks` / `graph`, TUI Tasks
  search, TUI Graph view query, preset resolution in both CLIs and both
  TUI preset pickers).
- An unknown/misspelled placeholder (e.g. `@datly`) is a hard error with
  a message naming the placeholder and the valid set, surfaced through
  the same error path as DSL parse errors (CLI exit code 2 for `tasks`/
  `graph`).

## Capabilities

### New Capabilities
- `query-sigil-interpolation`: pre-parse expansion of `@`-sigil
  placeholders (`@today`, `@daily`, `@weekly`, …, with optional signed
  offsets) into DSL string literals, resolved against `dates::today()`
  and the vault's periodic-note config.

### Modified Capabilities
- `task-presets`: the resolved preset DSL string is now run through sigil
  interpolation before parsing, so stored presets may contain dynamic
  `@…` placeholders.
- `graph-presets`: same — resolved graph-preset DSL strings are run
  through sigil interpolation before parsing.

## Impact

- **New code**: a new `ft_core::query::interpolate` module
  (`ft-core/src/query/interpolate.rs`) exposing a function that takes the
  source string + a resolver (periodic config + `NaiveDate`) and returns
  the expanded string or an interpolation error. Reuses
  `ft_core::periodic::resolve_periodic_path` (stripping the vault-root
  prefix to keep the vault-relative form the DSL already uses for `path`)
  and `ft_core::dates::parse` for offset grammar.
- **Call sites**: every `graph::query::parse_with` call site that holds a
  `&Vault` gains a one-line interpolation step before parsing. Roughly:
  `ft/src/cmd/tasks.rs`, `ft/src/cmd/graph.rs`, the Tasks-tab search
  (`ft/src/tui/tabs/tasks/search.rs`), the Graph-tab view apply path, and
  the two CLI `resolve_preset` helpers + two TUI preset-picker apply
  paths. The `parse`/`parse_with` signatures are unchanged.
- **Errors**: a new `InterpolationError` (thiserror, in `ft-core`)
  converted to `anyhow` at the binary boundary, mapped to exit code 2
  where DSL parse errors already map to 2.
- **Docs**: `docs/query-dsl.md` / `docs/graph-query-dsl.md` gain a short
  "Sigil interpolation" section; the `tasks`/`graph` CLI help mentions
  `@…`.
- **No grammar/AST/eval changes**: `Token`, `GraphQuery`, `Condition`,
  `eval_cond_on_node` are untouched. The `@` character is currently an
  `IllegalCharacter` DSL error, so introducing it as a pre-parse sigil
  cannot collide with any existing valid query.
- **Tests**: unit tests on the interpolator (each placeholder, offsets,
  missing periodic config, unknown sigil, no-sigil passthrough);
  integration tests via `assert_cmd` against a fixture vault with a
  `[periodic_notes.daily]` block; a preset-with-`@daily` round-trip test.

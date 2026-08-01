## 1. Core interpolator (`ft-core`)

- [x] 1.1 Add `ft-core/src/query/interpolate.rs` with `SigilCtx<'a> { today: NaiveDate, vault_root: &'a Path, periodic: &'a PeriodicNotes }` and `pub fn interpolate(src: &str, ctx: &SigilCtx<'_>) -> Result<String, InterpolationError>`.
- [x] 1.2 Implement the scanner: walk `src` char-by-char, treating `"…"`/`'…'` spans as opaque (honor `\\` escapes like the DSL lexer), copying them verbatim; outside strings, detect `@` + ASCII-alpha run + optional `[+-]\d+`.
- [x] 1.3 Implement sigil resolution: `@today` → `dates::today()`-style ISO date (use `ctx.today`); `@daily`/`@weekly`/`@monthly`/`@quarterly`/`@yearly` → `periodic::resolve_periodic_path(ctx.vault_root, cfg, date)` then `strip_prefix(ctx.vault_root)` to vault-relative, emitted as a double-quoted DSL string literal (escape `\` and `"` per DSL string rules).
- [x] 1.4 Implement offsets: parse the trailing `[+-]\d+`, shift `ctx.today` via `Period::offset_date` (map `@today`→`Period::Daily`, `@weekly`→`Period::Weekly`, …), then resolve. Reuse `Period::offset_date` for month-end clamping.
- [x] 1.5 Define `InterpolationError` (thiserror): `UnknownSigil { name, pos }`, `MissingPeriodicConfig { period, pos }`, `InvalidOffset { raw, pos }`. `@` not followed by a known name → `UnknownSigil`; period sigil with `ctx.periodic.<period> = None` → `MissingPeriodicConfig`; non-digit offset → `InvalidOffset`.
- [x] 1.6 Export `interpolate`, `SigilCtx`, `InterpolationError` from `ft_core::query` (and `ft_core` lib root if ergonomic).
- [x] 1.7 Unit tests in `interpolate.rs`: no-sigil passthrough; `@today` + `FT_TODAY` honor (via `ctx.today`); each period sigil; offsets (`@daily-1`, `@today+7`, `@weekly-2`, `@monthly+1` Jan-31 clamp); sigil inside `"…"` left untouched; idempotency; `UnknownSigil`/`MissingPeriodicConfig`/`InvalidOffset` error cases. Use `assert_fs` TempDir + a hand-built `PeriodicNotes` for path cases.

## 2. CLI wiring (`ft`)

- [x] 2.1 `ft/src/cmd/tasks.rs::run_list`: build `SigilCtx` from `vault.root_path()` + `&vault.config.config.periodic_notes` + `dates::today()`; interpolate `src` before `parse_query`. Map `InterpolationError` to the same exit-2 path as `DslError`.
- [x] 2.2 `ft/src/cmd/tasks.rs::resolve_preset`: interpolate the resolved DSL string before returning it (covers user presets with sigils; no-op for built-ins).
- [x] 2.3 `ft/src/cmd/tasks.rs` bulk/move/complete parse sites (lines ~706, ~1004, ~1429): interpolate the query argument before `parse_query`.
- [x] 2.4 `ft/src/cmd/graph.rs::run`: interpolate `src` before `parse_with`; map `InterpolationError` to exit 2.
- [x] 2.5 `ft/src/cmd/graph.rs::resolve_preset`: interpolate the resolved DSL string before returning it.

## 3. TUI wiring (`ft`)

- [x] 3.1 `ft/src/tui/tabs/tasks/search.rs`: build `SigilCtx` from the tab's `TabCtx.vault` + `dates::today()`; interpolate the trimmed search text before `parse_query`; surface `InterpolationError` as the inline query-error string (same as `DslError`).
- [x] 3.2 `ft/src/tui/tabs/tasks/modals.rs` preset-picker apply: interpolate the resolved preset DSL string before posting/appling (so `@daily` presets expand in the TUI too).
- [x] 3.3 `ft/src/tui/tabs/graph/view.rs::apply_query`: thread a `SigilCtx` (or `&Vault`) in so the user-typed query buffer is interpolated before `parse_query`; surface `InterpolationError` via the existing `parse_error` slot. Update all `apply_query` callers in `graph/mod.rs` to pass the context (available via `TabCtx.vault`).
- [x] 3.4 `ft/src/tui/tabs/graph/mod.rs::apply_preset_to_active_view` (and the preset-picker apply path / `GraphRequest::ApplyPreset`): interpolate the preset DSL string before it reaches `apply_query`.
- [x] 3.5 Update `ft/src/tui/tabs/graph/tests.rs` and any `for_test` callers of `apply_query` to pass the new context (build from the test's `Vault`).

## 4. Docs

- [x] 4.1 Add a "Sigil interpolation" section to `docs/query-dsl.md` (Tasks profile) and `docs/graph-query-dsl.md` (Default profile): the `@` sigils, offsets, the inside-string-exemption rule, and a preset example with `@daily`.
- [x] 4.2 Mention `@…` in the `ft tasks` and `ft graph query` CLI help/usage where the query argument is described (one line each).

## 5. Integration tests & build invariants

- [x] 5.1 `ft/tests/` integration test: fixture vault with `[periodic_notes.daily]` (`path = "journal/%Y"`, `format = "%Y-%m-%d"`); `FT_TODAY=2026-07-29`; assert `ft tasks list 'path includes @daily and status = Open'` matches the same set as `path includes "journal/2026/2026-07-29.md" and status = Open`.
- [x] 5.2 Integration test: a `[tasks.presets]` entry `daily-open = "path includes @daily and status in {Open, InProgress}"`; assert `ft tasks list --preset daily-open` with `FT_TODAY=2026-07-29` and `FT_TODAY=2026-07-30` produce different filtered sets (dynamic re-resolution).
- [x] 5.3 Integration test: `ft tasks list 'path includes @datly'` exits 2 with an error naming the sigil; `ft tasks list 'path = @daily'` against a vault with no `[periodic_notes.daily]` exits 2 naming the missing config.
- [x] 5.4 Run the full build-invariant suite: `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `cargo run --release -q -- commands docs --check`.

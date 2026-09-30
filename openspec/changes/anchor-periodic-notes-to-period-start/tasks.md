## 1. Core anchoring

- [x] 1.1 Add `WeekStart` (`Monday` default, `Sunday`) to `ft-core/src/config.rs` with `Deserialize`/`Serialize`, `#[serde(rename_all = "lowercase")]`, and `#[derive(Default)]`; add `#[serde(default)] pub week_start: WeekStart` to `PeriodicPeriod` and derive `Default` for `PeriodicPeriod`.
- [x] 1.2 Add `Period::start_of(self, date: NaiveDate, week_start: WeekStart) -> NaiveDate` to `ft-core/src/periodic.rs`: daily identity, weekly back-up to Monday/Sunday (reuse the `num_days_from_monday` math pattern from `timeblock::report::week_bounds`), first-of-month, first day of quarter, Jan 1.
- [x] 1.3 Unit-test `Period::start_of`: Wednesday/Sunday → Monday; Monday identity; `week_start = Sunday`; first-of-month; quarter start; Jan 1; daily identity. Cover the year boundary (late December → ISO/week anchor).
- [x] 1.4 Change `resolve_periodic_path` to take `period: Period` and anchor `date` before formatting `path`/`format`.
- [x] 1.5 Change `create_or_get_periodic_path` to `(vault_root, templates_dir, period, cfg, date, now)` — anchor once and pass the anchored date to `render_periodic_note` as `today`; drop the separate `today` argument.
- [x] 1.6 Update the ~16 `PeriodicPeriod { … }` literals (`periodic.rs` tests, `query/interpolate.rs` tests, `config.rs` tests) for the new field, using `..Default::default()` where convenient.
- [x] 1.7 Update the `create_or_get_periodic_path` unit tests in `periodic.rs` for the new signature.

## 2. Call sites

- [x] 2.1 `ft/src/cmd/notes.rs` `run_periodic_inner`: pass the resolved `period`; keep passing the `--offset`-adjusted `base_date` (anchoring happens in core); drop the separate `today` argument.
- [x] 2.2 `ft-core/src/query/interpolate.rs` `expand_sigil`: pass the sigil's `period` into `resolve_periodic_path`.
- [x] 2.3 `ft-core/src/vault.rs`: `resolve_target` passes `Period::Daily`; `ensure_target` drops its `today` parameter and passes `Period::Daily`; update the `ensure_target` unit tests.
- [x] 2.4 `ft/src/tui/notes_actions/periodic.rs`: pass `period` and drop the separate `today`.
- [x] 2.5 `ft/src/tui/tabs/graph/mod.rs`: pass `period` into `resolve_periodic_path`.
- [x] 2.6 `ft/src/tui/tabs/timeblocks/mod.rs` and `ft/src/cmd/timeblocks.rs`: pass `Period::Daily` and drop the separate `today`.
- [x] 2.7 Update the `ensure_target` callers for the dropped `today` argument: `ft/src/cmd/tasks.rs`, `ft/src/cmd/timeblocks.rs`, `ft/src/tui/tabs/graph/tasks.rs`, `ft/src/tui/tabs/tasks/search.rs`, `ft/src/tui/tabs/timeblocks/mod.rs`.

## 3. Docs

- [x] 3.1 `docs/config.md`: document `week_start` (values, default Monday, weekly-only) and state that `path`, `format`, and the template's `today` are resolved against the period start; refresh the `[periodic_notes.weekly]` example.
- [x] 3.2 `docs/guide/notes.md`: document the shift-then-anchor ordering for `--date`/`--offset` and the Monday behavior.
- [x] 3.3 `docs/graph-query-dsl.md`: note that `@weekly`/`@monthly`/`@quarterly`/`@yearly` anchor to the period start.

## 4. Tests

- [x] 4.1 `ft-core/src/config.rs`: parse `week_start = "sunday"`; default is Monday; `week_start = "tuesday"` fails to load.
- [x] 4.2 `ft/tests/notes_periodic.rs`: `weekly` + `format = "%Y-%m-%d"` at `FT_TODAY=2026-05-13` creates `journal/2026/2026-05-11.md`; a second run at `FT_TODAY=2026-05-17` reports `Opened` for the same file; `weekly --offset -1`; monthly/quarterly/yearly day-bearing formats; `--date 2026-01-31 --offset 1` monthly; a template whose `today` renders the anchored date; `week_start = "sunday"`.
- [x] 4.3 `ft-core/src/query/interpolate.rs`: `@weekly` and `@monthly` with day-bearing formats; `@weekly-2` anchoring.
- [x] 4.4 TUI tests: the Notes-tab periodic open and the Graph-tab periodic navigation resolve the anchored (Monday) path.
- [x] 4.5 Run the build invariants: `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `cargo run --release -q -- commands docs --check`.

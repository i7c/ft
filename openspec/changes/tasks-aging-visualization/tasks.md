## 1. Stamp `created` on every create path (prerequisite)

- [ ] 1.1 Add `age_band` pure function to `ft-core` (e.g. `ft-core/src/task/aging.rs`): `age_band(created: Option<NaiveDate>, today: NaiveDate) -> AgeBand` with `enum AgeBand { Fresh, Aging, Stale, Rotten, Unknown }` and the four absolute thresholds (0–3, 4–10, 11–30, >30). Unit-test boundaries: 0, 3, 4, 10, 11, 30, 31, and `None`.
- [ ] 1.2 Set `created: Some(dates::today())` in the TUI quickline create path (`ft/src/tui/tabs/tasks/search.rs`, `submit_quickline`'s `CreateInput`).
- [ ] 1.3 Set `created: Some(dates::today())` in the TUI edit-popup create path (`ft/src/tui/tabs/tasks/search.rs`, `submit_popup_new`'s `CreateInput`).
- [ ] 1.4 Set `created: Some(dates::today())` in the graph-tab create path (`ft/src/tui/tabs/graph/tasks.rs`'s `CreateInput` — uses `..Default::default()`, so set the `created` field explicitly).
- [ ] 1.5 Set `created: Some(today)` in the CLI `ft tasks add` path (`ft/src/cmd/tasks.rs`, ~line 554; `today` is already in scope).
- [ ] 1.6 Update existing create-path tests that assert the exact serialized task line to expect a `➕ <today>` segment, using `FT_TODAY` for stable dates.

## 2. Age badge rendering in the Tasks SearchView

- [ ] 2.1 Add four grey-shade constants to `ft/src/tui/palette.rs` (e.g. `AGE_FRESH`, `AGE_AGING`, `AGE_STALE`, `AGE_ROTTEN`) as `Color::Rgb(...)` values stepping from lightest to darkest.
- [ ] 2.2 Add an `AgeBand -> Color` mapping helper (in `search.rs` or a small render helper module) that returns the corresponding palette constant, and `Unknown -> None` (no background).
- [ ] 2.3 Extend `task_line` (`ft/src/tui/tabs/tasks/search.rs`) to compute the task's age band and age-in-days, and append a fixed-width age badge span. The span sets its own `bg` to the band's grey (not derived from `row_style`); `fg` is a fixed readable color; text is `{days}d` for known ages, blank for `Unknown`. Badge keeps its grey even when the row is selected.
- [ ] 2.4 Update the fixed-column-width accounting in `render_list` (the `inner_width.saturating_sub(34)` constant) to include the new age badge width, so `desc_width` flexes correctly.
- [ ] 2.5 Render the badge unconditionally (blank cell for `Unknown`) so column alignment stays stable regardless of whether `created` is present.

## 3. Tests & snapshots

- [ ] 3.1 Add `insta` snapshot test(s) for the Tasks SearchView covering each age band (Fresh, Aging, Stale, Rotten) plus the `Unknown` (no `created`) case, using `FT_TODAY` for deterministic band values.
- [ ] 3.2 Add a snapshot for a selected row whose task has a known age, asserting the badge retains its grey shade (not the brown selection bg).
- [ ] 3.3 Add a snapshot for a Done/Cancelled task with a known age, asserting the badge still renders its grey shade (row `DIM` doesn't suppress it).
- [ ] 3.4 Regenerate all existing Tasks-view `insta` snapshots affected by the new column.

## 4. Build invariants

- [ ] 4.1 `cargo build --release`
- [ ] 4.2 `cargo test --workspace`
- [ ] 4.3 `cargo clippy --workspace --tests -- -D warnings`
- [ ] 4.4 `cargo fmt --check`
- [ ] 4.5 `cargo run --release -q -- commands docs --check` (aging is render-only; no keymap/registry change, so this should pass unchanged — verify rather than regenerate)

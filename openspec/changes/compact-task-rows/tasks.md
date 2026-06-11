## 1. Relative date formatting in task rows

- [ ] 1.1 Add a `relative_date(d: NaiveDate, today: NaiveDate) -> String` helper in `ft/src/tui/tabs/tasks/search.rs` producing labels: `today`, `yesterday`, `tomorrow`, `Nd ago`, `in Nd`, `Nw ago`, `in Nw`, ISO fallback for >2 weeks away
- [ ] 1.2 Update `task_line` to use `relative_date` for due and scheduled instead of ISO `%Y-%m-%d`
- [ ] 1.3 Remove fixed 14-column padding for due/scheduled in `task_line` — each date renders as `📅 label` / `⏳ label` without trailing padding
- [ ] 1.4 Update `render_list` to pass `inner_width` (not a precomputed `desc_width`) to `build_lines`, and let `task_line` compute its own per-row description width as `inner_width - cursor(2) - status(2) - priority(4) - (due width if present) - (sched width if present)`
- [ ] 1.5 Update `build_lines` signature to accept `inner_width: u16` instead of `desc_width: usize`

## 2. Remove sidebar

- [ ] 2.1 Remove `SIDEBAR_WIDTH` constant, `render_sidebar` method, `select_prev_view`, `select_next_view` from `TasksTab` in `mod.rs`
- [ ] 2.2 Keep `views: Vec<Box<dyn View>>` and `active_view: usize` fields but simplify to always use `views[0]` — remove the `select_*` helpers and direct mutation of `active_view`
- [ ] 2.3 Remove `TASKS_KEYMAP` (only had Up/Down/Enter for sidebar)
- [ ] 2.4 Remove the 3 sidebar `CommandDef`s from `TASKS_COMMANDS`: `tasks.select-prev-view`, `tasks.select-next-view`, `tasks.confirm-view`
- [ ] 2.5 Remove sidebar dispatch arms from `dispatch_command`
- [ ] 2.6 Simplify `handle_event` — remove the `NotHandled` fall-through to tab-level keymap (view handles its own events, done)
- [ ] 2.7 Simplify `render`: single chunk passed to `render_viewport` (no horizontal split)

## 3. Snapshot test updates

- [ ] 3.1 Run `cargo test --workspace` and review insta snapshot diffs for TUI frame tests in `ft/src/tui/tests.rs`
- [ ] 3.2 Update snapshot tests that construct `TasksTab` with `with_clock` — some tests inject a fixed clock and may need adjustment for the sidebar removal
- [ ] 3.3 Accept snapshots with `cargo insta review` after verifying all diffs are intentional

## 4. Build verification

- [ ] 4.1 Run `cargo build --release` to ensure compilation
- [ ] 4.2 Run `cargo clippy --workspace --tests -- -D warnings` to ensure no new warnings
- [ ] 4.3 Run `cargo fmt --check` to ensure formatting
- [ ] 4.4 Run `cargo test --workspace` to ensure all tests pass

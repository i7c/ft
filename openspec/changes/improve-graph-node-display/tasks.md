## 1. Update task node display

- [x] 1.1 In `leaf_display`, change the `NodeKind::Task` arm to prefix the description with a checkbox-style status marker: `[ ]` for Open, `[x]` for Done, `[/]` for InProgress, `[-]` for Cancelled, with unknown status defaulting to `[ ]`

## 2. Update paragraph node display

- [x] 2.1 In `leaf_display`, change the `NodeKind::Paragraph` arm to show `line_start-line_end` range (or just `line_start` when start==end), followed by two spaces and the first 60 characters of `text` with `…` appended when truncated

## 3. Build verification

- [x] 3.1 Run `cargo build --release` and verify no compile errors
- [x] 3.2 Run `cargo clippy --workspace --tests -- -D warnings` and fix any warnings
- [x] 3.3 Run `cargo fmt --check` and apply formatting if needed

## 4. Update snapshot tests

- [x] 4.1 Run `INSTA_UPDATE=always cargo test --workspace` to regenerate TUI snapshots with new display text
- [x] 4.2 Review updated snapshots to verify paragraph and task display formats (no snapshots contain paragraph/task rows, so no changes needed)
- [x] 4.3 Run `cargo test --workspace` clean (without INSTA_UPDATE) and verify all tests pass

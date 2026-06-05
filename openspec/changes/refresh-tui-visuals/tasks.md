## 1. Color palette module

- [x] 1.1 Create `ft/src/tui/palette.rs` with semantic color constants (primary accent orange, secondary accent gold, tertiary accent warm red, dim warm gray, bg warm dark, success green, error red)
- [x] 1.2 Register the `palette` module in `ft/src/tui/mod.rs`
- [x] 1.3 Update `ft/src/tui/ui.rs` — tab bar, status bar, help overlay, git leader, sync conflict modal — to use palette constants
- [x] 1.4 Update `ft/src/tui/tabs/graph.rs` — view strip, tree items (selected/unselected, node kind colors), input bar, modals — to use palette constants
- [x] 1.5 Update `ft/src/tui/tabs/tasks/search.rs` — query bar, task list (selected, overdue, completed, priority), popup — to use palette constants
- [x] 1.6 Update `ft/src/tui/tabs/tasks/mod.rs` — sidebar — to use palette constants
- [x] 1.7 Update `ft/src/tui/tabs/timeblocks/view.rs` — sidebar, panes, form modal, tag modal, quickline — to use palette constants
- [x] 1.7 Update `ft/src/tui/tabs/timeblocks/view.rs` — sidebar, panes, form modal, tag modal, quickline — to use palette constants
- [x] 1.8 Update `ft/src/tui/tabs/notes/view.rs` — idle panel, all overlays — to use palette constants
- [ ] 1.8 Update `ft/src/tui/tabs/notes/view.rs` — idle panel, all overlays — to use palette constants
- [x] 1.9 Update `ft/src/tui/widgets/picker.rs` and `ft/src/tui/widgets/edit_buffer.rs` — picker highlights, edit buffer cursor — to use palette constants (if they use inline colors)

## 2. Graph tree frame

- [x] 2.1 In `GraphTab::render`, wrap the tree `List` widget with `Block::default().borders(Borders::ALL).title(...)` using the active view's query snippet as the title
- [x] 2.2 Position the "press / to edit query" empty-state hint inside the bordered frame (inner area)
- [x] 2.3 Position the parse error line inside the bordered frame (bottom row of the inner area)

## 3. Graph query bar at top

- [x] 3.1 Reorder the `Layout::vertical` constraints in `GraphTab::render` from `[strip, tree, input]` to `[input, strip, tree]`
- [x] 3.2 Update all areas/chunks references: `strip_area` → `chunks[1]`, `tree_area` → `chunks[2]`, `input_area` → `chunks[0]`
- [x] 3.3 Ensure cursor positioning in input mode uses the new `input_area` (which is now `chunks[0]`)
- [x] 3.4 Ensure the tree area scroll calculation uses `chunks[2].height`

## 4. Timeblocks single-day default

- [x] 4.1 In `TimeblocksTab::new()` and `TimeblocksTab::with_clock()`, change `view: ViewMode::Split` to `view: ViewMode::Single`
- [x] 4.2 Verify the sidebar view-mode label already reflects the mode correctly (it does per existing code)
- [x] 4.3 Verify `f` toggling between Single and Split still works end-to-end

## 5. Snapshot test updates

- [x] 5.1 Run `cargo test --workspace` and capture the updated insta snapshots for all TUI frame tests in `ft/src/tui/tests.rs`
- [x] 5.2 Review each snapshot diff for correctness — verify colors changed to warm palette, graph frame borders appear, query bar moved to top, timeblocks default is Single
- [x] 5.3 Accept snapshots with `cargo insta review` if all diffs are intentional

## 6. Build verification

- [x] 6.1 Run `cargo build --release` to ensure compilation
- [x] 6.2 Run `cargo clippy --workspace --tests -- -D warnings` to ensure no new warnings
- [x] 6.3 Run `cargo fmt --check` to ensure formatting
- [x] 6.4 Run `cargo test --workspace` to ensure all tests pass

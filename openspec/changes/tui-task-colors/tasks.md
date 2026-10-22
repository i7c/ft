## 1. Color model

- [x] 1.1 `tabs/tasks/color.rs`: `TaskColor` (six variants), `name`, `tag`, ratatui `color`, `ALL`, `of(&Task)` (first known `color/<name>` tag), `strip_tags(description)`; unit tests.

## 2. Rendering

- [x] 2.1 `DisplayRow::color`, computed in `rebuild_display` with a depth stack; root seeding from effective ancestors (breadth-first over `parent_keys`, bounded).
- [x] 2.2 `task_line`: hide known color tags; color the tree prefix + description span (done/cancelled keep DIM, selection keeps its background).

## 3. Assigning

- [x] 3.1 `tasks.color` command def + `m` binding; `TaskColorPick` modal (FuzzyPicker over six colors + `none`), modal commands/keymap registered, `ActiveModal` arms, `TasksRequest::ColorPicked`, `View::color_picked`.
- [x] 3.2 Apply via `ops::retag_task` (list = the six color tags) to the cut set (cleared on success) or the cursor task; stale-line toast + refresh; empty-selection toast.

## 4. Tests + docs

- [x] 4.1 TUI tests: own color, inherited (indented + `⤴`), own color wins, two parents two colors, parent filtered out (render-buffer fg checks), tag hidden; picker sets / clears / cut set (file contents); snapshot of the picker.
- [x] 4.2 Regenerate `docs/keybindings.md`; add colors to the Tasks-tab section of `docs/guide/tasks.md`.
- [x] 4.3 All five build invariants (clippy: the pre-existing `redundant_field_names` errors on main aside). (only the 4 pre-existing errors remain)

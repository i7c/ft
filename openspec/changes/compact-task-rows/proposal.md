## Why

The Tasks tab feels noisy and slow to scan for "what's relevant right now." Two specific problems:

1. **ISO dates require mental translation.** Seeing `📅 2026-06-08` forces the user to compute "that's 3 days ago" in their head. Relative dates (`3d ago`, `yesterday`, `today`, `tomorrow`, `in 4d`) give that judgment for free.

2. **The sidebar wastes 30% of horizontal space.** It shows a clock (duplicated in the status bar) and a single "Search" view entry that never changes. With only one view, there's nothing to select, yet it burns 24 columns on every frame.

## What Changes

- Task rows render due and scheduled dates as **relative to today** instead of ISO format — overdue dates in red, upcoming dates in dim white, with intuitive labels like `today`, `yesterday`, `3d ago`, `tomorrow`, `in 4d`
- **Remove the sidebar entirely** from `TasksTab` — the clock is redundant (status bar shows time), the view selector has one entry, and the freed 24 columns go to the task list

## Capabilities

### Modified Capabilities
- `tui-task-list` (new): Task row rendering uses relative-date formatting for due and scheduled columns. Overdue rows keep their red coloring; relative labels make urgency immediately scannable.
- `tui-tasks-sidebar` (new): The Tasks tab sidebar is removed. The tab renders as a single full-width viewport with the query bar at top and task list below.

## Impact

- **Affected code**: `ft/src/tui/tabs/tasks/mod.rs` (remove sidebar rendering, sidebar keymap, `SIDEBAR_WIDTH`, `views` Vec, `active_view`, `select_prev_view`, `select_next_view`, `render_sidebar`, the three sidebar `CommandDef`s, `TASKS_KEYMAP`, `dispatch_command` for sidebar commands, `handle_event` fall-through logic), `ft/src/tui/tabs/tasks/search.rs` (relative date formatting in `task_line`, adjust `desc_width` calculation now that there's no sidebar)
- **Snapshot tests** in `ft/src/tui/tests.rs` will need update: sidebar is gone, dates are relative
- No **breaking** changes to keybindings (list-level keys unaffected), APIs, CLI, or data formats
- The `View` trait and `SearchView` remain as a single-element `views` vec for future multi-view expansion; the sidebar is removed, not the view abstraction

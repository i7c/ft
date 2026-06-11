## Context

The Tasks tab currently renders as a horizontal split: 24-column sidebar (clock + view selector) on the left, query bar + task list on the right. Task rows format dates as ISO `YYYY-MM-DD` with fixed-width column padding. All rendering lives in `ft/src/tui/tabs/tasks/`.

## Goals / Non-Goals

**Goals:**
- Replace ISO date formatting in task rows with human-readable relative dates
- Remove the sidebar from the Tasks tab, passing the full terminal width to the viewport

**Non-Goals:**
- Adding a new sidebar or alternative navigation UI
- Changing the query bar, task list structure, or keybindings
- Adding support for more views (the `views` vec stays with one entry)
- Changing the overdue/upcoming section dividers
- Date formatting in the quickline preview or edit popup (those remain ISO)
- Session persistence or user preference for date format

## Decisions

### 1. Relative date format

Replace `task.due.map(|d| d.format("%Y-%m-%d").to_string())` with a relative formatter that compares the date to `today` (passed into `task_line`).

**Labels:**

| Days from today | Label |
|---|---|
| 0 | `today` |
| -1 | `yesterday` |
| 1 | `tomorrow` |
| -2..-6 | `Nd ago` (e.g. `3d ago`) |
| 2..13 | `in Nd` (e.g. `in 4d`) |
| -14..-7 | `1w ago` / `2w ago` |
| 14..30 | `in Nw` |
| ≤ -15 (past) | ISO fallback `2026-04-01` (old tasks where relative loses meaning) |
| ≥ 31 (future) | ISO fallback `2026-07-15` |

This keeps the date column compact and scannable while avoiding context-free dumps for very old or far-future tasks. The due color stays: red for overdue (`d < today`), white for today/future.

**Scheduled dates** use the same relative formatter, colored orange (existing `PRIMARY`).

**Column width:** Remove the fixed 14-column padding for date fields. Each date renders inline without padding — the `📅 ` emoji + label are self-delimiting. This frees horizontal space for the description column.

**Alternative considered:** Show both relative AND absolute as a tooltip — rejected as v2 polish. The relative label is sufficient.

**Alternative considered:** Using "natural language" like "3 days ago" instead of `3d ago` — rejected because it's wider and `Nd` is a well-known shorthand.

### 2. Remove the sidebar

**What's deleted in `mod.rs`:**

```
SIDEBAR_WIDTH constant
views: Vec<Box<dyn View>>  → keep as single SearchView field
active_view: usize          → remove
select_prev_view()          → remove
select_next_view()          → remove
render_sidebar()            → remove
TASKS_KEYMAP                 → remove (only bound Up/Down/Enter for sidebar)
dispatch_command sidebar arms → remove (select-prev-view, select-next-view, confirm-view)
handle_event fall-through to tab keymap → simplify (view always handles first, done)
TASKS_COMMANDS sidebar entries → remove 3 CommandDefs
render() horizontal split   → pass full area to viewport
```

**What stays:**
- `SearchView` as the single view (stored directly or as `views[0]`)
- `View` trait
- All SearchView commands (`TASKS_COMMANDS` minus the 3 sidebar entries)
- All SearchView keybindings (`SEARCH_KEYMAP`)
- `Tab` trait impl — `title`, `on_focus`, `commands`, `keymap`, `dispatch_command`, `handle_event`, `render`, `refresh`, `help_sections`
- The clock and view selector were the only sidebar content; neither moves elsewhere (status bar already shows time)

**Simplest path:** Keep the `views: Vec<Box<dyn View>>` and `active_view: usize` machinery but hardcode `views[0]` everywhere. The sidebar render, keymap, and fall-through event logic are what gets removed. This avoids touching the `View` trait or the SearchView integration. If a future change adds a second view, the sidebar can be re-added then.

**Alternative considered:** Flatten SearchView directly into TasksTab — rejected because we explicitly want to keep the View abstraction for future multi-view expansion.

### 3. Description width calculation

Current: `desc_width = inner_width.saturating_sub(36).max(16)` where 36 accounts for cursor(2) + status(2) + priority(4) + due block(14) + scheduled block(14).

After removing fixed date-column padding, the date columns are variable-width. The description width becomes: `inner_width - cursor(2) - status(2) - priority(4) - (due_label_width if due present) - (sched_label_width if scheduled present)`. Since each row can differ, we compute per-row or use a conservative estimate.

**Decision:** Compute per-row in `task_line`. For each task, measure the rendered due/scheduled label widths and subtract from `inner_width`. This gives max description space when dates are absent.

## Risks / Trade-offs

- [Relative dates lose ISO precision] → Mitigated by ISO fallback for dates >2 weeks away, and the edit popup (`e`) still shows full ISO dates
- [Removing sidebar loses the clock] → The status bar already shows `refreshed HH:MM:SS`. If a clock-in-tab is desired later, it can go in a narrow top strip.
- [Variable-width date columns mean columns don't align vertically] → The `📅 ` and `⏳ ` emoji act as visual anchors. Descriptions also won't align, but they never did in a meaningful way (they're variable-length text). The trade-off is acceptable for the space gain.
- [Snapshot churn] → Every TUI frame snapshot that includes the Tasks tab will change. Review diffs carefully.

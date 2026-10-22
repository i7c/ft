## Context

The Tasks tab flattens the task DAG into `DisplayRow`s via
`hierarchy::expand_forest_visible` (pre-order, `depth`, `via`). Rows are
drawn by `task_line`. Tags are an index derived from the description;
`ops::retag_task(path, line, format, expected, list, tag)` already swaps
one tag out of a switchable list (used by `T` retag) through the guarded
`update_task_line`.

## Decisions

### D1. Storage: `#color/<name>` tag

`TaskColor { Red, Yellow, Green, Blue, Magenta, Cyan }` lives in the TUI
(`tabs/tasks/color.rs`) with `name()`, `tag()` (`color/red`), `color()`
(ratatui ANSI `Color::Red`, …) and `of(task)`: the first tag in
`task.tags` that parses as `color/<name>`. Unknown names
(`#color/teal`) are ordinary tags and color nothing.

Considered: an emoji field (ft-core format change for a TUI concern) and
a `.ft` state file keyed by `🆔` (not visible in the vault, needs ids) —
both rejected by the user.

### D2. Inheritance in `rebuild_display`

Rows are emitted pre-order with `depth`, so a stack of `(depth, color)`
gives each row the nearest colored ancestor on its displayed path:
`color = own.or(stack_top_with_depth < row.depth)`. Roots (depth 0) seed
from `inherited_root_color(idx)`: walk effective parents breadth-first
in `parent_keys` order (physical first), first own color found wins,
bounded by the task count. This makes "goal filtered out, subtasks
matched" still colored.

`DisplayRow` gains `color: Option<TaskColor>`.

### D3. Rendering

In `task_line`, the description text is the task's description with any
`#color/<known name>` word removed (whitespace collapsed); the tree
prefix + description span gets `row_style.fg(color.color())` when the
row has a color and the task isn't done/cancelled (those keep `DIM`,
which still combines with the fg). Status, priority, due, scheduled and
the age badge keep their own styles. Selection keeps its background.

### D4. Assigning

`tasks.color` (`m`) opens `ActiveModal::TaskColorPick` — the shared
`FuzzyPicker` over seven entries (`red` … `cyan`, `none`), each label
showing a colored swatch name. On Enter it posts
`TasksRequest::ColorPicked(Option<TaskColor>)`; the view applies
`ops::retag_task` with the six color tags as the list and the picked tag
(or `""` for none) to every cut task (snapshots as guards; the cut set is
cleared on success) or else the cursor task, then refreshes with the
cursor anchored where it was. The first error stops the batch, toasts,
and requests a graph refresh (as other guarded writes do).

Sync preserves local hashtags on issue anchor lines, so coloring a
ticket survives sync.

## Risks / Trade-offs

- `retag_task` re-joins the description with single spaces (existing
  behavior of `T`); harmless for normal lines.
- Terminal themes remap ANSI colors; that's intended (follows the theme).
- Per-path inheritance means one task can appear in two colors — by
  design for multi-goal tasks.

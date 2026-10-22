## Why

The Tasks tab shows many tasks from many goals at once, and nothing but
text tells them apart. Coloring a goal — and, through it, everything
underneath — makes a dense tree scannable at a glance: everything blue
belongs to the Q4 goal, everything red to the incident.

## What Changes

- **A color is a tag.** A task is colored by a `#color/<name>` hashtag on
  its line, one of `red`, `yellow`, `green`, `blue`, `magenta`, `cyan`
  (the terminal's own ANSI colors). No task-format change; the tag is
  ordinary description text, survives sync/git, and is queryable
  (`tags includes "color/blue"`). The first color tag on a line wins.
- **Inheritance per tree path.** A row's color is its own tag, else the
  color of its nearest colored ancestor row in the displayed tree
  (indented or `⤴`-linked). A task with two parents may show in two
  colors. A root whose parents are filtered out inherits from its
  effective ancestors (physical chain first, then the first `⤴` chain).
- **Rendering.** Only the description and tree glyphs are colored;
  status / priority / due keep their colors, done and cancelled rows stay
  dimmed, the selected row keeps its highlight. The `#color/…` text is
  hidden in the row (it stays in the file and the edit popup).
- **Assigning.** `m` (`tasks.color`) opens a picker with the six colors
  plus `none`; it applies to the cut set if any tasks are marked with
  `d`, else to the cursor task, replacing any existing color tag.
- **Tasks tab only.** No CLI, ft-core or Graph tab changes.

## Capabilities

### New Capabilities

- `tui-task-colors`: color tags, path-based inheritance, row rendering,
  and the `m` color picker on the Tasks tab.

### Modified Capabilities

None.

## Impact

- `ft/src/tui/tabs/tasks/search.rs`: `DisplayRow::color`, computed in
  `rebuild_display`; `task_line` coloring and tag hiding; `tasks.color`
  dispatch applying `ops::retag_task` with the six color tags as the
  switchable list.
- `ft/src/tui/tabs/tasks/mod.rs` (command def), `modals.rs` (picker),
  `modal.rs` / `modal_commands.rs` / `tab.rs` (modal wiring and request).
- `docs/keybindings.md` regenerated; `docs/guide/tasks.md` TUI section.
- TUI snapshot tests for colored trees.

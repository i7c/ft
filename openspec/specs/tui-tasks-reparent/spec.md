# tui-tasks-reparent Specification

## Purpose
Restructuring the task tree from the TUI Tasks tab: a cut set (`d`), paste
as child or sibling (`p`/`P`), clearing it (`Esc`), and indent/outdent
(`>`/`<`). Created by change task-reparent.

## Requirements

### Requirement: Cut set on the Tasks tab

The Tasks tab SHALL keep a cut set: an ordered list of task entries, each holding the task's `(path, line)` key plus the scanned `Task` snapshot. The `tasks.cut-toggle` command SHALL add the cursor task to the cut set, or remove it if it's already there. Cut rows SHALL be rendered with a visible marker. While the cut set is non-empty, the status hint SHALL show its size and the paste chords. `tasks.cut-clear` SHALL empty the cut set.

#### Scenario: Mark two tasks
- **WHEN** the user presses `d` on one task, moves the cursor, and presses `d` on another
- **THEN** both rows show the cut marker and the status hint reads `2 cut — p child · P sibling · Esc clear`

#### Scenario: Toggle off
- **WHEN** the user presses `d` on a row that is already cut
- **THEN** that task leaves the cut set and its marker disappears

#### Scenario: Clear
- **WHEN** the cut set is non-empty and the user presses `Esc`
- **THEN** the cut set is emptied and no file is modified

### Requirement: Cut set survives graph refreshes

When the shared graph snapshot's generation changes, the Tasks tab SHALL re-resolve each cut entry against the new scan. An entry is still valid if the same file has a task whose serialized line equals the snapshot's. If several do, the one nearest the old line number wins, and the entry's key and snapshot are updated to it. Entries that can't be re-resolved SHALL be dropped, and a toast SHALL report how many were dropped.

#### Scenario: Unrelated mutation shifts lines
- **WHEN** a task is cut, and then completing a recurring task above it in the same file inserts a line
- **THEN** after the refresh the cut entry points at the task's new line and still shows the marker

#### Scenario: Cut task edited externally
- **WHEN** a cut task's line is changed on disk so it no longer matches
- **THEN** after the refresh the entry is dropped and a toast reads `1 cut task changed on disk — dropped`

### Requirement: Paste as child or sibling

`tasks.paste-child` and `tasks.paste-sibling` SHALL move every task in the cut set to the cursor task, using `MoveTarget::UnderTask` with placement `Child` or `SiblingAfter` respectively.

- **The move:** it SHALL be planned with one `MoveSource` per cut entry (`expected: Some(snapshot)`, in cut order) and the cursor task as the guarded target, then applied with `apply_move_plan`. After a successful apply the cut set SHALL be emptied, `ctx.request_graph_refresh()` raised, a pending cursor anchor stored on the first moved task's new location, and a toast shown: `moved N task(s) under <description>`, or `… after <description>` for siblings.
- **Nothing cut:** pasting with an empty cut set SHALL do nothing except show the toast `nothing cut — press d on a task first`.
- **Errors:** if the plan returns `IntoOwnSubtree`, `LineChanged`, `TargetChanged` or any other `MoveError`, no file SHALL be written, an error toast SHALL describe the problem, and the cut set SHALL be kept. The one exception: on `LineChanged` or `TargetChanged`, a graph refresh SHALL also be raised.

#### Scenario: Reparent across files
- **WHEN** a task from `journal/2026-10-21.md` is cut, the cursor is on a task in `projects/vendor.md`, and the user presses `p`
- **THEN** the task and its subtasks leave the journal note, become the last children of the cursor task in `projects/vendor.md`, and the cursor lands on the moved task after the refresh

#### Scenario: Paste into own subtree
- **WHEN** the cut set contains a task and the cursor is on one of that task's descendants
- **THEN** the toast reads `can't move a task under itself` and nothing is written

#### Scenario: Cut ancestor and descendant together
- **WHEN** a parent and one of its children are both in the cut set and the user pastes
- **THEN** the parent's block, with the child still inside it, is moved once, and the child isn't duplicated

### Requirement: Indent and outdent from the Tasks tab

`tasks.indent` and `tasks.outdent` SHALL call `ops::plan_indent` / `ops::plan_outdent` on the cursor task (with its snapshot as the guard), apply the plan, raise a graph refresh, and keep the cursor on the moved task through a pending anchor. `NoPreviousSibling` and `AlreadyTopLevel` SHALL be shown as informational toasts, with no write.

#### Scenario: Indent with `>`
- **WHEN** the cursor is on a task that has a preceding sibling and the user presses `>`
- **THEN** the task becomes that sibling's last child and stays selected

#### Scenario: Outdent a top-level task
- **WHEN** the cursor is on a top-level task and the user presses `<`
- **THEN** the toast reads `already top-level` and nothing is written

### Requirement: Help overlay coverage

The `?` overlay on the Tasks tab SHALL list `tasks.cut-toggle`, `tasks.cut-clear`, `tasks.paste-child`, `tasks.paste-sibling`, `tasks.indent` and `tasks.outdent` under a "Restructure" group.

#### Scenario: Overlay lists restructure chords
- **WHEN** the user opens `?` on the Tasks tab
- **THEN** a "Restructure" group shows `d`, `p`, `P`, `>`, `<` and `Esc` with their descriptions

## ADDED Requirements

### Requirement: Link the cut set under the cursor task

`tasks.link` SHALL link every task in the cut set (see `tui-tasks-reparent`) under the cursor task by calling `task::links::plan_link` with the cut snapshots and the cursor task as guards, applying the plan, emptying the cut set, raising a graph refresh, and keeping the cursor on the parent with the parent expanded. On success the toast SHALL read `linked N task(s) under <description>`.

- With an empty cut set, the toast SHALL read `nothing cut — press d on a task first`.
- `WouldCycle` SHALL toast `can't link a task under its own subtask`.
- `PhysicallyNested` SHALL toast `<description> is nested under another task — outdent it first`.

The cut set SHALL be kept on any error.

#### Scenario: Link a journal task without moving it
- **WHEN** the user marks a journal task with `d`, moves the cursor to `Vendor onboarding` in `proj.md`, and presses `L`
- **THEN** the journal line gains the parent link, `proj.md` gains a block id if needed, no line changes file, and the journal task appears under `Vendor onboarding` with the `↳` glyph after the refresh

### Requirement: Unlink the selected task

`tasks.unlink` SHALL remove the cursor task's parent link through `plan_unlink`, apply it, and raise a refresh. A task without a parent link SHALL toast `task has no parent link` and write nothing.

#### Scenario: Unlink
- **WHEN** the cursor is on a linked child and the user presses `U`
- **THEN** the link text is removed from its line, and after the refresh the task shows at its own position in the list, no longer under the parent

### Requirement: Linked-subtask quickline

`tasks.new-linked-subtask` SHALL open the same quickline as `tasks.quickline`, remembering the cursor task as the link parent. On submit, the task SHALL be created at the default target (the same resolution as `c`) with the parent link appended to its description, as specified by `ft tasks create --link-parent`. Without a selected task, the toast SHALL read `no task selected to link a subtask to`.

#### Scenario: Capture into today's note under a project task
- **WHEN** the cursor is on `Vendor onboarding`, the user presses `S`, types `call legal`, and presses Enter
- **THEN** today's daily note gains `- [ ] call legal [[proj#^<id>]]` and the new task appears under `Vendor onboarding`

### Requirement: Linked-child glyph

Rows whose effective parent comes from a resolved parent link SHALL use `↳ ` in place of the leaf `· ` gutter. A linked child that has children of its own SHALL keep the `▸`/`▾` affordance, followed by `↳`. A linked row SHALL omit its parent link's raw text from the displayed description (the file is unchanged).

#### Scenario: Mixed children render distinctly
- **WHEN** `Vendor onboarding` is expanded and has one physical child and one linked child
- **THEN** the physical child renders with `· ` and the linked child with `↳ `

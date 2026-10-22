## ADDED Requirements

### Requirement: Parent picker modal

The Tasks tab SHALL provide a parent picker as an `ActiveModal` variant. It lists a task's references with each parent's description (or the raw id and state when unresolved). It is used in two modes:
- **Remove mode** (from `tasks.unlink`): `Enter` removes the selected reference through `plan_unlink(Parent(..))` (by id for unresolved references), `a` removes all line references, `Esc` closes without writing.
- **Choose mode** (from `tasks.promote`): `Enter` returns the selected key to the caller, `Esc` cancels.

Frontmatter-origin references SHALL be listed but not removable; selecting one toasts the `ParentFromFrontmatter` message.

#### Scenario: Remove one of three
- **WHEN** the picker lists three parents and the user moves to the second and presses Enter
- **THEN** only that id is removed from the task's `⤴` field, the modal closes, and a graph refresh is raised

### Requirement: Rows per occurrence

Because a task can appear under several parents, each Tasks-tab row SHALL identify an occurrence `(TaskKey, via)`, where `via` is the parent it is displayed under (`None` for roots). The selection and the pending cursor anchor SHALL use the occurrence. After a refresh, the cursor SHALL return to the same occurrence when it still exists, else to the task's first occurrence, else to the existing fallback. Expansion state SHALL stay keyed by `TaskKey`: expanding a task expands every occurrence of it.

#### Scenario: Cursor stays on the occurrence
- **WHEN** the cursor is on a task shown under its second parent and the user edits the task's due date
- **THEN** after the refresh the cursor is on the same task under the same parent

## MODIFIED Requirements

### Requirement: Link the cut set under the cursor task

`tasks.link` SHALL link every task in the cut set (see `tui-tasks-reparent`) under the cursor task, adding the parent to each task's existing references. It calls `task::links::plan_link` with the cut snapshots and the cursor task as guards, applies the plan, empties the cut set, raises a graph refresh, and keeps the cursor on the parent with the parent expanded.

On success the toast SHALL read `linked N task(s) under <description>`, plus ` (M already linked)` when some children were unchanged.

- With an empty cut set, the toast SHALL read `nothing cut — press d on a task first`.
- `WouldCycle` SHALL toast `can't link a task under its own subtask`.
- `AlreadyPhysicalParent` SHALL toast `<description> is already nested under it`.

The cut set SHALL be kept on any error.

#### Scenario: Add a second parent without moving
- **WHEN** a journal task already linked under `Vendor onboarding` is marked with `d`, the user moves the cursor to `Ship Q4 goal` and presses `L`
- **THEN** the journal line's `⤴` field gains `Ship Q4 goal`'s id (which is added if missing), no line changes file, and after the refresh the journal task appears under both parents

### Requirement: Unlink the selected task

`tasks.unlink` SHALL remove a parent reference from the cursor task:
- If the cursor row is displayed under a logical parent (`via` is one of the task's resolved line references), it SHALL remove that parent's reference.
- Otherwise, if the task has exactly one line reference, it SHALL remove it.
- Otherwise, if it has several, it SHALL open the parent picker in remove mode.
- With no line reference, it SHALL toast `task has no parent link` and write nothing.

After a write it SHALL raise a refresh.

#### Scenario: Unlink from the parent in context
- **WHEN** a task with `⤴ k3x9qa,p2m7rd` is shown under `Ship Q4 goal` (`p2m7rd`), the cursor is on that row, and the user presses `U`
- **THEN** the line keeps `⤴ k3x9qa`, and after the refresh the task no longer appears under `Ship Q4 goal`

### Requirement: Linked-subtask quickline

`tasks.new-linked-subtask` SHALL open the same quickline as `tasks.quickline`, remembering the cursor task as the link parent. On submit, the task SHALL be created at the default target (the same resolution as `c`) with a `⤴` field naming the parent, as specified by `ft tasks create --link-parent`. Without a selected task, the toast SHALL read `no task selected to link a subtask to`.

#### Scenario: Capture into today's note under a project task
- **WHEN** the cursor is on `Vendor onboarding` (`🆔 k3x9qa`), the user presses `S`, types `call legal`, and presses Enter
- **THEN** today's daily note gains `- [ ] call legal ⤴ k3x9qa` and the new task appears under `Vendor onboarding`

### Requirement: Linked-child glyph

Rows displayed under a logical parent (`via` is a resolved reference, not the physical parent) SHALL use `↳ ` in place of the leaf `· ` gutter. A linked child that has children of its own SHALL keep the `▸`/`▾` affordance, followed by `↳`. Rows SHALL omit the `⤴ …` field from the displayed description (the file is unchanged). A row whose task has more than one effective parent SHALL show a dim `⤴N` badge after the description, where N is the number of effective parents.

#### Scenario: Mixed children render distinctly
- **WHEN** `Vendor onboarding` is expanded and has one physical child and one linked child, and the linked child also has a second parent
- **THEN** the physical child renders with `· `, the linked child with `↳ ` and a `⤴2` badge

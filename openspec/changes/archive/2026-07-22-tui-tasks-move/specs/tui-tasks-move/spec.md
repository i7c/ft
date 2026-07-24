## ADDED Requirements

### Requirement: Move the cursor task to a different file from the Tasks tab

The Tasks tab SHALL provide a `tasks.move` command that, when invoked with the cursor on a task row, opens a fuzzy picker over vault files and headings (`VaultFilePickerSource`) and, on selection, relocates that task (and its subtask block) to the picked target file (appended, or under the picked heading) via `ops::plan_move` + `ops::apply_move_plan`. The move SHALL use a `MoveSource` with `expected: Some(<scanned task>)` so a line that shifted on disk fails with `MoveError::LineChanged` rather than moving the wrong line. After a successful apply the tab SHALL raise `ctx.request_graph_refresh()` so the shared graph snapshot rebuilds.

#### Scenario: Move a task to a different file
- **WHEN** the cursor is on a task row and the user triggers `tasks.move`, types a file name, and confirms the picker
- **THEN** the task is removed from its source file and appended to the target file, the graph snapshot is refreshed, and a success toast names the target

#### Scenario: Move a task under a heading
- **WHEN** the user triggers `tasks.move`, types `file#heading`, and confirms
- **THEN** the task is appended under that heading in the target file (the heading is created at file end if absent), the graph snapshot is refreshed, and a success toast names the target and heading

#### Scenario: Move with no task selected
- **WHEN** the cursor is not on a task row and the user triggers `tasks.move`
- **THEN** no picker opens and an error toast reads "select a task first"

#### Scenario: Same-file target is rejected
- **WHEN** the picked target file is the same file the task already lives in
- **THEN** no plan is built and no write occurs, an error toast explains the target must be a different file, and the picker stays open for the user to pick again

#### Scenario: Line changed on disk before commit
- **WHEN** the task's source line was edited externally between opening the picker and confirming the target
- **THEN** `ops::plan_move` returns `MoveError::LineChanged`, no write occurs, an error toast reports the drift, the modal closes, and the graph snapshot is refreshed

#### Scenario: Picker cancellation
- **WHEN** the user cancels the picker (Esc) before confirming a target
- **THEN** no plan is built, no write occurs, no graph refresh is raised, and the modal closes returning focus to the Tasks tab

#### Scenario: Success toast format
- **WHEN** a move to `path#heading` succeeds
- **THEN** the toast reads `moved to <path>#<heading>` (or `moved to <path>` when no heading was picked), using vault-relative paths

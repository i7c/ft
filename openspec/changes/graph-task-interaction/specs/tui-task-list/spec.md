# tui-task-list

## Purpose

The Tasks tab renders a deduplicated list of matched tasks: a matched subtask
appears exactly once, nested under its matched parent (or as a depth-0 root
if its parent is not in the display set), matching the CLI `ft tasks list
--tree` forest semantics.

## Requirements

### Requirement: Tasks tab display is deduplicated

The Tasks tab's `rebuild_display` SHALL build the display forest such that a
task that is both matched and a child of another matched task appears exactly
once, nested under its parent. No task SHALL appear as both a depth-0 row and
a nested row. This mirrors `ft_core::task::hierarchy::expand_forest`'s
`seen`-set semantics.

#### Scenario: matched parent and subtask render once

- **WHEN** the Tasks tab query matches both a top-level task and its nested subtask (e.g. via `path includes "N"`) and the parent is expanded
- **THEN** the display contains exactly two rows for those tasks: the parent at depth 0 and the subtask nested at depth 1
- **AND** the subtask does NOT also appear as a separate depth-0 row

#### Scenario: matched subtask whose parent is not matched renders as a root

- **WHEN** the Tasks tab query matches a subtask but not its parent
- **THEN** the subtask appears as a depth-0 root (its un-matched parent is not shown)

#### Scenario: expanded parent pulls in un-matched subtasks

- **WHEN** a matched parent is expanded, its un-matched subtasks SHALL appear nested under it (existing behavior preserved), each exactly once

### Requirement: Tasks tab display matches CLI --tree forest

For any query, the set of tasks shown in the Tasks tab SHALL equal the set
shown by `ft tasks list --tree "<same query>"`, modulo the Tasks tab's
overdue/upcoming bucketing and live expand/collapse state.

#### Scenario: TUI and CLI --tree agree on a note's tasks

- **WHEN** a vault note `N` has top-level tasks and nested subtasks and the query `path includes "N"` is run both in the Tasks tab and via `ft tasks list --tree "path includes \"N\""`
- **THEN** both surfaces show each of the note's tasks exactly once, with the same parent/child nesting

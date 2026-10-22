# task-reparent Specification

## Purpose
Core and CLI reparenting of tasks: `MoveTarget::UnderTask` child/sibling
placement, re-indentation of moved blocks, indent-unit detection, subtree
and stale-target guards, indent/outdent planners, and the `ft tasks move
--under`, `ft tasks indent` and `ft tasks outdent` CLI surfaces with JSON
output. Created by change task-reparent.

## Requirements

### Requirement: Task-relative move target

`ops::MoveTarget` SHALL include an `UnderTask` variant carrying the target task's absolute `path`, 1-indexed `line`, an `expected: Option<Task>` guard, and a `placement` of `Child` or `SiblingAfter`. `plan_move` SHALL accept it alongside `Append` and `UnderHeading` without a signature change, and `apply_move_plan` SHALL apply the resulting plan unchanged (every file through `write_atomic`).

- With `placement = Child`, the moved blocks SHALL be inserted immediately after the last line of the target task's block (`block_end`). They SHALL become the target's last children, in source order.
- With `placement = SiblingAfter`, the moved blocks SHALL be inserted immediately after the last line of the target task's block, at the target task's indent.

#### Scenario: Move a subtask to a different parent in the same file
- **WHEN** `a.md` contains `- [ ] A`, `  - [ ] a1`, `- [ ] B` and the user moves `a.md:2` under `a.md:3` as `Child`
- **THEN** `a.md` becomes `- [ ] A`, `- [ ] B`, `  - [ ] a1`

#### Scenario: Move a task under a parent in another file
- **WHEN** `inbox.md:1` is `- [ ] call vendor` with child `  - [ ] find number`, and `proj.md` contains `- [ ] Vendor onboarding` with child `    - [ ] sign NDA`
- **AND** the user moves `inbox.md:1` under `proj.md:1` as `Child`
- **THEN** `inbox.md` no longer contains either line, and `proj.md` ends with `    - [ ] call vendor` followed by `        - [ ] find number`, after `sign NDA`

#### Scenario: Paste as sibling after the target's subtree
- **WHEN** `a.md` contains `- [ ] A`, `  - [ ] a1`, `- [ ] B`, `- [ ] C` and the user moves `a.md:4` with `SiblingAfter` targeting `a.md:1`
- **THEN** `a.md` becomes `- [ ] A`, `  - [ ] a1`, `- [ ] C`, `- [ ] B`

#### Scenario: Non-task lines in the block travel with it
- **WHEN** the moved task has an indented note line (`    some context`) beneath it
- **THEN** that line moves with the task and keeps its depth relative to the task line

### Requirement: Re-indentation of moved blocks

When the target is `UnderTask`, the head line of each moved block SHALL be re-indented to the new depth. Every other line in the block SHALL keep its depth relative to the head, expressed in the target file's indent unit.

- **New depth for `Child`:** if the target already has children, the head takes the leading whitespace of the target's first existing child, verbatim. Otherwise it takes the target's leading whitespace plus one indent unit.
- **New depth for `SiblingAfter`:** the head takes the target task's own leading whitespace.
- **Converting nested lines:** if the source file and the target file use the same indent unit, nested lines keep their relative leading whitespace verbatim. If the units differ, each nested line's relative depth (in source units) is re-expressed in target units. Any remainder that doesn't fill a whole unit is kept verbatim.

#### Scenario: Tabs to spaces
- **WHEN** a block indented with tabs (`- [ ] X`, `\t- [ ] x1`) is moved as a `Child` under a parent in a file whose indent unit is 4 spaces and the parent has no children
- **THEN** the lines are written as `    - [ ] X` and `        - [ ] x1`

#### Scenario: Same unit preserved verbatim
- **WHEN** source and target both use 2-space indents
- **THEN** each nested line keeps the exact whitespace offset it had relative to its head

### Requirement: Indent unit detection

`ops` SHALL provide `detect_indent_unit(lines)`, which returns the indent unit of a markdown file:
- **Tab**, if the first indented list item uses a leading tab.
- **N spaces** otherwise, where N is the smallest positive increase in leading spaces between a list item and the list item it's nested under.
- **Two spaces**, if the file has no nested list items.

Placing a new subtask (`Position::Subtask`, used by `ft tasks create --parent` and the TUI `tasks.new-subtask`) under a parent with no children SHALL use the parent's indentation plus one detected indent unit.

#### Scenario: File uses 4-space nesting
- **WHEN** a file contains `- [ ] A` and `    - [ ] a1` and nothing else nested
- **THEN** `detect_indent_unit` returns four spaces

#### Scenario: Flat file falls back
- **WHEN** a file contains only top-level list items
- **THEN** `detect_indent_unit` returns two spaces

### Requirement: Subtree and staleness guards

`plan_move` SHALL refuse to move a task into its own subtree. If the `UnderTask` target line falls inside any moved block (including being one of the moved heads), the result SHALL be `MoveError::IntoOwnSubtree` and no file SHALL be modified.

When the `UnderTask` target's `expected` is `Some`, `plan_move` SHALL re-parse the target line. If it doesn't match, the result SHALL be `MoveError::TargetChanged`. If the target line isn't a task at all, the result SHALL be `MoveError::NotATask`.

When the target sits in the same file as a source, the target's position SHALL be resolved against the original lines and then mapped through the removals, so that removing blocks above it doesn't misplace the insertion.

#### Scenario: Move under own descendant
- **WHEN** the user moves `a.md:1` (`- [ ] A`) under `a.md:2` (`  - [ ] a1`, a child of A)
- **THEN** `plan_move` returns `IntoOwnSubtree` and `a.md` is unchanged

#### Scenario: Target edited on disk
- **WHEN** the target task's line was edited after the caller scanned it
- **THEN** `plan_move` returns `TargetChanged` and no file is modified

#### Scenario: Removal above target in same file
- **WHEN** `a.md` contains `- [ ] X`, `  - [ ] x1`, `- [ ] T` and the user moves `a.md:1` under `a.md:3` as `Child`
- **THEN** `a.md` becomes `- [ ] T`, `  - [ ] X`, `    - [ ] x1`

### Requirement: Indent and outdent planners

`ops` SHALL provide `plan_indent(source, format)` and `plan_outdent(source, format)`, both returning a `MovePlan` built by `plan_move`.

- **Indent:** the task becomes the last child of its nearest preceding sibling, meaning the previous task in the same file with the same parent and the same indent. If there is no such sibling, the result SHALL be `MoveError::NoPreviousSibling`.
- **Outdent:** the task becomes a sibling placed directly after its parent's block (`SiblingAfter` its parent). Later siblings of the task stay children of the old parent. If the task is top-level, the result SHALL be `MoveError::AlreadyTopLevel`.

#### Scenario: Indent under previous sibling
- **WHEN** `a.md` contains `- [ ] A`, `- [ ] B` and the user indents `a.md:2`
- **THEN** `a.md` becomes `- [ ] A`, `  - [ ] B`

#### Scenario: Outdent leaves later siblings in place
- **WHEN** `a.md` contains `- [ ] P`, `  - [ ] c1`, `  - [ ] c2` and the user outdents `a.md:2`
- **THEN** `a.md` becomes `- [ ] P`, `  - [ ] c2`, `- [ ] c1`

#### Scenario: Indent first child errors
- **WHEN** the user indents a task with no preceding sibling
- **THEN** the result is `NoPreviousSibling` and no file is modified

### Requirement: `ft tasks move --under`

`ft tasks move` SHALL accept `--under <SELECTOR>` as an alternative to `--to`. Exactly one of the two SHALL be given; passing both, or neither, SHALL fail argument parsing.

- The `--under` selector SHALL resolve with the same rules and id→fuzzy fallback as the positional selector, and SHALL match exactly one task. Zero or several matches are an error that lists the candidates.
- `--sibling` (valid only with `--under`) selects `SiblingAfter` placement; the default is `Child`.
- `--query` bulk moves, `--dry-run` and `--yes` SHALL behave as they do with `--to`.
- The scanned target task SHALL be passed as the `expected` guard.

#### Scenario: Reparent from the CLI
- **WHEN** the user runs `ft tasks move inbox.md:4 --under "vendor onboarding"` and the fuzzy selector matches one task
- **THEN** the task at `inbox.md:4` and its subtasks become the last children of that task

#### Scenario: Ambiguous parent
- **WHEN** the `--under` selector matches several tasks
- **THEN** the command fails listing the candidates (file, line, description) and no file is modified

#### Scenario: `--to` and `--under` together
- **WHEN** the user passes both `--to x.md` and `--under abc`
- **THEN** argument parsing fails before any vault work

### Requirement: `ft tasks indent` and `ft tasks outdent`

The CLI SHALL provide `ft tasks indent <SELECTOR>` and `ft tasks outdent <SELECTOR>`, each taking a single-task selector and supporting `--dry-run`. They SHALL call `plan_indent` / `plan_outdent` with the scanned task as the guard, and SHALL report the core errors as human-readable messages, or as JSON under `--json-errors`.

#### Scenario: Outdent from CLI
- **WHEN** the user runs `ft tasks outdent proj.md:7` and the task has a parent
- **THEN** the task is placed after its parent's block, at the parent's indent

### Requirement: Machine-readable move results

`ft tasks move`, `ft tasks indent` and `ft tasks outdent` SHALL accept `--format text|json`, defaulting to `text`. With `json`, after a successful apply (or a `--dry-run`), stdout SHALL contain a JSON array with one object per moved block, of the form `{"description", "from": {"path", "line"}, "to": {"path", "line"}}`. Paths are vault-relative, and `to.line` is the block head's 1-indexed line in the post-move file.

#### Scenario: JSON output for an editor client
- **WHEN** an editor client runs `ft tasks move a.md:2 --under b.md:5 --format json`
- **THEN** stdout is a JSON array containing `{"from": {"path": "a.md", "line": 2}, "to": {"path": "b.md", "line": <new line>}, …}`

### Requirement: Physical nesting strips the parent link

When `plan_move` targets `MoveTarget::UnderTask` (including moves planned by `plan_indent` / `plan_outdent`), and a source's `expected` task carries a `parent_link` in any state, the raw text of that parent link, plus one adjacent space, SHALL be removed from the moved block's head line in the same plan. Other links on the line, and every non-head line, SHALL be left untouched. Moves to `Append` / `UnderHeading` targets SHALL NOT strip links.

#### Scenario: Paste a linked journal task under a project task
- **WHEN** `journal.md:3` is `- [ ] reproduce [[proj#^vo1]] 📅 2026-10-23` and it is moved under `proj.md:5` as `Child`
- **THEN** the moved line in `proj.md` reads `<indent>- [ ] reproduce 📅 2026-10-23`

#### Scenario: Move to a file keeps the link
- **WHEN** the same task is moved with `--to inbox.md`
- **THEN** the moved line still contains `[[proj#^vo1]]`

### Requirement: Cross-file moves retarget block references

`task::links::plan_retarget(plan, scan, ctx)` SHALL extend a `MovePlan` whose moved blocks change file, adding edits so that every wikilink or embed in the vault that resolved to `<source file>#^id` (for an `id` carried by any line in a moved block) points at the target file instead.

- The rewritten link keeps its anchor, display alias and embed marker, and uses the link-text formatting rules of `task-link-cli`.
- For files that already have an edit in the plan, the rewrite SHALL apply to that edit's `new` content: links are re-extracted from it, never located by stale byte offsets.
- Links inside moved blocks that pointed at the source file with `[[#^id]]` (same file) SHALL be rewritten to the explicit form when they end up in a different file from their target, and *vice versa*.

`ft tasks move` (with `--to` or `--under`), the TUI paste (`p`/`P`) and the TUI `M` move SHALL call `plan_retarget` before applying.

#### Scenario: Parent moved to another file
- **WHEN** `proj.md` contains `- [ ] Vendor onboarding ^vo1`, `journal.md` contains `- [ ] reproduce [[proj#^vo1]]`, and the user moves the parent with `ft tasks move proj.md:1 --to archive/2026.md`
- **THEN** `journal.md` now contains `[[2026#^vo1]]` (or the path form if `2026` is ambiguous), and the journal task's parent still resolves after a rescan

#### Scenario: Dry run shows the retarget
- **WHEN** the same move runs with `--dry-run`
- **THEN** the diff includes the `journal.md` link rewrite, and no file is written

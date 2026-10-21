## MODIFIED Requirements

### Requirement: subtask placement in the parent's file

When `--parent` resolves, the new task SHALL be written to the parent's file as an indented subtask at the end of the parent's indented block, using the existing `Position::Subtask` placement. Its indentation SHALL match the parent's first existing child; if the parent has no children yet, it SHALL be the parent's indentation plus one indent unit as detected by `ops::detect_indent_unit` for that file (two spaces when the file has no nested list items). The daily-note default and any default-section logic are bypassed (the parent's file is the target).

#### Scenario: Parent with existing children
- **WHEN** the user creates a task under a parent that already has an
  indented child at four spaces
- **THEN** the new task is written after the parent's last child, indented
  four spaces

#### Scenario: Parent without children in a flat file
- **WHEN** the user creates a task under a parent that has no children, in
  a file with no nested list items
- **THEN** the new task is written immediately after the parent line,
  indented two spaces deeper than the parent

#### Scenario: Parent without children in a tab-indented file
- **WHEN** the user creates a task under a childless parent in a file whose
  other nested list items are indented with tabs
- **THEN** the new task is written immediately after the parent line,
  indented one tab deeper than the parent

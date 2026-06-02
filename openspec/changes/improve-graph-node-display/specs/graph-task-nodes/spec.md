## MODIFIED Requirements

### Requirement: TUI graph renders task nodes
The TUI graph tab SHALL render `NodeKind::Task` nodes with `kind_char = 'T'` and display text set to a checkbox-style status marker followed by a space and the task description. The status marker SHALL be `[ ]` for `"Open"`, `[x]` for `"Done"`, `[/]` for `"InProgress"`, `[-]` for `"Cancelled"`. Unknown status values SHALL display as `[ ]`.

#### Scenario: Task node in graph tree
- **WHEN** the graph contains task nodes and the user expands a note that has `HasTask` edges
- **THEN** the task nodes SHALL appear as children with `T` as the kind character and the display text formatted as `"{marker} {description}"`

#### Scenario: Open task displays empty checkbox
- **WHEN** a task node has status `"Open"` and description `"Fix the login form"`
- **THEN** the display text SHALL be `"[ ] Fix the login form"`

#### Scenario: Done task displays checked box
- **WHEN** a task node has status `"Done"` and description `"Deploy to production"`
- **THEN** the display text SHALL be `"[x] Deploy to production"`

#### Scenario: In-progress task displays half-filled box
- **WHEN** a task node has status `"InProgress"` and description `"Refactor auth module"`
- **THEN** the display text SHALL be `"[/] Refactor auth module"`

#### Scenario: Cancelled task displays strikethrough box
- **WHEN** a task node has status `"Cancelled"` and description `"Migrate legacy API"`
- **THEN** the display text SHALL be `"[-] Migrate legacy API"`

#### Scenario: Task node selection
- **WHEN** a user selects a task node in the graph tree
- **THEN** the task SHALL be highlighted in the same manner as note/directory/ghost/paragraph nodes

## ADDED Requirements

### Requirement: Paragraph display shows line range and text snippet
The TUI graph tab SHALL render `NodeKind::Paragraph` nodes with display text formatted as `"{source}:{line_start}-{line_end}  {snippet}"` when `line_start != line_end`, or `"{source}:{line_start}  {snippet}"` when the range is a single line. `source` SHALL be the vault-relative path from `ParagraphData.source_file`. `snippet` SHALL be the first 60 characters of `ParagraphData.text`, with `…` appended if the text is longer than 60 characters.

#### Scenario: Multi-line paragraph display
- **WHEN** a paragraph node has `source_file = "Areas/finance.md"`, `line_start = 42`, `line_end = 45`, and `text` begins with "Revenue grew 12% YoY..."
- **THEN** the display text begins with `"Areas/finance.md:42-45  "` followed by the first 60 characters of the text

#### Scenario: Single-line paragraph omits duplicate line-end
- **WHEN** a paragraph node has `line_start = 10` and `line_end = 10`
- **THEN** the display text uses `"source:10  ..."` (not `"source:10-10  ..."`)

#### Scenario: Long paragraph text is truncated
- **WHEN** a paragraph node's `text` is 200 characters long
- **THEN** the snippet is the first 60 characters followed by `…`

#### Scenario: Short paragraph text is not truncated
- **WHEN** a paragraph node's `text` is 20 characters long
- **THEN** the snippet is the full 20 characters with no `…` appended

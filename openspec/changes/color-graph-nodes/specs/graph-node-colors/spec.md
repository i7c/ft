## ADDED Requirements

### Requirement: Graph tree rows are color-coded by node kind

The graph tab tree SHALL render each row with a foreground color determined by the row's node kind. The kind character prefix and display text SHALL both use the type color. Whitespace separators (indent, expand indicator, selection marker) SHALL use the base foreground color (white unselected, black selected).

The color mapping SHALL be:
- Note → Cyan
- Directory → Blue
- Ghost → DarkGray
- Task → Yellow
- Paragraph → Gray

#### Scenario: Mixed-type tree shows distinct colors

- **WHEN** the graph tab displays a tree containing Note, Directory, Ghost, Task, and Paragraph rows
- **THEN** each row's kind character and display text use a different foreground color according to the node kind mapping
- **AND** the indent, expand indicator (`▼`/`▶`/` `), and multi-select marker (`●`/` `) are rendered in the base foreground color

#### Scenario: Selected row preserves type color against highlight

- **WHEN** the user moves selection to a row of any node kind
- **THEN** the selected row's background is white
- **AND** the kind character and display text retain the node kind's type color as foreground
- **AND** the indent, indicator, and marker characters use black foreground on white background

#### Scenario: Ghost rows are visually de-emphasized

- **WHEN** the graph tab displays a Ghost row
- **THEN** the row's kind character and display text use DarkGray foreground
- **AND** the row is distinguishable from Note (Cyan) and Paragraph (Gray) rows

#### Scenario: Paragraph rows use a color distinct from parent Notes

- **WHEN** the graph tab displays a Paragraph row adjacent to a Note row
- **THEN** the Paragraph row's kind character and display text use Gray
- **AND** the Note row uses Cyan
- **AND** the two rows are visually distinct

### Requirement: Color mapping is centralized in a single function

The node-kind-to-color mapping SHALL be implemented as a single function in the graph tab module. The function SHALL accept a `&NodeKind` and return a `ratatui::style::Color`. This function SHALL be the only place where the color palette is defined.

#### Scenario: All tree render paths use the same color function

- **WHEN** any graph tab row is rendered
- **THEN** its type color is obtained from the centralized color function
- **AND** no inline color literals exist in the render path

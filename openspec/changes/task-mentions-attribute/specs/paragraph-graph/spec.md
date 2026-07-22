## ADDED Requirements

### Requirement: OwnsTask edges
The graph SHALL contain an `EdgeKind::OwnsTask` edge from each `NodeKind::Paragraph` to every `NodeKind::Task` whose `TaskData.source_line` falls within the paragraph's `[line_start, line_end]` range (within the same `source_file`), inserted during `Graph::build`. The edge is the paragraph-level ownership relation between a paragraph and the task lines it contains, distinct from the note-level `HasTask` edge (which connects a note to its top-level tasks only). Every task that lands in a paragraph SHALL receive exactly one incoming `OwnsTask` edge; subtasks receive their own `OwnsTask` edge independently of the `Subtask` edge from their parent. The edge is total over tasks once the task scanner shares `LineSkipState` semantics with `extract_paragraphs` (see the `task-mentions-attribute` capability's scanner-skip requirement).

#### Scenario: Paragraph owns a task line within its range
- **WHEN** a paragraph spans lines 3-5 of a note and a task line appears at line 4 of that same note
- **THEN** an `EdgeKind::OwnsTask` edge SHALL exist from that paragraph node to that task node

#### Scenario: Paragraph owns multiple tasks
- **WHEN** a paragraph spans lines 3-6 of a note and tasks appear at lines 4 and 5
- **THEN** the paragraph node SHALL have two outgoing `OwnsTask` edges, one to each task node

#### Scenario: Subtask receives its own OwnsTask edge
- **WHEN** a paragraph contains a top-level task at line 3 and an indented subtask at line 4
- **THEN** the paragraph SHALL have an `OwnsTask` edge to BOTH the top-level task and the subtask, AND the subtask's `OwnsTask` edge is independent of its `Subtask` edge from the parent task

#### Scenario: Task in a different paragraph
- **WHEN** two paragraphs P1 (lines 3-4) and P2 (lines 6-8) exist in a note, and a task appears at line 7
- **THEN** only P2 SHALL have an `OwnsTask` edge to that task; P1 SHALL NOT

## MODIFIED Requirements

### Requirement: Graph query DSL edge-kind filters for new edges
The graph query DSL SHALL accept `owns-paragraph`, `paragraph-link`, and `owns-task` as edge-kind traversal specifiers, usable in expansion steps.

#### Scenario: owns-paragraph traversal
- **WHEN** a query expands from a note node via `owns-paragraph`
- **THEN** the expansion yields only the note's paragraph nodes

#### Scenario: paragraph-link traversal
- **WHEN** a query expands from a paragraph node via `paragraph-link`
- **THEN** the expansion yields the notes (or ghosts) the paragraph links to

#### Scenario: owns-task traversal
- **WHEN** a query expands from a paragraph node via `owns-task`
- **THEN** the expansion yields the task nodes whose `source_line` falls within that paragraph's `[line_start, line_end]` range

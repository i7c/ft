# task-mentions-attribute Specification

## Purpose
TBD - created by archiving change task-mentions-attribute. Update Purpose after archive.
## Requirements
### Requirement: `mentions` DSL attribute
The graph query DSL SHALL recognize a `mentions` attribute on node-block conditions. The attribute SHALL have value type `Str` and SHALL be valid on `Subject::SelfNode`, `Subject::From`, and `Subject::To` (node-only; rejected on `Subject::Edge`). The attribute SHALL support the `=`, `!=`, `includes`, and `in` operators. It SHALL NOT be optional (so `is null` / `is not null` SHALL be rejected at parse time with a `TypeMismatch` error). The `mentions` attribute SHALL NOT be coupled to any specific node kind: evaluation against a node of any kind returns a boolean indicating whether the node's reachable concept-target set contains the value.

For the purposes of this attribute, a node's "concept-target set" is the set of concept identity strings reachable from that node via its originating link edges:
- `NodeKind::Task`: walk `incoming(OwnsTask)` to its owning paragraph (exactly one, since ownership is exclusive), then walk `outgoing(ParagraphLink)` from that paragraph; the target's identity string for each resolved `ParagraphLink` is included.
- `NodeKind::Paragraph`: walk `outgoing(ParagraphLink)`.
- `NodeKind::Heading`: walk `outgoing(HeadingLink)`.
- `NodeKind::Note`: walk `outgoing(NoteLink)`.
- `NodeKind::Ghost`, `NodeKind::Directory`: the concept-target set is empty; `mentions` SHALL evaluate to `false` for any value under `=` / `includes` / `in`, and to `true` for any value under `!=`.

A target's "concept identity string" is `NoteData.title` when the target is a `NodeKind::Note`, `GhostData.raw` when the target is a `NodeKind::Ghost`, and `HeadingData.text` when the target is a `NodeKind::Heading`. The wikilink display alias (`[[target|alias]]`'s `alias`) SHALL NOT be matched — `mentions` answers "which concept," not "which spelling."

#### Scenario: Task mentions via owning paragraph
- **WHEN** a task line `- [ ] chase Priya [[onboarding]] 📅 2026-06-15` is parsed in a note, the note has a note node, the paragraph containing the task line has a paragraph node, and the `[[onboarding]]` link resolves to a note `onboarding.md` with `title = "onboarding"`
- **AND** the user writes `node where kind = "Task" and mentions = "onboarding"`
- **THEN** the query SHALL return that task node

#### Scenario: Task mentions with no concept context
- **WHEN** a task line `- [ ] plain task with no links` is parsed, and its owning paragraph contains no wikilinks
- **AND** the user writes `node where kind = "Task" and mentions = "onboarding"`
- **THEN** the query SHALL NOT return that task node

#### Scenario: Task mentions unresolved target
- **WHEN** a task's owning paragraph contains `[[NonExistent]]` (no backing note)
- **AND** the user writes `node where kind = "Task" and mentions = "NonExistent"`
- **THEN** the query SHALL return that task node (matched via the ghost's `raw`)

#### Scenario: Paragraph mentions directly
- **WHEN** a paragraph contains `[[Foo]]` resolving to note `Foo.md`
- **AND** the user writes `node where kind = "Paragraph" and mentions = "Foo"`
- **THEN** the query SHALL return that paragraph node

#### Scenario: Note mentions via NoteLink
- **WHEN** a note contains `[[Bar]]` resolving to note `Bar.md`
- **AND** the user writes `node where kind = "Note" and mentions = "Bar"`
- **THEN** the query SHALL return that note node

#### Scenario: Heading mentions via HeadingLink
- **WHEN** a heading line `## See [[Baz]]` is parsed, the heading has a heading node, and `[[Baz]]` resolves to note `Baz.md`
- **AND** the user writes `node where kind = "Heading" and mentions = "Baz"`
- **THEN** the query SHALL return that heading node

#### Scenario: Ghost mentions is always false
- **WHEN** the user writes `node where kind = "Ghost" and mentions = "anything"`
- **THEN** the query SHALL return zero nodes

#### Scenario: mentions with `in` operator
- **WHEN** a task's owning paragraph contains `[[onboarding]]` and `[[analytics]]`
- **AND** the user writes `node where kind = "Task" and mentions in {"onboarding", "other"}`
- **THEN** the query SHALL return that task node (the `in` matches if any mentioned concept is in the set)

#### Scenario: mentions with `!=` operator
- **WHEN** a task's owning paragraph contains `[[onboarding]]`
- **AND** the user writes `node where kind = "Task" and mentions != "analytics"`
- **THEN** the query SHALL return that task node (the task does not mention `analytics`)

#### Scenario: mentions with `includes` operator
- **WHEN** a task's owning paragraph contains `[[onboarding]]`
- **AND** the user writes `node where kind = "Task" and mentions includes "onboarding"`
- **THEN** the query SHALL return that task node

#### Scenario: mentions does not match alias
- **WHEN** a task's owning paragraph contains `[[onboarding|onboarding-flow]]` resolving to note `onboarding.md` (title `"onboarding"`)
- **AND** the user writes `node where kind = "Task" and mentions = "onboarding-flow"`
- **THEN** the query SHALL NOT return that task node (alias is not matched, only concept identity)

#### Scenario: mentions rejected on edge subject
- **WHEN** the user writes `expand where edge.mentions = "Foo"`
- **THEN** the DSL parser SHALL reject the query with a `ScopeError` indicating `mentions` is a node attribute, not an edge attribute

#### Scenario: mentions rejects is null
- **WHEN** the user writes `node where kind = "Task" and mentions is null`
- **THEN** the DSL parser SHALL reject the query with a `TypeMismatch` error indicating `mentions` is not an optional attribute

#### Scenario: mentions under Profile::Tasks
- **WHEN** the user writes `mentions = "onboarding" and due < today` under `Profile::Tasks` (which prepends `node where kind = Task and …`)
- **THEN** the resulting query SHALL filter tasks whose owning paragraph mentions `"onboarding"` AND whose due date is before today

### Requirement: `OwnsTask` edge kind
The graph SHALL support `EdgeKind::OwnsTask` as a new edge kind. The edge SHALL be directed `Paragraph → Task`: an `OwnsTask` edge SHALL exist from the paragraph node whose `[line_start, line_end]` range contains `TaskData.source_line` (within the same `source_file`) to that task's task node. The edge SHALL be created during `Graph::build` after task nodes, `HasTask` edges, and `Subtask` edges have been inserted. The edge SHALL be total over tasks that have an owning paragraph (every task landing in a paragraph receives exactly one `OwnsTask` edge). The edge SHALL be distinct from `HasTask` (note → top-level task only) and `Subtask` (task → task): a top-level task may have both an incoming `HasTask` edge (from its note) and an incoming `OwnsTask` edge (from its owning paragraph); a subtask has an incoming `Subtask` edge (from its parent task) and an incoming `OwnsTask` edge (from its owning paragraph).

`edge_kind_str` SHALL return `"owns-task"` for `EdgeKind::OwnsTask`. The DSL SHALL accept `"owns-task"` as a valid `edge.kind` value.

#### Scenario: Paragraph owns its task
- **WHEN** a note contains a paragraph spanning lines 3-5 and a task at line 4 (`source_file` matches the note's path)
- **THEN** an `EdgeKind::OwnsTask` edge SHALL exist from that paragraph node to that task node

#### Scenario: Subtask also gets OwnsTask edge
- **WHEN** a note contains a top-level task at line 3 and a subtask at line 4 (indented under line 3), both within the same paragraph
- **THEN** the paragraph SHALL have `OwnsTask` edges to BOTH the top-level task and the subtask

#### Scenario: Task with no matching paragraph (after scanner-skip fix, should not occur)
- **WHEN** a task's `source_line` falls outside any paragraph's `[line_start, line_end]` range for its `source_file` (e.g. inside a fenced code block, which the scanner-skip fix prevents from producing a task in the first place)
- **THEN** no `OwnsTask` edge SHALL exist for that task, AND the task node SHALL still be created in the graph (consistent with the `HasTask` "task with no matching note" scenario)

#### Scenario: OwnsTask does not replace HasTask
- **WHEN** a top-level task at line 4 is owned by paragraph P and the note N has a `HasTask` edge to that task
- **THEN** the graph SHALL contain BOTH the `HasTask` edge from N to the task AND the `OwnsTask` edge from P to the task

#### Scenario: Filter edges by owns-task kind
- **WHEN** a user writes `expand where edge.kind = "owns-task"`
- **THEN** the expansion SHALL traverse only `OwnsTask` edges

### Requirement: Task scanner skips frontmatter and code blocks
`vault::parse_file` SHALL skip YAML/TOML frontmatter and fenced/indented code blocks when scanning for task lines, using the same `markdown::LineSkipState` rules as `extract_paragraphs` and `extract_headings`. A line that `LineSkipState` classifies as structural (frontmatter delimiter, frontmatter body, code-fence delimiter, or inside a fenced/indented code block) SHALL NOT be parsed as a task even if it matches the task-line grammar (`- [ ] …`). This guarantees the invariant that every task node lands in exactly one paragraph, which is required for `OwnsTask` to be total.

#### Scenario: Task in fenced code block is not parsed
- **WHEN** a note contains a fenced code block with a ` - [ ] example task` line inside it
- **THEN** the scan SHALL NOT produce a task for that line, AND the task count for that note SHALL be zero (assuming no other task lines outside the code block)

#### Scenario: Task in frontmatter is not parsed
- **WHEN** a note begins with YAML frontmatter containing a ` - [ ] fake task` line
- **THEN** the scan SHALL NOT produce a task for that line

#### Scenario: Real task after code block is parsed
- **WHEN** a note contains a fenced code block followed (after a blank line) by a real ` - [ ] real task` line
- **THEN** the scan SHALL produce exactly one task for the real line, AND that task SHALL land in the paragraph that contains it (receiving an `OwnsTask` edge)


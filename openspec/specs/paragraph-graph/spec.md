# paragraph-graph Specification

## Purpose
TBD - created by archiving change related-notes-journal. Update Purpose after archive.
## Requirements
### Requirement: Paragraph node kind
The graph SHALL support a `NodeKind::Paragraph(ParagraphData)` variant where `ParagraphData` carries `source_file: PathBuf` (vault-relative), `line_start: u32` (1-indexed, inclusive), `line_end: u32` (1-indexed, inclusive), and `text: String` (the paragraph's raw content).

#### Scenario: Paragraph node exists for each extracted paragraph
- **WHEN** `Graph::build` processes a markdown file containing two paragraphs separated by a blank line
- **THEN** the graph contains two `NodeKind::Paragraph` nodes with correct `line_start` / `line_end` values and the owning note's vault-relative path in `source_file`

#### Scenario: Paragraph text captures full block
- **WHEN** a paragraph spans three lines before a blank-line boundary
- **THEN** the paragraph node's `text` field contains all three lines joined with newlines

### Requirement: OwnsParagraph edges
The graph SHALL contain an `EdgeKind::OwnsParagraph` edge from each `NodeKind::Note` to every `NodeKind::Paragraph` it owns, inserted during `Graph::build`.

#### Scenario: Note owns its paragraphs
- **WHEN** a note file yields three paragraphs during extraction
- **THEN** `graph.outgoing(note_id)` includes exactly three `EdgeKind::OwnsParagraph` edges pointing at the three paragraph nodes

### Requirement: ParagraphLink edges
The graph SHALL contain an `EdgeKind::ParagraphLink` edge from each `NodeKind::Paragraph` to the note (or ghost) that the paragraph's `[[wiki link]]` resolves to. Each wiki link within a paragraph produces one `ParagraphLink` edge. Link resolution SHALL use the existing Obsidian shortest-path rules (case-insensitive).

#### Scenario: Wiki link in paragraph produces edge
- **WHEN** a paragraph contains `[[Foo]]` and note `Foo.md` exists in the vault
- **THEN** a `ParagraphLink` edge exists from that paragraph node to the `NodeKind::Note` for `Foo.md`

#### Scenario: Unresolved wiki link produces ghost edge
- **WHEN** a paragraph contains `[[NonExistent]]` and no such note exists
- **THEN** a `ParagraphLink` edge exists from the paragraph node to a `NodeKind::Ghost` with `raw = "NonExistent"`

#### Scenario: Multiple links in one paragraph
- **WHEN** a paragraph contains `[[Foo]]` and `[[Bar]]`
- **THEN** two `ParagraphLink` edges exist from that paragraph node, one to each target

### Requirement: Paragraph index
The graph SHALL maintain a `paragraph_index: HashMap<(PathBuf, u32), NoteId>` mapping `(vault-relative note path, line_start)` to the paragraph's `NoteId`, enabling O(1) lookup of a paragraph node by location.

#### Scenario: Lookup by path and line
- **WHEN** `graph.paragraph_by_loc(&path, line_start)` is called with a known paragraph's coordinates
- **THEN** it returns `Some(NoteId)` for that paragraph node

#### Scenario: Missing paragraph returns None
- **WHEN** `graph.paragraph_by_loc` is called with a line number that is not a paragraph start
- **THEN** it returns `None`

### Requirement: Paragraph extraction boundaries
`markdown::extract_paragraphs` SHALL split content into paragraphs at: one or more blank lines, a Markdown heading line (starting with one or more `#` followed by a space), or a `---` / `--` horizontal rule. Frontmatter blocks (between leading `---` delimiters) and fenced code blocks SHALL be skipped via `LineSkipState`. A heading line starts a new paragraph; it does not belong to the preceding one.

#### Scenario: Blank-line boundary
- **WHEN** content is `"line one\nline two\n\nline three\n"`
- **THEN** two paragraphs are extracted: `["line one\nline two", "line three"]`

#### Scenario: Heading boundary
- **WHEN** content is `"intro text\n## Section\nbody\n"`
- **THEN** two paragraphs: `["intro text", "## Section\nbody"]`

#### Scenario: Frontmatter skipped
- **WHEN** content begins with a YAML frontmatter block (`---\ntitle: Foo\n---\n`)
- **THEN** the frontmatter lines do not appear in any extracted paragraph

#### Scenario: Fenced code block skipped
- **WHEN** a fenced code block appears between two paragraphs
- **THEN** the code block lines do not appear in any extracted paragraph

### Requirement: Graph build includes paragraph extraction
Paragraphs SHALL be extracted for every markdown file in `Vault::scan`'s single parallel read pass (alongside `extract_links` and `extract_headings`, into `Scan::files`); `Graph::build` SHALL insert paragraph nodes and edges in its serial resolution phase from those artifacts, performing no file I/O of its own.

#### Scenario: Build produces paragraph nodes
- **WHEN** `Graph::build` runs on a vault with markdown files containing prose paragraphs
- **THEN** the resulting graph contains `NodeKind::Paragraph` nodes for each paragraph

### Requirement: refresh_note removes and re-inserts paragraph nodes
`Graph::refresh_note` SHALL remove all `OwnsParagraph`-connected paragraph nodes (and their outgoing `ParagraphLink` edges) for the refreshed note, then re-extract and re-insert them from the file's new content. Removed paragraph nodes SHALL be purged from `paragraph_index`.

#### Scenario: Refresh updates paragraph count
- **WHEN** a note file is modified to add a new paragraph and `refresh_note` is called
- **THEN** the graph contains one more paragraph node for that note than before the refresh

#### Scenario: Refresh cleans stale paragraph index entries
- **WHEN** `refresh_note` is called after a paragraph is deleted from a file
- **THEN** `paragraph_by_loc` returns `None` for the deleted paragraph's former coordinates

### Requirement: Graph query DSL node-kind filter for paragraphs
The graph query DSL SHALL accept `kind:paragraph` as a node-kind filter, selecting only `NodeKind::Paragraph` nodes. Existing kind filters (`kind:note`, `kind:directory`, `kind:task`, `kind:ghost`) SHALL also be formally supported.

#### Scenario: kind:paragraph filters to paragraph nodes
- **WHEN** a graph query uses `kind:paragraph` as the initial selector
- **THEN** only `NodeKind::Paragraph` nodes are included in the result

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


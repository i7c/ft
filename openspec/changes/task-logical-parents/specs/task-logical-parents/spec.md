## ADDED Requirements

### Requirement: Parent-link candidates

A task SHALL be able to name a logical parent through a wikilink (or wikilink embed) in its own line. When resolving, the link's target is resolved with the same rules as graph links (path form, then title form with the shortest-path tiebreak; an empty target means the task's own file). A link on the task line is a *parent-link candidate* when it is one of these:

- **Block-reference form** `[[target#^id]]`: the resolved note contains a task whose `block_link` equals `id`. A block id that exists on a non-task line in that note makes the link an ordinary link, not a candidate. A target note that doesn't resolve, or that has no line with that block id at all, makes the link a **dangling** candidate.
- **Anchor-note form** `[[target]]` (no anchor): the resolved note's frontmatter has `ft.tasks.anchor: <id>`, and the parent is the task in that note whose `🆔` equals `<id>`. If the frontmatter key is present but no task in the note carries that `🆔`, the link is a dangling candidate.

Links to notes without `ft.tasks.anchor`, links with heading anchors, and markdown-style links SHALL never be candidates. The first candidate in line order SHALL be the task's parent link. Every later candidate SHALL be recorded as *extra* for diagnostics and otherwise ignored.

#### Scenario: Block reference to a task in another note
- **WHEN** `proj.md` contains `- [ ] Vendor onboarding ^vo1` and `journal/2026-10-21.md` contains `- [ ] reproduce on staging [[proj#^vo1]]`
- **THEN** the journal task's parent link resolves to `proj.md`'s task, and that task's children include the journal task

#### Scenario: Same-file block reference
- **WHEN** a file contains `- [ ] A ^a1` on line 1 and `- [ ] B [[#^a1]]` on line 5 at the same indentation
- **THEN** B's parent link resolves to A

#### Scenario: Anchor note
- **WHEN** `jira/ABC-123.md` has frontmatter `ft:\n  tasks:\n    anchor: ABC-123` and contains `- [ ] Fix login 🆔 ABC-123`, and a journal task contains `[[ABC-123]]`
- **THEN** the journal task's parent link resolves to the `Fix login` task

#### Scenario: Ordinary note link is not a parent
- **WHEN** a task line contains `[[Claude]]` and `Claude.md` has no `ft.tasks.anchor`
- **THEN** the task has no parent link

#### Scenario: Block id on a non-task line
- **WHEN** a task links `[[notes#^p1]]` and `^p1` marks a paragraph in `notes.md`
- **THEN** the link is an ordinary link and the task has no parent link

#### Scenario: Two candidates
- **WHEN** a task line contains `[[a#^x]] … [[b#^y]]` and both resolve to tasks
- **THEN** the parent is `a`'s task and `[[b#^y]]` is recorded as an extra parent link

### Requirement: `ParentLink` resolution states

`Task` SHALL carry `parent_link: Option<ParentLink>`, filled in by a post-pass at the end of `Vault::scan` (after every file is parsed, using only data the scan already holds). It SHALL be `None` when the task has no candidate. `ParentLink` SHALL hold the candidate's raw link text, its byte range within the task line, the list of extra candidates' raw texts, and a state:

- `Resolved(TaskKey)`: the candidate resolved to a task, and the task is not physically nested.
- `Dangling`: the candidate did not resolve to a task.
- `Ignored`: the task already has a physical (indentation) parent. Physical nesting wins.
- `Cycle`: following effective parents from this task leads back to it. Within each cycle, exactly one link is marked `Cycle` and treated as unresolved: the link of the cycle member with the greatest `TaskKey` in `(path, line)` order. Every other member of that cycle keeps `Resolved`.

#### Scenario: Physical parent wins
- **WHEN** a task is indented under another task and also contains `[[proj#^vo1]]` that resolves
- **THEN** its state is `Ignored` and its effective parent is its physical parent

#### Scenario: Cycle broken deterministically
- **WHEN** `a.md:1` links `[[b#^b1]]` and `b.md:1` (`^b1`) links `[[a#^a1]]` back to `a.md:1` (`^a1`)
- **THEN** `b.md:1` gets `Cycle`, `a.md:1` gets `Resolved(b.md:1)`, and no task is its own ancestor

#### Scenario: Deleted parent
- **WHEN** the line carrying `^vo1` is deleted from `proj.md`
- **THEN** after the next scan, the linking task's state is `Dangling` and it shows up as top-level

### Requirement: Effective parent drives every hierarchy consumer

`task::hierarchy` SHALL expose `parent_key(task) -> Option<TaskKey>`. It returns the physical parent `(source_file, parent)` when present, otherwise the key of a `Resolved` parent link, otherwise `None`. `expand_forest`, `expand_forest_visible`, `child_index_map` and every other parent/child derivation in ft-core SHALL use it. Children of a task SHALL be ordered with physical children first (in source order), then linked children ordered by `(path, line)`.

#### Scenario: Tree output nests a linked child
- **WHEN** the user runs `ft tasks list --tree` and a journal task is linked under `Vendor onboarding`
- **THEN** the journal task appears once, nested under `Vendor onboarding`, and not as a separate root

#### Scenario: Child unmatched by the query still appears under its parent
- **WHEN** a query matches `Vendor onboarding` but not its linked child
- **THEN** the tree still shows the linked child under it (same rule as physical subtasks)

### Requirement: Graph `Subtask` edges across files

`Graph::build` SHALL insert one `EdgeKind::Subtask` edge from each task's effective parent to the task, including when the two live in different files. No `Subtask` edge SHALL be created for `Dangling`, `Ignored` or `Cycle` links.

#### Scenario: Cross-file subtask edge
- **WHEN** a journal task resolves its parent link to a task in `proj.md`
- **THEN** the graph has a `Subtask` edge from the `proj.md` task node to the journal task node, and the journal task keeps its own `HasTask` / `OwnsTask` edges from its own note

### Requirement: Hierarchy attributes in the query DSL

The query DSL SHALL support these task-node attributes:

- `parent` (Str, optional): the effective parent's description.
- `parent_status` (Enum, optional): the effective parent's status (`Open`, `InProgress`, `Done`, `Cancelled`).
- `parent_link` (Enum, optional): `resolved`, `dangling`, `ignored` or `cycle`. `is null` when the task has no candidate.
- `children` (Int): the number of direct effective children.

They SHALL be valid on `Subject::SelfNode` / `From` / `To` and rejected on edges. On non-task nodes, the optional attributes evaluate as null and `children` evaluates as `0`.

#### Scenario: Open subtasks under closed parents
- **WHEN** the user runs `ft tasks list --query 'status = Open and parent_status in {Done, Cancelled}'`
- **THEN** exactly the open tasks whose effective parent is done or cancelled are listed

#### Scenario: Parents with work left
- **WHEN** the user queries `children > 0 and status = Open`
- **THEN** tasks with at least one physical or linked child are listed

#### Scenario: Broken links
- **WHEN** the user queries `parent_link in {dangling, cycle}`
- **THEN** exactly the tasks whose parent link is dangling or marked as a cycle are listed

### Requirement: Built-in hierarchy presets

The task preset table SHALL include `orphaned` (`status in {Open, InProgress} and parent_status in {Done, Cancelled}`) and `broken-links` (`parent_link in {dangling, cycle, ignored}`). Both SHALL be listed by `builtin_names()`, and user presets with the same names SHALL shadow them.

#### Scenario: Preset by name
- **WHEN** the user runs `ft tasks list orphaned`
- **THEN** the `orphaned` DSL is applied

### Requirement: `ft.tasks.anchor` frontmatter reader

`frontmatter` SHALL provide `ft_tasks_anchor_in(fm) -> Option<String>`, which reads the nested `ft.tasks.anchor` scalar in the same way as `ft_tasks_section_in`. The flat key `ft-tasks-anchor` SHALL NOT be recognised.

#### Scenario: Nested key read
- **WHEN** frontmatter is `ft:\n  tasks:\n    anchor: ABC-123\n    section: Tasks`
- **THEN** `ft_tasks_anchor_in` returns `Some("ABC-123")`

### Requirement: Scan cost

The parent-link post-pass SHALL perform no file I/O, and SHALL run in time linear in the number of tasks plus the number of links on task lines.

#### Scenario: Perf gate
- **WHEN** the perf tests run with `FT_PERF_TESTS=1` on the `realistic` fixture
- **THEN** `Vault::scan` with the post-pass stays within 10% of the baseline without it

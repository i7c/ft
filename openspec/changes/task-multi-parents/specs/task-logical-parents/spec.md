## ADDED Requirements

### Requirement: Parent-reference field

The emoji task format SHALL support a parent-reference field, written `⤴ <id>[,<id>…]`. When parsing, `⤴` SHALL be accepted with or without a trailing U+FE0F. The value is a comma-separated list: items are trimmed, empty items are dropped, and the ids are stored in `Task::parent_ids` in the order written (duplicates kept). `serialize_line` SHALL write the bare `⤴` (no U+FE0F), followed by the ids joined by `,`, as the first field directly after the description and before the priority. A task with no ids SHALL have no `⤴` field. Round-tripping a canonical line SHALL be byte-identical.

#### Scenario: Parse and serialize
- **WHEN** the line is `- [ ] write regression test ⤴ k3x9qa, ABC-123 ⏫ 📅 2026-10-30`
- **THEN** `parent_ids` is `["k3x9qa", "ABC-123"]`, the description is `write regression test`, and serializing gives `- [ ] write regression test ⤴ k3x9qa,ABC-123 ⏫ 📅 2026-10-30`

#### Scenario: Emoji presentation selector accepted
- **WHEN** the line contains `⤴️ k3x9qa` (with U+FE0F)
- **THEN** `parent_ids` is `["k3x9qa"]`, and a rewrite of the line writes `⤴ k3x9qa`

#### Scenario: Field placed first
- **WHEN** a task with due date, priority, `🆔` and one parent id is serialized
- **THEN** the order is description, `⤴ …`, priority, `📅 …`, `🆔 …`

### Requirement: Vault-wide task ids

Parent references SHALL resolve through a vault-wide index of task `🆔` values, built in the scan post-pass from every scanned task (anchor tasks of issue notes included). An id carried by exactly one task resolves to that task. An id carried by more than one task is *ambiguous*. An id carried by no task is *dangling*. Wikilinks on a task line SHALL NOT make a parent, whatever their form.

#### Scenario: Local and remote parents in one field
- **WHEN** `proj.md` has `- [ ] Vendor onboarding 🆔 k3x9qa`, `jira/ABC-123.md` has the anchor `… 🆔 ABC-123`, and a journal task has `⤴ k3x9qa,ABC-123`
- **THEN** the journal task has two resolved parents: the `proj.md` task and the `ABC-123` anchor task

#### Scenario: Wikilinks are ordinary links
- **WHEN** a task line contains `[[proj#^vo1]]` and `proj.md`'s `^vo1` line is a task
- **THEN** the task has no logical parent from that link

#### Scenario: Duplicate id
- **WHEN** two tasks both carry `🆔 k3x9qa` and a third has `⤴ k3x9qa`
- **THEN** the third task's reference is `Ambiguous` and adds no parent

### Requirement: `ParentRef` resolution states

`Task` SHALL carry `parent_refs: Vec<ParentRef>`, filled in by the scan post-pass using only data the scan already holds. There is one entry per distinct id, in the order written, followed by the anchor-note frontmatter parent when present and not already listed. Each `ParentRef` SHALL hold the `id`, an `origin` (`Line` or `Frontmatter`), and a state:

- `Resolved(TaskKey)`: the id names exactly one task, and following it doesn't close a cycle.
- `Dangling`: no task carries the id.
- `Ambiguous`: more than one task carries the id.
- `Cycle`: following this reference would make the task its own ancestor. Cycles are found over all effective edges (physical and `Resolved`). For each cycle, exactly one logical reference is marked `Cycle`: the reference held by the cycle member with the greatest `TaskKey` in `(path, line)` order, among members holding a logical edge on the cycle. Physical edges alone can't form a cycle. A reference naming the task itself is always `Cycle`.

A reference whose target is also the task's physical parent SHALL stay `Resolved`. It contributes no second edge and is reported as `redundant` by `ft tasks check`.

#### Scenario: Cycle broken deterministically
- **WHEN** `a.md:1` (`🆔 aaaaaa`) has `⤴ bbbbbb` and `b.md:1` (`🆔 bbbbbb`) has `⤴ aaaaaa`
- **THEN** `b.md:1`'s reference is `Cycle`, `a.md:1`'s reference is `Resolved(b.md:1)`, and no task is its own ancestor

#### Scenario: Deleted parent
- **WHEN** the line carrying `🆔 k3x9qa` is deleted
- **THEN** after the next scan, references to `k3x9qa` are `Dangling`

## MODIFIED Requirements

### Requirement: Anchor-note frontmatter parent

When a note is an anchor note (`ft.tasks.anchor` names a task in it) and its frontmatter has `ft.tasks.parent: <id>` (a plain id scalar), the anchor task SHALL get a `ParentRef` with origin `Frontmatter` for that id, resolved with the same rules as line references. If the anchor line's `⤴` already lists the same id, only the line reference is kept. `plan_unlink` targeting a `Frontmatter` reference SHALL fail with `LinkError::ParentFromFrontmatter`, rendered as `parent comes from the note's ft.tasks.parent — edit the issue in its source`.

#### Scenario: Story under epic plus a local goal
- **WHEN** `jira/ABC-123.md` (anchor `ABC-123`) has `ft.tasks.parent: EPIC-7`, its anchor line has `⤴ p2m7rd`, `EPIC-7` is an anchor task and `p2m7rd` is a local task
- **THEN** the `ABC-123` anchor task has two resolved parents: `p2m7rd` (origin `Line`) and `EPIC-7` (origin `Frontmatter`)

### Requirement: Effective parent drives every hierarchy consumer

`task::hierarchy` SHALL expose `parent_keys(task)`, which yields the task's *effective parents*: the physical parent `(source_file, parent)` first when present, then the key of every `Resolved` reference in order, without duplicates. `children_map`, `child_index_map`, `expand_forest`, `expand_forest_visible` and every other parent/child derivation in ft-core SHALL use it. A task SHALL be a child of each of its effective parents. Children of a task SHALL be ordered with physical children first (in source order), then linked children ordered by `(path, line)`.

Forest expansion SHALL treat the hierarchy as a DAG:
- **Display set:** the matched tasks plus all their transitive descendants.
- **Roots:** a matched task is a root iff none of its effective parents is in the display set. Roots follow matched order.
- **Repeats:** every displayed task SHALL be emitted under every displayed effective parent, so subtrees repeat. Each emitted row SHALL record `via`, the parent it hangs under (`None` for roots).
- **Cycles:** a task already on the current root-to-row path SHALL NOT be emitted again on that path.
- **Row cap:** emission SHALL stop at 50,000 rows; the CLI warns on stderr when this happens.

#### Scenario: Task under two parents
- **WHEN** `Vendor onboarding` (`🆔 k3x9qa`) and `Ship Q4 goal` (`🆔 p2m7rd`) are both open, and a journal task has `⤴ k3x9qa,p2m7rd`, and the user runs `ft tasks list --tree`
- **THEN** the journal task appears nested under `Vendor onboarding` and again under `Ship Q4 goal`, and not as a separate root

#### Scenario: Only one parent in view
- **WHEN** a query matches the journal task and `Vendor onboarding` but not `Ship Q4 goal`
- **THEN** the tree shows the journal task once, under `Vendor onboarding`

#### Scenario: Physical plus logical parent
- **WHEN** a task is indented under `Ship Q4 goal` and has `⤴ k3x9qa`
- **THEN** its effective parents are `Ship Q4 goal` (first) then `Vendor onboarding`, and it appears under both

#### Scenario: Child unmatched by the query still appears under its parent
- **WHEN** a query matches `Vendor onboarding` but not its linked child
- **THEN** the tree still shows the linked child under it

### Requirement: Graph `Subtask` edges across files

`Graph::build` SHALL insert one `EdgeKind::Subtask` edge from each effective parent to the task, for every pair, including across files. No `Subtask` edge SHALL be created for `Dangling`, `Ambiguous` or `Cycle` references. A redundant reference (same as the physical parent) SHALL NOT create a second edge.

#### Scenario: Two incoming subtask edges
- **WHEN** a journal task has two resolved logical parents in different notes
- **THEN** the graph has a `Subtask` edge from each parent task node to the journal task node

### Requirement: Hierarchy attributes in the query DSL

The query DSL SHALL support these task-node attributes:

- `parent` (Str, optional): the descriptions of the effective parents. A comparison holds if it holds for any parent. `is null` holds iff there are no effective parents.
- `parent_status` (Enum, optional): the statuses of the effective parents, with the same any-semantics.
- `parent_ref` (Enum, optional): the states of all references (`resolved`, `dangling`, `ambiguous`, `cycle`), with the same any-semantics. `is null` holds iff the task has no references.
- `parents` (Int): the number of distinct effective parents.
- `open_parents` (Int): the number of effective parents whose status is `Open` or `InProgress`.
- `children` (Int): the number of distinct direct effective children.
- `id` (Str, optional): the task's `🆔`.

The attribute `parent_link` SHALL be removed. Unknown-attribute errors list the valid names. The attributes SHALL be valid on `Subject::SelfNode` / `From` / `To` and rejected on edges. On non-task nodes, the optional attributes evaluate as null and the Int attributes evaluate as `0`.

#### Scenario: Any parent matches
- **WHEN** a task has parents `Vendor onboarding` and `Ship Q4 goal` and the user queries `parent includes "Q4"`
- **THEN** the task is listed

#### Scenario: Look up a task by id
- **WHEN** the user runs `ft tasks list --query 'id = k3x9qa'`
- **THEN** exactly the task carrying `🆔 k3x9qa` is listed

#### Scenario: Broken references
- **WHEN** the user queries `parent_ref in {dangling, ambiguous}`
- **THEN** exactly the tasks with at least one dangling or ambiguous reference are listed

### Requirement: Built-in hierarchy presets

The task preset table SHALL include `orphaned` (`status in {Open, InProgress} and parents > 0 and open_parents = 0`) and `broken-links` (`parent_ref in {dangling, ambiguous, cycle}`). Both SHALL be listed by `builtin_names()`, and user presets with the same names SHALL shadow them.

#### Scenario: Orphaned needs every parent closed
- **WHEN** an open task has two parents, one done and one open
- **THEN** `ft tasks list orphaned` does not list it, and it is listed once the second parent is done too

### Requirement: Scan cost

The parent-reference post-pass SHALL perform no file I/O, and SHALL run in time linear in the number of tasks plus the number of references (plus the cycle passes, one per broken cycle).

#### Scenario: Perf gate
- **WHEN** the perf tests run with `FT_PERF_TESTS=1` on a synthetic 2,000-note vault where every note has tasks with ids and multi-parent references
- **THEN** the post-pass takes less than 10% of the full `scan_vault` time

## REMOVED Requirements

### Requirement: Parent-link candidates
**Reason**: Wikilinks no longer make parents. Logical parents are `⤴` id references (see "Parent-reference field" and "Vault-wide task ids").
**Migration**: None (user decision). Existing `[[note#^id]]` / `[[anchor-note]]` parents become ordinary links. Re-link with `ft tasks link … --under …`.

### Requirement: `ParentLink` resolution states
**Reason**: Replaced by the per-reference `ParentRef` states. `Ignored` no longer exists, because physical nesting combines with logical references, and `extra` no longer exists, because every reference counts.
**Migration**: Consumers read `Task::parent_refs`, and use `hierarchy::parent_keys` for effective parents.

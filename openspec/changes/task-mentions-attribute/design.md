## Context

`ft`'s task subsystem lives in `ft` because tasks arise during note-taking — so a task should be queryable by the concept it arose from. The premise review ([docs/2026-07-19-premise-review.md](../../docs/2026-07-19-premise-review.md) §2) found that this thesis is unreachable from the query surface today: a `Task` node has only `HasTask` (up to its note) and `Subtask` (down to children) edges, with no path to the concepts mentioned in its surrounding paragraph. The unified DSL (`graph::query`) is shared between graph and task queries via `Profile::Tasks` (a token-level prelude injecting `node where kind = Task and …`), so the task surface *looks* unified with the graph surface while being unable to express graph-shaped questions about a task's concept context.

The concept-reachable path that exists today is a value-match lookup at query time, not a graph walk:

```
Task  --(source_file + line ∈ [line_start, line_end])-->  owning Paragraph  →[ParagraphLink]→  concept
```

A full design (with code sketches) is in [docs/2026-07-19-task-mentions-design.md](../../docs/2026-07-19-task-mentions-design.md). This document records the architectural decisions and the reasoning behind them.

Current state of the graph's ownership model (relevant subset):

| Edge | Direction | Pattern |
|---|---|---|
| `OwnsParagraph` | Note/Heading → Paragraph | owner → owned |
| `OwnsHeading` | Note/Heading → Heading | owner → owned |
| `HasTask` | Note → Task (top-level only) | owner → owned |
| `Subtask` | Task → Task | parent → child |
| `ParagraphLink` | Paragraph → Note/Ghost/Heading | link |

Every containment edge points owner → owned. The natural shape for "paragraph contains task line" is `Paragraph →[OwnsTask]→ Task`, fitting the existing family.

The graph query DSL's `Attr` enum is a flat set shared across both profiles: graph attributes (`Kind`, `Path`, `Title`, `Form`, `Embed`, `Indegree`, `Outdegree`) and task attributes (`Status`, `Priority`, `Due`, `Scheduled`, `Created`, `Start`, `Completed`, `Description`, `Tags`). There is no `mentions` / `about` predicate.

## Goals / Non-Goals

**Goals:**
- Make the task thesis reachable from the query surface: `ft tasks list --query 'mentions = "onboarding" and due < today'` works end-to-end.
- Generalize the `mentions` attribute across node kinds, so the same predicate works for graph queries (`node where kind = Paragraph and mentions = "onboarding"`) and not just tasks. The attribute is not task-specific.
- Use the graph's existing ownership direction (`Paragraph → Task`), not a reverse or task-specific edge.
- Make the edge total over tasks, by fixing the latent scanner inconsistency that lets a ` - [ ]` line in a code block be parsed as a task with no owning paragraph.

**Non-Goals:**
- No direct `TaskMentions` edge (task → concept). That would duplicate `ParagraphLink` information and require maintaining parity; the `OwnsTask` + existing `ParagraphLink` composition is the right shape — one source of truth for "what concepts does this text mention," reached from tasks via the ownership edge.
- No `refresh_note` task updates. `refresh_note` doesn't touch task nodes today (only `Graph::build` does — see `insert_task_node`, `insert_hastask_edges`, `insert_subtask_edges`). `OwnsTask` will have the same behavior: created at `build` time only. This is a pre-existing limitation, not introduced here, and fixing it is a separate concern.
- No counting. `mentions` is a boolean existence predicate. "How many times does this paragraph mention X" is a separate future attribute if wanted.
- No alias matching. `mentions` matches concept identity (`Note.title` / `Ghost.raw`), not the display alias used in a wikilink (`[[onboarding|onboarding-flow]]`). Alias matching, if wanted, is a separate attribute.

## Decisions

### D1. Edge direction: `Paragraph → Task` (owner → owned)

**Decision.** The new edge is `EdgeKind::OwnsTask`, directed `Paragraph → Task`, mirroring `OwnsParagraph` / `OwnsHeading` / `HasTask`.

**Why.** Every existing containment edge in the graph points owner → owned. An earlier draft considered `Task → Paragraph` ("task points at owning paragraph"), but that reverses the convention for no benefit — the ownership relation is "paragraph contains task line," not "task references paragraph." Keeping the direction consistent means `mentions` on a `Task` walks *up* via `incoming(OwnsTask)` then *out* via `outgoing(ParagraphLink)`, which is the same "up then out" shape that any descendant-of-owner query would use.

**Alternatives considered.**
- *`Task → Paragraph` (reverse direction).* Rejected: breaks the owner → owned convention shared by `OwnsParagraph`, `OwnsHeading`, `HasTask`, `Contains`.
- *Direct `TaskMentions` edge (task → concept).* Rejected: duplicates `ParagraphLink` provenance, requires a second source of truth that must be kept in sync on every paragraph edit. The premise review's "option 2" was this; the design note explains why `OwnsTask` + `ParagraphLink` is preferred.

### D2. Scanner-skip invariant fix

**Decision.** `vault::parse_file` will use `markdown::LineSkipState` when scanning for task lines, identical to `extract_paragraphs` and `extract_headings`. This makes "every task lands in exactly one paragraph" hold by construction.

**Why.** The `OwnsTask` edge is total only if every task has an owning paragraph. Today the invariant does *not* hold: `parse_file` iterates every line with no skip logic, so a ` - [ ]` line inside a fenced code block or in frontmatter is parsed as a task, while `extract_paragraphs` skips those regions. A task in a code block has no owning paragraph. This is a latent bug independent of the edge question — ` - [ ]` lines in code blocks are examples, not tasks. Fixing it makes the edge total and removes a class of phantom tasks from the scan.

**Alternatives considered.**
- *Make the edge partial and fall back at query time.* `OwnsTask` exists only for tasks with an owning paragraph; `mentions` on a task with no `OwnsTask` edge returns `false`. Rejected: keeps the scanner inconsistency in place and silently drops concept context for code-block tasks, which is exactly the case the fix targets.
- *Make `extract_paragraphs` not skip code blocks.* Rejected: changes paragraph semantics for every consumer, clearly wrong.

### D3. `mentions` is generalized, not task-specific

**Decision.** `Attr::Mentions` evaluates against any node kind, walking the link edges that originate from that node (or, for `Task`, from its owning paragraph):

| Node kind | Edge(s) walked |
|---|---|
| `Task` | `incoming(OwnsTask)` → `outgoing(ParagraphLink)` from the owning paragraph |
| `Paragraph` | `outgoing(ParagraphLink)` |
| `Heading` | `outgoing(HeadingLink)` |
| `Note` | `outgoing(NoteLink)` |
| `Ghost`, `Directory` | (none, returns empty) |

**Why.** The premise review's "option 1" was a task-only `about` attribute. Generalizing keeps the DSL honest: "mentions" is a graph-shaped question, and the same attribute name should mean the same thing across node kinds. A `Paragraph` mentioning `[[onboarding]]` and a `Task` whose paragraph mentions `[[onboarding]]` both satisfy `mentions = "onboarding"`. The Task case is the only one that takes an extra hop, and that hop is exactly the new `OwnsTask` edge.

**Alternatives considered.**
- *Task-only `about` attribute.* Rejected: leaves graph queries unable to ask the same question, and bakes in the "tasks are a parallel system" framing the premise review flagged.
- *Counting (`mentions > 2`).* Rejected as out of scope; `mentions` is boolean existence for now.

### D4. `Note` granularity: walk `NoteLink`, not the union of paragraph `ParagraphLink`s

**Decision.** For `Note` nodes, `mentions` walks `outgoing(NoteLink)`, not the union of the note's paragraphs' `ParagraphLink` edges.

**Why.** Every `ParagraphLink` is also represented as a `NoteLink` at note level (the note-level edge is one per occurrence, the paragraph-level edge is one per occurrence per paragraph — same set of targets). For a pure existence check the two walks yield the same answer set. `NoteLink` is cheaper (one hop, no paragraph descent) and is the natural note-level signal. If counting is added later this decision can be revisited, but for boolean existence `NoteLink` loses nothing.

### D5. Target matching: concept identity, not alias

**Decision.** `mentions = "onboarding"` matches against `Note.title` (for resolved targets) and `Ghost.raw` (for unresolved targets). It does not match the wikilink display alias (`[[onboarding|onboarding-flow]]`'s `onboarding-flow`).

**Why.** `mentions` answers "which concept," not "which spelling." An unresolved `[[onboarding]]` still counts (matched via `Ghost.raw`), because the concept is real even if the note isn't. Aliases are a display concern; matching on them would make `mentions = "X"` depend on how the link was written, not on what it points at. If alias matching is wanted, it's a separate attribute (`alias = "..."`).

### D6. `OwnsTask` creation placement: separate `insert_ownstask_edges`, called from `build`

**Decision.** A new private method `insert_ownstask_edges(&mut self, tasks: &[Task])`, called from `Graph::build` after `insert_task_node`, `insert_hastask_edges`, and `insert_subtask_edges` (it needs both `paragraph_index` and `task_index` populated). Implementation groups tasks by file, then for each note walks `note_paragraphs(note_id)` (already sorted by `line_start`) and matches `task.source_line ∈ [paragraph.line_start, paragraph.line_end]`.

**Why.** Mirrors the shape of `insert_hastask_edges` / `insert_subtask_edges` exactly — one method per edge kind, called in sequence from `build`. Keeps `insert_paragraph_nodes_for` focused on paragraph creation (it already takes `&[Paragraph]`, `&[Heading]`, `&[RawLink]`; adding `&[Task]` would be a fourth parameter and a fourth responsibility). Build-time cost is O(tasks × paragraphs) per note in the naive form, acceptable for a one-shot build; `note_paragraphs` is already sorted so a merge-style walk is O(n+m) if profiling ever demands it.

**Alternatives considered.**
- *Create `OwnsTask` edges inside `insert_paragraph_nodes_for`.* Rejected: would require passing `&[Task]` into that method (signature change) and would mix "create paragraph node" with "wire up task ownership" responsibilities.
- *Index paragraphs by `(file, line) → owning paragraph` for O(1) lookup.* Rejected as premature; the per-note scan is fine for build-time cost.

## Risks / Trade-offs

- **[BREAKING: tasks in code blocks stop being recognized]** → The scanner-skip fix means a vault with ` - [ ]` lines inside ` ``` ` blocks or frontmatter will see those stop producing task nodes. This is the intended behavior (those are examples, not tasks), but it is a behavior change. The user has confirmed code-block ` - [ ]` lines in their vault are examples, not real tasks. Documented in the proposal's "What Changes."
- **[No `refresh_note` task updates]** → `OwnsTask` edges are rebuilt only on full `Graph::build`, not on `refresh_note`. This matches existing `HasTask`/`Subtask` behavior and is a pre-existing limitation. A note edited via `refresh_note` will have stale task edges until the next full `scan()` + `build()`. The TUI's shared graph snapshot already rebuilds on mutation via a background worker, so the practical impact is bounded. Documented as a non-goal.
- **[`mentions` on `Task` is O(paragraph links)] per task]** → Each `mentions` evaluation on a `Task` walks `incoming(OwnsTask)` (one edge, since ownership is exclusive) then `outgoing(ParagraphLink)` from the owning paragraph. This is the same cost as `mentions` on a `Paragraph`. Acceptable for query workloads; if a future query needs to evaluate `mentions` over every task in a large vault, an index could be added, but that's premature.
- **[`OwnsTask` edge and `HasTask` edge coexist]** → A top-level task now has both `HasTask` (from note) and `OwnsTask` (from paragraph). This is intentional: they answer different questions. `HasTask` is note-level ("tasks of note N", top-level only, deduped tree via `Subtask`); `OwnsTask` is paragraph-level ("which paragraph does this task belong to", every task including subtasks). The two are not redundant. Documented in the edge docstring.
- **[Heading links double-count for `Heading` mentions]** → A heading line begins a paragraph (Fork A2), so a link on a heading line produces both a `HeadingLink` (from the heading) and a `ParagraphLink` (from the paragraph at the same line). `mentions` on a `Heading` walks only `HeadingLink`, so there's no double-count; `mentions` on the `Paragraph` at that line walks `ParagraphLink`. The two views are consistent. No issue, but worth knowing.

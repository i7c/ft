## Why

The task subsystem's stated reason for living in `ft` is that tasks arise *during* note-taking, so they belong in the same vault and on the same query surface as notes. That thesis predicts one concrete query: "show me tasks about `[[onboarding]]` that are due today." Today that query is impossible to write — `Task` nodes have no edge to the concepts mentioned in their surrounding paragraph, and the task query surface (`Profile::Tasks` over the unified DSL) has no `mentions`/`about` predicate. The argument for the feature and the feature as built disagree at the query level. This change closes that gap. See [docs/2026-07-19-premise-review.md](../../docs/2026-07-19-premise-review.md) §2 for the full finding and [docs/2026-07-19-task-mentions-design.md](../../docs/2026-07-19-task-mentions-design.md) for the design.

## What Changes

- **Scanner skip fix.** `vault::parse_file` will use `markdown::LineSkipState` when scanning for task lines, so ` - [ ]` lines inside fenced code blocks or YAML frontmatter are no longer recognized as tasks. This makes the task scanner's skip rules identical to `extract_paragraphs` / `extract_headings`, which is the invariant the new edge depends on. This is also a latent bug fix (code-block examples are not tasks). **BREAKING** in the narrow sense that a vault with ` - [ ]` lines inside ` ``` ` blocks or frontmatter will see those stop being recognized as tasks after this change.
- **New `EdgeKind::OwnsTask`** — a `Paragraph → Task` edge, mirroring the existing ownership family (`OwnsParagraph`, `OwnsHeading`, `HasTask`, `Subtask`). Total over tasks once the scanner-skip invariant holds: every task lands in exactly one paragraph. Direction is owner → owned, consistent with every other containment edge in the graph. Distinct from `HasTask` (note → top-level task only) and `Subtask` (task → task).
- **New `Attr::Mentions`** in the graph query DSL — a generalized predicate, not task-specific. For `Task` nodes it walks the owning paragraph's `ParagraphLink` edges (via the new `OwnsTask` edge); for `Paragraph` / `Heading` / `Note` it walks that node's own link edges. Supports `=`, `!=`, `includes`, `in`. Matches against concept identity (`Note.title` for resolved targets, `Ghost.raw` for unresolved), not alias.
- **Documentation.** `docs/graph-query-dsl.md` updated to document the new attribute; `docs/graph-semantics.md` updated to document the new edge. The design note `docs/2026-07-19-task-mentions-design.md` already records the rationale.

## Capabilities

### New Capabilities
- `task-mentions-attribute`: the `mentions` DSL attribute and its evaluation semantics across node kinds, plus the `OwnsTask` edge that lets a `Task` reach the concepts mentioned in its owning paragraph.

### Modified Capabilities
- `graph-task-nodes`: tasks now participate in the graph's ownership model via `OwnsTask` edges from their owning paragraph (previously only `HasTask` from the note and `Subtask` from parents). The task scanner also gains skip semantics, changing which lines produce task nodes.
- `paragraph-graph`: paragraphs now own their task lines via outgoing `OwnsTask` edges (previously paragraphs had only `OwnsParagraph` incoming and `ParagraphLink` outgoing).

## Impact

- **`ft-core/src/vault.rs`** — `parse_file` task scan loop gains a `LineSkipState`.
- **`ft-core/src/graph/mod.rs`** — new `EdgeKind::OwnsTask` variant; new `insert_ownstask_edges` build step; `EdgeKind::link()` `None` arm updated.
- **`ft-core/src/graph/query/{mod,parser,eval,display}.rs`** — new `Attr::Mentions` variant, parser entry, eval arm, display name, value-type/optional/subject-validation rules.
- **`docs/graph-query-dsl.md`, `docs/graph-semantics.md`** — document the new attribute and edge.
- **Tests** — scanner skip (task in code block no longer parsed), `OwnsTask` edge creation (paragraph owns its tasks, subtasks included), `mentions` eval on each node kind (Task via owning paragraph, Paragraph direct, Note via NoteLink, Heading via HeadingLink, Ghost/Directory empty), `Profile::Tasks` query `mentions = "concept"` end-to-end.
- **No CLI/TUI/keymap changes** — the new attribute is reachable from both surfaces through the existing `Profile::Tasks` / `Profile::Default` parser. `ft commands docs --check` is unaffected.
- **No `refresh_note` changes** — task nodes (and therefore `OwnsTask` edges) are rebuilt only on full `Graph::build`, consistent with the existing `HasTask`/`Subtask` behavior. This is a pre-existing limitation, not introduced here.

## 1. Scanner-skip invariant fix

- [x] 1.1 In `ft-core/src/vault.rs::parse_file`, introduce a `markdown::LineSkipState` into the task-scan loop and `continue` past lines where `skip_line` returns `true`. Add a unit test in the `vault` module covering: task in fenced code block is not parsed, task in frontmatter is not parsed, real task after code block is parsed. Confirm `LineSkipState` is already `pub(crate)` and accessible from `vault.rs` (no visibility change needed).
- [x] 1.2 Verify existing task-scan tests still pass; update any fixture that relied on ` - [ ]` lines inside code blocks being parsed as tasks (if any). Run `cargo test -p ft-core vault` and `cargo test -p ft-core scan` if a scan module exists.

## 2. `OwnsTask` edge kind

- [x] 2.1 Add `OwnsTask` variant to `EdgeKind` in `ft-core/src/graph/mod.rs` with a docstring mirroring `HasTask` / `OwnsParagraph` (direction `Paragraph → Task`, total over tasks, distinct from `HasTask` and `Subtask`).
- [x] 2.2 Add `OwnsTask` to the `None` arm of `EdgeKind::link()` alongside the other unit variants.
- [x] 2.3 Add `"owns-task"` to `EDGE_KIND_VALUES` and `EdgeKind::OwnsTask => "owns-task"` to `edge_kind_str` in `ft-core/src/graph/query/eval.rs`. This keeps the parse-time-vs-runtime lockstep test passing.
- [x] 2.4 Implement `insert_ownstask_edges(&mut self, tasks: &[Task])` in `ft-core/src/graph/mod.rs`, mirroring `insert_hastask_edges` / `insert_subtask_edges`. Group tasks by `normalize_path(&task.source_file)`, look up the note via `path_index`, call `note_paragraphs(note_id)`, and for each paragraph match `task.source_line as u32 ∈ [paragraph.line_start, paragraph.line_end]` and add `EdgeKind::OwnsTask` from the paragraph to the task (via `task_index` lookup).
- [x] 2.5 Call `graph.insert_ownstask_edges(&scan.tasks)` from `Graph::build` after `insert_hastask_edges` and `insert_subtask_edges` (it needs both `paragraph_index` and `task_index` populated — `note_paragraphs` traverses `OwnsParagraph` edges, which are in place by then).

## 3. `Attr::Mentions` in the DSL

- [x] 3.1 Add `Mentions` variant to `Attr` in `ft-core/src/graph/query/mod.rs`. Document that it answers "does this node (or, for `Task`, its owning paragraph) link to a target whose concept identity matches the value."
- [x] 3.2 Set `Attr::value_type` for `Mentions` to `ValueType::Str`, and `is_optional` to `false` (so `is null` / `is not null` are rejected at parse time).
- [x] 3.3 Add `"mentions" => Ok(Attr::Mentions)` to `parse_attr` in `ft-core/src/graph/query/parser.rs`.
- [x] 3.4 Add `Attr::Mentions` to the node-only validation arm of `validate_attr_subject` in `parser.rs` (reject `Subject::Edge` with a `ScopeError`, accept `SelfNode` / `From` / `To`), mirroring `Path` / `Title`.
- [x] 3.5 Add `Attr::Mentions` to `attr_name` in `ft-core/src/graph/query/display.rs` returning `"mentions"`.
- [x] 3.6 Verify the parse-time op-vs-attr check in `check_op_vs_attr` accepts `=` / `!=` / `includes` / `in` on `Mentions` (it will, since `Str` type allows all of these) and that `is null` / `is not null` are rejected (they will be, since `is_optional()` returns `false`). Add a parser test for the `is null` rejection if no equivalent exists.

## 4. `mentions` evaluation

- [x] 4.1 In `ft-core/src/graph/query/eval.rs`, add an `Attr::Mentions` arm to `eval_cond_on_node` that collects the node's concept-target set via a new helper `mentioned_targets(graph, id) -> HashSet<String>` and evaluates against `=`, `!=`, `includes`, `in` (mirroring the `Tags` arm's structure for set-vs-literal handling).
- [x] 4.2 Implement `mentioned_targets(graph, id)` per the design: for `Task`, walk `incoming(OwnsTask)` to the owning paragraph then `outgoing(ParagraphLink)`; for `Paragraph`, walk `outgoing(ParagraphLink)`; for `Heading`, walk `outgoing(HeadingLink)`; for `Note`, walk `outgoing(NoteLink)`; for `Ghost` / `Directory`, return empty. For each link edge target, insert `Note.title` / `Ghost.raw` / `Heading.text` into the set.
- [x] 4.3 Implement a small helper `collect_link_targets(graph, id, out: &mut HashSet<String>)` that walks `outgoing(id)`, filters to edges where `edge.link().is_some()` (i.e. `NoteLink` / `HeadingLink` / `ParagraphLink`), and inserts the target's concept identity string. Reuse from the `Paragraph` / `Heading` / `Note` arms and from the `Task` arm after the owning-paragraph hop.

## 5. Tests

- [x] 5.1 `ft-core/src/graph/tests.rs`: add a test that `OwnsTask` edges are created for every task in a paragraph, including subtasks, and that the edge direction is `Paragraph → Task`.
- [x] 5.2 `ft-core/src/graph/tests.rs`: add a test that a top-level task has BOTH `HasTask` (from note) and `OwnsTask` (from paragraph) incoming edges.
- [x] 5.3 `ft-core/src/graph/tests.rs`: add a test that a task whose `source_line` falls outside any paragraph range receives no `OwnsTask` edge (defensive — should not occur post-scanner-fix, but the edge creation must not panic).
- [x] 5.4 `ft-core/src/graph/query/tests.rs`: add an `Attr::Mentions` eval test for each node kind — `Task` via owning paragraph (resolved + unresolved ghost target), `Paragraph` direct, `Note` via `NoteLink`, `Heading` via `HeadingLink`, `Ghost`/`Directory` empty.
- [x] 5.5 `ft-core/src/graph/query/tests.rs`: add parser tests for `mentions = "x"`, `mentions != "x"`, `mentions in {"a","b"}`, `mentions includes "x"`, and the `is null` rejection.
- [x] 5.6 `ft-core/src/graph/query/tests.rs`: add a test that `mentions` is rejected on `Subject::Edge` (e.g. `expand where edge.mentions = "x"` fails with `ScopeError`).
- [x] 5.7 `ft/tests/` integration test: `ft tasks list --query 'mentions = "onboarding" and due < today'` returns the expected tasks against a fixture vault. Add to an existing tasks integration test file or a new `ft/tests/tasks_mentions.rs`.
- [x] 5.8 Update or add snapshot tests (`insta`) for any `mentions` query output that becomes a stable format. Run `cargo insta review` if snapshots are added.

## 6. Documentation

- [x] 6.1 Update `docs/graph-query-dsl.md` to document the `mentions` attribute: value type, supported operators, per-kind semantics table, the alias non-matching rule, and a worked example under `Profile::Tasks` (`mentions = "onboarding" and due < today`).
- [x] 6.2 Update `docs/graph-semantics.md` to document the `OwnsTask` edge: direction, totality, relationship to `HasTask` and `Subtask`, and the invariant that depends on the scanner-skip fix.
- [x] 6.3 The design note `docs/2026-07-19-task-mentions-design.md` is already written; no changes needed unless the implementation diverges from the design (in which case update the design note to match).

## 7. Build invariants

- [x] 7.1 `cargo build --release` clean.
- [x] 7.2 `cargo test --workspace` clean (including the new tests from §5 and any existing tests touched by the scanner-skip change in §1).
- [x] 7.3 `cargo clippy --workspace --tests -- -D warnings` clean.
- [x] 7.4 `cargo fmt --check` clean.
- [x] 7.5 `cargo run --release -q -- commands docs --check` clean (no keymap/command changes expected, but verify).

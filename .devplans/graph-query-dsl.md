---
id: 017
name: graph-query-dsl
title: "Graph: Query DSL for subgraph selection and tree expansion"
status: finished
created: 2026-05-24
updated: 2026-05-24
---

# Graph: Query DSL for subgraph selection and tree expansion

## Goal

A small domain-specific language for querying the in-memory `Graph`.
The language has two parts that work together:

1. **Initial set** — one or more `node` blocks (unioned) that select the
   root nodes shown at distance 0.
2. **Expansion rule** — an `expand over` block that defines which
   outgoing edges to traverse and which child nodes to show when the
   user interactively expands a parent node.

The DSL ships as a library module (`ft-core/src/graph/query.rs`)
with a hand-rolled tokenizer + recursive-descent parser following the
same architecture as `ft-core/src/query/dsl.rs`. No CLI or TUI in this
plan — the parsed `GraphQuery` struct is consumed by Plan C's TUI.

## Motivation and Context

The graph foundation (plan 013) gives us `incoming`/`outgoing` and
node/edge attribute lookups. The directory nodes (plan 016) add the
file tree as a graph dimension. But there's no user-facing way to
*describe* which nodes and relationships to display. Every consumer
(currently: the CLI `backlinks`/`links` commands and the rename
planner) hard-codes its own filtering logic directly against the
`Graph` API.

A query language separates "what to show" from "how to show it." The
same DSL expression drives the TUI tree viewer, a future CLI `ft graph
query`, and any other graph consumer. The split between
initial-set/expansion maps directly onto the infinite-tree interaction
model: initial-set nodes are always visible at distance 0; expansion
governs which children appear when a node is expanded.

Why hand-rolled: the existing task DSL (~800 lines) is hand-rolled and
the graph DSL has similar predicate complexity (AND/OR/NOT
combinators, attribute matching, set membership). Copying the
architecture avoids a new dependency and keeps the error model
consistent (token-level errors pointing at the exact offending string).

## Acceptance Criteria

### Grammar (v1)

```
query        = node_block (";" node_block)* ";" expand_block? ";"

node_block   = "node" IDENT ("with" condition)+
               ["without" "(" edge_expr ")"]

edge_expr    = "edge" IDENT "(" ("_" | IDENT) "," IDENT ")"
               ("with" condition)*

expand_block = "expand" "over" IDENT "(" IDENT "," IDENT ")"
               ("with" condition)+

condition    = IDENT "." IDENT op value

op           = "=" | "!=" | "includes" | "in"

value        = literal | "{" literal ("," literal)* "}"

literal      = IDENT | STRING
```

Where `IDENT` matches `[A-Za-z][A-Za-z0-9_-]*` and `STRING` is
`"..."` or `'...'`.

Node attributes: `kind`, `path`, `title`  
Edge attributes: `kind`, `form`

Kind values: `Note`, `Directory`, `Ghost` (nodes);
`link`, `embed`, `directory-contains` (edges).  
Form values: `wiki`, `md`.

No `and`/`or`/`not` combinators in v1 — conditions inside a block are
implicitly AND'd. Multiple `node` blocks are implicitly OR'd (union).
This matches every example the user has sketched and keeps the
parser simple. Combinators are a v2 addition if needed.

### AST types

- [ ] `GraphQuery { initial: Vec<NodeSelector>, expansion: Option<ExpansionRule> }`
- [ ] `NodeSelector { var: String, conditions: Vec<Condition>, without: Option<EdgePattern> }`
- [ ] `EdgePattern { var: String, node_var: String, src_var: SourceSpec, conditions: Vec<Condition> }`
      where `SourceSpec` is `Wildcard` (for `_`) or `Named(String)`
- [ ] `ExpansionRule { edge_var: String, from_var: String, to_var: String, conditions: Vec<Condition> }`
- [ ] `Condition { var: String, attr: Attr, op: Op, value: Value }`
- [ ] `Attr` enum: `Kind`, `Path`, `Title`, `Form`
- [ ] `Op` enum: `Eq`, `NotEq`, `Includes`, `In`
- [ ] `Value` enum: `Single(Literal)`, `Set(Vec<Literal>)`
- [ ] `Literal` enum: `Ident(String)`, `String_(String)`

All types derive `Debug, Clone, PartialEq, Eq`.

### Parser

- [ ] Single-file module `ft-core/src/graph/query.rs` (no subdirectory).
      `pub mod query;` added to `ft-core/src/graph/mod.rs`.
- [ ] Tokenizer: `Token` enum covering keywords (`Node`, `With`,
      `Without`, `Edge`, `Expand`, `Over`, `In`, `Includes`), punctuation
      (`.`, `=`, `!=`, `(`, `)`, `{`, `}`, `,`, `_`, `;`), and values
      (`Ident(String)`, `String_(String)`). Case-sensitive keywords
      (lowercase: `node`, `with`, `without`, `edge`, `expand`, `over`,
      `in`, `includes`).
- [ ] Recursive-descent parser consuming `&[Token]` and returning
      `Result<GraphQuery, DslError>`.
- [ ] `DslError` with variants:
      - `UnexpectedToken { found, expected }` — what was seen vs expected
      - `UnknownIdentifier(String)` — unrecognized attribute name
      - `UnterminatedString` — missing closing quote
      - `EmptyInput` — user typed nothing
      - `TrailingTokens(String)` — content after the final `;`

### Evaluator

- [ ] `GraphQuery::select(&self, graph: &Graph) -> Result<Vec<NoteId>, EvalError>`
- [ ] `GraphQuery::expand(&self, graph: &Graph, parent: NoteId) -> Result<Option<Vec<NoteId>>, EvalError>`

`select` logic:
1. For each `NodeSelector`: iterate all `graph.nodes()`, filter by
   `with` conditions (all must match), filter by `without` (if present:
   check that NO incoming edge matches the edge pattern).
2. Union all selectors' results. Deduplication by `NoteId` (already
   `Hash + Eq`).
3. Return in insertion order (stable).

`expand` logic:
1. If no expansion rule in the query, return `Ok(None)` (nothing is
   expandable).
2. Check conditions on the parent (`from_var`): evaluate each condition
   where `var == from_var` against `graph.node(parent)` and current
   outgoing edges. If any fails, return `Ok(None)`.
3. Walk `graph.outgoing(parent)`. For each `(child_id, edge)`:
   - Evaluate conditions on the edge (`edge_var`) against `edge`.
   - Evaluate conditions on the child (`to_var`) against
     `graph.node(child_id)`.
   - If all pass, include `child_id` in the result.
4. Return `Ok(Some(children))` (may be empty vec — the node can be
   expanded but has zero children under this rule).

Condition evaluation:
- `kind` on a node: match against `NodeKind` variant name.
- `kind` on an edge: match against `EdgeKind` variant name.
- `path` on a node: match against `NoteData.path` or `DirData.path`
  (displayed as string). `Note` and `Directory` both have paths.
  `Ghost` nodes: path attribute is not present → evaluate as `""` or
  error. Decision: ghosts should not match path conditions; path match
  on a ghost returns `false` for any value.
- `title` on a node: match against `NoteData.title`. Not present on
  `Directory` or `Ghost` → returns `false` for any value.
- `form` on an edge: match against `LinkEdge.form` (WikiLink/MdLink).
  Only present on `Link` and `Embed` edges; `Contains` edges return
  `false` for any form condition.
- `=` / `!=`: string comparison (case-sensitive).
- `includes`: substring match (case-sensitive).
- `in`: set membership check.

EvalError variants:
- `UnknownNodeAttribute { attr, node_kind }` — e.g. checking `title`
  on a Directory.
- `UnknownEdgeAttribute { attr, edge_kind }` — e.g. checking `form`
  on a Contains edge.
  Actually, let's not error on attribute mismatches — just return
  `false`. This is simpler and more robust: the query author doesn't
  need to know which kinds have which attributes; conditions that don't
  apply simply don't match. The only hard errors should be parse errors
  and truly broken state (stale NoteId).

### Tests

- [ ] Unit tests in `graph/query.rs` (`#[cfg(test)] mod query_tests`):
  - Parser round-trip tests: for each example from the user, parse →
    assert AST shape → reformat → parse again (idempotent).
  - `parse_node_with_kind_equals`: `node n with n.kind = Note;` → AST.
  - `parse_node_with_kind_in_set`: `node n with n.kind in {Note, Directory};`
  - `parse_node_with_path_includes`: `node n with n.path includes "Project";`
  - `parse_node_with_title_equals`: `node n with n.title = "report";`
  - `parse_node_without_edge`: the full `without (edge e(_, n) with e.kind = directory-contains)` block.
  - `parse_node_with_multiple_conditions`: `with` blocks AND'd.
  - `parse_two_node_blocks_union`: two `node` blocks separated by `;` → two `NodeSelector`s.
  - `parse_expand_over`: `expand over e(n, m) with e.kind = link with m.kind = Note;`
  - `parse_expand_with_parent_filter`: conditions on `n`, `e`, and `m` all present.
  - `parse_no_expand`: query with only node blocks and no expand block → `expansion = None`.
  - Error cases: unterminated string, unknown keyword, missing `;`,
    unknown attribute, trailing garbage.
  - Empty input → `EmptyInput`.

- [ ] Integration-level graph tests using fixture data:
  - Use the existing `tests/fixtures/links/` vault (has links, ghosts,
    anchors) + new `tests/fixtures/dirs/` vault (has directory structure).
  - `select_all_notes`: `node n with n.kind = Note;` → returns all Note nodes.
  - `select_top_level_notes`: `node n with n.kind = Note without (edge e(_, n) with e.kind = directory-contains);` + dirs fixture → only notes not inside a directory.
  - `select_all_directories`: `node n with n.kind = Directory;` → returns root + all dir nodes.
  - `expand_directory_to_notes`: parent = Areas/ → expansion `expand over e(n, m) with n.kind = Directory with e.kind = directory-contains with m.kind = Note;` returns just `finance.md` (not the `operations/` subdir).
  - `expand_directory_to_directories`: parent = Areas/ → expansion filter `m.kind = Directory` returns `operations/`.
  - `expand_link_edges`: parent = hub.md (from links fixture) → expansion with `e.kind = link` returns all linked notes.
  - `expand_returns_none_when_parent_mismatch`: parent = a Note node → expansion with `n.kind = Directory` returns `None`.
  - `multiple_selectors_union`: two node blocks selecting disjoint sets → union includes both.

### Build invariants

- [ ] `cargo test --workspace` — all existing + new tests pass.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] `cargo fmt --check` clean.
- [ ] No new dependencies. No crate additions to the workspace.

## Technical Notes

- **Why single-file (not a subdirectory).** The task DSL grew into a
  directory (`query/` with 6 files) only after adding presets, sort
  keys, typed filters, and presets on top of the core parser. The
  graph DSL v1 has no presets, no sort keys (the TUI tree controls its
  own ordering), and no typed programmatic filter layer — just a
  tokenizer, parser, AST, and evaluator. ~600-800 lines fits a single
  file cleanly. Split into `graph/query/` when it grows.

- **Why no `and`/`or`/`not` keywords in v1.** The initial set
  naturally ORs via multiple `node` blocks. Each block's conditions
  AND implicitly. The `without` clause is the only negation. This
  covers the user's sketched examples without adding expression
  combinators. `and`/`or`/`not` can be added later as a
  condition-level expression (mirroring the task DSL's `Expr`) without
  breaking existing syntax.

- **Why attribute mismatches evaluate to `false`, not error.** A
  condition like `n.title = "foo"` on a Directory node doesn't match
  — but that's a legitimate query, not a bug. The user might write a
  `node n with n.kind in {Note, Directory} with n.path includes "Area";`
  that visits both Notes and Directories, and `path` works for both
  while `title` would be false for Directories. Erroring on attribute
  mismatch forces the query author to know type structure, which
  defeats the purpose of a uniform query language.

- **Why `in` is v1.** The user's examples all use it for kind
  selection. It's a small parser addition (brace-enclosed
  comma-separated list) that avoids the repetition of multiple `node`
  blocks for common patterns. The `in` operator is only meaningful
  for enumerated kinds; string attributes like `path` don't benefit
  from set membership (use `includes` for substring or `=` for exact).

- **Relationship to the existing task query DSL.** The graph DSL is a
  sibling module, not an extension. The task DSL evaluates against a
  `Task` record; the graph DSL evaluates against the graph topology
  (nodes + edges + adjacency). They share architecture (hand-rolled
  tokenizer + recursive-descent parser) but not types — a `Condition`
  on a graph node is fundamentally different from an `Atom` on a task.
  Future: if we add task nodes to the graph, graph queries could
  reference task attributes; that's a v2 concern.

- **Why no sort in v1.** The TUI tree viewer controls its own ordering
  (the tree structure IS the order). A future `ft graph query` CLI
  command might add `sort by path` etc., mirroring the task DSL's
  sort keys, but it's not needed for the tree interactor.

## Sessions

### Session 1 · 2026-05-24 · done
**Goal:** Tokenizer + parser. New `ft-core/src/graph/query.rs`.
Define all AST types (`GraphQuery`, `NodeSelector`, etc.). Tokenize
the grammar into `Token` enum. Implement recursive-descent parser
returning `Result<GraphQuery, DslError>`. Parser unit tests covering
every node-block shape, edge-expr shape, expand-block shape, error
cases, and all operators.
**Outcome:** All ~1000 lines written in one pass (both sessions).
New module `ft-core/src/graph/query.rs` with AST types, Lexer,
recursive-descent Parser, evaluator (`select` + `expand`), 15 parser
unit tests, 10 eval integration tests. AST: `GraphQuery`,
`NodeSelector`, `EdgePattern`, `ExpansionRule`, `Condition`,
`SrcSpec`, `Attr`/`Op`/`Value`/`Literal` enums. Lexer handles
keywords, punctuation, identifiers, strings with proper error
reporting. Parser implements the full grammar (node blocks with
unioned selectors, without-clause edge patterns, expand-over rules).
Evaluator: `select()` iterates all nodes, checks conditions per
selector, handles `without` exclusions, deduplicates results.
`expand()` checks parent conditions (from_var), then walks outgoing
edges matching edge conditions (edge_var) and child conditions
(to_var). Returns `None` when parent doesn't match expansion rule.
25 total tests. 583 workspace tests green. Clippy + fmt clean.

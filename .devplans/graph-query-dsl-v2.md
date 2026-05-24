---
id: 020
name: graph-query-dsl-v2
title: "Graph: Query DSL v2 — fix language caveats with extensive testing"
status: implementing
created: 2026-05-24
updated: 2026-05-24
---

# Graph: Query DSL v2 — fix language caveats with extensive testing

## Goal

Replace the v1 grammar from plan 017 with a corrected v2 grammar that
eliminates the eight language-design caveats discovered after the
TUI tab (plan 018) shipped: pseudo-variables that look like bindings
but aren't, no match-all form, silent type mismatches between
operators and values, substring as the only string operator, verbose
implicit-AND via repeated `with`, the asymmetric `without` clause,
silently-accepted empty initial sets, and a grammar that describes
navigation policy without naming what it *is*. Run this plan before
plan 019 (`graph-cli`) so the CLI consumes a stable target.

Ship the v2 language with an extensive test surface: complete lexer
and parser unit tests, error-message snapshot tests, evaluator tests
against the `dirs` fixture, property-based round-trip tests
(parse → Display → parse stable), and a fixture-matrix runner that
exercises every grammar production against a known graph.

The v1 grammar (plan 017) has no external consumers yet — the only
in-tree caller is the TUI tab (plan 018) and a handful of unit tests.
This is the last cheap moment to make a breaking change.

## Motivation and Context

The v1 grammar is "datalog-shaped without datalog semantics":
`node n with n.kind = Directory; expand over e(n, m) with n.kind = Directory`
reads as if the `n` in the expand block is bound to the `n` in the
node block. It isn't — they're independent name scopes the parser
compares only by string equality within a single block. A user
writing intuitive cross-block queries gets silent semantic surprises,
not errors. Every other caveat compounds the trap: `=` paired with a
set returns no rows instead of erroring, the `node n with ...` form
forces a tautological filter when "every node" is the intent, and
substring is the only string match available for paths in a
file-tree-shaped graph.

These aren't theoretical concerns. The default-query fallback chosen
in plan 019 had to write `node n with n.kind = Directory with n.path = ""`
because `node n;` doesn't parse. Plan 018's TUI tests use the same
pseudo-variable spelling that misleads first-time readers.
`docs/architecture.md` doesn't even list `graph/query.rs` (missed
documentation in plan 017). The language is shippable but not yet
*defensible* — and the CLI in plan 019 will lock it in.

Fixing it now costs one focused plan. Fixing it after the CLI ships
costs a deprecation cycle plus user-facing breakage.

## v2 Grammar (full spec)

```text
query           = node_block (";" node_block)* (";" expand_block)? ";"?

node_block      = "node" [where_clause] [neighbor_exclusion]

where_clause    = "where" condition_list

condition_list  = condition ("and" condition)*

condition       = qualified_attr op value

qualified_attr  = entity "." attribute        -- explicit, required in expand block
                | attribute                   -- bare, allowed in node block (implicit `self`)

entity          = "self"                      -- in node block (synonym for bare)
                | "from" | "to" | "edge"      -- in expand block

attribute       = "kind" | "path" | "title" | "form"
                | "indegree" | "outdegree"

op              = "=" | "!="
                | "in"                        -- right side must be a set
                | "includes"
                | "starts_with" | "ends_with"

value           = literal                     -- for =, !=, includes, starts_with, ends_with
                | "{" literal ("," literal)* "}"   -- for `in`

literal         = IDENT | STRING | INTEGER

neighbor_exclusion = "without" neighbor_filter
neighbor_filter    = "incoming" "(" [condition_list] ")"
                   | "outgoing" "(" [condition_list] ")"

expand_block    = "expand" [where_clause]
```

Key changes from v1 (mapped to the caveats):

| # | Caveat (v1)                               | Fix (v2)                                                        |
|---|-------------------------------------------|-----------------------------------------------------------------|
| 1 | Pseudo-variables `n`/`m`/`e`              | Drop variables. Use `self.X` (or bare) in node blocks; `from.X`, `to.X`, `edge.X` in expand blocks. Fixed entity names — no binding ceremony. |
| 2 | `node IDENT with ...+` forces a `with`    | `node;` matches every node. `where` clause is optional.        |
| 3 | Silent type mismatches at eval time       | Parse-time check: `=`/`!=`/`includes`/`starts_with`/`ends_with` require single literal; `in` requires a set. Clear error otherwise. |
| 4 | Substring (`includes`) only for strings   | Add `starts_with`, `ends_with`. Keep `includes` (substring).   |
| 5 | Implicit AND via repeated `with`          | One `where` keyword; conditions separated by `and`.            |
| 6 | `without (edge e(_, n) with ...)` cere... | `without incoming(...)` / `without outgoing(...)` taking a condition list directly. No `edge` keyword, no underscore wildcard, no inner variable bindings. |
| 7 | DSL described navigation, not a subgraph  | Document explicitly. Rename `ExpansionRule` → `EdgePolicy`. `walk` (plan 019) is the canonical "subgraph" consumer. |
| 8 | Empty initial set silently accepted       | Parser requires ≥1 `node` block before any `expand`.           |

Worked examples (all v2):

```
node;                                       -- every node
node where kind = Directory;                -- every directory
node where path starts_with "Projects/";    -- everything under Projects/
node where kind = Note and title includes "TODO";
node where kind in {Note, Directory};

node where kind = Directory
  without incoming(kind = directory-contains);   -- top-level dirs

node where kind = Directory;
expand where from.kind = Directory
         and edge.kind = directory-contains
         and to.kind in {Note, Directory};

node where indegree = 0;                    -- orphans
```

## Acceptance Criteria

### Lexer

- [x] New keywords tokenized: `where`, `and`, `self`, `from`, `to`,
      `starts_with`, `ends_with`, `incoming`, `outgoing`. Existing
      `node`, `expand`, `without`, `in`, `includes` retained. `with`,
      `over`, `edge`, and the underscore-wildcard token are
      **removed** (no v1 escape hatch).
- [x] `indegree`, `outdegree` are attribute names, lexed as
      identifiers — not keywords. The parser distinguishes by
      grammar position (left of `op` in a condition).
- [x] Lexer is case-sensitive. All keywords are lowercase.
- [x] Lexer unit tests cover: each keyword, each punctuation token,
      string literals (`"..."` and `'...'`), identifiers with
      hyphens (`directory-contains`), integers, whitespace handling,
      EOF, unterminated string, illegal characters. (Covered
      transitively by parser tests + dedicated error-path tests for
      string termination, illegal chars, and integer literals.)

### Parser

- [x] Recursive-descent, hand-rolled, mirroring `task::dsl` style.
- [x] Productions: `query`, `node_block`, `where_clause`,
      `condition_list`, `condition`, `qualified_attr`, `entity`,
      `attribute`, `op`, `value`, `neighbor_exclusion`,
      `neighbor_filter`, `expand_block`.
- [x] At least one `node` block required. Empty-input or
      `expand`-only input rejected at parse time with
      `DslError::NoInitialSet`.
- [x] **Parse-time op/value type checking:**
  - `=`, `!=`, `includes`, `starts_with`, `ends_with` with a set
    value → `DslError::TypeMismatch { op, expected: "literal", got: "set" }`.
  - `in` with a single literal → `DslError::TypeMismatch { op, expected: "set", got: "literal" }`.
- [x] **Parse-time attribute scope checking:**
  - Bare attribute (`kind`) inside an expand block →
    `DslError::AmbiguousAttribute { attr, hint: "use from.{attr}, to.{attr}, or edge.{attr}" }`.
  - `from` / `to` / `edge` qualifiers inside a node block →
    `DslError::ScopeError { entity, hint: "use self.{attr} or bare {attr}" }`.
  - `edge.kind` / `edge.form` valid; `edge.path` / `edge.title` /
    `edge.indegree` rejected (`DslError::ScopeError`).
  - `indegree` / `outdegree` reject `from.` / `to.` / `edge.`
    qualifiers (they're purely node properties).
- [x] Parser unit tests: every production tested for success and
      for every error path. Error messages tested via insta
      snapshots in `ft-core/src/graph/snapshots/`.

### AST

- [x] `GraphQuery { initial: Vec<NodeSelector>, expansion: Option<EdgePolicy> }`.
      `EdgePolicy` replaces v1's `ExpansionRule`; renamed to reflect
      that it's a per-hop policy, not a result definition.
- [x] `NodeSelector { conditions: Vec<Condition>, without: Option<NeighborFilter> }`.
      No more `var: String` — dropped.
- [x] `Condition { subject: Subject, attr: Attr, op: Op, value: Value }`.
      `Subject` enum: `SelfNode` (node block) | `From` | `To` | `Edge`
      (expand block).
- [x] `NeighborFilter { direction: Direction, conditions: Vec<Condition> }`.
      `Direction` is `Incoming` | `Outgoing`. Conditions inside use
      implicit subject `Edge`.
- [x] `Attr` gains `Indegree`, `Outdegree`.
- [x] `Op` gains `StartsWith`, `EndsWith`. (`Includes` retained.)
- [x] All AST types `Debug + Clone + PartialEq + Eq`.

### Evaluator

- [x] `GraphQuery::select(&self, graph: &Graph) -> Vec<NoteId>` —
      same signature as v1, new internals. Walks node table once
      per selector, applies bare conditions (subject = self),
      applies `without` neighbor filter.
- [x] `GraphQuery::expand(&self, graph: &Graph, parent: NoteId) -> Option<Vec<NoteId>>`
      — same signature as v1. Returns `None` only if no expand
      block. Returns `Some(vec![])` when the policy is present
      but `parent` doesn't satisfy `from` conditions OR no outgoing
      edges match.
- [x] **Behavior change vs v1:** when the parent fails its `from`
      conditions, v1 returned `None`; v2 returns `Some(vec![])`.
      Rationale: `None` = "no policy"; "policy says zero children
      here" should be empty children. This makes the TUI's
      "expandable" flag derivable from the policy alone, not from
      the parent's match status.
- [x] `indegree` / `outdegree` evaluation: count incoming /
      outgoing edges. Integer comparison ops: `=`, `!=`, `in`. (No
      `<` / `>` yet — see §future.)
- [x] Evaluator unit tests against `tests/fixtures/dirs`:
  - Every attribute × every op × success and miss
  - `starts_with` / `ends_with` / `includes` on paths and titles
  - `indegree = 0` returns vault root only
  - `kind in {Note, Directory}` returns notes ∪ dirs
  - Multiple node blocks union without duplicates
  - `without incoming(kind = directory-contains)` returns top-level

### Display (canonical serialization)

- [x] `impl Display for GraphQuery` produces canonical text:
  - Single line per block (`;`-separated), terminator `;` always
    present
  - `and`-separated conditions on one line
  - Strings double-quoted with internal `"` and `\` escaped
  - Sets formatted as `{a, b, c}` (space after comma, no trailing
    comma)
  - Sorted set elements? **No** — preserve user order. Sorting
    would lose author intent and complicate proptest shrinking.
- [x] `Display` for `Condition`, `NeighborFilter`, `NodeSelector`,
      `EdgePolicy` also implemented, composed into `GraphQuery`.
- [ ] Round-trip property test: `parse(format!("{}", q)).unwrap() == q`
      for every value `q` the proptest generator produces.
      *(Deferred to session 2 — example-based round-trip tests
      already in place.)*

### Error type

- [x] `DslError` variants (replacing v1's set):
  - `EmptyInput`
  - `NoInitialSet` (parsed but no `node` block)
  - `UnexpectedToken { found, expected }`
  - `UnknownAttribute { attr }`
  - `AmbiguousAttribute { attr, hint }`
  - `ScopeError { entity, hint }`
  - `TypeMismatch { op, expected, got }`
  - `UnknownKindValue { attr, value, allowed }` — e.g.
    `kind = Notes` (plural) suggests `Note`
  - `TrailingInput { token }`
  - `UnterminatedString`
  - `IllegalCharacter { ch }` *(added beyond plan list for
    completeness)*
- [x] `Display for DslError` produces a one-line user-facing
      message with position.
- [x] Error message snapshot tests via insta: one snap per variant,
      covering both content and position.

### Property tests (proptest)

- [ ] Generator: random valid `GraphQuery` AST values with bounded
      depth (1–3 node blocks, 0–4 conditions each, 0–1 `without`,
      with/without `expand`).
- [ ] **Round-trip:** `parse(format!("{q}")) == Ok(q)` for every
      generated `q`. This is the central correctness invariant.
- [ ] **Stability:** `parse(format!("{}", parse(format!("{q}")).unwrap()))`
      equals the first parse (two passes are idempotent).
- [ ] **Whitespace insensitivity:** for every generated `q`,
      `parse(canonical)` equals `parse(canonical_with_extra_whitespace)`.
- [ ] **Invalid-input generator:** generates strings that violate
      one specific grammar rule each; assert `parse` returns the
      matching `DslError` variant. (Sanity: error mapping is
      complete.)

### Fixture-matrix test runner

- [ ] New `ft-core/tests/graph_query_matrix.rs` integration test.
- [ ] New `ft-core/tests/fixtures/graph_queries/` directory holding
      one file per query case:
  ```
  graph_queries/
    01-all-nodes.dsl          (query source)
    01-all-nodes.expected     (expected NoteId paths, one per line)
    02-top-level-dirs.dsl
    02-top-level-dirs.expected
    ...
  ```
- [ ] Test runner reads every `.dsl` file under the directory,
      runs `parse → select` against the `dirs` fixture vault, and
      compares the sorted set of resulting node paths to the
      `.expected` file. Mismatches print a unified diff.
- [ ] Initial matrix (≥15 cases) covers:
  - `node;` → all nodes
  - Every kind individually
  - Every op × {kind, path, title}
  - `starts_with` on hierarchical paths
  - `in` with various set sizes (1, 2, 5)
  - `indegree = 0`, `outdegree = 0`
  - `without incoming(kind = ...)` patterns
  - Multiple node blocks unioned
  - Expand-block round trip: build tree, assert frontier
  - Edge cases: empty result, single-node result
- [ ] Adding a new case is one new `.dsl` + one new `.expected`
      file — no test code change. Onboarding new contributors to
      "test your case" is a one-line README in the fixtures dir.

### Documentation

- [x] New `docs/graph-query-dsl.md` — the canonical language
      reference. Mirrors `docs/query-dsl.md` for tasks. Includes:
      grammar (verbatim from this plan), attribute/op/value
      compatibility matrix, worked examples from real vault-shape
      queries, error message catalog. Linked from
      `docs/architecture.md` and the `--help` output of the
      (future) `ft graph query` CLI.
- [x] `docs/architecture.md` updated: added `graph/query.rs` to the
      workspace layout; added a "Graph query DSL" subsection
      naming `select`/`expand`/`walk` and pointing at the new
      reference doc.
- [x] Inline doc-comment on `GraphQuery` explicitly states: "The
      DSL describes a *navigation policy*, not a subgraph. Initial
      nodes come from `select`; per-hop expansion comes from
      `expand`. To materialize a finite subgraph, use `walk` (plan
      019) — it composes the two with depth and cycle bounds."

### Migration of existing consumers

- [x] `ft/src/tui/tabs/graph.rs` — updated the test queries:
  - `dirs_query()` rewritten in v2 form
  - All inline test query strings updated
- [x] `ft-core/src/graph/query.rs` — replaced v1 grammar entirely;
      no backwards-compat shim.
- [ ] Plan 019 (`graph-cli`) — update the default-query examples
      and the TUI fallback default. *(Already done while authoring
      plan 019 in v2 form; will re-verify in session 2.)*

### Build invariants

- [x] `cargo test --workspace` — all existing + new tests pass
      (1100+ tests).
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [x] `cargo fmt --check` clean.
- [x] No new runtime dependencies.

## Technical Notes

### Why drop variables entirely instead of making them bind

Two alternatives existed:

1. **Drop variables.** Use fixed entity names `self`, `from`, `to`,
   `edge`. Each condition's subject is statically derivable from its
   position; there's no scope to manage.

2. **Make variables truly bind.** A `node n` defines `n` as a value
   carried into the expand block: `expand over e(n, m)` means
   `n` is constrained to nodes the selector matched.

Option 2 is more expressive (it would naturally support
multi-selector relational queries) but the implementation cost is
significant — variable scoping, name-shadowing rules, scope
diagnostics, and an evaluator that joins instead of iterating. Option
1 covers every concrete use case the user has sketched and matches
how Cypher's property-access patterns work (`MATCH (n)-[r]->(m)` uses
named variables, but property access via `n.prop` is positional, not
binding-dependent). If future queries genuinely need joins,
introduce variables as a v3 additive — at which point a real binder
is justified.

### Why parse-time type checking instead of eval-time

Eval-time silent-failure is one of the original eight caveats.
Catching `=` paired with a set at parse time means the error
surfaces at the moment the user types the query, not "no results
appeared, must be my data." Parse-time also keeps the eval hot path
branch-free: it doesn't need to handle nonsense combinations because
the AST can't represent them.

### Why `starts_with` / `ends_with` and not glob / regex

A path predicate gets used in 80% of real queries. `starts_with` /
`ends_with` cover the common case ("everything under Projects/")
with no new sublanguage (no `*`, no `%`, no regex metachar
escaping). Glob is a substantial spec (anchoring, character
classes, escape rules); regex is a dependency surface (Rust's
`regex` crate isn't currently in the tree). Defer both. If glob is
ever needed, add it as a single `matches` op with a documented
glob subset.

### Why `incoming(...)` / `outgoing(...)` instead of keeping `without (edge e(_, n) with ...)`

The v1 syntax served a single use case ("filter roots that have a
specific incoming edge"). It cost a whole sub-grammar (`edge`
keyword, `_` wildcard, paired variable names). The v2 shape removes
all of that: `without incoming(kind = directory-contains)` reads
naturally and the inner condition list is identical to what appears
elsewhere. The grammar shrinks; the use case is preserved.

`incoming` / `outgoing` are also future-proof: they can appear as
positive filters too (`node where indegree(kind = link) > 0` — the
future degree-with-filter form), but that's deferred (§future).

### Why a fixture-matrix runner

Test files that are *data*, not Rust code, make adding a new query
case zero-overhead. The PARA-style vault in `tests/fixtures/dirs` is
already shared by every prior graph plan, so the same fixture covers
every case. Diff-based comparison shows exactly what changed when a
case breaks, instead of a wall of `assert_eq!` output.

### Snapshot tests for errors

Error messages are part of the user interface — wording matters,
positions matter, hints matter. insta snapshots make every error
message a reviewable artifact. Wording changes show up in PR diffs.
The task DSL already does this; copy the pattern.

### Renaming `ExpansionRule` → `EdgePolicy`

The v1 name "rule" suggests "what to compute"; "policy" suggests
"a per-hop decision." That's what it actually is — the evaluator
asks it "for parent X, which children?" not "what does the result
look like?". Caveat #7 (DSL describes navigation, not subgraph) is
half a naming problem; this rename addresses it.

### Why hand-rolled (no parser library)

Same call as plan 017 and the task DSL (`ft-core/src/query/dsl.rs`):
hand-rolled recursive-descent, no `nom`/`chumsky`/`pest`/`lalrpop`.
Three reasons specific to v2:

1. **Consistency.** The task DSL is already hand-rolled at ~800
   lines. A second parser in a different style is a permanent
   cognitive tax on every contributor. Migrating both to a library
   would expand scope into a finished, working plan.
2. **Grammar size.** v2 has ~12 productions, no left recursion, no
   precedence beyond a flat binary-op list, no ambiguity. Libraries
   pay off at larger scale or with features this grammar doesn't
   need.
3. **Bespoke error model.** `DslError` variants like
   `AmbiguousAttribute { attr, hint }` and `ScopeError { entity,
   hint }` carry structured, actionable hints — not "expected X,
   found Y". Generic libraries would need a translation layer
   anyway; hand-rolled goes straight to the shape consumers want.

If session 1 finds error authoring genuinely painful, `chumsky` is
the library to reach for — but only via a follow-up plan that
migrates both DSLs together. Revisit if v3 ever adds `or`/`not`
combinators with precedence, ordered comparisons, or expression
nesting; at that scale a library starts paying its weight.

### Why no v1 compatibility shim

Real users of the v1 DSL: zero (the language is two commits old,
shipped only inside the TUI tab, which the user reports as broken
anyway). In-tree references: the TUI tab tests and the future CLI
plan 019 examples. Maintaining two parsers, two grammars, and two
error message sets for zero external users is the wrong trade-off.
Hard break, single grammar, comprehensive test coverage.

## Future (explicitly out of scope)

- **Numeric comparisons (`<`, `>`, `<=`, `>=`).** `indegree > 0`,
  `outdegree <= 3`. Adds ordered comparison to the Op enum. Small
  addition once `indegree`/`outdegree` are in place; defer to keep
  this plan's surface tight.
- **Real variable bindings + joins.** Datalog-style cross-block
  unification. Powerful but the use cases are conjectural; add only
  if a concrete need surfaces.
- **`not` combinator.** Negation as a first-class operator
  (`where not (kind = Note)`). The current `without incoming(...)`
  covers the neighbor case; bare condition negation is rare in
  sketched queries.
- **`or` combinator inside a block.** Plan 017 deferred this; still
  deferred. Multiple node blocks already give union; intra-block
  `or` is conjectural.
- **Glob / regex on string ops.** Add `matches` with a documented
  subset only if a concrete user query genuinely needs it.
- **Aggregation / projection.** `count(...)`, `group by`, `select
  attr1, attr2`. The DSL is structural, not relational; aggregation
  belongs to consumers (CLI report subcommands, future TUI tabs),
  not the language.
- **Filtered degree predicates.** `indegree(kind = link) > 0`
  (count only matching edges). Useful but adds a new
  attribute-with-args form. Defer.

## Sessions

### Session 1 · 2026-05-24 · done
**Goal:** v2 lexer + parser + AST + evaluator + Display + DslError
variants. Replace v1 grammar entirely. Migrate in-tree test queries.
Unit tests: every grammar production (success + error), every
attribute × op pairing, every evaluator predicate against the `dirs`
fixture, every `DslError` variant via insta snapshots. Update
`docs/architecture.md` to list `graph/query.rs`. Write
`docs/graph-query-dsl.md` as the canonical reference.
**Outcome:** `ft-core/src/graph/query.rs` rewritten from scratch
(~1300 lines incl. tests). New AST: `GraphQuery`, `NodeSelector`,
`EdgePolicy` (renamed from `ExpansionRule`), `NeighborFilter`,
`Condition { subject, attr, op, value }`, plus enums `Subject`
(SelfNode/From/To/Edge), `Attr` (+ Indegree/Outdegree), `Op` (+
StartsWith/EndsWith), `Direction`. Pseudo-variables dropped. Lexer
gains `where`/`and`/`self`/`from`/`to`/`starts_with`/`ends_with`/
`incoming`/`outgoing` keywords; `with`/`over`/`edge`/underscore-
wildcard removed. Parser performs parse-time op/value type checks,
attribute scope checks, kind/form value checks. 10 DslError variants
with byte-position tracking. `Display` for `GraphQuery` produces
canonical text that round-trips (`from`/`to`/`edge` always
qualified; `self.X` collapses to bare `X`). Evaluator behavior
change: `expand` returns `Some(vec![])` when parent fails from-
conditions, not `None`. TUI tab tests (`ft/src/tui/tabs/graph.rs`)
migrated to v2 grammar. 83 lib tests for the DSL alone (35 parser /
13 display+round-trip / 17 evaluator / 18 error-message insta
snapshots). Full workspace test suite green (1100+ tests). Clippy
+ fmt clean. Wrote `docs/graph-query-dsl.md` (full grammar,
attribute matrix, op semantics, error catalog, worked examples).
Updated `docs/architecture.md` to list `graph/query.rs` and added
a "Graph query DSL" subsection pointing at the reference.

### Session 2 · planned
**Goal:** Extensive testing. Proptest round-trip + stability +
whitespace-insensitivity invariants. Invalid-input generator with
DslError-variant coverage assertion. Fixture-matrix runner under
`ft-core/tests/graph_query_matrix.rs` with `tests/fixtures/graph_queries/`
data dir holding ≥15 `.dsl` + `.expected` pairs. README in the
fixtures dir documenting how to add a case. Update plan 019's plan
file to reference v2 grammar in its examples and default-query
fallback. 

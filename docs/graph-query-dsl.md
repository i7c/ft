# Graph Query DSL

`ft` ships a small domain-specific language for querying the in-memory
note graph. It powers the TUI graph tab (plan 018, fixed in plan 019)
and the `ft graph query` CLI (plan 019).

The DSL describes a **navigation policy**, not a result subgraph:

- **`select`** — which nodes form the initial set (distance 0).
- **`expand`** — for each parent, which outgoing edges to follow and
  which child nodes to keep on the next hop.

Consumers compose those two with their own depth and cycle handling.
The interactive tree (TUI) expands one hop per keystroke; the CLI
materializes a finite subgraph via `walk` (depth bound + cycle stop).

## Profiles

The parser entry point is `parse_with(src, profile, today)`. The
`Profile` controls how much sugar is applied:

- **`Profile::Default`** — the verbose graph syntax shown in this
  doc. Every query starts with an explicit `node` block. This is what
  `ft graph query` and the TUI Graph tab use.
- **`Profile::Tasks`** — used by `ft tasks list <query>` and the TUI
  Tasks tab query bar. A bare predicate list is wrapped in an implicit
  `node where kind = Task and …` prelude, so you can type
  `priority = High` or `due < today` without the `node where kind =
  Task and` prefix. Bare attribute references default to `self`.

`parse(src)` is `parse_with` defaulted to `Profile::Default` and the
system clock. Enum values (`Open`, `High`, `Note`, …) are
case-insensitive on the parser side; canonical `Display` capitalises
them. Sort and limit are CLI flags (`--sort`, `--limit`), not part of
the DSL — task queries that used to embed `sort by` / `limit` now use
the flags (see [migrating-task-queries.md](migrating-task-queries.md)).

## Grammar

```text
query           = node_block (";" node_block)* (";" expand_block)? ";"?

node_block      = "node" [where_clause] [neighbor_exclusion]

where_clause    = "where" condition_list

condition_list  = condition ("and" condition)*

condition       = qualified_attr op value

qualified_attr  = entity "." attribute       -- explicit
                | attribute                  -- bare; implicit `self`

entity          = "self" | "from" | "to" | "edge"

attribute       = "kind" | "path" | "title" | "form"
                | "indegree" | "outdegree"

op              = "=" | "!=" | "in" | "includes"
                | "starts_with" | "ends_with"

value           = literal | "{" literal ("," literal)* "}"

literal         = IDENT | STRING | INTEGER

neighbor_exclusion = "without" neighbor_filter
neighbor_filter    = "incoming" "(" [condition_list] ")"
                   | "outgoing" "(" [condition_list] ")"

expand_block    = "expand" [where_clause]
```

- All keywords are lowercase. Case matters: `Note` (a kind value) is
  not `note` (the keyword).
- Identifiers: `[A-Za-z][A-Za-z0-9_-]*`. Hyphens are allowed, so edge
  kind values like `directory-contains` are bare identifiers.
- Strings: double-quoted `"..."` or single-quoted `'...'`. Escapes:
  `\\`, `\"`, `\'`, `\n`, `\t`.
- Integers: bare digits, used for `indegree` / `outdegree`.

## Entities and attributes

Conditions reference one of four entities, depending on the block they
appear in:

| Entity | Where valid                  | Notes                                |
|--------|------------------------------|--------------------------------------|
| `self` | node block (bare = self)     | The candidate node.                  |
| `from` | expand block                 | The parent node of the candidate edge.|
| `to`   | expand block                 | The child node (destination).        |
| `edge` | expand block, neighbor filter| The edge itself.                     |

Attribute compatibility:

| Attribute   | Subjects         | Description                                |
|-------------|------------------|--------------------------------------------|
| `kind`      | self, from, to, edge | Node kind or edge kind — see [Kind values](#kind-values) |
| `path`      | self, from, to   | Vault-relative path (notes and directories) |
| `title`     | self, from, to   | Note title (first heading or filename stem) |
| `form`      | edge             | `wiki` or `md` (link form, link/embed edges only) |
| `indegree`  | self only        | Count of incoming edges (integer)          |
| `outdegree` | self only        | Count of outgoing edges (integer)          |

`indegree` / `outdegree` are selection-time properties only — they
cannot appear in `expand` (which is per-hop) or in neighbor filters.

## Operators

| Op            | Value shape    | Applies to     | Semantics                          |
|---------------|----------------|----------------|------------------------------------|
| `=`           | single literal | any            | Exact match                        |
| `!=`          | single literal | any            | Not equal                          |
| `in`          | set            | any            | Membership in the set              |
| `includes`    | single literal | strings        | Substring match                    |
| `starts_with` | single literal | strings        | Prefix match                       |
| `ends_with`   | single literal | strings        | Suffix match                       |

**Type checking is at parse time.** Pairing `=` with a set or `in`
with a single literal is a `TypeMismatch` error, not a silent "zero
results."

For `indegree` / `outdegree`, only `=`, `!=`, and `in` are meaningful
(string operators always return false on integers).

## Kind values

| Attribute            | Allowed values                                |
|----------------------|-----------------------------------------------|
| `self.kind` / `from.kind` / `to.kind` | `Note`, `Directory`, `Ghost`, `Task`, `Paragraph` |
| `edge.kind`          | `link`, `embed`, `directory-contains`, `has-task`, `subtask`, `links-into`, `owns-paragraph`, `paragraph-link` |
| `edge.form`          | `wiki`, `md`                                  |

`has-task` runs note → task; `subtask` runs parent task → child task
(indentation-derived, always intra-file). Both point from container to
contained, like `directory-contains`, so the same expand/walk machinery
follows them.

Unknown values fail at parse time with a hint listing the allowed set.

## Worked examples

```dsl
node;                                       -- every node in the graph
node where kind = Directory;                -- every directory
node where path starts_with "Projects/";    -- everything under Projects/
node where kind = Note and title includes "TODO";
node where kind in {Note, Directory};
node where indegree = 0;                    -- orphans
node where outdegree = 0;                   -- leaves
```

Top-level directories (no parent directory):

```dsl
node where kind = Directory
  without incoming(kind = directory-contains);
```

Full directory tree (initial set = vault root, expand follows
`directory-contains` edges to notes and subdirectories):

```dsl
node where indegree = 0;
expand where from.kind = Directory
         and edge.kind = directory-contains
         and to.kind in {Note, Directory};
```

Notes only inside a specific area:

```dsl
node where kind = Note and path starts_with "Areas/finance/";
```

All notes' outbound link graph (initial set: every note, expand over
link edges):

```dsl
node where kind = Note;
expand where edge.kind in {link, embed};
```

A task tree (initial set: matching top-level tasks, expand follows
`subtask` edges down through every nested level):

```dsl
node where kind = Task and priority = High;
expand where edge.kind = subtask;
```

Note → task → subtask in one walk (every note's full task hierarchy):

```dsl
node where kind = Note;
expand where edge.kind in {has-task, subtask};
```

## Errors

| Variant              | Condition                                            |
|----------------------|------------------------------------------------------|
| `EmptyInput`         | Source is whitespace-only.                           |
| `NoInitialSet`       | Parsed, but no `node` block before `expand`.         |
| `UnexpectedToken`    | Token doesn't match the expected production.         |
| `UnknownAttribute`   | Attribute name not in the table above.               |
| `AmbiguousAttribute` | Bare attribute in an `expand` block (needs `from.`/`to.`/`edge.`). |
| `ScopeError`         | Entity prefix invalid in this block, or attribute incompatible with the entity. |
| `TypeMismatch`       | `=`/`!=`/`includes`/`starts_with`/`ends_with` paired with a set, or `in` paired with a single literal. |
| `UnknownKindValue`   | Value for `kind`/`form` not in the allowed set; error lists the allowed values. |
| `TrailingInput`      | Extra tokens after the query ends.                   |
| `UnterminatedString` | String literal not closed.                           |
| `IllegalCharacter`   | Character that can't start a token (e.g. `@`).       |

Every variant carries a byte position pointing at the offending token
or character.

## Canonical form

`GraphQuery` implements `Display`. The serialized form is canonical:
re-parsing it yields the same AST (`parse(format!("{q}")) == Ok(q)`).
The serializer:

- Always qualifies `from` / `to` / `edge` subjects (`from.kind`, not
  `kind`), to round-trip safely from `expand` blocks where bare
  attributes are ambiguous.
- Collapses `self.X` to bare `X` in node blocks.
- Preserves the user's set element order.
- Escapes `\`, `"`, `\n`, `\t` inside string literals.

## Relationship to the task DSL

The task DSL (`docs/query-dsl.md`) and the graph DSL share a hand-rolled
recursive-descent style but are otherwise independent. They evolve
separately; sharing parser code would couple them in ways that don't
match how users think about tasks vs. graphs.

## Context

The task DSL (`ft-core/src/query/dsl.rs`) was the first DSL in the project (sessions 4–8 of the original tasks plan). The graph DSL (`ft-core/src/graph/query.rs`) came later, was initially shaped as a graph-traversal policy language, and grew task attributes during `graph-task-nodes` and `harden-graph-task-nodes`. Today, the predicate vocabulary on tasks is almost identical between the two:

| Task DSL | Graph DSL | Notes |
| --- | --- | --- |
| `status is X` | `self.status = X` | Identical semantics |
| `priority is X` | `self.priority = X` | Identical |
| `tag is T` / `has tag T` | `self.tags includes T` | Identical |
| `path includes S` | `self.path includes S` | Identical |
| `description includes S` | `self.description includes S` | Identical |
| `due before D` | — | Missing in graph DSL |
| `due after D` / `due on D` | — | Missing |
| `today` / `tomorrow` value | — | Missing |
| `has due` / `no due` | — | Missing |
| `done` / `not done` | — | Missing (needs `status in {…}`) |
| `sort by due reverse` | — | Missing — moves to CLI |
| `limit N` | — | Missing — moves to CLI |

Three operator gaps, one value gap, two semantic shortcuts, and two cross-cutting clauses. After this change, the table collapses to one column.

The `Profile` concept is the key UX move. Without it, users on the tasks tab would have to type the verbose graph syntax. With it, they keep typing what they're used to, and the parser unwraps it into the canonical form.

## Goals / Non-Goals

**Goals:**

- One DSL engine. Delete `query/dsl.rs`.
- Comparison operators (`<`/`<=`/`>`/`>=`) for Integer and Date values.
- `Date` value type with `YYYY-MM-DD`, `today`/`tomorrow`/`yesterday`, and relative `+Nd`/`-Nw`/`-Nm`.
- `is null` / `is not null` for optional attributes.
- `Profile::Tasks` desugaring: implicit `kind = Task` and implicit `self.<attr>`.
- Tasks built-in presets re-expressed in the graph DSL.
- `sort` and `limit` removed from the DSL grammar (already available as CLI flags).
- Hard break on user-visible task query syntax — documented migration.

**Non-Goals:**

- No surface-syntax compat with the old task DSL. No `is` alias for `=`, no `before`/`after` aliases.
- No sort/limit retained in the DSL.
- No third profile in this change.
- No new attributes beyond what graph DSL already has.

## Decisions

### Comparison operators are scope-checked

```rust
enum Op {
    Eq, NotEq, In, Includes, StartsWith, EndsWith,
    Lt, Le, Gt, Ge,                      // new
    IsNull, IsNotNull,                   // new (postfix, no rhs value)
}
```

`Lt` / `Le` / `Gt` / `Ge` are valid only for:

- Integer attrs: `indegree`, `outdegree`.
- Date attrs: `due`, `scheduled`, `created`, `start`, `completed`.

A parse-time validator rejects `self.title < "x"` with a `TypeMismatch` error pointing at the operator span.

### `Date` value type with relative literals

```rust
enum Value {
    Single(Literal),
    Set(Vec<Literal>),
}

enum Literal {
    Ident(String),       // enums like Task, Done, High
    String(String),
    Integer(i64),
    Date(NaiveDate),     // new
}
```

`Date` literals are parsed via `ft_core::dates::parse_date_value` which accepts:

- `YYYY-MM-DD`
- `today`, `tomorrow`, `yesterday`
- `+Nd`, `-Nd`, `+Nw`, `-Nw`, `+Nm`, `-Nm` (resolved against `FT_TODAY` or today)

The parser disambiguates `today` from an `Ident` by attribute type at parse time: the right-hand-side of a `Date` attribute is parsed in "date mode" and `today` resolves to a date literal there. Outside that context, `today` is an unknown identifier (error).

### Nullability ops are postfix and value-less

```text
due is null
due is not null
```

Parser: `is` followed by `null` or `not null` becomes an `IsNull` or `IsNotNull` op with no rhs `Value`. Only valid on optional attributes (`due`, `scheduled`, `created`, `start`, `completed`). Scope-checked.

**Alternative considered: `exists`/`missing` keywords.** Rejected — `is null` reads better in compound expressions (`due is null and priority = high`).

### `Profile::Tasks` desugaring rules

Two transformations applied during parsing when `profile == Tasks`:

1. **Implicit initial node block.** If the source string does not start with `node` (after optional whitespace), the parser injects `node where kind = Task and ` before the first predicate. Example: `priority = high` → parsed as `node where kind = Task and self.priority = high`.
2. **Implicit `self.` subject on bare attributes.** Inside a Tasks-profile query, an attribute reference without an explicit subject defaults to `Subject::SelfNode`. So `priority = high` is the same as `self.priority = high`.

These are pure desugarings — the AST after parsing is identical to what the verbose form would have produced. Roundtrip (`serialize(parse(s))` under `Profile::Default`) produces the canonical verbose form; users see the verbose form in error messages.

**Alternative considered: a separate `TaskQuery` AST.** Rejected — divergent AST means divergent evaluator, which is exactly what we're collapsing.

### Sort and limit move to CLI flags

`ft tasks list` already supports `--sort due,-priority` and `--limit 10`. The DSL parser stops at the boolean expression; sort and limit are command-line concerns. The TUI tasks tab uses its own UI control for sorting (existing behaviour). The query string is now purely a filter.

The shared `query::sort::SortKey` machinery survives — it's how the CLI applies `--sort`. Only its DSL entry point is removed.

### Built-in presets re-expressed

```rust
pub fn builtin(name: &str) -> Option<&'static str> {
    Some(match name {
        "today" => "(status in {Open, InProgress}) and (due = today or scheduled = today)",
        "overdue" => "(status in {Open, InProgress}) and due < today",
        "upcoming" => "(status in {Open, InProgress}) and due > today",
        "done-today" => "status = Done and completed = today",
        "not-done" => "status in {Open, InProgress}",
        _ => return None,
    })
}
```

These are parsed under `Profile::Tasks` so the implicit `node where kind = Task and …` wrapper applies. `ft tasks list today` calls `parse(builtin("today"), Profile::Tasks, today)`.

### Filter composition unchanged

CLI flags (`--status open`, `--due-before today`, `--tag work`) still produce a `Filter`. The `Filter` is compiled into a `Vec<Condition>` and AND-ed with the user's parsed query expression. This is the same composition pattern as today (`expr.matches(&task)` AND-ed with `filter.matches(&task)`); the only change is that `Filter` now produces `Condition`s of the new shape.

### Migration guide

`docs/migrating-task-queries.md` ships with this change. It's a table:

| Old | New |
| --- | --- |
| `status is open` | `status = Open` |
| `priority is high` | `priority = High` |
| `tag is foo` / `has tag foo` | `tags includes foo` |
| `due before today` | `due < today` |
| `due after 2026-01-01` | `due > 2026-01-01` |
| `due on tomorrow` | `due = tomorrow` |
| `done` | `status = Done` |
| `not done` | `status in {Open, InProgress}` or `--preset not-done` |
| `has due` | `due is not null` |
| `no due date` | `due is null` |
| `sort by due reverse` | CLI flag `--sort -due` |
| `limit 10` | CLI flag `--limit 10` |

Plus a note on the enum-value casing convention (`Open`, `High`, `Done` — capitalized, matching the existing graph DSL).

## Risks / Trade-offs

- **[Hard break breaks user scripts and habits]** → Accepted per Q3.2 / Q4 direction. The migration guide and the four-line diff for the average user query make this manageable. No deprecation aliases.
- **[Enum casing change (`open` → `Open`)]** → The graph DSL parser is case-insensitive on enum idents (verified by reading the existing parser's value resolution); document this explicitly so users don't think they need to change case.
- **[`today` keyword as a date literal only in date-attr context]** → A bare `today` outside a date context would be `Ident("today")`, which is currently rejected as an unknown enum. We make this explicit: `self.title = today` errors with "title is a string attribute; date keywords are only valid for date attributes (`due`, `scheduled`, `completed`, …)".
- **[`Profile::Tasks` makes errors more surprising]** → Errors for Tasks-profile queries refer to the desugared form. Mitigation: include both the original source and the desugared form in the error message ("error in `due < today` (desugared to `self.due < today`): …").
- **[Removing sort/limit from DSL breaks one CLI surface]** → `ft tasks list 'status is open sort by due'` no longer parses. The CLI flag form (`--sort due`) is the replacement. Documented in the migration guide.
- **[Deleting `query/dsl.rs` is irreversible]** → It is. The `query` module survives with `filter.rs`, `preset.rs`, `sort.rs`. The DSL parser is gone.

## Open Questions

- Should we keep `path includes "Areas/"` working with the same exact syntax in Tasks profile? **Leaning:** yes — already identical between the two DSLs.
- Should we expose `--profile tasks` on `ft graph query` for ad-hoc task subgraph queries? **Leaning:** yes — it's free once the profile mechanism exists, and it's useful (`ft graph query --profile tasks 'priority = high'` lists task nodes by graph traversal rules).
- Should the canonical serializer output the verbose form (always `self.<attr>`, always `node where kind = Task and …`) or preserve the profile sugar? **Leaning:** always verbose — round-trips and machine-generated queries should be unambiguous. Users keep typing the sugared form.

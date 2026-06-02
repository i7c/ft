## Why

ft has two hand-rolled query DSLs that solve overlapping problems with zero shared infrastructure: `ft-core/src/query/dsl.rs` (812 lines, task queries) and `ft-core/src/graph/query.rs` (3,320 lines, graph navigation policy). The graph DSL already knows about task nodes and exposes task attributes (`status`, `priority`, `due`, `scheduled`, `description`, `tags`). The only reason the task DSL still exists is three missing pieces in the graph DSL:

1. No date comparison operators (`<`, `<=`, `>`, `>=`).
2. No `Date` value type with keyword literals (`today`, `tomorrow`, `+3d`).
3. No nullability ops (`is null` / `is not null`).

Plus a UX gap: the task tab would need users to type `node where kind = Task and self.priority = high` instead of `priority = high`. That's the "Profile" mechanism this change adds.

With those three operators + the `Date` value + a `TasksProfile`, the graph DSL fully subsumes the task DSL. We then delete the task DSL, migrate built-in presets to graph DSL strings, and relocate `sort` and `limit` from the DSL into CLI flags (where they already exist as `--sort` / `--limit` on `ft tasks list`). One engine, one error catalog, one docs file, one parser.

This change sequences after `text-input-ux` so users get the better editing experience when their query strings change.

## What Changes

### Graph DSL extensions

- New operators: `<`, `<=`, `>`, `>=`. Applicable to `Integer` (indegree / outdegree) and `Date` value types. Scope-checked at parse time: comparison on a non-comparable attribute is a clear parse error.
- New `Date` value type with: `YYYY-MM-DD` literal; keywords `today`, `tomorrow`, `yesterday`; relative literals `+Nd`, `-Nd`, `+Nw`, `-Nw`, `+Nm`, `-Nm`. Parsed through `ft_core::dates`, honours `FT_TODAY` for reproducibility.
- New nullability ops: `is null`, `is not null`. Applicable to optional attributes (`due`, `scheduled`, `created`, `start`).
- The existing operator set (`=`, `!=`, `in`, `includes`, `starts_with`, `ends_with`) and value types (string, integer, set) are unchanged.

### Profile

- New `Profile` enum: `Default`, `Tasks`. The parser takes a profile parameter.
- `Profile::Tasks` enables two pieces of sugar:
  - The initial `node where kind = Task and …` block is implicit when the query opens with a bare predicate. `priority = high and due < today` parses as `node where kind = Task and self.priority = high and self.due < today;`.
  - Bare attribute references resolve to `self.<attr>` (no explicit `self.` prefix needed).
- `Profile::Default` keeps the verbose graph syntax. Users on the Graph tab who want to query task subgraphs use the explicit form unless they pass `--profile tasks`.
- CLI: `ft tasks list <query>` uses `Profile::Tasks`; `ft graph query <query>` uses `Profile::Default` by default with `--profile tasks` opt-in.

### Built-in presets migrated

Task presets re-expressed in graph DSL:

```
today      = "(status in {Open, InProgress}) and (due = today or scheduled = today)"
overdue    = "(status in {Open, InProgress}) and due < today"
upcoming   = "(status in {Open, InProgress}) and due > today"
done-today = "status = Done and completed = today"
not-done   = "status in {Open, InProgress}"
```

User-defined presets in config (`Config.presets`) must be migrated by users (hard break). The migration guide ships in `docs/migrating-task-queries.md`.

### Sort and limit leave the DSL

- `sort by due reverse` and `limit 10` are removed from the DSL grammar.
- `ft tasks list` already accepts `--sort due,-priority` and `--limit 10`. No new CLI surface needed.
- The TUI tasks tab will use its existing sort-flag UI for sorting (separate UI control, not in the query string).

### Deletions

- `ft-core/src/query/dsl.rs` — deleted.
- `ft-core/src/query/expr.rs` — `Atom` enum deleted. `Expr` enum survives as a generic boolean tree (used by `Filter` composition) but its variants are now over the unified graph DSL's `Condition` type.
- `ft-core/src/query/preset.rs` — repurposed as a thin shim that returns the new graph-DSL strings.
- `ft-core/src/query/filter.rs` — `Filter` (the programmatic typed filter built from CLI flags) stays; it compiles into a `Condition` list AND-ed with the user's query expression.
- `ft-core/src/query/sort.rs` — stays. Sorting is now driven by CLI flags only, not by the DSL parser.

### Hard break

- `ft tasks list 'status is open and due before today'` no longer parses. Same query is now `ft tasks list 'status = Open and due < today'`.
- `tag is foo` becomes `tags includes foo`.
- `path includes "Areas/"` stays the same.
- `not done` becomes the `not-done` preset or `status in {Open, InProgress}` literally.

## Capabilities

### New Capabilities

- `tasks-query-profile`: A `Profile::Tasks` parser mode that desugars bare predicates into `node where kind = Task and self.<attr> ...`. Used by `ft tasks list` and the TUI Tasks tab.

### Modified Capabilities

- `graph-query-dsl`: Add comparison operators (`<`/`<=`/`>`/`>=`), `Date` value type with keyword and relative literals, nullability ops (`is null`/`is not null`). Existing surface unchanged for non-task usage.

### Removed Capabilities

- `tasks` (the task query DSL surface in `docs/query-dsl.md`): the dedicated grammar, atom set, and `query::dsl::parse` entry point are removed. The user-visible "tasks query" surface is now a `Profile::Tasks` view of the graph DSL.

## Impact

- **Modified**: `ft-core/src/graph/query.rs` (~+400 lines for operators + Date value + profile; mostly additive in the parser and evaluator).
- **Modified**: `ft-core/src/dates.rs` (expose `parse_date_keyword`, `parse_relative_date` if not already public).
- **Deleted**: `ft-core/src/query/dsl.rs` (~−812 lines).
- **Deleted from**: `ft-core/src/query/expr.rs` (`Atom` enum and impls, ~−180 lines).
- **Modified**: `ft-core/src/query/preset.rs` (rewrite preset strings in graph DSL).
- **Modified**: `ft-core/src/query/filter.rs` (return `Vec<Condition>` for AND-composition with parsed queries).
- **Modified**: `ft/src/cmd/tasks.rs` — `run_list` now calls `graph_query::parse(query, Profile::Tasks, today)` instead of `dsl::parse`. AND-composes with `Filter` conditions.
- **Modified**: `ft/src/tui/tabs/tasks/` — query bar parsing switches to graph DSL + Tasks profile.
- **Modified**: `docs/query-dsl.md` → renamed/rewritten as `docs/graph-query-dsl.md` consolidation. `docs/migrating-task-queries.md` is new and documents every renamed predicate.
- **Tests**: every existing task DSL test ported to the new syntax; new tests for `<`/`>` on dates and integers, `is null`, `Profile::Tasks` desugaring. Proptest round-trips for the new operators and Date value.
- All four build invariants stay green.

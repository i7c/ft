# Migrating task queries

`ft` used to ship two query DSLs: one for tasks (`ft-core/src/query/dsl.rs`)
and one for the graph tab (`ft-core/src/graph/query/`). The task DSL has
been removed; task queries now run the **graph DSL** under
`Profile::Tasks`, which gives you the same short syntax (`priority = high`,
`due < today`) backed by a single engine.

This is a **hard break** — old task DSL queries do not parse. The
translation is short. Translate your own presets, scripts, and TUI
snippets using the table below.

## Predicate translation

| Old (removed)                          | New                                           | Notes |
| -------------------------------------- | --------------------------------------------- | ----- |
| `status is open`                       | `status = Open`                               | Enum values are case-insensitive on the parser side; canonical form capitalises (`Open`, `Done`, `InProgress`, `Cancelled`). |
| `priority is high`                     | `priority = High`                             | Same set of values: `Highest`, `High`, `Medium`, `Low`, `Lowest`. |
| `tag is foo` / `has tag foo`           | `tags includes "foo"`                         | `tags` is a list-valued attribute; use `includes` for single membership or `in {…}` for set match. The leading `#` is no longer stripped — write the bare tag name. |
| `due before today`                     | `due < today`                                 | Date keywords `today` / `tomorrow` / `yesterday` work the same way. Also supported: `+Nd`, `-Nd`, `+Nw`, `-Nw`, `+Nm`, `-Nm`. |
| `due after 2026-01-01`                 | `due > 2026-01-01`                            | ISO dates are first-class — no need to quote. |
| `due on tomorrow`                      | `due = tomorrow`                              |  |
| `scheduled before today`               | `scheduled < today`                           |  |
| `completed after 2026-01-01`           | `completed > 2026-01-01`                      | The graph DSL exposes `completed` directly (it used to live only on the task DSL). |
| `done`                                 | `status = Done`                               | The bare `done` keyword is gone — be explicit. |
| `not done`                             | `status in {Open, InProgress}` or `not-done`  | The implicit "still actionable" predicate ships as the `not-done` built-in preset. |
| `has due`                              | `due is not null`                             | `is null` and `is not null` work on every optional date attribute (`due`, `scheduled`, `created`, `start`, `completed`). |
| `no due date`                          | `due is null`                                 |  |
| `path includes "Areas/"`               | `path includes "Areas/"`                      | Unchanged. |
| `description includes "report"`        | `description includes "report"`               | Unchanged. |
| `sort by due reverse`                  | CLI flag `--sort -due` or `--sort due:reverse` | Sort is no longer part of the query string. |
| `limit 10`                             | CLI flag `--limit 10`                         | Same — limit is now CLI-only. |

## Boolean composition

`and`, `or`, `not`, and grouping parens all work the same way:

```text
(status in {Open, InProgress}) and (due = today or scheduled = today)
priority = High and not (status = Done)
```

Precedence: `and` binds tighter than `or`. Parens override.

## CLI surface unchanged

The shape of `ft tasks list` is unchanged — same flags, same positional
preset-or-query argument, same output formats. The only change is what
the query string parses as.

```sh
# Same flags, new query syntax
ft tasks list --query 'status = Open and due < today' --sort due --limit 20
ft tasks list overdue           # built-in preset
ft tasks list not-done          # NEW built-in preset
```

## Built-in presets

| Preset       | Expansion                                                                  |
| ------------ | -------------------------------------------------------------------------- |
| `today`      | `(status in {Open, InProgress}) and (due = today or scheduled = today)`    |
| `overdue`    | `(status in {Open, InProgress}) and due < today`                           |
| `upcoming`   | `(status in {Open, InProgress}) and due > today`                           |
| `done-today` | `status = Done and completed = today`                                      |
| `not-done`   | `status in {Open, InProgress}`                                             |

Presets are parsed under `Profile::Tasks` so the implicit
`node where kind = Task and …` prelude is added for you. User-defined
presets in `Config.presets` follow the same convention — write them in
the unified DSL, omit the prelude.

## Why

- One engine, one parser, one error catalog, one docs file.
- The graph DSL already understood task attributes; it was missing only
  comparison operators (`<`, `<=`, `>`, `>=`), a `Date` value type, and
  nullability ops (`is null` / `is not null`). Those filled, the task
  DSL had nothing left to do.
- `sort` and `limit` were always CLI concerns; treating them as DSL
  clauses made the parser drag in concerns that didn't belong.

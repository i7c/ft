# Task Query DSL (removed)

The standalone task query DSL was removed in favor of the unified graph
DSL. Task queries (`ft tasks list <query>` and the TUI Tasks tab query
bar) now run the graph DSL parser under `Profile::Tasks`, which lets
you keep typing the short form (`priority = high`, `due < today`) while
giving you the same engine the graph tab uses.

- [Graph query DSL reference](./graph-query-dsl.md) — the unified grammar.
- [Migrating task queries](./migrating-task-queries.md) — predicate
  translation table for users coming from the old task DSL.

# Changelog

## Unreleased

### Hard break: task DSL replaced by unified graph DSL under `Profile::Tasks`

`ft tasks list` and the TUI Tasks tab now use the graph DSL parser
(`ft-core::graph::query`) with `Profile::Tasks`. The dedicated task DSL
(`ft-core::query::dsl`) and its `Atom` enum have been removed.

User-visible changes:

- Predicate syntax is the graph DSL form: `priority = High`, `due < today`,
  `tags includes "work"`, `status in {Open, InProgress}`, `due is null`.
- Operators added to the graph DSL: `<`, `<=`, `>`, `>=`, `is null`,
  `is not null`. New `Date` value type accepts `YYYY-MM-DD`, `today`,
  `tomorrow`, `yesterday`, and relative offsets (`+Nd`, `-Nw`, `+Nm`).
- New `not-done` built-in preset for the common "still actionable" filter.
- The graph DSL now supports `or` and grouping parens (the only way to
  express the `today` preset's `due = today or scheduled = today`
  branch).
- `sort` and `limit` are no longer part of the DSL — use the existing
  `--sort` flag and the new `--limit N` flag on `ft tasks list`.
- `ft graph query` gained a `--profile {default|tasks}` flag so
  ad-hoc task-subgraph queries can use the same Tasks-profile sugar.

See [`docs/migrating-task-queries.md`](docs/migrating-task-queries.md)
for the predicate translation table.

Internal cleanup:

- Deleted `ft-core/src/query/dsl.rs` and `ft-core/src/query/expr.rs`
  (the `Atom`/`Expr` types).
- `query::preset::builtin` now returns Tasks-profile graph DSL strings.
- `TaskData` gained `created`, `start`, and `completed` date fields so
  every `Profile::Tasks` date predicate has a backing field on the graph
  task node.

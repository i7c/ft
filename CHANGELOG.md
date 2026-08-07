# Changelog

## Unreleased

## 0.1.7

### Added

- scripts: add release.sh — version bump, changelog, tag
- ft notes export: --format slack target (mrkdwn conversion + fence context)

## 0.1.5

### Added

- `ft notes quote <file> --lines A-B` (`-l` alias): read-only plumbing
  that validates the source note is committed and unmodified at HEAD,
  slices the 1-indexed inclusive line range from the working tree, and
  emits the canonical `[!ft-source]` callout to stdout. It reuses the
  pinning mechanics scaffold/grow share, exposed for scripts and
  ft.nvim. See `docs/guide/synthesis.md` → "Quoting a section
  (plumbing)".
- New `synth::slice::{count_lines, slice_lines}` core helper (1-indexed
  inclusive range; trailing newline is not a line), now shared by
  verify, reslice, repair (`body_matches_pin`) and quote.
- `head_short_sha` + `SHORT_SHA_LEN` moved from the callout module to
  `ft-core::git`.

## 0.1.4

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

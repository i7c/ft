# Changelog

## Unreleased

### Removed

- feat: remove the Pulse and Recent functionality — `ft notes pulse`, `ft notes
  recent`, the Pulse and Recent TUI tabs, and the modules behind them
  (`ft_core::pulse`, `ft_core::recent`, `ft_core::synth::citations`, the CLI
  feed renderer, the pulse-only git diff helpers). The sweep flow is now
  `ft notes search --sort date` → `ft notes synth scaffold`. **Breaking:** both
  subcommands are gone (clap exits 2), the default tabs renumber to
  `1 Graph  2 Notes  3 Search`, and `tab/pulse` / `tab/recent` `[keymap]`
  scopes no longer validate; scripts and overrides that reference them must be
  deleted.

## 0.1.13

### Added

- feat(tasks): reparent tasks — `ft tasks move --under [--sibling]`, `ft tasks indent|outdent`, `--format json`; TUI cut/paste (`d`, `p`/`P`) and `>`/`<` (priority cycling moved to `+`/`-`)
- feat(tasks): cross-file parents via wikilinks (`[[note#^id]]`, anchor notes with `ft.tasks.anchor`); `ft tasks link|unlink|check`, `create --link-parent`; TUI `L`/`U`/`S`; query attributes `parent`, `parent_status`, `parent_link`, `children`; presets `orphaned`, `broken-links`
- feat(sources): Jira Cloud and GitHub issues synced into issue notes — `[sources.*]` config, `ft sources login|logout|list`, `ft tasks sync`, TUI background sync and `g r`; query attributes `source`, `key`, `remote_status`; preset `remote`
- feat(sources): push local status changes on every sync (Jira transitions, GitHub close/reopen), stored conflicts with `ft tasks sync --resolve|--take` and a TUI modal, and `ft tasks promote` (TUI `I`)

### Fixed

- fix(periodic): anchor note resolution to the period start
- fix(tasks): moves keep block references valid across files

## 0.1.12

### Added

- feat(export): resolve soft-wrapped lines (--unwrap)
- feat: remove the deprecated gather functionality

### Fixed

- fix(tui): expand tabs in paragraph previews so they don't garble
- fix(tui): wrap paragraph previews by column width, not char count

## 0.1.11

### Added

- feat(search): add c to clear the query and results

### Fixed

- fix(search): keep negation on phrase and link clauses through render
- fix(search): stop stale cells leaking across frames in the TUI tab

## 0.1.10

### Added

- feat(search): feed-split layout with boxed query in the TUI tab
- test: stop git auto-maintenance racing the no-IO assertion
- feat: paragraph search replaces gather as the synth sourcing engine
- feat(export): slack lists re-indented to 4 spaces per level
- Architecture Review Skill

### Fixed

- fix(search): split trigrams on chars, not bytes

## 0.1.9

### Internal cleanup

- refactor(core): serve vault-reading consumers from the scan
- refactor(core): extract ft-core::scan — the single-read-pass scan module

## 0.1.8

### Added

- scripts: add release.sh — version bump, changelog, tag
- ft notes export: --format slack target (mrkdwn conversion + fence context)
- Fix path normalization

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

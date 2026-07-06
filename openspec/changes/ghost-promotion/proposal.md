# Proposal: ghost-promotion

## Why

A ghost link with many mentions and no note is the vault saying a note
has earned its existence — the purest expression of the connect-later
thesis (exploration record, session 3). ft already has everything
*except the view*: `ParagraphLink` edges carry mention counts in the
graph, a `ghosts` preset selects ghosts, and the graph tab already
promotes a ghost by creating its note in one keystroke. But no surface
ranks ghosts by weight, the TUI shows no counts, and `review`'s `?`
markers are window-scoped. You cannot currently answer "which concepts
have earned their page?" vault-wide.

## What Changes

- **Core ghost ranking.** `ft_core::graph::ghosts::rank_ghosts(graph)`:
  every ghost with its distinct-paragraph mention count (dedup by
  paragraph, mirroring `review`'s counting rule), sorted count-desc
  then name. Pure graph — no git required.
- **CLI ranked list.** New `ft notes ghosts`: `(N) [[ghost]]` rows in
  review's format, `--limit`, `--min-mentions`, `--json`. (Naming is
  provisional; session 5 owns final verbs.)
- **TUI counts + ordering.** Ghost rows in the graph tab render a
  mention-count suffix, and ghost-selecting queries order ghosts
  count-desc (then name, keeping determinism) — so the existing
  `ghosts` preset becomes the ranked view in both the TUI and
  `ft graph query`.
- **Seeded promotion (TUI).** New `graph.promote-ghost` command on a
  ghost row: creates the note as a synth note scaffolded with every
  paragraph mentioning the ghost (`plan/apply_synth_scaffold`,
  `ft-synth-targets` set), then opens the editor — "notes earn their
  existence" made concrete. The CLI equivalent already exists verbatim
  (`ft synth scaffold <path> --link "[[ghost]]"`); the blank/template
  creates on ghost rows stay unchanged for concept notes that should
  start empty.
- **Out of scope:** DSL-level sorting (ordering machinery in the
  query language), review integration (review stays window-shaped),
  renames (session 5).

## Capabilities

### New Capabilities

- `ghost-promotion`: vault-wide ghost ranking (core + CLI), mention
  counts and ranked ordering in the graph surfaces, and the
  scaffold-seeded promote action in the graph tab.

### Modified Capabilities

<!-- none — existing requirements are unchanged; ghost-count display,
ordering, and the new command are additive concerns captured in the new
capability spec -->

## Impact

- `ft-core`: new `graph::ghosts` module (reads `ParagraphLink` edges);
  possible ordering tweak where ghost siblings/roots are sorted
  (`graph::query::eval`'s deterministic sort) — existing walk
  snapshots containing ghosts may reorder and need review.
- `ft` CLI: `ft/src/cmd/notes.rs` new subcommand + integration tests.
- TUI: graph tab row rendering (count suffix), `GRAPH_COMMANDS` /
  `GRAPH_KEYMAP` gain `graph.promote-ghost` (regenerate
  `docs/keybindings.md`), new `TestBackend` snapshots.
- Docs: `docs/guide/notes.md` and/or `docs/guide/graph.md` +
  `docs/guide/synthesis.md` cross-link for seeded promotion.

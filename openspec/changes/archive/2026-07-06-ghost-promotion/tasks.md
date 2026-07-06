# Tasks: ghost-promotion

## 1. Core: ghost ranking

- [x] 1.1 Add `ft_core::graph::ghosts` with `GhostRank { id, raw,
  mentions }` and `rank_ghosts(graph)` counting distinct paragraph
  nodes with `ParagraphLink` edges to each ghost, sorted mentions-desc
  then name-asc
- [x] 1.2 Unit tests: multi-mention paragraph counts once; tie-break
  alphabetical; vault with no ghosts returns empty; note-level links
  don't inflate the count

## 2. CLI: ft notes ghosts

- [x] 2.1 Add the `ghosts` subcommand to `ft notes` (`ft/src/cmd/notes.rs`):
  ranked `(N) [[ghost]]` table (review's row grammar), `--limit`,
  `--min-mentions` (default 1), `--json` (`[{target, mentions}]`),
  `--no-color`; empty vault prints `no ghosts in the vault`, exit 0
- [x] 2.2 Integration tests against a fixture vault: ranked order,
  filter composition (`--min-mentions` + `--limit`), JSON shape, empty
  case

## 3. Ranked ordering in graph walks

- [x] 3.1 Order ghosts by `(mentions desc, name asc)` in the walk's
  deterministic sort (root selection and `child_sort_key` sibling
  order in `graph::query::eval`), leaving non-ghost ordering unchanged
- [x] 3.2 Round-trip test: `--preset ghosts` walk lists ghosts in
  ranked order; review existing walk/TUI snapshots containing multiple
  ghosts deliberately (design D3 churn budget)

## 4. TUI: counts + seeded promotion

- [x] 4.1 Render mention-count suffix on ghost rows in the graph tab,
  computed once per snapshot generation in derived view state (not per
  frame)
- [x] 4.2 Add `graph.promote-ghost` to `GRAPH_COMMANDS` + keymap
  (proposed `P`; verify with `ft commands check-keymap`), dispatch arm,
  `?` help entry
- [x] 4.3 Implement the action: ghost row → `build_journal` for the
  ghost (git required; toast on failure), `plan/apply_synth_scaffold`
  to `<ghost>.md` with `ft-synth-targets`, graph refresh + editor
  open; non-ghost row → explanatory toast
- [x] 4.4 `TestBackend` snapshots: ghost rows with counts (ranked
  order); promote flow test asserting the created synth note's
  sections + frontmatter and the toast on a non-ghost row
- [x] 4.5 Regenerate `docs/keybindings.md` and confirm
  `commands docs --check` passes

## 5. Docs + wrap-up

- [x] 5.1 Document the ranked list and promotion flavors in
  `docs/guide/notes.md` (ghosts section) and `docs/guide/graph.md`;
  cross-link `docs/guide/synthesis.md` for seeded promotion, noting
  the CLI equivalent `ft synth scaffold <path> --link "[[ghost]]"`;
  keep the no-"ritual" wording rule
- [x] 5.2 Update `openspec/explorations/note-flow-reframe.md` session 3
  status and record the decisions (dedicated `ft notes ghosts`
  command; scaffold-seeded promotion included; ranked ordering in
  walks instead of DSL sort syntax)
- [x] 5.3 Full invariant sweep: `cargo build --release`,
  `cargo test --workspace`, `cargo clippy --workspace --tests -- -D
  warnings`, `cargo fmt --check`, `commands docs --check`

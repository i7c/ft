# Tasks: drift-detection

## 1. Core: drift detector

- [x] 1.1 Add `ft_core::graph::drift` with the concept universe
  (mentioned notes + ghosts, distinct-paragraph counting shared with
  `graph::ghosts`), name normalization + similarity (containment,
  token overlap, inline Levenshtein — no new deps), and the gate
  constant (design D2)
- [x] 1.2 Neighborhood overlap for gated pairs via
  `related::score_related` profiles (each side excluded from the
  other), direct-co-occurrence counting, weight, and the final score
  (design D3/D4)
- [x] 1.3 Suggestion policy (design D5): ghost-involving → rename-merge
  command with the note (or heavier ghost) as keeper; note↔note →
  Related-alias advice
- [x] 1.4 Unit tests: compound-name pair detected; dissimilar names
  never pair; co-occurring near-names rank below true drift; stakes
  raise rank; keeper policy (note beats ghost; heavier beats lighter);
  clean vault → empty

## 2. CLI: ft notes drift

- [x] 2.1 Add the `drift` subcommand to `ft notes`: header rows
  `[[keeper]] (N) ↔ [[lesser]]? (M)` + indented suggestion line,
  `--limit`, `--json` (signals + suggestion per pair), `--no-color`;
  empty → `no drift candidates found`, exit 0
- [x] 2.2 Integration tests: drifted fixture vault (report shape, ghost
  marker, merge suggestion), note↔note alias case, `--limit`/`--json`,
  clean-vault empty case
- [x] 2.3 Round-trip sanity: run the suggested rename from the report
  against the fixture and assert the pair disappears from the next
  report

## 3. Docs + wrap-up

- [x] 3.1 Document the drift report in `docs/guide/notes.md` (next to
  the Ghosts section) and cross-reference it from philosophy.md's
  "Keeping names honest" defense list
- [x] 3.2 Update `openspec/explorations/note-flow-reframe.md` session 4
  status + decisions (all pair kinds; CLI-only v1; signals =
  name gate + neighborhood confirm + co-occurrence penalty; rename
  merge verified pre-existing)
- [x] 3.3 Full invariant sweep: `cargo build --release`,
  `cargo test --workspace`, `cargo clippy --workspace --tests -- -D
  warnings`, `cargo fmt --check`, `commands docs --check`

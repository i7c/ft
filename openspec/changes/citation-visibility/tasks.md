# Tasks: citation-visibility

## 1. Core: citation index (ft-core)

- [ ] 1.1 Add `ft_core::synth::citations` with `CitationIndex`,
  `CitationState { Cited, CitedStale, Uncited }`, `build` from a scan
  (iterate `ft-synth: true` notes, `callout::parse`, index by
  `(source_path, hash_prefix)` + per-path interval list), and
  `lookup(source_path, line_range, body)`
- [ ] 1.2 Unit tests: exact match → Cited; overlapping range +
  different body → CitedStale; no overlap → Uncited; multiple citing
  notes collected; malformed synth note skipped with diagnostic;
  hash-prefix collision falls through to body compare
- [ ] 1.3 Consistency test with `accrete::filter_missing`: for a
  synthetic note + feed, `lookup == Cited` ⇔ entry dropped by
  `filter_missing` (spec "Consistency with scaffold dedup")

## 2. CLI: journal + history badges and filter

- [ ] 2.1 Build the index in `ft notes journal` / `ft notes history`
  runs; annotate rendered entries with `cited:` / `cited*:` badge
  lines (first note stem + `+N` overflow); leave builders'
  signatures untouched (design D3)
- [ ] 2.2 Add `cited_in: [{note, stale}]` to both commands' `--json`
  output (additive; existing fields unchanged)
- [ ] 2.3 Add `--uncited` to both commands (keeps non-`Cited`
  entries; composes with `--link`, window flags, `--json`)
- [ ] 2.4 Integration + insta snapshot tests against a fixture vault
  with a synth note: badge rendering, JSON shape, `--uncited`
  filtering, stale case

## 3. TUI: badges + uncited toggles

- [ ] 3.1 Carry the citation index with the shared graph snapshot:
  build in the background rebuild worker, expose via `TabCtx::snapshot`
  generation-consistently (resolve design open question: member vs
  sibling `Arc`, keeping `pump_graph_rebuild_for_test` untouched)
- [ ] 3.2 Render badges on Journal and History rows from the snapshot
  index; update existing `TestBackend` snapshots deliberately
- [ ] 3.3 Add `journal.toggle-uncited` + `history.toggle-uncited`
  commands and `u` bindings in the tabs' command/keymap statics;
  add `dispatch_command` arms and `help_sections()` rows
- [ ] 3.4 Regenerate `docs/keybindings.md`
  (`cargo run --release -q -- commands docs > docs/keybindings.md`)
  and confirm `commands docs --check` passes
- [ ] 3.5 New `TestBackend` snapshots: badged rows (cited + stale)
  and the uncited-only toggle on both tabs

## 4. TUI: note-context mode

- [ ] 4.1 When the `s` append-to-existing flow has a target synth
  note picked, re-scope row badges to that note (`in note` /
  `missing`, filter_missing rule) with a status-line indicator;
  restore global badges when the flow ends
- [ ] 4.2 Same re-scope when a journal is opened from a synth note's
  `ft-synth-targets` (the grow-style entry point)
- [ ] 4.3 Snapshot test: feed with a target note in context shows
  `in note` / `missing` badges matching what `plan_synth_scaffold`
  would dedup

## 5. Docs + invariants

- [ ] 5.1 Document badges, `--uncited`, the `u` toggles, and
  note-context mode in `docs/guide/synthesis.md` and
  `docs/guide/notes.md` (vocabulary: cited / cited-stale / uncited;
  no "ritual")
- [ ] 5.2 Record the dismiss deferral trigger in
  `openspec/explorations/note-flow-reframe.md` session 2 (status →
  proposed/in-progress; decisions: defer dismiss, three-state)
- [ ] 5.3 Full invariant sweep: `cargo build --release`,
  `cargo test --workspace`, `cargo clippy --workspace --tests -- -D
  warnings`, `cargo fmt --check`, `commands docs --check`

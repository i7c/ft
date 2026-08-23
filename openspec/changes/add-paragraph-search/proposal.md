# Add paragraph search, deprecate gather

## Why

The synthesis flow's sourcing engine — `ft notes gather` — is a 1,100-line
graph walk (blame feed, Related-alias resolution, heading-section expansion,
multi-target `matched` attribution) that answers one question: *"which
paragraphs mention this `[[link]]`?"* A fast scan-derived search index answers
that question more simply and more generally: any term, not just links;
cross-reference queries ("both `[[X]]` and `Y` in the same paragraph") the
graph walk cannot express; and a live as-you-type TUI. If search proves
sufficient as the sourcing front-end, gather and its engine can be removed
entirely, leaving the synth flow at its minimal shape: pulse, protected
sections/quotes with provenance, and checking/healing.

## What Changes

- **New search engine** (`ft-core::search`): a paragraph index built from the
  existing vault scan (no graph, no git), a small query DSL, and a
  relevance/date ranking. Modes: substring (default), `=word`, `~fuzzy`
  (levenshtein), `"phrase"`, `[[link]]` (link-target match, spaces allowed
  inside brackets), `-exclude`. Space-separated terms AND by default; `--any`
  switches to OR. `--sort relevance|date` (date = `git blame`, kept cheap
  because `recent` keeps the blame cache alive).
- **`ft notes search <query>`** CLI: `--any`, `--sort`, `--limit`, `--json`.
- **Search TUI tab**: input line with live as-you-type results, all/any toggle
  (`a`), sort toggle (`s`), multi-select + send-to-synth (`s`/`S`), Enter opens
  the source at the paragraph. Becomes the default tab slot where Gather sat.
- **`ft notes synth scaffold --search "<query>"`**: new sourcing path with the
  same plan/apply/dedup/dirty-source machinery; sections emit in result order
  (relevance by default, `--sort date` restores newest-first).
- **Pulse handoff rewired**: Enter on Pulse opens the Search tab prefilled
  with the selected links as `[[…]]` terms in any-mode (gather parity).
- **Gather deprecated (transitional)**: `ft notes gather`/`ft notes journal`
  hidden from help and emit a deprecation warning; the Gather TUI tab is
  removed from the default lineup (restorable via a `[tui]` config flag);
  `--link`/`--from` on scaffold stay functional as a transitional path
  (`--link` lowers to a search clause). The engine stays until the follow-up
  removal change, because `recent`, `related`, and ghost promotion still use it.
- **`grow` removed** — **BREAKING**: `ft notes synth grow`, `--new-only`,
  `--limit`, the last-synth watermark, and the self-describing
  `ft.synth.targets` frontmatter are deleted. Scaffold's append path already
  dedups, so re-running scaffold with the same query is idempotent.
- **Scaffold flag cleanup** — **BREAKING**: `--in-window`, `--since`,
  `--range` removed (they only existed to feed gather's window filter; search
  has no window concept — `--sort date` covers recency).
- **Unchanged in this change**: pulse, quote/export, verify, repair, reslice,
  recent, citation badges/`--uncited` (fate deferred to the removal change),
  ghost promotion (re-sources via `--search "[[ghost]]"`).

## Capabilities

### New Capabilities
- `paragraph-search`: scan-derived paragraph index, query DSL (substring /
  word / fuzzy / phrase / wikilink / exclude; AND default, `--any` OR),
  relevance + date sorts, `ft notes search` CLI, Search TUI tab, scaffold
  `--search` sourcing, pulse handoff.

### Modified Capabilities
- `synth-notes`: scaffold gains `--search` sourcing; `--link`/`--from`
  become transitional; window flags removed; self-describing targets removed.
- `notes-journal`: `ft notes gather` deprecated (hidden + warning); `recent`
  unaffected.
- `journal-tui-tab`: Gather tab removed from default lineup, restorable via
  config.
- `synth-grow`: capability removed (grow, watermark, missing-entry filter).

## Impact

- **`ft-core`**: new `search/` module (index, DSL parser, matcher, ranking,
  optional date attachment); scaffold sourcing change; removal of
  `accrete::last_synth_watermark` and `parse_synth_targets` /
  `upsert_synth_frontmatter`; error-variant churn in `thiserror` enums.
- **`ft` binary**: new `ft/src/cmd/search.rs`; `synth.rs` scaffold rework;
  `notes.rs` gather deprecation (clap `hide` + warning); new Search TUI tab in
  `ft/src/tui/tabs/`; pulse handoff request type; `[tui]` config flag;
  keybindings doc regeneration.
- **`ft.nvim`**: unaffected — it only consumes `quote`/`export`; the search
  CLI is additive. The protocol contract is unchanged.
- **Dependencies**: none new for exact/substring (memchr-style scan over the
  in-memory corpus); a trigram map for fuzzy candidate generation is
  hand-rolled (no new crate).
- **Tests**: unit tests for DSL parse + matcher + ranking; `proptest`
  round-trip for the DSL; `ft/tests/search_cli.rs`, Search-tab `TestBackend`
  snapshots, `synth_cli.rs` `--search` cases; perf gate for per-keystroke
  latency under `FT_PERF_TESTS=1`; deprecation-warning tests for gather.
- **Docs**: `docs/guide/synthesis.md` (search step replaces gather step),
  `docs/architecture.md` §Synthesis, keybindings, config schema.

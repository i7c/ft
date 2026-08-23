## 1. Search engine core (`ft-core::search`)

- [x] 1.1 Add `ft-core/src/search/mod.rs` (+ `index.rs`, `query.rs`, `match.rs`, `rank.rs` as needed) with `SearchIndex::build(&Scan)` — one pass over `ParsedFile.paragraphs`; store per-doc vault-relative path, `line_start`/`line_end`, verbatim text, case-folded text; token dictionary (case-folded alphanumeric runs + `[[…]]` link tokens with anchor stripped / alias used) → sorted postings; trigram map over dictionary tokens; expose `paragraph_count()` and immutable accessors
- [x] 1.2 Implement `search::query::parse(&str) -> Result<Query>` (`Query { any, clauses }`; `Clause { negated, mode, term }`; modes Substring/Word/Fuzzy/Phrase/Link) — scan `[[…]]` (atomic, spaces allowed) before whitespace splitting; `"…"` phrase; `~`/`=`/`-` prefixes; unterminated `[[`/`"` degrade to substring; empty query → `Query` with no clauses
- [x] 1.3 Implement `search::match` per mode: case-insensitive substring/phrase over folded text; word and link clauses against the token index; fuzzy via trigram candidates + levenshtein (threshold 1 for len ≤ 4 else len/4, prefix-scan for len ≤ 3); exclude clauses filter after positives; AND (intersect) and any (union) combinators
- [x] 1.4 Implement ranking (`search::rank`): mode weights (phrase 3, word 2, link 2, substring 1.5, fuzzy 1) × occurrence boost (1 + 0.5×(occ−1), cap ×3) × position bonus; deterministic tiebreak path asc, line asc; `SearchResult { doc, matched: Vec<ClauseLabel>, score }`
- [x] 1.5 Implement `--sort date` support: `search::search_with_dates(index, query, vault, cache, sort)` blaming only result-set files via `BlameCache` (lazy); blame-failure sorts oldest; return `date: Option<NaiveDate>` per result
- [x] 1.6 Apply `synth.exclude_prefixes` filtering in the search entry point (consistent with pulse)
- [x] 1.7 Unit tests: DSL parse table (modes, atomic `[[Bar Foo]]`, degrade cases, exclude, any), per-mode matcher scenarios, ranking determinism, tiebreak, exclude-prefix filter; `proptest` round-trip: `parse(serialize-ish)` — every parsed query re-parses stably
- [x] 1.8 Perf gate test under `FT_PERF_TESTS=1`: build index + substring/fuzzy query over a generated 5,000-paragraph corpus under the 10 ms per-query budget

## 2. `ft notes search` CLI

- [x] 2.1 Add `ft/src/cmd/search.rs` (Args: query, `--any`, `--sort relevance|date`, `--limit N`, `--json`; run: discover vault, require git only when `--sort date`, scan → `SearchIndex::build` → query → rank/sort → render), register module + `Commands` variant in `ft/src/main.rs`
- [x] 2.2 Text renderer: `path Lstart-end  <clause labels>  <paragraph text>` with owo-colors auto-off non-TTY; JSON renderer: `{path, line_start, line_end, body, matched, score, date?}`; empty query/results → no output, exit 0
- [x] 2.3 Integration tests `ft/tests/search_cli.rs` (assert_cmd): substring/word/fuzzy/phrase/link matching, `[[Bar Foo]]` spaces, AND vs `--any`, exclude, `--sort date`, `--limit`, `--json` shape, `--vault` flag, exclude_prefixes
- [x] 2.4 Add `search` to docs: `docs/guide/synthesis.md` (new Step 2 replacing gather), `docs/architecture.md` §Synthesis, README command list

## 3. Scaffold `--search` + grow removal

- [x] 3.1 Rework `ft/src/cmd/synth.rs::run_scaffold`: add `--search <query>`, `--any`, `--sort relevance|date`; source results → `SynthSource` deduped by `(source_path, line_start)`; require at least one of `--search`/`--link`/`--from`; sections in result order; remove `--in-window`/`--since`/`--range`
- [x] 3.2 Transitional `--link`: lower each `[[X]]` to a link clause, any-mode, via the search path (drop Related-alias resolution); keep `--from` (existing `pick_paragraph` path) unchanged; deprecation note in `--help` text for `--link`
- [x] 3.3 Remove `grow`: delete `SynthCommand::Grow` + `GrowArgs` + `run_grow` + `run_grow_with_targets`; delete `accrete::last_synth_watermark` (+ its tests) and `callout::{parse_synth_targets, upsert_synth_frontmatter}`; delete `ensure_synth_targets`; keep `accrete::filter_missing` (scaffold append-dedup) and `SYNTH_FRONTMATTER`
- [x] 3.4 Update `ft/tests/synth_cli.rs` and any fixtures for the new flag surface; add `--search` scenarios (create, append-dedup idempotent, `--sort date` order, search+`--from`); remove grow tests
- [x] 3.5 `cargo clippy --workspace --tests -- -D warnings` clean after removals (dead code, unused imports)

## 4. TUI: Search tab, handoff, gather tab opt-in

- [x] 4.1 Extract the Gather tab's send-to-synth machinery (`SynthSendState`, pickers, 3-way non-synth-note prompt) into a shared `ft/src/tui/synth_send.rs`; Gather tab imports it (no behavior change)
- [x] 4.2 Add `ft/src/tui/tabs/search.rs`: `SearchTab` with `InlineInput`/`EditBuffer` query bar, live synchronous re-query against the snapshot's `Arc<SearchIndex>` (index stored beside the graph snapshot; rebuilt on generation change), status bar `N terms · AND/ANY · sort: …`, keys `a`/`o`/`Space`/`Enter`/`s`/`S`/`R`; `<TAB>_COMMANDS`/`<TAB>_KEYMAP` slices, keymap overlay, `dispatch_command` arm, `help_sections()`
- [x] 4.3 Wire the search index into the TUI: add `Arc<SearchIndex>` to the shared snapshot type + rebuild path (alongside the graph worker), `TabCtx::snapshot` exposure, `on_graph_ready`/`on_focus` re-derive
- [x] 4.4 Register the Search tab in `build_tabs_with_overlays` in the Gather tab's default slot; add `TestBackend` snapshot under `ft/src/tui/tests/`; re-run `ft commands docs > docs/keybindings.md`
- [x] 4.5 Rewire Pulse handoff: replace `AppRequest::GatherForMulti` from Pulse with `AppRequest::SearchWithQuery { query, any: true }` prefilling `[[title1]] [[title2]]`; keep the old gather request types for the hidden Gather tab
- [x] 4.6 Gather tab opt-in: add `Tui::show_gather: bool` (default false) to `ft-core/src/config.rs`; include the Gather tab only when set, with a deprecation note rendered when focused; add `[tui]` config test
- [x] 4.7 Ghost promotion (`Shift+p` on the Graph tab): re-source via the search path (`--search "[[ghost]]"`) instead of `build_gather`

## 5. Deprecation + docs

- [x] 5.1 Hide `ft notes gather` / `ft notes journal` from clap help (`hide = true`) and print a single stderr deprecation warning per invocation pointing at `ft notes search`; add deprecation-warning tests in `ft/tests/`
- [x] 5.2 Update `docs/config.md` (`[tui] show_gather`), `docs/keybindings.md` (Search tab keys), `docs/guide/synthesis.md` (search step, scaffold `--search`, grow removal, gather deprecation), `docs/architecture.md` §Synthesis (engine map, search index, deprecation notes)
- [x] 5.3 Update the `docs/architecture.md` build-invariant checks if the engine map changes; ensure `cargo run --release -q -- commands docs --check` passes

## 6. Verification

- [x] 6.1 Full green: `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `cargo run --release -q -- commands docs --check`
- [x] 6.2 Smoke-test the flow end-to-end in a temp fixture vault: `ft notes search`, `ft notes synth scaffold --search`, append-dedup idempotency, `ft notes synth verify --all` on the result, pulse → search handoff in the TUI
- [x] 6.3 Update the openspec change status and mark the change apply-ready; confirm the follow-up (gather engine/tab/citation removal) is captured as a separate future change

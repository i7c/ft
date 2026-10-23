# Tasks: search relevance overhaul

## 1. Relevance eval harness (before any ranking change)

- [x] 1.1 Create `tests/fixtures/search-eval/` (≥ 40 notes): focused vs long-list paragraphs, rare/common terms, title/alias/heading/tag-only matches, English + Portuguese accented prose, stem variants, `proj/…` tag hierarchy, a small link cluster for `near:`, and a few `archive/` notes for negated filters
- [x] 1.2 Write `tests/fixtures/search-eval/judgments.toml` with ≥ 25 graded cases; each case's `note` names its probe class (length-norm, idf, context-title/alias/heading/tag, diacritics-en/pt, stemming, filter-path/tag/scope — date filters are unit-tested, near, regression-phrase/link/fuzzy/exclude)
- [x] 1.3 Add a `search_eval` helper (judgment loader with case-indexed errors, range-contained matching, nDCG@10 with `2^g−1` gain, MRR, recall@10, per-case table printer) + unit tests for each metric on hand-computed examples
- [x] 1.4 Add `ft-core/tests/search_eval.rs`: build the index from the fixture vault (pinned `FT_TODAY`), run all cases, assert means against `baseline.toml`; cases using syntax the old engine can't parse simply score 0 for now
- [x] 1.5 Record the **current** engine's means in `tests/fixtures/search-eval/baseline.toml` and commit the harness on its own (the before-numbers for the change)
- [x] 1.6 Add the gated real-vault run (`FT_REAL_VAULT_TESTS=1` + `FT_VAULT` + `FT_SEARCH_EVAL_JUDGMENTS`; report-only, skip when unset); draft a private judgments file for `~/git/fortytwo` outside the repo and record its baseline numbers in the change notes

## 2. Text layer: folding, tokenization, stemming

- [x] 2.1 Add `unicode-normalization` and `rust-stemmers` to `ft-core/Cargo.toml` (workspace deps)
- [x] 2.2 New `ft-core/src/search/text.rs`: `fold` (lowercase + NFD + drop `\p{Mn}`) and `fold_with_offsets` (folded → original byte map); unit tests incl. `Ônibus`, `estações`, `straße`, emoji, and offset round-trip
- [x] 2.3 Move tokenization into `text.rs`: Unicode alphanumeric runs, `[[…]]` link targets (folded, anchor-stripped, alias-resolved), inline `#tags` (non-digit, `[\p{Alnum}_/-]+`, not `# ` headings); port the existing tokenizer tests and add accent/tag cases
- [x] 2.4 Stemmer wrapper: stem the unfolded lowercase token, fold the stem; `[search] stemmers` config (`Config::search`, default `["english"]`, unknown name → load error listing valid names) + config tests
- [x] 2.5 Route `trigrams_of` and the query parser's term folding through `text::fold`

## 3. Context fields

- [x] 3.1 `frontmatter::aliases_in` / `tags_in`: block list, inline list, scalar, quoted, leading `#` stripped, `alias`/`tag` singular keys; unparseable → empty; unit tests
- [x] 3.2 Heading-path derivation from `ParsedFile.headings` per paragraph line (ATX nesting, outermost first); unit tests incl. skipped levels and a paragraph that is itself a heading
- [x] 3.3 Index structures per design §3: `NoteCtx` / `SectionCtx`, note→doc and section→doc ranges, per-field postings with `tf`, per-field lengths and averages, per-field df (notes for title/alias/fm-tags, sections for headings, docs for body/inline tags), stem maps per algorithm
- [x] 3.4 Keep `is_protected_callout` skipping and `exclude_prefixes`; index-build unit tests for every scenario under "Paragraph index built from the scan"

## 4. Query grammar

- [x] 4.1 Extend `search/query.rs` with `Clause::Filter { key, value, negated }` (`path`, `tag`, `after`, `before`, `scope`) and `Clause::Near { target }`; quoted filter values; unknown keys / empty values / invalid dates degrade to substring
- [x] 4.2 `render()` round-trip for the new clauses; extend the existing proptest to generate filter and near clauses
- [x] 4.3 Parser unit tests for every "Query grammar" scenario

## 5. Matching and BM25F ranking

- [x] 5.1 Clause resolution per design §2: word (exact + stem-mates), link (body link postings), fuzzy (trigram + char-counted levenshtein), single-run substring (dictionary tokens containing the term, trigram-narrowed for ≥ 3 chars), multi-run substring/phrase (folded-text scan per field)
- [x] 5.2 Field OR for positives (link → body only), body-only excludes, AND/ANY over positive text clauses
- [x] 5.3 BM25F scorer with named consts (field weights, `b_f`, `k1`, mode weights), max-over-resolved-tokens per clause, deterministic tiebreak; unit tests for each "Relevance ranking" scenario
- [x] 5.4 `MatchReason` collection (clause label × field) on each result
- [x] 5.5 Run the eval; tune constants; record the means in the change notes

## 6. Filters

- [x] 6.1 `path:` / `tag:` (hierarchical, `#` optional) filters over note/doc metadata; negation
- [x] 6.2 `scope:<preset>`: resolve user → builtin preset, `GraphQuery::walk` with default `WalkOptions`, note-id set; `SearchError::{UnknownScope{name, available}, NoGraph}`; make `search`/`search_with_dates` return `Result`
- [x] 6.3 `after:` / `before:` via `dates::parse_date_value` + `dates::today()`; blame only survivors' files through `BlameCache`; undated paragraphs fail the filter and are counted for the CLI warning
- [x] 6.4 Filter-only queries start from all docs, ranked prior → path → line; tests for every "Filter clauses" scenario (date tests use a temp git repo + `FT_TODAY`)

## 7. Priors and `near:`

- [x] 7.1 `SearchIndex::build_with(scan, exclude, &SearchCfg)`; per-note prior from in-degree over scan links resolved with `graph::resolve` (log-scaled to max) + whole-day relative-mtime recency, clamped to `[1, 1.25]`; no graph build
- [x] 7.2 `near:[[X]]`: resolve via graph name resolution, undirected BFS depth ≤ 2, boosts 1.5 / 1.3 / 1.15; unresolved → no boost; near-only query → empty
- [x] 7.3 Tests for every "Graph and recency priors" scenario; re-run the eval and tune prior weights within the bound

## 8. Result presentation (ft-core)

- [x] 8.1 Extend `SearchResult` with `title`, `heading_path`, `highlights` (byte ranges into `text`, merged/sorted), `reasons`; compute highlights only for the returned (post-limit) docs via `fold_with_offsets`
- [x] 8.2 `search::group(&[SearchResult]) -> Vec<NoteGroup>` (best-member order, members in result order) + tests
- [x] 8.3 Highlight tests: accented match maps back to the original bytes (`Ônibus`), phrase spans, fuzzy hits, title-only match yields no body highlight

## 9. CLI

- [x] 9.1 `ft notes search`: same `build_with` index as the TUI, graph built only for `scope:`; map `UnknownScope` to exit 2 with available names; print the undated-paragraphs warning once on stderr
- [x] 9.2 Grouped text output (header: path + title-if-not-stem; rows: `L…`, heading path, matched labels, 160-char snippet, highlight color on TTY); `--flat` keeps today's exact format
- [x] 9.3 JSON: keep existing keys, add `title`, `heading_path`, `highlights`, `reasons`; `date` also present with a date filter
- [x] 9.4 `ft notes synth scaffold --search`: same index (shared `run_query`) and error mapping; ordering unchanged (flat result order)
- [x] 9.5 `assert_cmd` integration tests + insta snapshots for grouped, `--flat`, and `--json` output; scenarios from "ft notes search command" incl. exclude prefixes and unknown scope

## 10. TUI

- [x] 10.1 `tui/snapshot.rs`: build the index with `build_with` and the `[search]` config (the snapshot graph serves `scope:`)
- [x] 10.2 Search tab row model (`Row::Header` / `Row::Item`), grouped by default, selection keyed by result index; `Space` on a header toggles its members, `Enter` on a header opens the first member; send-to-synth ships in result order
- [x] 10.3 Rows show line range + heading path + matched labels; preview renders highlights with a new `palette` match style; status bar adds filter count and grouped/flat; `SearchError` shown in the status bar with empty results
- [x] 10.4 `search.toggle-grouping` command on `g` in `SEARCH_COMMANDS` / `SEARCH_KEYMAP`, `help_sections()` entry, `dispatch_command` arm; regenerate `docs/keybindings.md`
- [x] 10.5 `TestBackend` snapshots: grouped list, flat list, highlighted preview, unknown-scope status

## 11. Performance, docs, finish

- [x] 11.1 Extend the `FT_PERF_TESTS` gate with the `path:` + `near:` query; confirm < 10 ms at 5k paragraphs; time `ft notes search` on the real vault before/after and note the graph-build cost
- [x] 11.2 Raise `baseline.toml` thresholds to achieved − 0.01; record before/after means (fixture + real vault) in the change notes
- [x] 11.3 Docs: `docs/architecture.md` §Synthesis (index fields, BM25F, filters, priors, `group`), README search section + query reference (filters, `near:`, `[search] stemmers`, ranking change note), `CLAUDE.md` synthesis bullet if the API names changed
- [x] 11.4 Build invariants: `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `cargo run --release -q -- commands docs --check`

## Context

`ft_core::search` holds two engines. This change touches only the
**paragraph engine** (`search/{index,engine,query}.rs`) behind
`ft notes search`, the Search tab, and `synth scaffold --search`. The
file/heading fuzzy finder in `search/mod.rs` (`fuzzy_find`, nucleo) is
left alone.

Current state:

- **Index.** `SearchIndex::build(scan, exclude)` turns every
  `ParsedFile.paragraphs` entry into a doc with lowercased text, a token
  dictionary (ASCII alphanumeric runs plus `[[…]]` link targets) with
  postings that record no frequencies, and a trigram map for fuzzy
  matching.
- **Matching.** Matching is set-based per clause. Substring and phrase
  clauses scan every doc's folded text.
- **Scoring.** The score is `Σ mode_weight × occurrence_boost +
  position`.
- **Where the index lives.** The TUI builds the index once per snapshot
  (`tui/snapshot.rs`), alongside `Graph::build` from the same scan. The
  CLI builds it per invocation and never builds a graph.

Measured on the real vault (2,047 notes):

- `ft notes search memory` takes 0.3 s wall time, dominated by the scan.
- `=ônibus` returns 0 results and `=nibus` returns 6 (ASCII tokenizer).
- `=memory` returns 61 results and `=memories` returns 12 (no
  stemming).
- `ft graph query` (scan + graph) takes about 0.54 s.

Constraints:

- **Per-keystroke latency.** The TUI re-queries synchronously on every
  keystroke, so the 10 ms / 5k-paragraph budget is load-bearing.
- **Determinism.** `FT_TODAY` must be honoured, and the ranking must not
  depend on wall-clock or checkout mtimes, so fixture tests are stable.
- **No runtime downloads and no async.**
- **Spec dependency.** `remove-pulse-and-recent` (complete, not yet
  archived) rewrites the `Search TUI tab` requirement. This change's
  delta is based on that rewritten text.

## Goals / Non-Goals

**Goals:**

- Measurable relevance: an eval harness with a ratchet in `cargo test`,
  built before the ranking changes land.
- BM25F ranking over the body plus note context fields.
- Unicode tokenization, diacritic folding, and optional Snowball
  stemming.
- In-query filters, including graph-preset scoping.
- Bounded graph and recency priors, and a `near:` proximity boost.
- Highlighted snippets, heading paths, grouping by note, and per-clause
  match reasons, all identical between CLI and TUI.

**Non-Goals:**

- **Embeddings, re-ranking, and query expansion.** These come in a
  follow-up change, which fuses an embedding ranking with this one via
  Reciprocal Rank Fusion.
- **Flagging already-synthesized paragraphs.** `CitationIndex` was just
  removed.
- **A persistent on-disk index**, and **changes to `fuzzy_find`**.

## Decisions

### 1. Eval harness first, baseline recorded from today's engine

The first task builds the fixture vault, the judgments, and the metrics.
It then records the **current** engine's nDCG@10 and MRR as the
baseline. Every later task must keep the ratchet green, and the final
task raises the thresholds.

- **Why first:** without numbers, tuning the field weights, `k1`, `b`,
  and the prior strength is guesswork, and "better" is unfalsifiable.
- **Judgment identity is `path + line ∈ range`**, not `line_start`, so a
  paragraph-boundary fix in `extract_paragraphs` doesn't invalidate the
  judgments.
- **Fixture location:** `tests/fixtures/search-eval/`, next to `tiny/`,
  `realistic/`, and `pathological/`. The test is
  `ft-core/tests/search_eval.rs`, plus a shared `search_eval` helper
  module so the real-vault run reuses it.
- **Alternatives considered:**
  - *Snapshot tests of result order:* these catch changes, not
    regressions in quality.
  - *Eval only against the real vault:* not reproducible in CI, and the
    vault is private.

### 2. Every clause resolves to dictionary tokens; BM25F scores tokens

The current engine scans text for substring clauses. Instead, each
**single-run** clause (a term with no non-alphanumeric characters)
resolves to a set of token ids per field:

| Clause | Resolves to |
|---|---|
| word | the exact token, plus stem-mates |
| link | the link token (body field only) |
| fuzzy | trigram candidates, then a levenshtein check |
| substring | every dictionary token containing the term (trigram-narrowed when the term is 3 characters or more, otherwise a dictionary scan) |

Postings store `(doc, tf)`. The clause's contribution for a doc is the
**max** over its resolved tokens of the BM25F token score, not the sum,
so `mem` matching both `memory` and `memo` doesn't double-count.

**Multi-run** clauses (phrases, and substrings such as `eigen-dec` or
`foo bar`) keep the folded-text scan, with occurrence count as `tf` and
the matched-doc count as document frequency.

BM25F, per token `t` and doc `d`:

- `tf̃ = Σ_f w_f · tf_{f,d} / (1 − b_f + b_f · len_{f,d} / avglen_f)`
- `score = idf(t) · tf̃ / (k1 + tf̃)`
- Starting weights: body 1.0, title 3.0, alias 2.5, heading 2.0, tag 2.0.
- Starting `b`: body 0.75; title, alias, and tag 0.0 (short fields);
  heading 0.3.
- `k1 = 1.2`.
- Mode weights: phrase 1.2, word 1.0, link 1.0, substring 0.85,
  fuzzy 0.6.

These are starting values for the harness to tune; they live as named
`const`s in `engine.rs`.

- **Why tokens:** the dictionary scan is O(dictionary), not O(corpus
  text). Frequencies and IDF then come for free, and the trigram map
  already exists.
- **Why max, not sum:** substring expansion can hit dozens of tokens
  (`mem` → memory, memo, member, …), and a sum would reward short,
  promiscuous prefixes.
- **Alternatives considered:**
  - *Tantivy:* too heavy for a per-keystroke in-memory index. It is also
    a second tokenizer to keep in sync with link and tag semantics, and a
    persistent index this change does not want.
  - *Keeping the additive weights plus a length penalty:* fails the
    rare-vs-common probe.

### 3. Context fields are per-note, shared, not copied per paragraph

Title, aliases, and tags (frontmatter) are per note. The heading path is
per heading section. The index therefore stores:

- `NoteCtx { title_tokens, alias_tokens, fm_tag_tokens, prior }`, one
  per file;
- `SectionCtx { heading_path, heading_tokens }`, one per distinct
  heading path;
- per doc: a `note_id`, a `section_id`, and its own body tokens plus
  inline-tag tokens.

Field postings for title, alias, heading, and tag are keyed by
`note_id` or `section_id` and expanded to doc ids at match time through
`note → doc range` and `section → doc range` tables. Docs are appended in
file order, so these ranges are contiguous.

- **Why:** copying the note title into every paragraph would multiply
  index size by the paragraphs-per-note count and distort IDF (a title
  token would look common).
- **IDF for context fields** is computed over notes for title, alias,
  and frontmatter tags, and over sections for headings. This is the
  per-field-statistics form of BM25F, and it keeps a title word that
  appears in one note rare.

**Matching rule.** Positive clauses OR across fields. Excludes are
body-only, because excluding by title would silently drop whole notes —
`-task` would remove every paragraph of `Task Notes.md`. Link clauses
are body-only, because a link is an explicit act in prose.

**Readers.**

- **Frontmatter:** `frontmatter::aliases_in(fm)` and
  `frontmatter::tags_in(fm)`. Both read the block-list and inline-list
  forms (`[a, b]`) and a scalar string, and strip a leading `#`. They
  reuse the existing line-based frontmatter reader style; no YAML crate
  is added.
- **Inline tags:** `#` preceded by line start or whitespace, followed by
  `[\p{Alnum}_/-]+` containing at least one non-digit. Paragraph
  extraction already skips code fences. Heading markers (`# `) are not
  tags, because a tag needs no space after the `#`.

### 4. Folding and tokenization live in one module, `search/text.rs`

- **`fold(s) -> String`:** lowercase, NFD, drop `\p{Mn}` (via the
  `unicode-normalization` crate), returning a folded string plus a
  `Vec<u32>` **offset map** from folded byte index to original byte
  index. The map is needed only for highlighting: it is computed lazily
  for result docs, not stored for the whole index.
- **`tokens(folded)`:** splits on `char::is_alphanumeric` runs and pulls
  out `[[…]]` link targets (the existing logic, made Unicode-aware) and
  inline tags.
- **Stemming:** `rust-stemmers` (Snowball). The **unfolded lowercase**
  token is stemmed and the stem is then folded, because Snowball
  Portuguese and German need their accents to apply suffix rules. Stem
  maps are `stem → token ids`, one per configured algorithm. A `=word`
  clause unions the token ids sharing its stem under each configured
  stemmer.
- **Why stem only word clauses:** substring already covers prefixes
  (`memor`), and fuzzy and phrase are explicit intents. Stemming the
  default mode would change set semantics for every existing query.
- **Default `["english"]`:** most English vault prose benefits.
  Portuguese and German are opt-in, because an aggressive stemmer on the
  wrong language causes false conflations (for example, English words
  run through the Portuguese stemmer).
- The query side goes through the same `fold`, which makes the trigram
  index Unicode-safe as well (it is already char-based).

### 5. Filters evaluate after text matching; dates last

Order of evaluation:

1. Positive text clauses produce a candidate set (AND or OR).
2. Cheap filters (`path:`, `tag:`, `scope:`) intersect it.
3. Excludes subtract.
4. Date filters blame only the files of the survivors (`BlameCache`,
   shared with `--sort date`).

A filter-only query starts from all docs.

- **`scope:<preset>`** resolves the preset (user `GraphCfg::presets`,
  then `graph::preset::builtin`), parses it with the graph DSL, and runs
  `GraphQuery::walk` with default `WalkOptions` against the index's
  graph. The set of note paths visited becomes a `HashSet<note_id>`.
  `walk` is used rather than `select`, so `scope:work` matches exactly
  what the Graph tab shows for that preset.
- **No graph:** the scope filter fails with `SearchError::NoGraph`. The
  CLI and TUI always supply one, so this occurs only for direct
  library calls.
- **Date values** go through `dates::parse_date_value` against
  `dates::today()` (the graph DSL's closed date grammar, `FT_TODAY`
  aware). An invalid value degrades to a literal substring term at
  parse time, keeping the parser total.
- **Search becomes fallible.** `search(index, query, opts) ->
  Result<Vec<SearchResult>, SearchError>`, with `UnknownScope { name,
  available }` and `NoGraph` as the error variants. The CLI maps
  `UnknownScope` to exit 2, consistent with the `--preset`
  unknown-name convention.
- **Alternative considered:** embedding the graph DSL inline
  (`node where path starts_with …`) inside search queries. It is verbose
  on a one-line input and ambiguous with free text. Presets give the
  full DSL power behind one short token.

### 6. Priors are bounded, relative, and computed once per index build

`SearchIndex::build_with_graph(scan, exclude, Some(&graph), &cfg)` is
the new constructor. `build(scan, exclude)` remains as the no-graph form
used by unit tests.

- **In-degree:** `Graph::incoming(note).count()`, excluding
  directory-containment edges if present.
- **Prior formula:**
  `prior = 1 + 0.15 · ln(1+in)/ln(1+max_in) + 0.10 · exp(−age_days/90)`.
  Each part is clamped to its share, so the total lies in `[1, 1.25]`.
- **`age_days`** is measured as `newest_mtime − mtime`, not
  `today − mtime`. Fixture copies all share one mtime, so recency is
  neutral in tests, and `FT_TODAY` needs no special-casing. The cost is
  that a vault with nothing touched for a year still has a "newest"
  note. That is acceptable: recency is a tiebreaker, not a filter.
- **`near:[[X]]`** resolves `X` through the graph's existing note-name
  resolution, then runs a breadth-first search to depth 2 over
  undirected edges at query time (cheap at depth 2). It returns
  `note_id → boost`.
- **Bound:** the priors cap of 1.25 is below the 1/0.8 = 1.25 ratio in
  the "prior is bounded" scenario, so a 0.8× text match can tie with a
  maxed prior but never overtake a strong match. The harness tunes
  within this bound.
- **The CLI now builds the graph** (about 0.2 s on 2k notes). The
  alternative was a scan-only in-degree approximation, but name
  resolution lives in `Graph::build`, and duplicating it would let the
  CLI and TUI disagree.

### 7. Results carry presentation data; rendering stays in `ft`

`SearchResult` gains:

- `title: String`
- `heading_path: Vec<String>`
- `highlights: Vec<Range<usize>>` (byte ranges into `text`, merged and
  sorted)
- `reasons: Vec<MatchReason { clause: String, field: Field }>`

Highlights are computed only for the returned docs (after the limit),
with these rules:

- **Word, link, substring, and fuzzy clauses:** the token or substring
  occurrences in the folded body, mapped back through the offset map.
- **Phrase clauses:** the folded-text match positions.
- **Context-field-only matches:** no body highlight; the reason records
  the field.

Grouping is a pure function in `ft-core`, `search::group(results) ->
Vec<NoteGroup>`, so the CLI and TUI share the ordering rule (group by
best member, members in result order). The snippet window (160
characters, word-boundary trim, ellipses) is an `ft` output concern in
`cmd/search.rs`. The TUI preview shows the whole paragraph with
highlights, using a new `palette` match style.

`--flat` keeps the current text format exactly, which gives scripts an
escape hatch. JSON stays flat (one object per paragraph) with additive
keys, so existing consumers keep working.

### 8. TUI grouping uses a row model, not new tab state

The tab's list becomes `Vec<Row>` with `Row::Header { note, members:
Range<usize> }` and `Row::Item(result_idx)`, derived from the results
and the grouped flag. Cursor movement visits both row kinds:

- `Space` on a header toggles all its members;
- `Enter` on a header opens the first member;
- selection is stored by result index, so toggling `g` keeps it.

The new command is `search.toggle-grouping` on `g`, registered in
`SEARCH_COMMANDS` and `SEARCH_KEYMAP`; `docs/keybindings.md` is
regenerated. The status bar shows the filter count and the
`grouped`/`flat` mode. A `SearchError` renders in the status bar with
empty results.

## Risks / Trade-offs

- **[Context-field matching enlarges result sets]** `memoization` now
  returns every paragraph of `Memoization.md`. → Grouping collapses
  them visually, and BM25F ranks body hits above title-only hits within
  the note (the body adds to the title score). `s`/`S` with no selection
  still ship everything, which is unchanged behaviour, but the status
  bar count makes the size visible.
- **[Substring-to-token expansion misses cross-token substrings]**
  `o fo` or `eigen-dec`. → These contain non-alphanumeric characters,
  so they take the multi-run text-scan path. Covered by a unit test and
  an eval case.
- **[Ranking churn surprises users]** → This is the intended breaking
  change, measured by the harness. Date sort is unaffected, and
  `scaffold --search` ordering changes only for relevance.
- **[CLI latency +0.2 s from the graph build]** → Acceptable for an
  interactive command. If it bites, the follow-up persistent index will
  absorb it. Measure before and after in the perf task.
- **[Per-keystroke budget with fold, BM25F, and highlights]** →
  Highlights are computed only for the visible or limited results, the
  offset map is lazy, and dictionary scans are trigram-narrowed. The
  perf gate gains `path:` + `near:` queries.
- **[Stemmer false conflations]** (English `university`/`universe` →
  `univers`) → Stemming is limited to `=word`, where the user asked for
  word semantics, and it can be disabled with `stemmers = []`.
- **[Frontmatter readers are hand-rolled]** → Cover the block-list,
  inline-list, scalar, and quoted forms with unit tests. Anything
  unparseable yields no aliases or tags, never an error.
- **[Eval fixture overfits the weights]** → Keep the cases
  probe-oriented (one failure class each) and run the real-vault report
  before finalizing the weights.

## Migration Plan

There are no data migrations; the index is in-memory and rebuilt per
scan.

- **Config:** `[search] stemmers` is new and optional, with a default.
- **Ordering:** the user-visible ranking change ships in one release,
  noted in the README. `--flat` preserves the old text shape.
- **Rollback:** revert the commit. No on-disk state is introduced; the
  blame cache is unchanged.

## Open Questions

- **Note title source.** The file stem is used. Should a frontmatter
  `title:` override it? Not used in the real vault today; default to the
  stem and revisit.
- **Default stemmers.** Should they include `"portuguese"` for this
  vault's owner? It is a config choice, not a code default; recommend
  setting it in the user's config after measuring with the real-vault
  eval.
- **`near:` from the TUI.** Should the Graph tab's `J` handoff add
  `near:[[current]]` automatically? This is deferred to keep the
  `graph-to-search-jump` spec unchanged in this change.

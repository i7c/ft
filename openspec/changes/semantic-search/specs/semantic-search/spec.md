## ADDED Requirements

### Requirement: Optional build feature

Semantic search SHALL be compiled behind a Cargo feature `semantic` on `ft-core`
and `ft`, enabled by default, depending on `model2vec-rs` with only `local-only`
(no Hugging Face hub client) and on `tokenizers` with only its `fancy-regex`
backend. The feature SHALL NOT require a C++ toolchain: the dependency graph
SHALL NOT enable `onig` or `esaxx-rs/cpp`, and it SHALL add no native compile
steps beyond those ft already has (`blake3`, `ring`). A build without the feature SHALL
compile and behave exactly like lexical search; with `[search.semantic] enabled =
true` it SHALL print one warning that semantic search is not compiled in and
continue lexically.

#### Scenario: No C++ toolchain needed

- **WHEN** `cargo tree -p ft -e features -i esaxx-rs` runs on the default feature set
- **THEN** the `cpp` feature of `esaxx-rs` is not enabled and no `onig` crate is in the graph

#### Scenario: Build without the feature

- **WHEN** `cargo build --no-default-features --features <the rest>` is run and a vault enables semantic search
- **THEN** the build succeeds, `ft notes search` returns lexical results, and stderr carries one "not compiled in" warning

### Requirement: Semantic configuration

The `[search.semantic]` config table SHALL accept `enabled` (bool, default
`false`), `model` (a Hugging Face model id or a local directory path, default
`"minishlab/potion-base-8M"`), `candidates` (integer ≥ 1, default 50), and
`min_similarity` (float in `[-1, 1]`, default tuned against the evaluation
harness). Unknown keys and out-of-range values SHALL be config load errors.

#### Scenario: Defaults

- **WHEN** the config has no `[search.semantic]` table
- **THEN** semantic search is disabled and the model id is `minishlab/potion-base-8M`

#### Scenario: Invalid candidates

- **WHEN** `[search.semantic] candidates = 0`
- **THEN** loading the config fails with an error naming `candidates`

### Requirement: Model fetch and resolution

`ft notes embed fetch [--model <id>]` SHALL download `config.json`,
`tokenizer.json`, and `model.safetensors` for a Hugging Face model id from
`https://huggingface.co/<id>/resolve/main/<file>` into
`<user cache dir>/ft/models/<id>/`, writing each file atomically and printing the
total size. A `model` value that names an existing local directory SHALL be used
as-is and never fetched. Search, `embed build`, and the TUI SHALL NOT download;
when the model is missing they SHALL degrade to lexical search with one hint
naming `ft notes embed fetch`. `ft notes embed status` SHALL print the resolved
model location, whether it is present, its embedding dimension, and the vault's
embedding coverage (embedded paragraphs / indexed paragraphs).

#### Scenario: Fetch then search

- **WHEN** `ft notes embed fetch` completes and `ft notes search "how to stay focused" --semantic` runs
- **THEN** the model loads from the cache dir and the search includes semantic candidates

#### Scenario: Missing model degrades

- **WHEN** semantic search is enabled but the model was never fetched
- **THEN** `ft notes search cache` returns lexical results and prints one hint to run `ft notes embed fetch`

#### Scenario: Local model directory

- **WHEN** `model = "/opt/models/potion"` names a directory holding the three files
- **THEN** it is loaded directly and `ft notes embed fetch` reports there is nothing to fetch

### Requirement: Embedding cache

Paragraph embeddings SHALL be stored at
`<vault>/.ft/cache/embeddings/<model-slug>.bin` as a versioned msgpack file of
L2-normalized f16 vectors keyed by the blake3 hash of the embedded text. The
embedded text of a paragraph SHALL be its note title, its heading path, and its
body joined by `. `, so context fields inform the vector. Building SHALL embed
only paragraphs whose key is missing, SHALL prune keys not present in the current
scan on save, and SHALL write via the atomic-write path. A cache written for a
different model or dimension SHALL be ignored and rebuilt.
`ft notes embed build` SHALL run this incremental build and report how many
paragraphs were embedded, reused, and pruned.

#### Scenario: Incremental build

- **WHEN** `ft notes embed build` runs twice and one paragraph changes between the runs
- **THEN** the second run embeds exactly one paragraph and reuses the rest

#### Scenario: Model change rebuilds

- **WHEN** the configured model changes
- **THEN** the next build ignores the old cache file and embeds every paragraph

### Requirement: Hybrid retrieval and fusion

A query with at least one positive text clause SHALL, when semantic search is
active (config `enabled` or `--semantic`, model present, embeddings available), be
embedded from its positive text clauses (terms, phrase text, and link targets,
joined by spaces) and compared by cosine similarity against every indexed
paragraph. The top `candidates` paragraphs with similarity ≥ `min_similarity`
SHALL join the lexical result set; filter clauses and exclude clauses SHALL
apply to them exactly as to lexical results. The final order SHALL be Reciprocal
Rank Fusion over the lexical ranking and the semantic ranking
(`Σ 1 / (60 + rank)`), multiplied by the priors and `near:` boost, with the
existing deterministic tiebreak. A paragraph found only semantically SHALL carry
no matched clause labels and a `semantic` match reason; the CLI SHALL mark such
rows with `≈` and JSON SHALL include `{"clause": "≈", "field": "semantic"}` in
`reasons`. With semantic search inactive, results SHALL be identical to lexical
search. Paragraphs with no embedding (for example, added since the last build)
SHALL still be found lexically.

#### Scenario: Concept query finds a paragraph without shared words

- **WHEN** semantic search is active and the query `storing results of expensive function calls` runs over a vault whose `Memoization.md` paragraph says "stores the result of a pure function call"
- **THEN** that paragraph is in the results with a `semantic` reason

#### Scenario: Filters bound semantic candidates

- **WHEN** the query `how to stay focused path:areas/` runs with semantic search active
- **THEN** no semantic candidate outside `areas/` is returned

#### Scenario: Semantic off is lexical

- **WHEN** the same query runs with `--no-semantic`
- **THEN** the results equal those of a build without the feature

#### Scenario: Lexical and semantic agreement ranks first

- **WHEN** one paragraph ranks third lexically and first semantically and another ranks second lexically and is not a semantic candidate
- **THEN** the first paragraph ranks above the second

### Requirement: More like this

`ft notes search --like <path>:<line> [QUERY]` SHALL rank paragraphs by cosine
similarity to the embedding of the paragraph containing that line (excluding the
paragraph itself), applying any filter and exclude clauses in `QUERY` and fusing
with QUERY's lexical ranking when it has positive text clauses. It SHALL require
an available model and embedding, failing with exit 2 and the `embed` hint
otherwise. The Search tab SHALL bind `M` to `search.more-like-this`, replacing
the result list with paragraphs like the focused one (the query box shows
`like:<path>:<line>` as a read-only prefix until edited or cleared).

#### Scenario: CLI more like this

- **WHEN** `ft notes search --like know/Memoization.md:6` runs with embeddings available
- **THEN** results list paragraphs most similar to that paragraph, not including it

#### Scenario: More like this in the TUI

- **WHEN** the user focuses a result and presses `M`
- **THEN** the list shows paragraphs similar to it and the status bar shows the `like:` source

### Requirement: TUI semantic state

After each snapshot install, a background worker SHALL load the embedding cache,
embed missing paragraphs (when the model is present), save, and post
`BgEvent::EmbeddingsReady { generation, embeddings }`; it SHALL stop when a newer
generation lands. The Search tab SHALL bind `m` to `search.toggle-semantic`
(session-scoped, initialized from config) and SHALL show the semantic state in
its status: `≈ off`, `≈ no model`, `≈ building N%`, or `≈ on`. Queries typed
before embeddings are ready SHALL run lexically and re-run when they arrive.

#### Scenario: Toggle semantic

- **WHEN** embeddings are ready and the user presses `m`
- **THEN** the status flips between `≈ on` and `≈ off` and the current query re-runs accordingly

#### Scenario: Re-run on readiness

- **WHEN** the user types a query while the status shows `≈ building`
- **THEN** results are lexical at first and update to hybrid when `EmbeddingsReady` arrives

### Requirement: Semantic evaluation

An `Embedder` trait SHALL abstract the model so unit tests use a deterministic
test embedder. The relevance harness SHALL gain a second judgment file of
concept queries for the fixture vault, run only when `FT_SEMANTIC_TESTS=1` and
the default model is present locally; it SHALL report lexical vs hybrid nDCG@10
and MRR on both judgment files, and SHALL fail when hybrid lowers the mean
nDCG@10 of the existing lexical judgments by more than 0.02. Default `candidates`
and `min_similarity` SHALL be the values recorded from this run.

#### Scenario: Gated semantic eval

- **WHEN** `FT_SEMANTIC_TESTS=1` is set with the model fetched and the harness runs
- **THEN** it prints lexical and hybrid scores for both judgment files and passes only if the lexical judgments did not drop by more than 0.02

#### Scenario: Skipped by default

- **WHEN** `cargo test --workspace` runs without `FT_SEMANTIC_TESTS`
- **THEN** the semantic eval is skipped and no model is required

### Requirement: Semantic performance budget

With embeddings loaded, embedding a query and scoring 20,000 paragraphs SHALL
take under 15 ms (`FT_PERF_TESTS=1` gate, release build). Loading the embedding
cache for 20,000 paragraphs SHALL take under 100 ms. An incremental build SHALL
embed only changed paragraphs.

#### Scenario: Perf gate holds

- **WHEN** `FT_PERF_TESTS=1` runs the semantic perf test over 20,000 synthetic paragraphs with the test embedder's dimension set to 256
- **THEN** query embedding plus scoring completes under 15 ms

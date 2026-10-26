## Context

Paragraph search is lexical: BM25F over the body plus note context, with
filters and priors (`search-relevance-overhaul`). It cannot connect "how to
stay focused" to a *Deep Work* paragraph that never says "focus".

A spike on 2026-10-23 (`model2vec-rs` 0.3.0, a scratch crate on top of
`ft-core`'s scan) measured:

| | potion-base-8M | potion-retrieval-32M |
|-|-|-|
| model files | 30 MB (3 files) | larger, 512-dim |
| load | 77 ms (warm FS) | 220 ms |
| embed 16,326 paragraphs (real vault) | 1.0 s | 1.4 s |
| query (unoptimized brute force) | about 7 ms | about 14 ms |
| fixture "storing results of expensive function calls" | Memoization #1 | not run |
| fixture "undo a bad release" (context-prefixed) | Runbook revert #2 | Runbook revert #1 |
| fixture "how to stay focused" | Deep Work #3 (with context prefix; absent without) | absent from top 3 |
| real "backup" concept query | misses the borgmatic note | misses |

Build cost:

- `model2vec-rs` with `local-only`, plus `tokenizers` with only
  `fancy-regex`, adds 50 crates (179 → 229 for `ft-core`).
- The default features would add 89, including `hf-hub` and `ureq` 2
  alongside ft's `ureq` 3.
- A clean build of the spike took 40 s wall.
- **No C++ toolchain.** Both `model2vec-rs` backend features (`onig`,
  `fancy-regex`) also enable `tokenizers/esaxx_fast`, which compiles a C++
  suffix array that only tokenizer training uses. Enabling the backend on
  `tokenizers` directly avoids it. A clean `-vv` build showed no C++
  compiler invocation and produced identical embeddings (same similarity
  scores, same 1.03 s for the real vault). The only native compiles left
  are `blake3` and `ring`, which ft already has. Checked on Linux only;
  macOS has the same crate graph and needs only the C compiler Xcode's
  command-line tools already provide for Rust linking.

The conclusion: semantic recall is real but noisy. It belongs as an opt-in,
fused second retriever, not a replacement.

## Goals / Non-Goals

**Goals:**

- Opt-in hybrid retrieval with measurable impact.
- Model handling that is fully local after one explicit fetch.
- Incremental, persisted embeddings.
- "More like this" for synthesis.
- No behaviour change when off.

**Non-Goals:**

- Re-rankers, query expansion, ONNX, ANN indexes, remote APIs.
- Semantic search on by default.
- A multilingual default model.

## Decisions

### 1. Static Model2Vec embeddings via `model2vec-rs`, not ONNX

- **Why:** it is CPU-only, deterministic and pure inference: one safetensors
  matrix plus a tokenizer. It embeds a 16k-paragraph vault in about a
  second, single-threaded.
- **ONNX / `fastembed`:** better quality, but a native runtime of around
  100 MB, and roughly 100× slower per paragraph without a GPU. That is the
  wrong trade for a CLI that must stay snappy.

### 2. `local-only` + ft-owned download

The crate is compiled with `default-features = false, features =
["local-only"]`, next to a direct `tokenizers = { version = "0.21",
default-features = false, features = ["fancy-regex"] }`. The tokenizer
backend is selected there rather than through `model2vec-rs`'s own
`fancy-regex` / `onig` features, because those also enable
`tokenizers/esaxx_fast` (C++). `fancy-regex` is pure Rust; `onig` would
need a C build of Oniguruma.

`ft notes embed fetch` downloads the three files (`config.json`,
`tokenizer.json`, `model.safetensors`) from
`https://huggingface.co/<id>/resolve/main/<file>`:

- with ft's existing `ureq` 3;
- into `dirs::cache_dir()/ft/models/<id>/`;
- each file via the atomic-write path.

Then it loads them with `StaticModel::from_pretrained(<dir>, None, None,
None)`.

- **Why:** this avoids a second HTTP stack and 34 crates. Every network
  access is an explicit user command, and searches never download.
- **Alternative considered:** the `hf-hub` default (shared HF cache). It was
  rejected for the duplicate `ureq`, and because downloads would happen
  implicitly on first search.

### 3. Context-prefixed embedding text

The embedded text is `"{title}. {heading path joined by ' '}. {body}"`. In the
spike this moved the *Deep Work* paragraph from absent to #3 for "how to stay
focused". It mirrors BM25F's context fields: a paragraph's meaning includes
where it lives.

The cache key is `blake3(embedded text)`. A renamed note or a moved heading
changes the key, which is correct because the meaning context changed.

### 4. Cache: versioned msgpack of normalized f16 vectors

The cache lives at `<vault>/.ft/cache/embeddings/<model-slug>.bin`:

```text
{ version, model_id, dim, entries: Vec<(key: [u8; 16], vec: Vec<u16 /* f16 bits */>)> }
```

At 16k × 256 dims that is about 8 MB. It is loaded into memory as a dense
`Vec<f32>` matrix aligned with doc ids (about 16 MB), plus a
`doc → Option<row>` map. Vectors are L2-normalized at build, so cosine
similarity is a dot product. Writes go through `write_atomic_bytes`.

- **Why f16 on disk:** half the size, and the precision loss is far below
  the noise of a static model.
- **Why no ANN:** 20k × 256 dot products is about 5M multiply-adds,
  roughly 2 ms. An index only pays off past about 100k paragraphs.

### 5. Hybrid fusion by Reciprocal Rank Fusion

The lexical engine runs as today and produces its ranking. The semantic side
takes these steps:

1. Embed the query text: the positive text clauses' terms, phrase contents
   and link targets, joined.
2. Score all docs.
3. Keep the top `candidates` with similarity ≥ `min_similarity`.
4. Apply filters and excludes.

The union is ordered by weighted RRF, `1/(60 + lexical rank) + 0.5/(60 +
semantic rank)`, then multiplied by the note prior, paragraph recency and the
`near:` boost. Ties break as today.

*Revised during implementation.* Equal weights let the noisy static-embedding
list reorder confident lexical results: the fixture's lexical judgments fell
from nDCG@10 0.910 to 0.873, which failed the 0.02 no-regression gate, and no
`candidates` / `min_similarity` setting fixed it (the best was 0.873). The
semantic weight was swept over 1.0, 0.7, 0.5, 0.35 and 0.25. At 0.5 the
lexical judgments *improve* to 0.927 (MRR 0.983), and the concept judgments go
from 0.066 to 0.632. Weights below 0.5 change nothing further.

- **Why RRF:** BM25F scores and cosine similarities live on incomparable
  scales. RRF needs no calibration, is the standard in hybrid search (used
  by VaultSearch and qmd, per the earlier research), and keeps agreement
  between the two retrievers on top.
- **Alternative considered:** a weighted score sum. It needs per-vault
  normalization and tuning.

`SearchResult.score` becomes the fused score when semantic is active. The
date sort still sorts by date.

A semantic-only hit gets `matched = []` and a `MatchReason { clause: "≈",
field: Field::Semantic }`. The CLI marks it with `≈`, and highlights stay
empty: there is no term to highlight.

### 6. Integration seam: the `Embedder` trait and `SemanticIndex`

These live in `ft_core::search::semantic`:

- `trait Embedder { fn id(&self) -> &str; fn dim(&self) -> usize; fn
  embed(&self, texts: &[String]) -> Vec<Vec<f32>>; }`
- `Model2VecEmbedder`, behind the `semantic` feature;
- `HashEmbedder`, a deterministic test implementation (a hashed bag of
  words), always compiled for tests;
- `SemanticIndex { rows: Vec<f32>, dim, doc_rows: Vec<Option<u32>>,
  doc_count }`, built from a `SearchIndex`, an embedder and a cache
  (incremental).

`SearchCtx` gains:

- `semantic: Option<SemanticQuery<'a>>`, which holds the index plus an
  embedder handle for the query;
- `like: Option<doc>` for more-like-this.

The engine stays agnostic of model2vec, so the core compiles without the
feature.

### 7. TUI worker and state

On `GraphReady`, an embeddings worker runs. It owns the model handle (loaded
once per App and kept in an `Arc` behind the feature), the snapshot `Arc` and
the generation. It:

1. loads the cache;
2. embeds missing paragraphs in batches of 1,024, posting progress;
3. saves;
4. posts `EmbeddingsReady { generation, Arc<SemanticIndex> }`.

The Search tab keeps `semantic_on`, initialized from config. `m` toggles it.
It runs lexically until a matching `EmbeddingsReady` arrives, then re-runs.

`M` sets a `like` source (the focused result's doc), shows `like:<path>:<line>`
as a dim prefix in the query box, and clears on `c` or on edit.

### 8. Evaluation gates the defaults

A new file, `tests/fixtures/search-eval/semantic.toml`, holds about 12
concept queries over the existing fixture vault (no shared words with their
targets). The harness gains a `--hybrid` mode, run when
`FT_SEMANTIC_TESTS=1` and the default model sits in the cache dir. It:

- reports lexical vs hybrid on both judgment files;
- asserts hybrid does not lower the lexical judgments' mean nDCG@10 by more
  than 0.02;
- records the chosen `min_similarity` and `candidates`.

CI never needs the model. Unit tests use `HashEmbedder`.

## Risks / Trade-offs

- **[Static embeddings are noisy; the spike shows misses]** → Off by default,
  fused rather than replacing, gated by the no-regression check, with
  meaning-only hits marked `≈` so users learn to read them.
- **[With semantic on, AND stops bounding the results]** → This is the point
  of recall. It is documented, marked, and switches off per query with
  `--no-semantic` / `m`.
- **[A future `model2vec-rs` / `tokenizers` release re-enables
  `esaxx_fast` or `onig` transitively]** → A build-invariant check
  (`cargo tree -e features`) fails CI when either appears.
- **[Adds 50 crates]** → The feature can be disabled. Measure release
  binary growth in the tasks and document it. The spike binary was
  7.8 MB including `ft-core`.
- **[English model on a mixed English/Portuguese vault]** →
  `potion-multilingual-128M` is configurable. The default stays small. The
  real-vault report decides whether to recommend it.
- **[Cache keyed on embedded text: renaming a note re-embeds all its
  paragraphs]** → Cheap (about 60 µs per paragraph).
- **[First TUI launch embeds the whole vault, about 1–2 s of CPU in the
  background]** → Fine. The cache makes later launches incremental.

## Migration Plan

The change is additive: the config defaults to off, and cache files are new.
To roll back, revert the commit; the cache files are inert and can be
deleted.

## Open Questions

- **Default model:** resolved. `potion-base-8M` at `min_similarity` 0.2
  (lexical 0.927, concept 0.632). `potion-retrieval-32M`'s best setting
  (0.15) was comparable (0.933 / 0.630) at four times the size and twice the
  dimension. The similarity floor is model-specific, so a user switching
  models should re-tune it.
- **Should `semantic` be a default Cargo feature?** Proposed yes. With no
  C++ requirement, the remaining cost is 50 crates of compile time, and it
  stays off at runtime.

# Tasks: semantic search

## 1. Evaluation first

- [x] 1.1 Write `tests/fixtures/search-eval/semantic.toml`: about 12 concept queries over the existing fixture vault whose targets share no words with the query (extend `generate.py` with any notes they need, keeping the lexical judgments valid)
- [x] 1.2 Harness: a `--hybrid` path gated by `FT_SEMANTIC_TESTS=1` + model present; report lexical vs hybrid on both judgment files; assert the lexical judgments do not drop more than 0.02 nDCG@10
- [x] 1.3 Record lexical-only scores on `semantic.toml` (the expected low baseline) in the change notes

## 2. Core seam (no model yet)

- [x] 2.1 `search/semantic/mod.rs`: `Embedder` trait, `HashEmbedder` (deterministic hashed bag of words, configurable dim), `SemanticIndex` (dense normalized `f32` rows aligned to doc ids, `doc_count`)
- [x] 2.2 Embedded text = title + heading path + body; `blake3` key; incremental build from `SearchIndex` + cache + embedder
- [x] 2.3 `search/semantic/cache.rs`: versioned msgpack of f16 vectors at `.ft/cache/embeddings/<model-slug>.bin`; model/dim mismatch ignored; prune on save; atomic write; tests (round-trip, incremental reuse, prune, model change)
- [x] 2.4 `search/semantic/fusion.rs`: query text from positive text clauses; cosine top-k ≥ `min_similarity`; filters and excludes applied to candidates; RRF (k = 60) then prior × recency × near; `Field::Semantic` reason; tests with `HashEmbedder` for every "Hybrid retrieval and fusion" scenario
- [x] 2.5 `SearchCtx::{semantic, like}`; with semantic absent, results are byte-identical to today (regression test over the fixture)
- [x] 2.6 More-like-this ranking (excludes the source paragraph, fuses with QUERY's lexical ranking when present)

## 3. Model and config

- [x] 3.1 `Cargo.toml`: optional `model2vec-rs = { default-features = false, features = ["local-only"] }` and `tokenizers = { version = "0.21", default-features = false, features = ["fancy-regex"] }` behind feature `semantic` (default on) in `ft-core` and `ft`; do NOT enable `model2vec-rs`'s `fancy-regex` / `onig` features (they turn on the C++ `esaxx_fast`); CI and `--no-default-features` build stays green
- [x] 3.1a No-C++ guard: a check (script or test) failing when `cargo tree -p ft -e features -i esaxx-rs` shows `cpp`, or when any `onig` crate is in the graph; wire it into the build invariants
- [x] 3.2 `Model2VecEmbedder` (load from a local dir via `StaticModel::from_pretrained`)
- [x] 3.3 `SearchCfg::semantic` (`enabled`, `model`, `candidates`, `min_similarity`) with validation and config tests; the "not compiled in" warning path
- [x] 3.4 Model resolution: local dir as-is, else `dirs::cache_dir()/ft/models/<id>/`

## 4. CLI

- [x] 4.1 `ft notes embed fetch [--model]` (`ureq` 3, three files, atomic writes, size report), `status` (location, presence, dim, coverage), `build` (embedded / reused / pruned counts)
- [x] 4.2 `ft notes search --semantic/--no-semantic/--like <path>:<line>`; `≈` row marker (grouped and flat); JSON `semantic` reason; missing-model hint; `--like` without a model exits 2
- [x] 4.3 `assert_cmd` tests with a tiny local test model directory or `HashEmbedder` via a test-only env hook; snapshots for `≈` rows

## 5. TUI

- [x] 5.1 Embeddings worker (load the model once per App, embed missing paragraphs in batches, save, post `EmbeddingsReady { generation, Arc<SemanticIndex> }` and progress); cancel on a newer generation
- [x] 5.2 Search tab: `m` toggle (`search.toggle-semantic`), `M` more-like-this (`search.more-like-this`) with the `like:` prefix, status `≈ off / no model / building N% / on`, `≈` row marker, re-run on readiness
- [x] 5.3 `COMMANDS` / `KEYMAP` / help rows; regenerate `docs/keybindings.md`; `TestBackend` snapshots (hybrid rows, building status, like prefix)

## 6. Tune, measure, document

- [x] 6.1 Run the gated semantic eval with `potion-base-8M` and `potion-retrieval-32M`; choose the default model, `candidates` and `min_similarity`; record them in the change notes
- [x] 6.2 Real-vault report: lexical vs hybrid on the private judgments plus a few concept queries; note whether `potion-multilingual-128M` is worth recommending for mixed-language vaults
- [x] 6.3 `FT_PERF_TESTS` semantic gate (20k paragraphs, < 15 ms per query; cache load < 100 ms); measure release binary size growth and clean-build time; record them
- [x] 6.4 Docs: `docs/guide/synthesis.md` (semantic search, `embed` commands, `≈`, more like this), `docs/config.md` (`[search.semantic]`), `docs/architecture.md` §Synthesis, README pointer, `docs/guide/install.md` (the `semantic` feature; no extra toolchain needed)
- [x] 6.5 Build invariants: `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `cargo run --release -q -- commands docs --check`, plus `cargo build --no-default-features --features <non-semantic set>` and the no-C++ guard

## Why

Lexical search only finds the words you remember. Synthesis is about
concepts: "how to stay focused" should surface the *Deep Work* routine,
"undo a bad release" the *Rollback* runbook paragraph, even when they share
no words.

A spike on 2026-10-23 measured static Model2Vec embeddings (`model2vec-rs`
0.3, `potion-base-8M` and `potion-retrieval-32M`):

- **Embedding is cheap:** all 16,326 paragraphs of the real vault in 1.0 s
  with the 8M model (1.4 s with the 32M model), single-threaded.
- **Querying is cheap:** about 7 ms brute force before any optimization.
- **Quality is complementary, not a replacement.**
  - Concept queries with related vocabulary land well: "storing results of
    expensive function calls" puts *Memoization* first.
  - Some miss entirely, for example a backup query never reaches the
    *borgmatic* note.
  - Prefixing each paragraph with its note title and headings helped.

So the right shape is **hybrid**: meaning-based candidates fused with the
BM25F ranking, opt-in, and gated by the relevance harness.

## What Changes

- **Optional Cargo feature `semantic`** (default on) in `ft-core` and `ft`,
  using `model2vec-rs` with only `local-only`, plus a direct `tokenizers`
  dependency with only its pure-Rust `fancy-regex` backend.
  - No `hf-hub` / `ureq` 2: ft fetches model files with its existing
    `ureq` 3.
  - About 50 extra crates and **no C++ toolchain**. Both of
    `model2vec-rs`'s backend features (`onig`, `fancy-regex`) also switch
    on `tokenizers/esaxx_fast`, a C++ suffix array that only tokenizer
    *training* uses. Selecting the backend on `tokenizers` directly keeps
    it off; `esaxx-rs` then builds as pure Rust. Verified by a clean
    build: no C++ compiler invocation, identical embeddings. The only
    native compiles left are `blake3` and `ring`, which ft already
    builds today.
- **`[search.semantic]` config**, off by default:
  - `enabled` (false);
  - `model`, a Hugging Face model id or a local directory, default
    `minishlab/potion-base-8M`;
  - `candidates` (50);
  - `min_similarity`.
- **Model management under `ft notes embed`:**
  - `fetch` downloads the model's `config.json`, `tokenizer.json` and
    `model.safetensors` (about 30 MB for the default) into ft's cache dir;
  - `status` shows model presence and embedding coverage;
  - `build` embeds new or changed paragraphs.
  - Searches never download anything.
- **Embedding cache** at `<vault>/.ft/cache/embeddings/<model>.bin`.
  - One normalized f16 vector per paragraph, keyed by a blake3 hash of the
    embedded text (note title + heading path + paragraph).
  - Incremental, and pruned to the current scan.
- **Hybrid ranking.**
  - When semantic search is on, the query's text terms are embedded and the
    top `candidates` paragraphs by cosine similarity join the lexical
    results.
  - Filters and excludes apply to both.
  - The two rankings fuse by Reciprocal Rank Fusion (k = 60), then priors
    and `near:` apply.
  - Meaning-only hits are marked (`≈`, reason field `semantic`). **BREAKING
    only when enabled:** with semantic on, results can include paragraphs
    that match no text clause, so AND no longer bounds the result set.
- **More like this.**
  - CLI: `ft notes search --like <path>:<line> [query]` ranks paragraphs by
    similarity to that paragraph (the query adds filters and terms).
  - TUI: `M` on the Search tab does the same for the focused result.
- **TUI.** A background worker builds or loads embeddings after each
  snapshot. The Search tab gets `m` to toggle semantic for the session, and
  a status indicator (off / model missing / building N% / ready).
- **CLI flags:** `--semantic` / `--no-semantic` override the config per
  call.

## Non-goals

- Re-ranking with a cross-encoder or LLM, query expansion (HyDE).
- ONNX / `fastembed`, GPU, remote embedding APIs, Ollama.
- An approximate nearest-neighbour index: brute force over normalized
  vectors is well under budget up to about 100k paragraphs.
- Making semantic the default. That waits for evidence from the harness on
  a reviewed real-vault judgment set.
- A multilingual model as the default. `potion-multilingual-128M` is
  configurable, but is about 4× larger.

## Capabilities

### New Capabilities
- `semantic-search`: the feature flag, config, model fetch, embedding
  cache, hybrid fusion, more-like-this, the TUI toggle and background
  worker, and the semantic evaluation.

### Modified Capabilities
<!-- none: semantic behaviour is additive and specified in its own capability;
     paragraph-search behaviour with semantic off is unchanged -->

## Impact

- **ft-core:**
  - new `search/semantic/{mod,model,cache,fusion}.rs` behind
    `#[cfg(feature = "semantic")]`;
  - an `Embedder` trait, with a deterministic test implementation;
  - `config.rs`: `SearchCfg::semantic`;
  - `search/engine.rs`: a hook for fusing semantic candidates;
  - `Field::Semantic` for match reasons.
- **ft:**
  - `cmd/notes.rs` / new `cmd/embed.rs` (`fetch`, `status`, `build`);
  - `cmd/search.rs` (`--semantic`, `--no-semantic`, `--like`, the `≈`
    marker, the JSON reason);
  - TUI: an embeddings worker plus `BgEvent::EmbeddingsReady`, Search tab
    `m` / `M`, a status indicator, and a regenerated `docs/keybindings.md`.
- **Dependencies:** `model2vec-rs` (optional). `half` comes with it, for
  f16 storage.
- **Disk:**
  - the model (about 30 MB) in `$XDG_CACHE_HOME/ft/models/`;
  - the vault cache (about 8 MB for 16k paragraphs at 256-dim f16).
- **ft.nvim:** none. It can adopt `--like` later.

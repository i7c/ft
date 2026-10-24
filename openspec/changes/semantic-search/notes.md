# Implementation notes: semantic-search

## Fixture eval (`tests/fixtures/search-eval/`)

Run with `FT_SEMANTIC_TESTS=1 [FT_SEMANTIC_MODEL_DIR=…] cargo test --release
-p ft-core --test search_eval fixture_semantic_hybrid -- --nocapture`.
`FT_SEMANTIC_MIN` / `FT_SEMANTIC_K` override the floor and the candidate count.

Each cell is nDCG@10 / MRR.

| Configuration | Lexical judgments (29) | Concept judgments (12) |
|---|---|---|
| Lexical only | 0.910 / 0.960 | 0.066 / 0.083 |
| Hybrid, equal-weight RRF, min 0.3 (as first specified) | 0.873 / 0.856 ✗ gate | 0.394 / 0.361 |
| Hybrid, semantic weight 0.5, min 0.25 | 0.925 / 0.966 | 0.565 / 0.528 |
| **Hybrid, semantic weight 0.5, min 0.2 (default)** | **0.927 / 0.983** | **0.632 / 0.611** |
| Same, `potion-retrieval-32M`, min 0.15 | 0.933 / 0.983 | 0.630 / 0.750 |

- **Candidates:** `candidates` (10 / 20 / 50) made no difference at these
  floors. The floor binds first, so the default stays 50.
- **Semantic weight:** values below 0.5 (0.35, 0.25) gave identical scores.

## Real vault (2,047 notes, 9 draft lexical judgments)

| Engine | nDCG@10 | MRR | recall@10 |
|---|---|---|---|
| Lexical (blame recency) | 0.832 | 0.861 | 0.907 |
| Hybrid (`potion-base-8M`, defaults) | 0.828 | 0.833 | 0.944 |

- Within the gate's tolerance, and recall goes up.
- Embedding all 16,304 paragraphs took 0.85 s (release, single-threaded,
  cold cache).
- `potion-multilingual-128M` was not measured. Recommending it for
  mixed-language vaults needs a Portuguese judgment set first.

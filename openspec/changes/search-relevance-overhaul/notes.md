# Implementation notes: search-relevance-overhaul

## Relevance measurements

| Engine | Fixture nDCG@10 | Fixture MRR | Fixture recall@10 | Real-vault nDCG@10 | Real-vault MRR |
|---|---|---|---|---|---|
| Pre-overhaul (additive weights, ASCII tokens) | 0.379 | 0.409 | 0.466 | 0.605 | 0.619 |

- Fixture: `tests/fixtures/search-eval/` (41 notes, 29 cases), run with
  `cargo test -p ft-core --test search_eval -- --nocapture`.
- Real vault: `~/git/fortytwo` with the private draft judgments at
  `~/.local/share/ft/search-eval/fortytwo-judgments.toml` (9 cases), run with
  `FT_REAL_VAULT_TESTS=1 FT_VAULT=… FT_SEARCH_EVAL_JUDGMENTS=…`.

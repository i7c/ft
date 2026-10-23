# Implementation notes: search-relevance-overhaul

## Relevance measurements

| Engine | Fixture nDCG@10 | Fixture MRR | Fixture recall@10 | Real-vault nDCG@10 | Real-vault MRR | Real-vault recall@10 |
|---|---|---|---|---|---|---|
| Pre-overhaul (additive weights, ASCII tokens) | 0.379 | 0.409 | 0.466 | 0.605 | 0.619 | 0.667 |
| BM25F + context fields, no damping | 0.916 | 0.983 | 1.000 | 0.763 | 0.747 | 0.833 |
| Final (`CONTEXT_ONLY = 0.4`) | **0.910** | **0.960** | **1.000** | **0.833** | **0.861** | **0.907** |

- Fixture: `tests/fixtures/search-eval/` (41 notes, 29 cases), run with
  `cargo test -p ft-core --test search_eval -- --nocapture`
  (`FT_SEARCH_EVAL_SHOW=1` prints each case's top ten).
- Real vault: `~/git/fortytwo` with the private draft judgments at
  `~/.local/share/ft/search-eval/fortytwo-judgments.toml` (9 cases, drafted
  by Claude from search output — review and extend them), run with
  `FT_REAL_VAULT_TESTS=1 FT_VAULT=… FT_SEARCH_EVAL_JUDGMENTS=…`.
- Ratchet raised to nDCG@10 0.900 / MRR 0.950.

## Tuning record

- Context-only damping sweep (fixture / real nDCG@10): 1.0 → 0.916 / 0.763,
  0.6 → 0.899 / 0.833, **0.4 → 0.910 / 0.833**, 0.25 → 0.914 / 0.792,
  0.15 → 0.914 / 0.792.
- Title weight 1.5–4.0, alias 1.5–4.0, heading 1.0–3.0: no meaningful change
  once damping is in; design defaults kept.
- Heading-only paragraph penalty (0.7 / 0.5 / 0.3): helped the fixture
  (up to 0.932) but hurt the real vault (down to 0.785), whose draft
  judgments grade heading paragraphs as relevant. Not adopted.
- Priors off vs on: fixture 0.903 → 0.910, real MRR 0.843 → 0.861. Kept.

## Performance (2,047-note real vault, release, median of 7)

| Query | Before | After |
|---|---|---|
| `memory` | 0.246 s | 0.338 s |
| `~memoizaton` | 0.226 s | 0.332 s |
| `eigen memoization` | 0.212 s | 0.359 s |
| `memory scope:links` | 0.247 s (literal text) | 0.823 s (builds the graph) |

- Index build about 225 ms on 16,326 paragraphs / 764k tokens, after
  switching hot maps to `foldhash` and deduplicating stems per token
  (was about 280 ms). Tokenizing is about 65 ms of that.
- The first cut built priors from `Graph::build` (479 ms) on every CLI
  search (0.95 s). Priors now come from the scan's links via
  `graph::resolve`; the graph is built only for `scope:`.
- `FT_PERF_TESTS=1` gate (5k paragraphs, < 10 ms per query including
  highlights) passes in both debug and release.

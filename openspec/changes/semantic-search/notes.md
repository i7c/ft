# Implementation notes: semantic-search

## Concept-query eval (`tests/fixtures/search-eval/semantic.toml`, 12 cases)

| Engine | nDCG@10 | MRR | recall@10 |
|---|---|---|---|
| Lexical only | 0.066 | 0.083 | 0.042 |

Eleven cases share no words with their judged paragraphs (checked
mechanically on the folded text and the file title); `eigen matrix` is a
lexical control.

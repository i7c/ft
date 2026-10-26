# Implementation notes: search-proximity-and-blame-recency

## Real-vault report (2,047 notes, 9 draft judgments)

| Recency source | nDCG@10 | MRR | recall@10 |
|---|---|---|---|
| File mtime (no table) | 0.833 | 0.861 | 0.907 |
| Blame (warm cache, 16,304 / 16,304 paragraphs) | 0.832 | 0.861 | 0.907 |

The result is neutral, as expected: recency only breaks near-ties, and the
draft judgments are topical. The fixture results are unchanged (0.910 /
0.960) because the fixture runs without a recency table.

Run it with:

```sh
FT_REAL_VAULT_TESTS=1 FT_VAULT=… FT_SEARCH_EVAL_JUDGMENTS=… \
  [FT_SEARCH_EVAL_WARM=1] cargo test -p ft-core --test search_eval real_vault -- --nocapture
```

## Cost

- **Cold warm-up:** all 2,032 eligible files took about 80 s in a release
  harness run, with `git blame` running sequentially. The cache is 4.6 MB.
  Later launches only blame the files that changed.
- **CLI relevance search:** median 0.34 s → 0.41 s, from the validity
  snapshot plus loading the cache.

## Deviations

- **Warm-up progress location:** it shows in the Search tab's status line
  (`· blame d/t`) instead of the global status bar, whose right cell has no
  room.
- **Task 1.1's `dirty_paths` helper:** not added. The existing `git::status`
  already parses dirty paths, and `BlameValidity` uses it.
- **Date sort and date filters:** blame of the surviving results happens
  after ranking and is saved, so the next search benefits. Design decision 4
  is updated to match.

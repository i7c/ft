## MODIFIED Requirements

### Requirement: Graph and recency priors

Each result's score SHALL be multiplied by a prior in `[1.0, 1.25]` made of two
parts. (a) A **note part** in `[0, 0.15]` from the note's in-degree — the number
of distinct other scanned notes whose links resolve to it (links from files
excluded from the index still count) — log-scaled against the vault's maximum
in-degree. (b) A **paragraph part** in `[0, 0.10]` from the paragraph's recency
date with exponential decay over whole days relative to the newest recency date
among indexed paragraphs (not wall-clock "today", so copies of a vault rank
identically). A paragraph's recency date SHALL be its blame date (the newest
commit touching its line range) when the blame cache holds a valid entry for its
file, and otherwise the date of its file's mtime. Recency dates SHALL be supplied
at query time through a recency table, so new blame data changes ranking without
rebuilding the index; without a table, every paragraph SHALL use its file's
mtime date. A
`near:[[Note]]` clause SHALL multiply the score of paragraphs by a boost that
decreases with undirected link-graph distance from the resolved seed note —
`1.5` for the seed itself, `1.3` at distance 1, `1.15` at distance 2, `1.0`
beyond. An unresolved seed SHALL apply no boost and SHALL NOT error. `near:`
SHALL never filter; a query whose only clauses are `near:` boosts SHALL return
nothing. Priors and boosts SHALL only reorder results that the text and filter
clauses already selected.

#### Scenario: Hub note ranks above an orphan on a tie

- **WHEN** two paragraphs have equal BM25F scores and one note has twenty backlinks while the other has none
- **THEN** the paragraph in the linked note ranks first

#### Scenario: Prior is bounded

- **WHEN** a paragraph with a strong text match sits in an orphan note and a paragraph with a text score less than 0.8 times as high sits in the most-linked note and was edited most recently
- **THEN** the strong text match still ranks first

#### Scenario: Fresh paragraph in an old note

- **WHEN** one file holds two equally matching paragraphs, its blame shows one edited today and the other three years ago, and the blame cache holds the file
- **THEN** the paragraph edited today ranks first

#### Scenario: Uncached file falls back to mtime

- **WHEN** the blame cache has no valid entry for a file
- **THEN** every paragraph of that file uses the file's mtime date as its recency date, and the search succeeds without invoking `git blame`

#### Scenario: Near boost reorders without filtering

- **WHEN** the query `cache near:[[Caching]]` is run and paragraphs mention `cache` in `Caching.md`, in a note linked from it, and in an unrelated note
- **THEN** all three are returned, with the boost applied to the first two

#### Scenario: Unresolved near seed

- **WHEN** the query `cache near:[[Does Not Exist]]` is run
- **THEN** results equal those of `cache` alone, in the same order

## ADDED Requirements

### Requirement: Recency table loading

`ft notes search`, `ft notes synth scaffold --search`, and the TUI SHALL build
the recency table from the on-disk blame cache and one validity snapshot (blob
ids from `git ls-tree -r HEAD` and dirty paths from `git status`), without
invoking `git blame` for a plain relevance search. Outside a git repository the
table SHALL be empty (every paragraph falls back to mtime) and the search SHALL
succeed.

#### Scenario: Plain search does not blame

- **WHEN** `ft notes search cache` runs in a git vault with a cold blame cache
- **THEN** no `git blame` subprocess is spawned and results are ranked with mtime recency

#### Scenario: Warm cache changes ranking without reindexing

- **WHEN** the TUI's blame warm-up finishes after the Search tab already shows results
- **THEN** the tab re-ranks the current query with the new recency table without a vault rescan

### Requirement: Search near the focused result

The Search tab SHALL bind `n` to `search.near-focused`: it SHALL set the query's
`near:` clause to the note of the focused result (`near:[[<title>]]`, replacing
any existing `near:` clause, keeping every other clause and the any-mode flag),
update the input line, and re-run the query. On an empty result list it SHALL do
nothing. The command SHALL appear in `SEARCH_COMMANDS`, `SEARCH_KEYMAP`, the `?`
overlay, and `docs/keybindings.md`.

#### Scenario: Pin the neighbourhood of a result

- **WHEN** the query is `cache` and the focused result is in `Caching.md`, and the user presses `n`
- **THEN** the query becomes `cache near:[[Caching]]` and the results re-rank with the boost

#### Scenario: Replace an existing near clause

- **WHEN** the query is `cache near:[[Other]]` and the focused result is in `Caching.md`, and the user presses `n`
- **THEN** the query becomes `cache near:[[Caching]]`

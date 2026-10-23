## Context

After `search-relevance-overhaul`, ranking is BM25F times a per-note prior in
`[1, 1.25]`. That prior is in-degree (0.15) plus file-mtime recency (0.10),
computed at index build (`SearchIndex::apply_links`). `near:[[X]]` boosts
paragraphs by link distance from `X`.

Measured on the 2,047-note vault:

- **The blame cache is almost never warm.** `.ft/cache/blame.msgpack`
  (230 KB) is keyed on `(path, HEAD)`. Auto-sync made 7 commits in the last
  week, and each one invalidates every entry.
- **A validity snapshot is cheap.** `git ls-tree -r HEAD` takes 15 ms;
  `git status --porcelain` takes 25 ms.
- **`git blame` is not.** It costs roughly 10–30 ms per file, so blaming
  the whole vault is 20–60 s of CPU. That is fine in the background, never
  on a keystroke.

Constraints:

- The index is immutable and lives in the shared snapshot. The TUI rebuilds
  it only on a generation change (a rescan).
- Background work follows the TUI pattern: the worker owns its inputs,
  posts a `BgEvent`, and exits. There is no async runtime.
- Ranking must stay deterministic for a given cache state. The eval fixture
  must stay deterministic too: the harness passes no recency table, so the
  fixture's in-repo commit times never leak into it.

## Goals / Non-Goals

**Goals:**

- Paragraph-level recency from git when available, with mtime as the
  fallback.
- A blame cache that survives commits to other files.
- One-key "search near this note" from the Graph tab, and "near this
  result" from the Search tab.

**Non-Goals:**

- Citation and "already synthesized" marks (declined).
- Paragraph-level graph distance.
- Blaming during plain CLI searches.

## Decisions

### 1. Recency moves out of the index into a query-time `RecencyTable`

`ft_core::search::recency::RecencyTable` holds two things:

- one `Option<u32>` day number per doc id (the paragraph's blame date as days
  since the epoch);
- the `doc_count` it was built for.

`RecencyTable::build(index, cache, validity)` maps each doc to its paragraph's
blame date when the cache has a valid entry for its file. It is pure map
lookups, a few milliseconds.

`SearchCtx` gains `recency: Option<&RecencyTable>`. A table whose `doc_count`
differs from the index is ignored (a stale table after a rescan), and mtime
applies until the fresh one arrives.

The index keeps the mtime day of each doc's file as the fallback and stops
folding recency into `NoteCtx::prior`, which becomes in-degree only, capped at
0.15. At query time the paragraph part is
`0.10 · exp(−(ref − day)/90)`, where `day` is the paragraph's blame day or its
mtime day, and `ref` is the newest such day across the index. The table
precomputes it when it is present.

- **Why query time:** a warm-up that finishes 40 s after startup must not
  force a full rescan just to re-rank.
- **Alternative considered:** pass blame into `SearchIndex::build`.
  Rejected, because every warm-up batch would mean a full rebuild.

### 2. Blame cache keyed on `(path, blob id)`, with a validity snapshot

`git::ls_tree_blobs(repo) -> HashMap<PathBuf, String>` (`ls-tree -r HEAD`) and
`git::dirty_paths(repo) -> HashSet<PathBuf>` (`status --porcelain -z`,
including untracked files) form a `BlameValidity`. Paths are mapped through the
existing `RepoMap` (the vault may be a subdirectory of the repository).

The API changes:

- `BlameCache::get(path, &validity)`
- `insert(path, blob, blame)`
- `CacheFile { version: 2, entries }`

A version-1 file, or one that fails to decode, loads empty. That is the
existing corruption path.

Blame of an unchanged committed blob is invariant, so the entry stays correct
across unrelated commits. A dirty file is never served from, or written to, the
cache: its blame would carry "not committed yet" lines.

`search_with_dates` switches to the validity API, so date sort and date filters
get warmer caches for free.

**Alternative considered:** key on a content hash of the working-tree file.
It would avoid the `ls-tree` call, but it caches blame for uncommitted edits,
which goes stale on the next commit.

### 3. TUI warm-up worker

On `GraphReady`, the App spawns one blame-warm worker, unless one is already
running for this generation. The worker receives:

- the vault path;
- `Arc<GraphSnapshot>`;
- the generation;
- a shared `Arc<AtomicU64>` holding the current generation, so the worker
  stops when a newer snapshot lands.

The worker does this:

1. Load the cache and validity, build a `RecencyTable`, and post
   `BgEvent::RecencyReady { generation, table }`. Ranking improves
   immediately from whatever is already cached.
2. Blame the indexed files that are clean and lack a valid entry, newest
   mtime first, so the notes you are working on are covered first. Save the
   cache every 50 files and post `BgEvent::BlameWarmProgress { done, total }`
   for the status bar.
3. At the end, save, rebuild the table, and post `RecencyReady` again.

The App stores the table as `Arc<RecencyTable>` next to the snapshot. Tabs
read it via `TabCtx`. The Search tab re-runs its current query when a
`RecencyReady` matches its snapshot's generation.

Blame runs sequentially on that one thread to keep the UI responsive. It can
overlap a git-sync job; both only read, except that sync takes `index.lock`,
which blame does not need.

### 4. CLI loads the table and never blames for relevance

`cmd::search::run_query` loads the cache plus validity (about 40 ms), builds
the table, and passes it in `SearchCtx`. With `--sort date` or a date filter,
the existing lazy blame of survivors runs first and also feeds the table. A
vault outside git gets no table, and mtime applies.

### 5. `N` and `n` reuse the `near:` clause

- **Graph `N`** raises `SearchWithQuery { query: "near:[[Title]] ", any:
  false }`. `SearchTab::queue_query` already enters edit mode, and
  `EditBuffer::from` puts the cursor at the end. A near-only query matches
  nothing, so the list stays empty until the user types: a deliberate
  "type what you are looking for" state, with a status hint
  ("near Caching — type to search").
- **Search `n`** parses the current input, drops every `Mode::Near` clause,
  appends `near:[[<focused result title>]]`, renders the query back to the
  input (render round-trips by construction), and re-queries.
- **Titles** are file stems. An ambiguous stem boosts every note sharing it,
  which is today's `near:` behaviour.

### 6. Eval

The fixture harness passes no table, so its results are unchanged and
deterministic. Recency is covered by unit tests on a temporary git repo:

- two paragraphs with different blame dates in one file;
- an unrelated commit keeps the entry valid;
- a dirty file falls back.

Run the real-vault report before and after with a warm cache. No threshold is
asserted, since the judgments are drafts.

## Risks / Trade-offs

- **[CLI +~40 ms per search for the validity snapshot]** → Acceptable at
  0.34 s total. Skip it when the vault is not in git.
- **[Warm-up CPU on first launch, about 30 s]** → Background thread, newest
  files first, cancellable; persisted, so later launches only blame changed
  files.
- **[Ranking depends on cache warmth, so CLI and TUI can differ while the
  TUI warms]** → Deterministic for a given cache file. The CLI reads the same
  file, so they converge once it is saved.
- **[Huge files make slow blames]** → A per-file timeout is not available
  from `git blame`, so skip files over 5,000 lines (they use the mtime
  fallback).
- **[Old cache files are discarded once]** → The cache is derived data; the
  next date query or the warm-up rebuilds it.

## Migration Plan

The cache schema bumps itself on the first save. There is no config, and
rollback means reverting the commit; a v2 cache file is then discarded as
corrupt by the old code, which falls back to an empty cache.

## Open Questions

- Should the Notes tab also get a "search near this note" key? Deferred
  until the Graph and Search keys prove useful.

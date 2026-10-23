## Why

The `search-relevance-overhaul` change shipped the cheap half of graph- and
git-aware ranking: note in-degree, file-mtime recency, and an explicit
`near:[[Note]]` boost. Two gaps remain.

- **Search does not know where you are.** `near:` exists, but you have to
  type the note name. Nothing seeds it from the note on screen.
- **Recency is per file, and often wrong.**
  - A five-year-old note with one paragraph edited today makes every
    paragraph in it "fresh".
  - A fresh clone or `git checkout` resets every mtime.
  - The paragraph-level truth, git blame, is already used by `--sort date`
    and date filters. But the blame cache is keyed on HEAD, so every commit
    (and the auto git sync commits several times a week) invalidates every
    entry. The cache is almost always cold.

## What Changes

- **Search from here.**
  - Graph tab `N` (`graph.search-near`) opens the Search tab with
    `near:[[<selected note>]] ` prefilled and the query editor active, so
    whatever you type next ranks by proximity to that note.
  - Search tab `n` (`search.near-focused`) sets the query's `near:` clause
    to the focused result's note (replacing an existing one) and re-runs:
    "more from around here".
- **Paragraph recency from git blame.**
  - The recency part of the prior moves from the note (file mtime) to the
    paragraph: the paragraph's blame date when the blame cache holds the
    file's current version, else the file mtime.
  - It is computed at query time from a recency table, so the immutable
    index does not need rebuilding when blame arrives.
  - The total prior stays bounded in `[1.0, 1.25]`. **BREAKING** (ranking
    only): tie order changes where blame and mtime disagree.
- **Blame cache keyed on the file's blob, not HEAD.**
  - An entry stays valid while the file's committed blob is unchanged and
    the file is clean, so unrelated commits no longer invalidate it.
  - Validity comes from one `git ls-tree -r HEAD` plus one `git status`
    per load (about 40 ms on a 2,000-note vault).
  - Existing cache files load as empty; the schema bump is detected.
- **Background blame warming in the TUI.** After a snapshot is installed, a
  worker blames indexed files whose entry is missing or stale, saving as it
  goes, and posts the refreshed recency table to the Search tab. The CLI
  never blames for plain searches; it reads whatever the cache holds, and
  `--sort date` / date filters keep blaming lazily, as today.

## Non-goals

- Flagging or hiding already-synthesized paragraphs. Declined: the citation
  lookup stays removed.
- Meaning-based search (the separate `semantic-search` change).
- Paragraph-level `near:` (the distance stays note-level).
- An ft.nvim "search near current buffer" command. It can come later as a
  plain CLI call (`ft notes search "near:[[X]] …"`).

## Dependencies

Builds on `search-relevance-overhaul`. Its `paragraph-search` deltas are
written against that change's requirement text, so archive
`remove-pulse-and-recent` and `search-relevance-overhaul` before this one.

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
- `paragraph-search`: recency becomes per-paragraph (blame, mtime fallback)
  and is applied at query time; adds the Search tab `n` near-focused
  command.
- `graph-to-search-jump`: adds the Graph tab `N` search-near handoff.
- `blame-cache`: key changes from `(path, HEAD)` to `(path, blob id)` with a
  clean-file check; background warming becomes a sanctioned population path.

## Impact

- **ft-core:**
  - `blame_cache.rs`: new key, validity snapshot via `ls-tree` + `status`,
    schema version;
  - `git.rs`: `ls_tree_blobs`, `dirty_paths` helpers;
  - `search/engine.rs`: a `SearchCtx` recency table, prior split into the
    note-level in-degree part and the paragraph-level recency part;
  - `search/index.rs`: the recency part is removed from the note prior;
  - new `search/recency.rs`: building the table from a scan, the blame
    cache and the validity snapshot.
- **ft:**
  - `cmd/search.rs::run_query`: loads the recency table from the cache;
  - TUI: a new `BgEvent` blame-warm worker modelled on the git-sync worker,
    an `Arc<RecencyTable>` on `App`, the Graph `N` and Search `n` commands
    and keymap rows, and a regenerated `docs/keybindings.md`.
- **Eval:** recency is not measurable on the fixture (commit times). Unit
  tests use a temporary git repo, and the real-vault report is re-run.
- **ft.nvim:** none.

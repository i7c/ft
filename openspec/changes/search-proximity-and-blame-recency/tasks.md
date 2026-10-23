# Tasks: search proximity and blame recency

## 1. Blame cache keyed on blob id

- [x] 1.1 `git.rs`: `ls_tree_blobs(repo) -> HashMap<PathBuf, String>` and `dirty_paths(repo) -> HashSet<PathBuf>` (untracked included), with unit tests on a temp repo
- [x] 1.2 `blame_cache.rs`: `BlameValidity` (built through `RepoMap` for vault-in-subdirectory repos), `CacheFile { version: 2, entries }`, `get(path, &validity)`, `insert(path, blob, blame)`; v1 and corrupt files load empty
- [x] 1.3 Tests: round-trip, old schema loads empty, an unrelated commit keeps the entry, a changed file misses, a dirty file misses
- [x] 1.4 `search_with_dates` and its callers move to the validity API; skip blaming files over 5,000 lines

## 2. Query-time recency

- [x] 2.1 `search/index.rs`: the note prior becomes in-degree only (cap 0.15); store each doc's file mtime day and the newest mtime day
- [x] 2.2 New `search/recency.rs`: `RecencyTable::build(index, cache, validity)` (per-doc blame day, `doc_count`, newest day); `SearchCtx::recency`; a stale table (doc-count mismatch) is ignored
- [x] 2.3 `search/engine.rs`: paragraph recency part `0.10 · exp(−age/90)` over whole days, blame day else mtime day; total prior ≤ 1.25
- [x] 2.4 Tests: fresh paragraph in an old note (temp git repo with two commit dates), uncached file falls back to mtime, prior stays bounded, no table equals mtime-only ranking
- [ ] 2.5 Fixture eval unchanged (no table); record the real-vault report with a warm cache in the change notes

## 3. CLI

- [x] 3.1 `cmd::search::run_query`: load the cache and validity, build the table, pass it in `SearchCtx`; no table outside git
- [x] 3.2 Integration test: a plain search in a git vault spawns no blame (cache file untouched), and `--sort date` populates the v2 cache

## 4. TUI warm-up

- [ ] 4.1 `BgEvent::{RecencyReady { generation, table }, BlameWarmProgress { done, total }}`; a blame-warm worker modelled on the git-sync worker (owns its inputs, cancels on a generation change through a shared `AtomicU64`)
- [ ] 4.2 The worker posts the table built from the cached entries first, then blames clean, uncached, indexed files newest-mtime first, saves every 50 files, and posts the final table
- [ ] 4.3 `App` stores `Arc<RecencyTable>` and exposes it via `TabCtx`; the status bar shows warm-up progress; the Search tab re-runs on a `RecencyReady` matching its generation
- [ ] 4.4 Tests: pump the worker synchronously in tests (like `pump_graph_rebuild_for_test`); a recency update re-ranks the open query; a newer generation cancels the worker

## 5. Search near

- [ ] 5.1 Graph tab `N` → `graph.search-near` (Note rows only; otherwise a toast), raising `SearchWithQuery` with `near:[[Title]] ` and the editor active; help section row
- [ ] 5.2 Search tab `n` → `search.near-focused`: replace or add the `near:` clause from the focused result's title, update the input, re-run; status hint for a near-only query
- [ ] 5.3 `COMMANDS` / `KEYMAP` rows, `dispatch_command` arms, regenerate `docs/keybindings.md`
- [ ] 5.4 `TestBackend` tests: `N` from a Note row then typing; `N` on a Ghost row toasts; `n` adds and replaces `near:`

## 6. Docs and finish

- [ ] 6.1 `docs/guide/synthesis.md` (recency from git, `N` / `n`), `docs/architecture.md` §Synthesis (recency table, blame-warm worker), `docs/guide/graph.md` (the `N` key)
- [ ] 6.2 Build invariants: `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `cargo run --release -q -- commands docs --check`

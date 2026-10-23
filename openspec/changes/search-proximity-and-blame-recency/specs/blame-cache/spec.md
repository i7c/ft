## MODIFIED Requirements

### Requirement: BlameCache persistence
A `BlameCache` type SHALL load from and save to `.ft/cache/blame.msgpack` in the vault root using `rmp-serde`. The cache SHALL be a single flat file containing all entries for the entire vault, tagged with a schema version. The `.ft/cache/` directory SHALL be created on first write if absent. A file with a different or missing schema version SHALL load as an empty cache.

#### Scenario: Cache round-trips through msgpack
- **WHEN** a `BlameCache` with entries is saved and then loaded from `.ft/cache/blame.msgpack`
- **THEN** all entries are present and identical after the round-trip

#### Scenario: Missing cache file loads empty cache
- **WHEN** `.ft/cache/blame.msgpack` does not exist
- **THEN** `BlameCache::load` returns an empty cache without error

#### Scenario: Old schema loads empty
- **WHEN** `.ft/cache/blame.msgpack` was written by the HEAD-keyed schema
- **THEN** `BlameCache::load` returns an empty cache without error and the next save writes the new schema

### Requirement: BlameCache key and invalidation
Cache entries SHALL be keyed on `(vault-relative path as String, blob id as String)`, where the blob id is the file's object id in the `HEAD` tree. Validity SHALL be judged against a `BlameValidity` snapshot taken once per load from `git ls-tree -r HEAD` (path → blob id) and `git status --porcelain` (dirty paths): an entry is valid if and only if its stored blob id equals the file's current blob id and the file is not dirty. Commits that do not change a file SHALL NOT invalidate its entry. Invalid entries SHALL be silently ignored and recomputed on next access.

#### Scenario: Unrelated commit keeps the entry
- **WHEN** a cache entry exists for `notes/a.md` and a commit changes only `notes/b.md`
- **THEN** `BlameCache::get` for `notes/a.md` returns the cached blame without invoking `blame_file`

#### Scenario: Changed file misses
- **WHEN** a commit changes `notes/a.md`
- **THEN** the entry for `notes/a.md` is invalid and the caller must invoke `blame_file` to repopulate

#### Scenario: Dirty file misses
- **WHEN** `notes/a.md` has uncommitted edits
- **THEN** its entry is invalid even if its committed blob is unchanged

### Requirement: BlameCache lazy population
The `BlameCache` SHALL NOT be populated at graph-build or index-build time. It SHALL be populated on demand — by a date sort or date filter for the files of its surviving results, or by the TUI's background blame warm-up worker for indexed files with missing or invalid entries. After computing, results SHALL be inserted into the cache and the cache SHALL be saved to disk (the warm-up saves periodically while it runs and once when it finishes). The warm-up SHALL run on the background-worker thread, post its progress and its final recency table as `BgEvent`s, skip dirty files, and stop when the snapshot generation changes.

#### Scenario: Date query populates cache
- **WHEN** `ft notes search cache --sort date` runs on a cold cache
- **THEN** blame data is computed only for files containing matching paragraphs, and the cache file is created or updated

#### Scenario: Second date query uses cache
- **WHEN** the same query runs again with no change to those files
- **THEN** no `git blame` subprocesses are spawned for them

#### Scenario: TUI warms the cache in the background
- **WHEN** the TUI installs a snapshot of a git vault with a cold cache
- **THEN** a background worker blames the indexed files, the UI stays responsive, and the Search tab receives the new recency table when the worker finishes

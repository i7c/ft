## ADDED Requirements

### Requirement: blame_file function
`ft_core::git::blame_file(repo: &Path, rel_path: &Path) -> Result<Vec<LineBlame>>` SHALL shell out to `git blame --porcelain <rel_path>` in `repo`, parse the output, and return one `LineBlame { line: u32, commit_hash: String, timestamp: i64 }` per source line (1-indexed, `timestamp` is the `author-time` unix epoch from the porcelain output).

#### Scenario: Blame returns per-line records
- **WHEN** `blame_file` is called on a file with 10 lines across two commits
- **THEN** the result contains exactly 10 `LineBlame` entries, each with a non-empty `commit_hash` and a positive `timestamp`

#### Scenario: Single-commit file
- **WHEN** a file was created in one commit and never modified
- **THEN** all returned `LineBlame` entries share the same `commit_hash`

#### Scenario: File outside git repo returns error
- **WHEN** `blame_file` is called on a path not tracked by git
- **THEN** it returns an `Err`

### Requirement: BlameCache persistence
A `BlameCache` type SHALL load from and save to `.ft/cache/blame.msgpack` in the vault root using `rmp-serde`. The cache SHALL be a single flat file containing all entries for the entire vault. The `.ft/cache/` directory SHALL be created on first write if absent.

#### Scenario: Cache round-trips through msgpack
- **WHEN** a `BlameCache` with entries is saved and then loaded from `.ft/cache/blame.msgpack`
- **THEN** all entries are present and identical after the round-trip

#### Scenario: Missing cache file loads empty cache
- **WHEN** `.ft/cache/blame.msgpack` does not exist
- **THEN** `BlameCache::load` returns an empty cache without error

### Requirement: BlameCache key and invalidation
Cache entries SHALL be keyed on `(vault-relative path as String, HEAD commit hash as String)`. An entry is valid if and only if the stored HEAD hash matches the current HEAD of the vault's git repository. Stale entries (HEAD changed) SHALL be silently ignored and recomputed on next access.

#### Scenario: Cache hit on matching HEAD
- **WHEN** a cache entry exists for `(path, HEAD)` and HEAD has not advanced
- **THEN** `BlameCache::get` returns the cached `Vec<LineBlame>` without invoking `blame_file`

#### Scenario: Cache miss on stale HEAD
- **WHEN** a cache entry exists for `(path, old_HEAD)` but current HEAD is different
- **THEN** `BlameCache::get` returns `None` and the caller must invoke `blame_file` to repopulate

### Requirement: BlameCache lazy population
The `BlameCache` SHALL NOT be populated at graph-build time. It SHALL be populated on demand when a caller requests blame data for a specific file. After computing, the result SHALL be inserted into the cache and the cache SHALL be saved to disk.

#### Scenario: First journal query populates cache
- **WHEN** `ft notes journal` is invoked for the first time on a vault with a cold cache
- **THEN** blame data is computed only for files containing matching paragraphs, and the cache file is created/updated after the query

#### Scenario: Second journal query uses cache
- **WHEN** `ft notes journal` is invoked a second time with no intervening commits
- **THEN** no `git blame` subprocesses are spawned for previously cached files

### Requirement: Paragraph section date derivation
Given a `Paragraph` node with `line_start` and `line_end`, its date SHALL be derived as the maximum `timestamp` among all `LineBlame` entries in the range `[line_start, line_end]`, converted to `NaiveDate` (UTC). This is the date the section was most recently modified in git history.

#### Scenario: Section date from blame range
- **WHEN** a paragraph spans lines 5–8, with blame timestamps `[T1, T2, T3, T4]`
- **THEN** the paragraph's date is `max(T1, T2, T3, T4)` converted to `NaiveDate`

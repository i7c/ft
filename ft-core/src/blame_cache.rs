//! Lazy on-disk cache for `git blame` output, keyed on
//! `(vault-relative path, blob id)`.
//!
//! Running `git blame` on every matching file every query is too slow
//! (~10–30 ms per file), so blame is cached per file. The blame of an
//! unchanged committed blob never changes, so an entry is keyed on the
//! file's object id in the `HEAD` tree and stays valid across commits
//! that touch other files. A [`BlameValidity`] snapshot — one
//! `git ls-tree -r HEAD` plus one `git status` — tells which entries
//! still hold; dirty and untracked files are never cached (their blame
//! carries uncommitted lines that go stale on the next commit).
//!
//! Stored as a single versioned msgpack file at
//! `<vault>/.ft/cache/blame.msgpack`. Missing, corrupt, or older-schema
//! files load as an empty cache — the next blame call rebuilds.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::git::{self, LineBlame, RepoMap};

const CACHE_REL: &str = ".ft/cache/blame.msgpack";

/// Bumped whenever the on-disk shape or key semantics change.
const SCHEMA_VERSION: u32 = 2;

/// Files longer than this are never blamed: `git blame` has no timeout
/// and a huge file would stall the caller. They fall back to mtime.
pub const MAX_BLAME_LINES: u32 = 5_000;

/// On-disk blame cache. Construct via [`BlameCache::load`]; persist
/// modifications with [`BlameCache::save`].
#[derive(Debug, Default)]
pub struct BlameCache {
    /// Vault-relative path (as a string, cross-platform) → entry.
    entries: HashMap<String, CacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    /// Blob id of the file in the `HEAD` tree the blame was taken at.
    blob: String,
    blame: Vec<LineBlame>,
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    entries: HashMap<String, CacheEntry>,
}

/// Which cache entries are valid right now: the `HEAD` blob id of every
/// committed file and the set of dirty (modified, untracked, deleted, or
/// conflicted) files, both vault-relative.
#[derive(Debug, Clone)]
pub struct BlameValidity {
    repo: RepoMap,
    blobs: HashMap<PathBuf, String>,
    dirty: HashSet<PathBuf>,
}

impl BlameValidity {
    /// Snapshot the repository enclosing `vault_root`. `Ok(None)` when the
    /// vault is not inside a git repository.
    pub fn snapshot(vault_root: &Path) -> Result<Option<Self>> {
        let Ok(repo) = RepoMap::discover(vault_root) else {
            return Ok(None);
        };
        let blobs = git::ls_tree_blobs(repo.root())?
            .into_iter()
            .filter_map(|(p, oid)| repo.to_vault(&p).map(|v| (v, oid)))
            .collect();
        let status = git::status(repo.root())?;
        let dirty = status
            .modified
            .iter()
            .chain(&status.untracked)
            .chain(&status.deleted)
            .chain(&status.conflicted)
            .filter_map(|p| repo.to_vault(p))
            .collect();
        Ok(Some(Self { repo, blobs, dirty }))
    }

    /// The cacheable blob id of `vault_rel`: its `HEAD` blob when the file
    /// is committed and clean, else `None`.
    pub fn blob(&self, vault_rel: &Path) -> Option<&str> {
        if self.dirty.contains(vault_rel) {
            return None;
        }
        self.blobs.get(vault_rel).map(String::as_str)
    }

    /// The repository the snapshot was taken from.
    pub fn repo(&self) -> &RepoMap {
        &self.repo
    }
}

impl BlameCache {
    /// Load the cache from `<vault_root>/.ft/cache/blame.msgpack`.
    /// Returns an empty cache if the file does not exist, fails to
    /// deserialize, or was written by another schema version.
    pub fn load(vault_root: &Path) -> Result<Self> {
        let path = vault_root.join(CACHE_REL);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(crate::error::Error::Io {
                    path: path.clone(),
                    source: e,
                });
            }
        };
        match rmp_serde::from_slice::<CacheFile>(&bytes) {
            Ok(f) if f.version == SCHEMA_VERSION => Ok(Self { entries: f.entries }),
            // Corrupt, old-schema, or future-schema — start fresh.
            _ => Ok(Self::default()),
        }
    }

    /// Persist the cache to `<vault_root>/.ft/cache/blame.msgpack` via
    /// [`crate::fs::write_atomic_bytes`] (same-dir tempfile + rename, so
    /// a crash mid-write leaves the prior cache intact rather than
    /// truncated).
    pub fn save(&self, vault_root: &Path) -> Result<()> {
        let path = vault_root.join(CACHE_REL);
        let file = CacheFile {
            version: SCHEMA_VERSION,
            entries: self.entries.clone(),
        };
        let bytes = rmp_serde::to_vec(&file)
            .map_err(|e| crate::error::Error::Notes(format!("serialize blame cache: {e}")))?;
        crate::fs::write_atomic_bytes(&path, &bytes)
    }

    /// Cached blame for `vault_rel` when its entry matches the file's
    /// current blob in `validity`. `None` on a miss, a stale entry, or a
    /// dirty file.
    pub fn get(&self, vault_rel: &Path, validity: &BlameValidity) -> Option<&Vec<LineBlame>> {
        let blob = validity.blob(vault_rel)?;
        let entry = self.entries.get(&key(vault_rel))?;
        (entry.blob == blob).then_some(&entry.blame)
    }

    /// Insert (or overwrite) blame for `vault_rel` taken at `blob`.
    pub fn insert(&mut self, vault_rel: &Path, blob: String, blame: Vec<LineBlame>) {
        self.entries
            .insert(key(vault_rel), CacheEntry { blob, blame });
    }

    /// Blame for `vault_rel`, from the cache or computed and cached.
    /// Dirty files are blamed without caching; files over
    /// [`MAX_BLAME_LINES`] and blame failures yield `None`. Returns
    /// whether `git blame` ran, so callers can save or report progress.
    pub fn ensure(
        &mut self,
        vault_rel: &Path,
        line_count: u32,
        validity: &BlameValidity,
    ) -> (Option<Vec<LineBlame>>, bool) {
        if let Some(b) = self.get(vault_rel, validity) {
            return (Some(b.clone()), false);
        }
        if line_count > MAX_BLAME_LINES {
            return (None, false);
        }
        let repo = validity.repo();
        let Ok(blame) = git::blame_file(repo.root(), &repo.to_repo(vault_rel)) else {
            return (None, true);
        };
        if let Some(blob) = validity.blob(vault_rel) {
            self.insert(vault_rel, blob.to_string(), blame.clone());
        }
        (Some(blame), true)
    }

    /// Number of cached files (for status reporting).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn key(vault_rel: &Path) -> String {
    vault_rel.to_string_lossy().replace('\\', "/")
}

// ── Date helper ─────────────────────────────────────────────────────────

/// Derive a `NaiveDate` (UTC) for a paragraph spanning the inclusive
/// 1-indexed line range `[line_start, line_end]`. Returns the date of
/// the most recent commit touching any line in the range, or `None`
/// when the range produces no blame entries.
pub fn paragraph_date(
    blame: &[LineBlame],
    line_start: u32,
    line_end: u32,
) -> Option<chrono::NaiveDate> {
    let max_ts = blame
        .iter()
        .filter(|b| b.line >= line_start && b.line <= line_end)
        .map(|b| b.timestamp)
        .max()?;
    chrono::DateTime::<chrono::Utc>::from_timestamp(max_ts, 0).map(|dt| dt.date_naive())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn sample(line: u32, sha: &str, ts: i64) -> LineBlame {
        LineBlame {
            line,
            commit_hash: sha.to_string(),
            timestamp: ts,
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .output()
            .expect("git on PATH");
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }

    /// A repo with committed `a.md` and `b.md`.
    fn repo() -> assert_fs::TempDir {
        let tmp = assert_fs::TempDir::new().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        git(d, &["config", "user.name", "T"]);
        git(d, &["config", "user.email", "t@e.com"]);
        git(d, &["config", "commit.gpgsign", "false"]);
        std::fs::write(d.join("a.md"), "one\ntwo\n").unwrap();
        std::fs::write(d.join("b.md"), "bee\n").unwrap();
        git(d, &["add", "."]);
        git(d, &["commit", "-q", "-m", "init"]);
        tmp
    }

    fn validity(dir: &Path) -> BlameValidity {
        BlameValidity::snapshot(dir).unwrap().expect("in git")
    }

    #[test]
    fn missing_file_loads_empty_cache() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let c = BlameCache::load(tmp.path()).unwrap();
        assert!(c.is_empty());
    }

    #[test]
    fn round_trip_through_msgpack() {
        let tmp = repo();
        let v = validity(tmp.path());
        let blob = v.blob(Path::new("a.md")).unwrap().to_string();
        let mut c = BlameCache::default();
        c.insert(
            Path::new("a.md"),
            blob,
            vec![
                sample(1, "abc123", 1_700_000_000),
                sample(2, "abc123", 1_700_000_001),
            ],
        );
        c.save(tmp.path()).unwrap();

        let loaded = BlameCache::load(tmp.path()).unwrap();
        let entries = loaded.get(Path::new("a.md"), &v).expect("entry exists");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].timestamp, 1_700_000_000);
    }

    #[test]
    fn old_schema_loads_empty() {
        // The v1 shape: a bare `{ entries: { path: { head, blame } } }`.
        #[derive(Serialize)]
        struct V1 {
            entries: HashMap<String, CacheEntry>,
        }
        let tmp = assert_fs::TempDir::new().unwrap();
        let mut entries = HashMap::new();
        entries.insert(
            "a.md".to_string(),
            CacheEntry {
                blob: "head".into(),
                blame: vec![sample(1, "x", 1)],
            },
        );
        let bytes = rmp_serde::to_vec(&V1 { entries }).unwrap();
        crate::fs::write_atomic_bytes(&tmp.path().join(CACHE_REL), &bytes).unwrap();
        assert!(BlameCache::load(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn unrelated_commit_keeps_the_entry() {
        let tmp = repo();
        let d = tmp.path();
        let mut c = BlameCache::default();
        let (blame, ran) = c.ensure(Path::new("a.md"), 2, &validity(d));
        assert!(ran && blame.is_some());
        std::fs::write(d.join("b.md"), "bee changed\n").unwrap();
        git(d, &["commit", "-q", "-am", "b only"]);
        let (_, ran) = c.ensure(Path::new("a.md"), 2, &validity(d));
        assert!(!ran, "a.md is served from the cache");
    }

    #[test]
    fn changed_file_misses() {
        let tmp = repo();
        let d = tmp.path();
        let mut c = BlameCache::default();
        c.ensure(Path::new("a.md"), 2, &validity(d));
        std::fs::write(d.join("a.md"), "one\nTWO\n").unwrap();
        git(d, &["commit", "-q", "-am", "a edit"]);
        assert!(c.get(Path::new("a.md"), &validity(d)).is_none());
    }

    #[test]
    fn dirty_file_misses_and_is_not_cached() {
        let tmp = repo();
        let d = tmp.path();
        let mut c = BlameCache::default();
        c.ensure(Path::new("a.md"), 2, &validity(d));
        std::fs::write(d.join("a.md"), "one\ntwo\nthree\n").unwrap();
        let v = validity(d);
        assert!(c.get(Path::new("a.md"), &v).is_none());
        let before = c.entries["a.md"].blob.clone();
        let (blame, ran) = c.ensure(Path::new("a.md"), 3, &v);
        assert!(
            ran && blame.unwrap().len() == 3,
            "dirty file is still blamed"
        );
        assert_eq!(c.entries["a.md"].blob, before, "but not cached");
    }

    #[test]
    fn huge_files_are_not_blamed() {
        let tmp = repo();
        let mut c = BlameCache::default();
        let (blame, ran) = c.ensure(
            Path::new("a.md"),
            MAX_BLAME_LINES + 1,
            &validity(tmp.path()),
        );
        assert!(blame.is_none() && !ran);
    }

    #[test]
    fn no_validity_outside_git() {
        let tmp = assert_fs::TempDir::new().unwrap();
        assert!(BlameValidity::snapshot(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn paragraph_date_picks_max_timestamp_in_range() {
        let blame = vec![
            sample(1, "x", 1_700_000_000),
            sample(2, "y", 1_800_000_000),
            sample(3, "z", 1_750_000_000),
            sample(4, "w", 1_600_000_000), // outside range
        ];
        let d = paragraph_date(&blame, 1, 3).unwrap();
        // 1_800_000_000 = 2027-01-15
        assert_eq!(d.to_string(), "2027-01-15");
    }

    #[test]
    fn paragraph_date_returns_none_for_empty_range() {
        let blame = vec![sample(5, "x", 1_700_000_000)];
        assert!(paragraph_date(&blame, 1, 3).is_none());
    }

    #[test]
    fn cache_dir_is_created_on_first_save() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let cache_dir = tmp.path().join(".ft/cache");
        assert!(!cache_dir.exists());
        let c = BlameCache::default();
        c.save(tmp.path()).unwrap();
        assert!(cache_dir.exists());
    }
}

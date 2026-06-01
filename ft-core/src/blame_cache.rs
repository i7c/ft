//! Lazy on-disk cache for `git blame` output, keyed on
//! `(vault-relative path, HEAD commit hash)`.
//!
//! The journal feature needs per-line commit dates to assign a "section
//! date" to each paragraph node it surfaces. Running `git blame` on
//! every matching file every query is too slow (~30ms per file × dozens
//! of matches). This cache stores the blame for files we've already
//! queried, scoped to the HEAD hash that produced it. When HEAD moves
//! (a commit happens), cache entries for unmodified files stay valid
//! only if they were keyed on the *current* HEAD; entries for modified
//! files automatically become stale because their HEAD is a previous
//! commit.
//!
//! Stored as a single msgpack file at `<vault>/.ft/cache/blame.msgpack`.
//! Cold loads of missing files return an empty cache; corrupt files
//! (incompatible schema after we change `LineBlame`) also fall back to
//! empty — the next blame call rebuilds.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::git::LineBlame;

const CACHE_REL: &str = ".ft/cache/blame.msgpack";

/// On-disk blame cache. Construct via [`BlameCache::load`]; persist
/// modifications with [`BlameCache::save`].
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BlameCache {
    /// Outer key: vault-relative path as a string (cross-platform).
    /// Inner: `(head_hash, blame entries)`. Only one head_hash is
    /// retained per path — the most recently written.
    entries: HashMap<String, CacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    head: String,
    blame: Vec<LineBlame>,
}

impl BlameCache {
    /// Load the cache from `<vault_root>/.ft/cache/blame.msgpack`.
    /// Returns an empty cache if the file does not exist or fails to
    /// deserialize (treating corruption as cold start).
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
        match rmp_serde::from_slice::<BlameCache>(&bytes) {
            Ok(c) => Ok(c),
            // Corrupt or schema-incompatible — drop and start fresh.
            Err(_) => Ok(Self::default()),
        }
    }

    /// Persist the cache to `<vault_root>/.ft/cache/blame.msgpack`,
    /// creating the parent directory if absent. Written via a same-dir
    /// temp file + rename so a crash mid-write leaves the prior cache
    /// intact rather than truncated.
    pub fn save(&self, vault_root: &Path) -> Result<()> {
        use std::io::Write;
        let path = vault_root.join(CACHE_REL);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| crate::error::Error::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        let bytes = rmp_serde::to_vec(self)
            .map_err(|e| crate::error::Error::Notes(format!("serialize blame cache: {e}")))?;
        let parent = path.parent().unwrap_or(Path::new("."));
        let mut tmp =
            tempfile::NamedTempFile::new_in(parent).map_err(|e| crate::error::Error::Io {
                path: path.clone(),
                source: e,
            })?;
        tmp.write_all(&bytes).map_err(|e| crate::error::Error::Io {
            path: tmp.path().to_path_buf(),
            source: e,
        })?;
        tmp.as_file_mut()
            .sync_all()
            .map_err(|e| crate::error::Error::Io {
                path: tmp.path().to_path_buf(),
                source: e,
            })?;
        tmp.persist(&path).map_err(|e| crate::error::Error::Io {
            path: path.clone(),
            source: e.error,
        })?;
        Ok(())
    }

    /// Return cached blame if an entry exists for `path` and its
    /// stored HEAD matches `head`. Returns `None` on cache miss
    /// (no entry at all, or stale).
    pub fn get(&self, path: &str, head: &str) -> Option<&Vec<LineBlame>> {
        let entry = self.entries.get(path)?;
        if entry.head == head {
            Some(&entry.blame)
        } else {
            None
        }
    }

    /// Insert (or overwrite) blame for `path` at HEAD `head`. The new
    /// entry replaces any previously stored entry for `path`.
    pub fn insert(&mut self, path: String, head: String, blame: Vec<LineBlame>) {
        self.entries.insert(path, CacheEntry { head, blame });
    }
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

    fn sample(line: u32, sha: &str, ts: i64) -> LineBlame {
        LineBlame {
            line,
            commit_hash: sha.to_string(),
            timestamp: ts,
        }
    }

    #[test]
    fn missing_file_loads_empty_cache() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let c = BlameCache::load(tmp.path()).unwrap();
        assert!(c.get("any.md", "deadbeef").is_none());
    }

    #[test]
    fn round_trip_through_msgpack() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let mut c = BlameCache::default();
        c.insert(
            "a.md".to_string(),
            "abc123".to_string(),
            vec![
                sample(1, "abc123", 1_700_000_000),
                sample(2, "abc123", 1_700_000_001),
            ],
        );
        c.save(tmp.path()).unwrap();

        let loaded = BlameCache::load(tmp.path()).unwrap();
        let entries = loaded.get("a.md", "abc123").expect("entry exists");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].timestamp, 1_700_000_000);
    }

    #[test]
    fn stale_head_returns_none() {
        let mut c = BlameCache::default();
        c.insert(
            "a.md".to_string(),
            "old_head".to_string(),
            vec![sample(1, "old_head", 1_700_000_000)],
        );
        assert!(c.get("a.md", "new_head").is_none());
        assert!(c.get("a.md", "old_head").is_some());
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

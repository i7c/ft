//! Paragraph recency for ranking, supplied at query time.
//!
//! The index is immutable and rebuilt only on a rescan, but blame data
//! arrives later (the TUI warms the blame cache in the background), so
//! recency lives in a separate [`RecencyTable`] passed through
//! [`crate::search::SearchCtx`]. A paragraph's recency day is its blame
//! date when the cache holds a valid entry for its file, else its file's
//! mtime day. The ranking prior's recency part decays exponentially with
//! the age in whole days relative to the newest recency day in the
//! index — not wall-clock "today", so copies of a vault rank alike.

use std::path::{Path, PathBuf};

use crate::blame_cache::{paragraph_date, BlameCache, BlameValidity, MAX_BLAME_LINES};
use crate::search::index::SearchIndex;

/// Save the cache after this many newly blamed files during a warm-up,
/// so an interrupted warm-up keeps its progress.
const SAVE_EVERY: usize = 50;

/// Max prior share from recency.
pub(crate) const PRIOR_RECENCY: f64 = 0.10;
/// Recency decay constant, in days.
const RECENCY_DAYS: f64 = 90.0;

/// Per-paragraph blame days for one index build.
#[derive(Debug, Clone, Default)]
pub struct RecencyTable {
    /// Doc id → blame day (days since the epoch), `None` when the file
    /// has no valid cached blame.
    days: Vec<Option<i32>>,
    /// Newest effective day (blame day, else mtime day) across docs.
    newest: Option<i32>,
}

impl RecencyTable {
    /// Read blame days for every indexed paragraph from the cache. Never
    /// runs `git blame`.
    pub fn build(index: &SearchIndex, cache: &BlameCache, validity: &BlameValidity) -> Self {
        let mut days = vec![None; index.docs.len()];
        for note in &index.notes {
            let Some(blame) = cache.get(&note.path, validity) else {
                continue;
            };
            for doc in note.docs.clone() {
                let d = &index.docs[doc as usize];
                days[doc as usize] =
                    paragraph_date(blame, d.line_start, d.line_end).map(day_number);
            }
        }
        let newest = index
            .docs
            .iter()
            .enumerate()
            .map(|(i, d)| days[i].unwrap_or(index.notes[d.note as usize].mtime_day))
            .max();
        RecencyTable { days, newest }
    }

    /// Paragraphs whose recency comes from blame (for status reporting).
    pub fn covered(&self) -> usize {
        self.days.iter().filter(|d| d.is_some()).count()
    }

    /// Whether the table was built for an index with `docs` paragraphs.
    /// A stale table (from before a rescan) is ignored by the engine.
    pub(crate) fn fits(&self, docs: usize) -> bool {
        self.days.len() == docs
    }
}

/// Days since the Unix epoch for a calendar date.
fn day_number(d: chrono::NaiveDate) -> i32 {
    (d - chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid epoch")).num_days() as i32
}

/// The recency part of a paragraph's prior, in `[0, PRIOR_RECENCY]`.
pub(crate) fn recency_part(index: &SearchIndex, table: Option<&RecencyTable>, doc: u32) -> f64 {
    let table = table.filter(|t| t.fits(index.docs.len()));
    let mtime_day = index.notes[index.docs[doc as usize].note as usize].mtime_day;
    let day = table
        .and_then(|t| t.days[doc as usize])
        .unwrap_or(mtime_day);
    let newest = match table {
        Some(t) => t.newest,
        None => index.newest_mtime_day(),
    }
    .unwrap_or(day);
    let age = f64::from((newest - day).max(0));
    PRIOR_RECENCY * (-age / RECENCY_DAYS).exp()
}

/// Indexed files whose paragraphs lack valid cached blame and can be
/// cached (committed, clean, not huge), newest mtime first so the notes
/// being worked on are covered first.
pub fn warm_targets(
    index: &SearchIndex,
    cache: &BlameCache,
    validity: &BlameValidity,
) -> Vec<(PathBuf, u32)> {
    let mut targets: Vec<(i32, PathBuf, u32)> = index
        .notes
        .iter()
        .filter(|n| !n.docs.is_empty() && n.line_count <= MAX_BLAME_LINES)
        .filter(|n| validity.blob(&n.path).is_some() && cache.get(&n.path, validity).is_none())
        .map(|n| (n.mtime_day, n.path.clone(), n.line_count))
        .collect();
    targets.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    targets.into_iter().map(|(_, p, l)| (p, l)).collect()
}

/// Blame every [`warm_targets`] file into `cache`, saving under
/// `vault_root` every [`SAVE_EVERY`] files and at the end. Stops early
/// (after saving) as soon as `keep_going` returns `false`; reports
/// `(done, total)` after each file. Returns how many files were blamed.
pub fn warm_blame(
    index: &SearchIndex,
    vault_root: &Path,
    validity: &BlameValidity,
    cache: &mut BlameCache,
    keep_going: impl Fn() -> bool,
    mut progress: impl FnMut(usize, usize),
) -> usize {
    let targets = warm_targets(index, cache, validity);
    let total = targets.len();
    let mut blamed = 0;
    for (i, (path, lines)) in targets.into_iter().enumerate() {
        if !keep_going() {
            break;
        }
        if cache.ensure(&path, lines, validity).1 {
            blamed += 1;
            if blamed % SAVE_EVERY == 0 {
                let _ = cache.save(vault_root);
            }
        }
        progress(i + 1, total);
    }
    if blamed > 0 {
        let _ = cache.save(vault_root);
    }
    blamed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("git on PATH");
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }

    fn vault() -> assert_fs::TempDir {
        let tmp = assert_fs::TempDir::new().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        git(d, &["config", "user.name", "T"]);
        git(d, &["config", "user.email", "t@e.com"]);
        git(d, &["config", "commit.gpgsign", "false"]);
        for n in ["a", "b", "c"] {
            std::fs::write(d.join(format!("{n}.md")), format!("{n} text.\n")).unwrap();
        }
        git(d, &["add", "."]);
        git(d, &["commit", "-q", "-m", "init"]);
        // Dirty and untracked files are not warmed.
        std::fs::write(d.join("c.md"), "c edited.\n").unwrap();
        std::fs::write(d.join("new.md"), "untracked.\n").unwrap();
        tmp
    }

    #[test]
    fn warm_covers_clean_committed_files_and_persists() {
        let tmp = vault();
        let scan = crate::scan::scan_vault(tmp.path(), &[]);
        let index = SearchIndex::build(&scan, &[]);
        let validity = BlameValidity::snapshot(tmp.path()).unwrap().unwrap();
        let mut cache = BlameCache::default();
        let mut seen = Vec::new();
        let blamed = warm_blame(
            &index,
            tmp.path(),
            &validity,
            &mut cache,
            || true,
            |d, t| seen.push((d, t)),
        );
        assert_eq!(blamed, 2, "a.md and b.md; c.md is dirty, new.md untracked");
        assert_eq!(seen.last(), Some(&(2, 2)));
        let table = RecencyTable::build(&index, &cache, &validity);
        assert_eq!(table.covered(), 2);
        // Persisted: a fresh load has nothing left to warm.
        let reloaded = BlameCache::load(tmp.path()).unwrap();
        assert!(warm_targets(&index, &reloaded, &validity).is_empty());
    }

    #[test]
    fn warm_stops_when_told() {
        let tmp = vault();
        let scan = crate::scan::scan_vault(tmp.path(), &[]);
        let index = SearchIndex::build(&scan, &[]);
        let validity = BlameValidity::snapshot(tmp.path()).unwrap().unwrap();
        let mut cache = BlameCache::default();
        let blamed = warm_blame(
            &index,
            tmp.path(),
            &validity,
            &mut cache,
            || false,
            |_, _| {},
        );
        assert_eq!(blamed, 0);
        assert!(cache.is_empty());
    }
}

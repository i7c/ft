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

use crate::blame_cache::{paragraph_date, BlameCache, BlameValidity};
use crate::search::index::SearchIndex;

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

//! `SynthSource` — the honest input type for synth-note scaffolding.
//!
//! `plan_synth_scaffold` and `accrete::filter_missing` consume
//! [`SynthSource`]. A `SynthSource` carries exactly the
//! four fields a protected section pins: source path, inclusive line
//! range, and the verbatim body text. Feed-specific fields (blame date,
//! matched link targets) stay on the feed entry type and are dropped by
//! the [`From`] conversion at the call boundary.
//!
//! This keeps the pinning engine honest: search results, `--from` picks,
//! and the paragraph-synth TUI flow construct a `SynthSource` directly
//! with no fabricated `date`/`matched`, and the Recent feed lowers
//! through `From`.

use std::path::PathBuf;

use crate::recent::RecentEntry;

/// One source paragraph slated to become a protected `[!ft-source]`
/// callout. Carries exactly the fields the scaffold planner pins; no
/// feed-specific metadata (blame date, matched link targets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthSource {
    /// Vault-relative path of the source note.
    pub source_path: PathBuf,
    /// 1-indexed line number of the body's first line in `source_path`.
    pub line_start: u32,
    /// 1-indexed line number of the body's last line.
    pub line_end: u32,
    /// The verbatim body text (lines joined with `\n`, no trailing
    /// newline). Hashed and pinned as the callout body.
    pub body: String,
}

impl From<&RecentEntry> for SynthSource {
    fn from(e: &RecentEntry) -> Self {
        SynthSource {
            source_path: e.source_path.clone(),
            line_start: e.line_start,
            line_end: e.line_end,
            body: e.section_text.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn recent_entry_lowers_keeping_only_four_fields() {
        let e = RecentEntry {
            source_title: "source".into(),
            source_path: PathBuf::from("notes/source.md"),
            line_start: 5,
            line_end: 5,
            section_text: "Second paragraph.".into(),
            date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
        };
        let s = SynthSource::from(&e);
        assert_eq!(s.source_path, PathBuf::from("notes/source.md"));
        assert_eq!((s.line_start, s.line_end), (5, 5));
        assert_eq!(s.body, "Second paragraph.");
    }
}

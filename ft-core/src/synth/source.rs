//! `SynthSource` — the honest input type for synth-note scaffolding.
//!
//! `plan_synth_scaffold` and `accrete::filter_missing` consume
//! [`SynthSource`] rather than the feed-specific [`crate::gather::GatherEntry`]
//! / [`crate::recent::RecentEntry`]. A `SynthSource` carries exactly the
//! four fields a protected section pins: source path, inclusive line
//! range, and the verbatim body text. Feed-specific fields (blame date,
//! matched link targets) stay on the feed entry types and are dropped by
//! the [`From`] conversions at the call boundary.
//!
//! This keeps the pinning engine honest: a source-driven pick (the
//! paragraph-synth TUI flow) constructs a `SynthSource` directly with no
//! fabricated `date`/`matched`, and feed callers lower through `From`.

use std::path::PathBuf;

use crate::gather::GatherEntry;
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

impl From<&GatherEntry> for SynthSource {
    fn from(e: &GatherEntry) -> Self {
        SynthSource {
            source_path: e.source_path.clone(),
            line_start: e.line_start,
            line_end: e.line_end,
            body: e.section_text.clone(),
        }
    }
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
    fn gather_entry_lowers_keeping_only_four_fields() {
        let e = GatherEntry {
            source_title: "source".into(),
            source_path: PathBuf::from("notes/source.md"),
            line_start: 1,
            line_end: 2,
            section_text: "First paragraph.\nLine two.".into(),
            date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
            matched: vec![],
        };
        let s = SynthSource::from(&e);
        assert_eq!(s.source_path, PathBuf::from("notes/source.md"));
        assert_eq!((s.line_start, s.line_end), (1, 2));
        assert_eq!(s.body, "First paragraph.\nLine two.");
    }

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

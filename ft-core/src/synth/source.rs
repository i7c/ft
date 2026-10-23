//! `SynthSource` — the honest input type for synth-note scaffolding.
//!
//! `plan_synth_scaffold` and `accrete::filter_missing` consume
//! [`SynthSource`]. A `SynthSource` carries exactly the
//! four fields a protected section pins: source path, inclusive line
//! range, and the verbatim body text.
//!
//! This keeps the pinning engine honest: search results, `--from` picks,
//! and the paragraph-synth TUI flow construct a `SynthSource` directly
//! with no fabricated `date`/`matched`.

use std::path::PathBuf;

/// One source paragraph slated to become a protected `[!ft-source]`
/// callout. Carries exactly the fields the scaffold planner pins.
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

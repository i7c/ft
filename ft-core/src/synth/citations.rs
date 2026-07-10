//! Citation index: which synth notes cite which source paragraphs.
//!
//! Built by walking the vault's synth notes (`ft.synth.enabled: true`) and
//! parsing their `[!ft-source]` callouts. Lookup classifies a paragraph
//! into one of three states:
//!
//! - **Cited** — some callout pins the same source path with a
//!   byte-identical body. The matching rule (header content-hash prefix
//!   as fast reject, exact body compare to confirm) is deliberately the
//!   same as [`crate::synth::accrete::filter_missing`], so feed badges
//!   never disagree with scaffold/grow plan-time dedup.
//! - **CitedStale** — no exact match, but a callout pins the same
//!   source path with a line range overlapping the paragraph's current
//!   range and a different body: the paragraph was edited after being
//!   cited. The two ranges come from different revisions, so this is an
//!   advisory heuristic, not provenance.
//! - **Uncited** — neither of the above.
//!
//! The index is derivative data: cheap to rebuild (synth notes are
//! few), never persisted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::synth::callout::{
    compute_section_hash, is_synth_note, parse as parse_callouts, CONTENT_HASH_PREFIX_LEN,
};
use crate::vault::Vault;

/// Citation state of one paragraph, as classified by
/// [`CitationIndex::lookup`]. Citing note paths are vault-relative,
/// sorted, and deduplicated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CitationState {
    Cited { notes: Vec<PathBuf> },
    CitedStale { notes: Vec<PathBuf> },
    Uncited,
}

impl CitationState {
    /// True when the paragraph is pinned byte-identically in `note`
    /// specifically — the note-context question ("already in this
    /// note?"), equivalent to `filter_missing` dropping the entry on
    /// append to `note`.
    pub fn cited_in(&self, note: &Path) -> bool {
        match self {
            CitationState::Cited { notes } => notes.iter().any(|n| n == note),
            _ => false,
        }
    }

    /// The citing notes, regardless of staleness. Empty when uncited.
    pub fn notes(&self) -> &[PathBuf] {
        match self {
            CitationState::Cited { notes } | CitationState::CitedStale { notes } => notes,
            CitationState::Uncited => &[],
        }
    }

    pub fn is_cited(&self) -> bool {
        matches!(self, CitationState::Cited { .. })
    }

    pub fn is_stale(&self) -> bool {
        matches!(self, CitationState::CitedStale { .. })
    }
}

/// One callout occurrence, reduced to what lookup needs.
#[derive(Debug, Clone)]
struct CalloutRef {
    /// Vault-relative path of the citing synth note.
    note: PathBuf,
    /// Header tokens: pinned inclusive line range in the source (at the
    /// pinned commit — a different revision than the paragraph being
    /// looked up).
    line_start: u32,
    line_end: u32,
    /// Header content-hash prefix (may be stale if hand-edited; used
    /// only as the fast reject, same as `filter_missing`).
    content_hash: String,
    /// Callout body, unquoted.
    body: String,
}

/// Index from source path to the callouts citing it, across every
/// synth note in the vault.
#[derive(Debug, Default, Clone)]
pub struct CitationIndex {
    by_path: HashMap<PathBuf, Vec<CalloutRef>>,
    /// Every synth note seen by the build (vault-relative, walk order),
    /// including ones with zero callouts. Consumers use this as the
    /// authoritative "which notes are synth notes" list without
    /// re-reading the vault.
    pub synth_notes: Vec<PathBuf>,
    /// Synth notes that contributed nothing or partially: unreadable
    /// files, or notes containing `[!ft-source]` markers that did not
    /// parse as callouts (malformed headers). Diagnostics only — the
    /// build never aborts on them.
    pub skipped: Vec<PathBuf>,
}

impl CitationIndex {
    /// Walk the vault, read every synth note, and index its callouts.
    ///
    /// Mirrors `synth::verify::verify_all`'s discovery: every markdown
    /// file is read and checked for the `ft.synth.enabled: true` marker.
    /// Unreadable files and malformed callout headers are recorded in
    /// [`CitationIndex::skipped`] rather than aborting.
    pub fn build(vault: &Vault) -> CitationIndex {
        let mut index = CitationIndex::default();
        for note_rel in crate::synth::verify::walk_markdown_files(&vault.path) {
            let absolute = vault.path.join(&note_rel);
            let content = match std::fs::read_to_string(&absolute) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if !is_synth_note(&content) {
                continue;
            }
            index.add_note(&note_rel, &content);
        }
        index
    }

    /// Index one synth note's callouts from its raw content. Exposed
    /// for tests and for callers that already hold note contents.
    pub fn add_note(&mut self, note_rel: &Path, content: &str) {
        self.synth_notes.push(note_rel.to_path_buf());
        let callouts = parse_callouts(content);
        // A `[!ft-source]` marker that didn't survive parsing is a
        // malformed header; flag the note but keep its valid callouts.
        if content.matches("[!ft-source]").count() > callouts.len() {
            self.skipped.push(note_rel.to_path_buf());
        }
        for c in callouts {
            self.by_path
                .entry(c.source_path.clone())
                .or_default()
                .push(CalloutRef {
                    note: note_rel.to_path_buf(),
                    line_start: c.line_start,
                    line_end: c.line_end,
                    content_hash: c.content_hash,
                    body: c.body,
                });
        }
    }

    /// Classify the paragraph at `line_range` (1-indexed inclusive,
    /// current revision) of `source_path` with text `body`.
    pub fn lookup(&self, source_path: &Path, line_range: (u32, u32), body: &str) -> CitationState {
        let Some(cands) = self.by_path.get(source_path) else {
            return CitationState::Uncited;
        };

        let hash = compute_section_hash(body);
        let prefix = &hash[..CONTENT_HASH_PREFIX_LEN.min(hash.len())];

        let mut cited: Vec<PathBuf> = cands
            .iter()
            .filter(|c| c.content_hash == prefix && c.body == body)
            .map(|c| c.note.clone())
            .collect();
        if !cited.is_empty() {
            cited.sort();
            cited.dedup();
            return CitationState::Cited { notes: cited };
        }

        let (start, end) = line_range;
        let mut stale: Vec<PathBuf> = cands
            .iter()
            .filter(|c| c.line_start <= end && start <= c.line_end && c.body != body)
            .map(|c| c.note.clone())
            .collect();
        if !stale.is_empty() {
            stale.sort();
            stale.dedup();
            return CitationState::CitedStale { notes: stale };
        }

        CitationState::Uncited
    }

    /// True when no synth note contributed any callout.
    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::callout::{serialize, ProtectedSection};

    fn synth_note(sections: &[ProtectedSection]) -> String {
        let mut out = String::from("---\nft:\n  synth:\n    enabled: true\n---\n\n");
        for s in sections {
            out.push_str(&serialize(s));
            out.push_str("\n\n");
        }
        out
    }

    fn section(path: &str, lines: (u32, u32), body: &str) -> ProtectedSection {
        ProtectedSection {
            source_path: PathBuf::from(path),
            line_start: lines.0,
            line_end: lines.1,
            commit_sha: "abc1234".into(),
            content_hash: compute_section_hash(body),
            body: body.into(),
        }
    }

    #[test]
    fn exact_match_is_cited() {
        let body = "A paragraph about [[foo]].";
        let mut idx = CitationIndex::default();
        idx.add_note(
            Path::new("Synthesis/foo.md"),
            &synth_note(&[section("notes/a.md", (10, 10), body)]),
        );
        let state = idx.lookup(Path::new("notes/a.md"), (10, 10), body);
        assert_eq!(
            state,
            CitationState::Cited {
                notes: vec![PathBuf::from("Synthesis/foo.md")]
            }
        );
        assert!(state.cited_in(Path::new("Synthesis/foo.md")));
        assert!(!state.cited_in(Path::new("Synthesis/other.md")));
    }

    #[test]
    fn edited_since_cited_is_stale() {
        let mut idx = CitationIndex::default();
        idx.add_note(
            Path::new("Synthesis/foo.md"),
            &synth_note(&[section("notes/a.md", (10, 12), "old text")]),
        );
        // Same file, overlapping range, different body.
        let state = idx.lookup(Path::new("notes/a.md"), (10, 13), "new text");
        assert_eq!(
            state,
            CitationState::CitedStale {
                notes: vec![PathBuf::from("Synthesis/foo.md")]
            }
        );
        assert!(state.is_stale());
        assert!(!state.cited_in(Path::new("Synthesis/foo.md")));
    }

    #[test]
    fn non_overlapping_or_unknown_is_uncited() {
        let mut idx = CitationIndex::default();
        idx.add_note(
            Path::new("Synthesis/foo.md"),
            &synth_note(&[section("notes/a.md", (10, 12), "old text")]),
        );
        // Same file, disjoint range.
        assert_eq!(
            idx.lookup(Path::new("notes/a.md"), (20, 22), "unrelated"),
            CitationState::Uncited
        );
        // Unknown file.
        assert_eq!(
            idx.lookup(Path::new("notes/b.md"), (10, 12), "old text"),
            CitationState::Uncited
        );
    }

    #[test]
    fn multiple_citing_notes_collected_sorted() {
        let body = "shared paragraph";
        let mut idx = CitationIndex::default();
        idx.add_note(
            Path::new("Synthesis/b.md"),
            &synth_note(&[section("notes/a.md", (1, 1), body)]),
        );
        idx.add_note(
            Path::new("Synthesis/a.md"),
            &synth_note(&[section("notes/a.md", (1, 1), body)]),
        );
        assert_eq!(
            idx.lookup(Path::new("notes/a.md"), (1, 1), body),
            CitationState::Cited {
                notes: vec![
                    PathBuf::from("Synthesis/a.md"),
                    PathBuf::from("Synthesis/b.md"),
                ]
            }
        );
    }

    #[test]
    fn malformed_callout_flags_note_keeps_valid_ones() {
        let body = "good paragraph";
        let good = serialize(&section("notes/a.md", (1, 1), body));
        let content = format!(
            "---\nft:\n  synth:\n    enabled: true\n---\n\n> [!ft-source] totally broken header\n> junk\n\n{good}\n"
        );
        let mut idx = CitationIndex::default();
        idx.add_note(Path::new("Synthesis/foo.md"), &content);
        assert_eq!(idx.skipped, vec![PathBuf::from("Synthesis/foo.md")]);
        assert!(idx.lookup(Path::new("notes/a.md"), (1, 1), body).is_cited());
    }

    #[test]
    fn lookup_agrees_with_filter_missing() {
        // Spec "Consistency with scaffold dedup": for a note's callout
        // set, lookup == Cited-in-note ⇔ filter_missing drops the entry.
        use crate::gather::GatherEntry;
        use crate::synth::accrete::filter_missing;
        use crate::synth::callout::parse as parse_callouts;

        let note = Path::new("Synthesis/foo.md");
        let pinned = "pinned paragraph";
        let content = synth_note(&[
            section("notes/a.md", (1, 1), pinned),
            section("notes/b.md", (5, 7), "another pinned"),
        ]);
        let mut idx = CitationIndex::default();
        idx.add_note(note, &content);
        let callouts = parse_callouts(&content);

        let entry = |path: &str, lines: (u32, u32), text: &str| GatherEntry {
            source_title: "t".into(),
            source_path: PathBuf::from(path),
            line_start: lines.0,
            line_end: lines.1,
            section_text: text.into(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 7, 6).unwrap(),
            matched: Vec::new(),
        };

        let cases = vec![
            entry("notes/a.md", (1, 1), pinned),           // exact → dropped
            entry("notes/a.md", (1, 2), "edited text"),    // stale → kept
            entry("notes/c.md", (9, 9), "brand new"),      // uncited → kept
            entry("notes/b.md", (5, 7), "another pinned"), // exact → dropped
        ];
        let kept = filter_missing(&callouts, cases.clone());
        for e in &cases {
            let dropped = !kept.contains(e);
            let cited = idx
                .lookup(&e.source_path, (e.line_start, e.line_end), &e.section_text)
                .cited_in(note);
            assert_eq!(
                cited, dropped,
                "lookup/filter_missing disagree on {:?}",
                e.source_path
            );
        }
    }

    #[test]
    fn mangled_header_hash_rejects_exact_match() {
        // filter_missing treats a hand-mangled header hash as "not
        // pinned" (the hash reject fails before the body compare);
        // lookup must agree.
        let body = "paragraph text";
        let mut s = section("notes/a.md", (1, 1), body);
        s.content_hash = "000000".into();
        let mut idx = CitationIndex::default();
        idx.add_note(Path::new("Synthesis/foo.md"), &synth_note(&[s]));
        // Identical body is excluded from the stale set too → Uncited.
        assert_eq!(
            idx.lookup(Path::new("notes/a.md"), (1, 1), body),
            CitationState::Uncited
        );
    }
}

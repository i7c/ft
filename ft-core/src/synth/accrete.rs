//! Accrete support for synth notes: dedup incoming entries against
//! what's already pinned in a note.
//!
//! [`filter_missing`] drops entries whose `(source_path, body)` is
//! already pinned, making scaffold's append path idempotent. Pure
//! (no I/O). The former `grow --new-only` watermark primitive
//! (`last_synth_watermark`) was removed with the deprecated gather tab;
//! append-dedup alone keeps re-running scaffold idempotent.
//!
//! See [`crate::synth`] for the higher-level synth-note contract and
//! `docs/architecture.md` §"Synthesis".

use std::collections::HashMap;
use std::path::Path;

use crate::synth::callout::{compute_section_hash, ParsedCallout, CONTENT_HASH_PREFIX_LEN};
use crate::synth::source::SynthSource;

/// Drop journal entries whose `(source_path, body)` is already pinned in
/// `existing`. The dedup key is the pair of the vault-relative source
/// path and the entry's `body` compared byte-for-byte against a
/// callout's unprefixed body. The 6-hex `content_hash` is used as a fast
/// pre-filter (a `hash → Vec<(&path, &body)>` map) before the exact body
/// compare; the body compare is the source of truth (the 6-hex prefix
/// could collide on distinct bodies). The `commit_sha` of an existing
/// callout is deliberately NOT part of the key — same body at a newer
/// commit means the paragraph is unchanged and there is no reason to
/// re-pin it (refreshing a stale pin is `repair`/`reslice`, a different
/// flow). Input order is preserved among the survivors.
///
/// Pure: no I/O, no git. Cheap (bodies are small).
pub fn filter_missing(existing: &[ParsedCallout], entries: Vec<SynthSource>) -> Vec<SynthSource> {
    // hash-prefix → list of (path, body) for that prefix. The prefix is
    // a fast reject; the body compare below is exact.
    let mut by_hash: HashMap<&str, Vec<(&Path, &str)>> = HashMap::new();
    for c in existing {
        by_hash
            .entry(&c.content_hash)
            .or_default()
            .push((&c.source_path, &c.body));
    }

    entries
        .into_iter()
        .filter(|e| {
            let h = compute_section_hash(&e.body);
            let prefix = &h[..CONTENT_HASH_PREFIX_LEN.min(h.len())];
            // No existing callout with this hash prefix → definitely new.
            let Some(cands) = by_hash.get(prefix) else {
                return true;
            };
            // Hash matched: confirm via exact (path, body) compare.
            !cands
                .iter()
                .any(|(p, b)| *p == e.source_path.as_path() && *b == e.body)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ── filter_missing ──────────────────────────────────────────────

    fn callout(path: &str, body: &str) -> ParsedCallout {
        ParsedCallout {
            source_path: PathBuf::from(path),
            line_start: 1,
            line_end: 1,
            commit_sha: "abc1234".to_string(),
            content_hash: compute_section_hash(body),
            body: body.to_string(),
            byte_range: 0..0,
            header_line: 1,
        }
    }

    fn entry(path: &str, body: &str, _date: &str) -> SynthSource {
        SynthSource {
            source_path: PathBuf::from(path),
            line_start: 1,
            line_end: 1,
            body: body.to_string(),
        }
    }

    #[test]
    fn filter_unchanged_paragraph_dropped() {
        let existing = vec![callout("notes/foo.md", "the original body")];
        let entries = vec![entry("notes/foo.md", "the original body", "2026-06-01")];
        let out = filter_missing(&existing, entries);
        assert!(out.is_empty(), "unchanged paragraph should be dropped");
    }

    #[test]
    fn filter_updated_paragraph_kept() {
        let existing = vec![callout("notes/foo.md", "the original body")];
        let entries = vec![entry("notes/foo.md", "the EDITED body", "2026-06-02")];
        let out = filter_missing(&existing, entries);
        assert_eq!(out.len(), 1, "updated paragraph should be kept");
    }

    #[test]
    fn filter_brand_new_paragraph_kept() {
        let existing = vec![callout("notes/foo.md", "the original body")];
        let entries = vec![entry("notes/bar.md", "a different paragraph", "2026-06-03")];
        let out = filter_missing(&existing, entries);
        assert_eq!(out.len(), 1, "brand-new paragraph should be kept");
    }

    #[test]
    fn filter_order_preserved_among_survivors() {
        let existing = vec![callout("notes/b.md", "B body")];
        let entries = vec![
            entry("notes/a.md", "A body", "2026-06-01"),
            entry("notes/b.md", "B body", "2026-06-02"), // pinned → dropped
            entry("notes/c.md", "C body", "2026-06-03"),
        ];
        let out = filter_missing(&existing, entries);
        let bodies: Vec<&str> = out.iter().map(|e| e.body.as_str()).collect();
        assert_eq!(bodies, vec!["A body", "C body"], "order must be preserved");
    }

    #[test]
    fn filter_empty_existing_keeps_all() {
        let entries = vec![
            entry("notes/a.md", "A body", "2026-06-01"),
            entry("notes/b.md", "B body", "2026-06-02"),
        ];
        let out = filter_missing(&[], entries);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn filter_distinct_bodies_with_same_hash_prefix_kept_via_body_compare() {
        // Two genuinely different bodies whose blake3 6-hex prefixes
        // could (in principle) collide. The body compare must be the
        // source of truth — a hash-prefix match on a *different* body
        // must NOT cause a drop. We can't easily force a real blake3
        // collision, so we test the inverse contract: a callout with a
        // hand-set colliding hash but a different body does not drop a
        // distinct entry. We craft the callout's content_hash to match
        // the entry's real hash while its body differs.
        let body_a = "distinct content A";
        let body_b = "distinct content B (different body)";
        let hash_a = compute_section_hash(body_a);
        let mut c = callout("notes/foo.md", body_b);
        // Force the callout's hash to match the entry's hash, simulating
        // a prefix collision while the bodies differ.
        c.content_hash = hash_a.clone();
        let existing = vec![c];
        let entries = vec![entry("notes/foo.md", body_a, "2026-06-01")];
        let out = filter_missing(&existing, entries);
        assert_eq!(
            out.len(),
            1,
            "hash-prefix match on a different body must NOT drop the entry"
        );
    }
}

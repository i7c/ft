//! Accrete support for synth notes: dedup incoming journal entries
//! against what's already pinned, and compute a "last synth" watermark
//! from a note's existing `[!ft-source]` callouts.
//!
//! These are the two pure-ish primitives behind `ft synth grow` and the
//! Journal tab's new-only flow:
//!
//! - [`filter_missing`] drops entries whose body text is already pinned
//!   in the note, making scaffold's append path idempotent. Pure (no I/O).
//! - [`last_synth_watermark`] computes the newest pinned commit SHA among
//!   a note's callouts and its committer date — the scope for `--new-only`.
//!   It reads the git repo (one `rev-list` + one `log` call), so it is
//!   **not** pure; callers pass a `repo_root`.
//!
//! See [`crate::synth`] for the higher-level synth-note contract and
//! `docs/architecture.md` §"Synthesis" for the grow flow.

use std::collections::HashMap;
use std::path::Path;

use chrono::NaiveDate;

use crate::error::{Error, Result};
use crate::gather::GatherEntry;
use crate::git;
use crate::synth::callout::{compute_section_hash, ParsedCallout, CONTENT_HASH_PREFIX_LEN};

/// Drop journal entries whose `(source_path, body)` is already pinned in
/// `existing`. The dedup key is the pair of the vault-relative source
/// path and the entry's `section_text` compared byte-for-byte against a
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
pub fn filter_missing(existing: &[ParsedCallout], entries: Vec<GatherEntry>) -> Vec<GatherEntry> {
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
            let h = compute_section_hash(&e.section_text);
            let prefix = &h[..CONTENT_HASH_PREFIX_LEN.min(h.len())];
            // No existing callout with this hash prefix → definitely new.
            let Some(cands) = by_hash.get(prefix) else {
                return true;
            };
            // Hash matched: confirm via exact (path, body) compare.
            !cands
                .iter()
                .any(|(p, b)| *p == e.source_path.as_path() && *b == e.section_text)
        })
        .collect()
}

/// Compute the last-synth watermark from a note's existing callouts: the
/// topological tip among the callouts' pinned `commit_sha` values (the
/// descendant reachable from all of them), paired with that commit's
/// committer date.
///
/// Each pinned short SHA is verified reachable via `git cat-file -e`
/// before inclusion; unreachable SHAs (shallow clone, branch switch,
/// dropped-by-rebase) are skipped. When all SHAs are unreachable, or the
/// callout list is empty, returns `Ok(None)` — the caller degrades
/// `--new-only` to "all missing" with a warning. An ambiguous short SHA
/// (matches more than one commit) surfaces as `Err(SynthWatermark)`.
///
/// `repo_root` is the git repository root (not the vault root); callers
/// typically obtain it via `git::RepoMap::discover`.
pub fn last_synth_watermark(
    repo_root: &Path,
    existing: &[ParsedCallout],
) -> Result<Option<(String, NaiveDate)>> {
    // Collect distinct short SHAs.
    let mut distinct: Vec<&str> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for c in existing {
        if seen.insert(c.commit_sha.as_str()) {
            distinct.push(c.commit_sha.as_str());
        }
    }
    if distinct.is_empty() {
        return Ok(None);
    }

    // Keep only reachable SHAs.
    let reachable: Vec<&str> = distinct
        .iter()
        .copied()
        .filter(|sha| git::object_exists(repo_root, sha))
        .collect();
    if reachable.is_empty() {
        return Ok(None);
    }

    // Topological tip = the descendant reachable from all of them.
    let tip = git::rev_list_tip(repo_root, &reachable).map_err(|e| Error::SynthWatermark {
        sha: reachable.join(", "),
        detail: format!("rev-list failed: {e}"),
    })?;

    let date_iso =
        git::commit_committer_date_iso(repo_root, &tip).map_err(|e| Error::SynthWatermark {
            sha: tip.clone(),
            detail: format!("log -1 --format=%cI failed: {e}"),
        })?;
    let date = parse_iso_date(&date_iso).ok_or_else(|| Error::SynthWatermark {
        sha: tip.clone(),
        detail: format!("unparseable committer date `{date_iso}`"),
    })?;

    Ok(Some((tip, date)))
}

/// Parse the date portion (`YYYY-MM-DD`) out of an ISO 8601 committer
/// timestamp like `2026-06-01T12:34:56+00:00`. Returns `None` on a
/// malformed prefix.
fn parse_iso_date(iso: &str) -> Option<NaiveDate> {
    // Take everything up to 'T' (or the whole string if no 'T') and parse.
    let date_part = iso.split('T').next()?;
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

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

    fn entry(path: &str, body: &str, date: &str) -> GatherEntry {
        GatherEntry {
            source_title: path.to_string(),
            source_path: PathBuf::from(path),
            line_start: 1,
            line_end: 1,
            section_text: body.to_string(),
            date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
            matched: vec![],
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
        let bodies: Vec<&str> = out.iter().map(|e| e.section_text.as_str()).collect();
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

    // ── last_synth_watermark ────────────────────────────────────────

    /// Build a temp git repo, return its root path.
    fn init_repo(tmp: &Path) {
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .current_dir(tmp)
                .env("GIT_TERMINAL_PROMPT", "0")
                .args(args)
                .output()
                .expect("git");
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.name", "T"]);
        run(&["config", "user.email", "t@e.com"]);
        run(&["config", "commit.gpgsign", "false"]);
    }

    fn commit_at(tmp: &Path, msg: &str, date: &str) -> String {
        let out = Command::new("git")
            .current_dir(tmp)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .args(["commit", "--allow-empty", "-m", msg])
            .output()
            .expect("git commit");
        assert!(out.status.success());
        // Resolve the new commit's full SHA.
        let out = Command::new("git")
            .current_dir(tmp)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git rev-parse");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn callout_with_sha(sha: &str, body: &str) -> ParsedCallout {
        ParsedCallout {
            source_path: PathBuf::from("notes/foo.md"),
            line_start: 1,
            line_end: 1,
            commit_sha: sha.to_string(),
            content_hash: compute_section_hash(body),
            body: body.to_string(),
            byte_range: 0..0,
            header_line: 1,
        }
    }

    #[test]
    fn watermark_empty_callouts_is_none() {
        let tmp = assert_fs::TempDir::new().unwrap();
        init_repo(tmp.path());
        commit_at(tmp.path(), "c1", "2026-01-01T00:00:00");
        let out = last_synth_watermark(tmp.path(), &[]).unwrap();
        assert!(out.is_none(), "empty callouts → None");
    }

    #[test]
    fn watermark_descendant_tip_among_two() {
        let tmp = assert_fs::TempDir::new().unwrap();
        init_repo(tmp.path());
        let sha_a = commit_at(tmp.path(), "c1", "2026-01-01T00:00:00");
        let sha_b = commit_at(tmp.path(), "c2", "2026-06-01T00:00:00");
        // Short SHAs to match callout storage.
        let short_a = &sha_a[..7];
        let short_b = &sha_b[..7];
        let callouts = vec![
            callout_with_sha(short_a, "body A"),
            callout_with_sha(short_b, "body B"),
        ];
        let out = last_synth_watermark(tmp.path(), &callouts)
            .unwrap()
            .unwrap();
        // Tip is the descendant (sha_b). Full SHA returned.
        assert_eq!(out.0, sha_b);
        assert_eq!(out.1, NaiveDate::from_ymd_opt(2026, 6, 1).unwrap());
    }

    #[test]
    fn watermark_unreachable_sha_skipped() {
        let tmp = assert_fs::TempDir::new().unwrap();
        init_repo(tmp.path());
        let sha_a = commit_at(tmp.path(), "c1", "2026-01-01T00:00:00");
        let sha_b = commit_at(tmp.path(), "c2", "2026-06-01T00:00:00");
        // Add a callout pinned to a clearly-bogus (unreachable) SHA.
        let callouts = vec![
            callout_with_sha("deadbee", "bogus body"),
            callout_with_sha(&sha_b[..7], "body B"),
        ];
        let out = last_synth_watermark(tmp.path(), &callouts)
            .unwrap()
            .unwrap();
        assert_eq!(out.0, sha_b, "unreachable SHA skipped, reachable tip used");
        let _ = sha_a;
    }

    #[test]
    fn watermark_all_unreachable_is_none() {
        let tmp = assert_fs::TempDir::new().unwrap();
        init_repo(tmp.path());
        commit_at(tmp.path(), "c1", "2026-01-01T00:00:00");
        let callouts = vec![
            callout_with_sha("deadbee", "bogus A"),
            callout_with_sha("feedfac", "bogus B"),
        ];
        let out = last_synth_watermark(tmp.path(), &callouts).unwrap();
        assert!(out.is_none(), "all unreachable → None");
    }

    #[test]
    fn parse_iso_date_extracts_yyyy_mm_dd() {
        assert_eq!(
            parse_iso_date("2026-06-01T12:34:56+00:00"),
            Some(NaiveDate::from_ymd_opt(2026, 6, 1).unwrap())
        );
        assert_eq!(
            parse_iso_date("2026-06-01"),
            Some(NaiveDate::from_ymd_opt(2026, 6, 1).unwrap())
        );
        assert!(parse_iso_date("not-a-date").is_none());
    }
}

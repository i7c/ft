//! Plan + apply for repairing broken `[!ft-source]` pins.
//!
//! A protected section's provenance can break without the quoted text
//! being wrong: a history rewrite (rebase, squash-merge, aggressive gc)
//! strands the pinned commit, or a hand edit to the header invalidates
//! the SHA or content hash. `ft synth verify` then reports
//! `source-missing`/`drifted` forever, with no recovery path short of
//! re-scaffolding. Repair closes that gap.
//!
//! Policy: **the callout body is the source of truth.** For each
//! section that fails verification, repair tries, in order:
//!
//! 1. **Rehash at the pin** — the body still matches the blob slice at
//!    the pinned commit and only the content hash is wrong (hand-mangled
//!    header). Fix: recompute the hash, keep SHA and range.
//! 2. **Re-pin to HEAD** — search the source file's blob at HEAD for
//!    the body text (exact line match first, then trailing-whitespace-
//!    insensitive). On a hit, pin to HEAD's short SHA with the matched
//!    line range; on multiple hits, the one nearest the old range wins.
//! 3. **Unrecoverable** — the body doesn't appear in the source at
//!    HEAD (or the file is gone). The section is left untouched and
//!    reported; `ft synth reslice` (restore canonical text from a
//!    still-valid pin) or re-scaffolding are the manual escape hatches.
//!
//! Sections that already verify are never touched. Same plan/apply
//! split as [`crate::synth::scaffold`]: `plan_synth_repair` only reads;
//! `apply_synth_repair` splices the re-serialized sections over their
//! byte ranges in descending order and writes once via
//! [`crate::fs::write_atomic`].

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::git;
use crate::synth::callout::{
    compute_section_hash, is_synth_note, parse as parse_callouts, serialize, ParsedCallout,
    ProtectedSection, CONTENT_HASH_PREFIX_LEN, SHORT_SHA_LEN,
};
use crate::vault::Vault;

/// What repair decided for one section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairAction {
    /// Section already verifies — untouched.
    AlreadyOk,
    /// Body still matches the blob slice at the pinned commit; only the
    /// content hash was wrong and has been recomputed.
    Rehashed,
    /// Body found in the source file at HEAD; section re-pinned to
    /// HEAD's short SHA with the matched line range. `matches` counts
    /// the candidate locations (1 = unambiguous; >1 = nearest-to-old
    /// chosen).
    Repinned { matches: usize },
    /// Not repairable; `reason` says why. Section left untouched.
    Unrecoverable { reason: String },
}

/// One section's planned repair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionRepair {
    /// 1-indexed header line in the synth note (as printed by
    /// `ft synth verify`).
    pub header_line: u32,
    /// Byte span of the old callout in the note, for the apply splice.
    pub byte_range: Range<usize>,
    /// The section as it currently stands on disk.
    pub old: ProtectedSection,
    /// The repaired section. `None` for [`RepairAction::AlreadyOk`] and
    /// [`RepairAction::Unrecoverable`].
    pub new: Option<ProtectedSection>,
    pub action: RepairAction,
}

/// Planned repairs for one synth note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthRepairPlan {
    /// Vault-relative path of the synth note.
    pub note: PathBuf,
    /// One entry per `[!ft-source]` callout, in document order.
    pub sections: Vec<SectionRepair>,
    /// Short HEAD SHA re-pinned sections point at.
    pub head_sha: String,
}

impl SynthRepairPlan {
    /// Sections that will actually be rewritten by `apply`.
    pub fn changed(&self) -> impl Iterator<Item = &SectionRepair> {
        self.sections.iter().filter(|s| s.new.is_some())
    }

    /// Sections repair could not fix.
    pub fn unrecoverable(&self) -> impl Iterator<Item = &SectionRepair> {
        self.sections
            .iter()
            .filter(|s| matches!(s.action, RepairAction::Unrecoverable { .. }))
    }
}

/// Build a [`SynthRepairPlan`] for the synth note at `note`
/// (vault-relative). Pure read: inspects the note, the pinned blobs,
/// and the HEAD blobs; writes nothing.
pub fn plan_synth_repair(vault: &Vault, note: &Path) -> Result<SynthRepairPlan> {
    let repo = git::RepoMap::discover(&vault.path)?;
    let head = git::head_hash(repo.root())?;
    let head_short = head[..SHORT_SHA_LEN.min(head.len())].to_string();

    let absolute = vault.path.join(note);
    let content = std::fs::read_to_string(&absolute).map_err(|e| Error::Io {
        path: absolute,
        source: e,
    })?;
    let callouts = parse_callouts(&content);

    let mut sections = Vec::with_capacity(callouts.len());
    for c in &callouts {
        sections.push(plan_one(&repo, &head, &head_short, c));
    }

    Ok(SynthRepairPlan {
        note: note.to_path_buf(),
        sections,
        head_sha: head_short,
    })
}

/// Plan repairs for every synth note in the vault (files carrying the
/// `ft-synth: true` frontmatter marker), in vault-walk order. Notes
/// whose sections all verify still appear (with `AlreadyOk` entries) so
/// callers can report a complete sweep.
pub fn plan_repair_all(vault: &Vault) -> Result<Vec<SynthRepairPlan>> {
    let mut out = Vec::new();
    for note_rel in super::verify::walk_markdown_files(&vault.path) {
        let absolute = vault.path.join(&note_rel);
        let Ok(content) = std::fs::read_to_string(&absolute) else {
            continue;
        };
        if !is_synth_note(&content) {
            continue;
        }
        out.push(plan_synth_repair(vault, &note_rel)?);
    }
    Ok(out)
}

/// Apply a [`SynthRepairPlan`]: splice every repaired section over its
/// old callout's byte range (descending byte order so earlier ranges
/// stay valid) and rewrite the note atomically. No-op (and no write)
/// when the plan changes nothing. Returns the number of sections
/// rewritten.
pub fn apply_synth_repair(vault: &Vault, plan: &SynthRepairPlan) -> Result<usize> {
    let mut changed: Vec<&SectionRepair> = plan.changed().collect();
    if changed.is_empty() {
        return Ok(0);
    }
    changed.sort_by_key(|s| std::cmp::Reverse(s.byte_range.start));

    let absolute = vault.path.join(&plan.note);
    let mut content = std::fs::read_to_string(&absolute).map_err(|e| Error::Io {
        path: absolute.clone(),
        source: e,
    })?;
    for s in &changed {
        let new = s.new.as_ref().expect("changed() yields Some(new)");
        content.replace_range(s.byte_range.clone(), &serialize(new));
    }
    crate::fs::write_atomic(&absolute, &content)?;
    Ok(changed.len())
}

/// Decide the repair for one callout.
fn plan_one(repo: &git::RepoMap, head: &str, head_short: &str, c: &ParsedCallout) -> SectionRepair {
    let old = ProtectedSection {
        source_path: c.source_path.clone(),
        line_start: c.line_start,
        line_end: c.line_end,
        commit_sha: c.commit_sha.clone(),
        content_hash: c.content_hash.clone(),
        body: c.body.clone(),
    };
    let base = |action: RepairAction, new: Option<ProtectedSection>| SectionRepair {
        header_line: c.header_line,
        byte_range: c.byte_range.clone(),
        old: old.clone(),
        new,
        action,
    };

    // Does the pin still hold? (Same checks as the verifier.)
    let pin_blob = git::show_file_at(repo.root(), &c.commit_sha, &repo.to_repo(&c.source_path));
    let body_matches_pin = pin_blob.as_ref().is_ok_and(|blob| {
        let lines: Vec<&str> = blob.split('\n').collect();
        let (start, end) = (c.line_start as usize, c.line_end as usize);
        start >= 1
            && end >= start
            && end <= lines.len()
            && lines[start - 1..end].join("\n") == c.body
    });
    let hash_ok = {
        let recomputed = compute_section_hash(&c.body);
        let prefix = &c.content_hash[..CONTENT_HASH_PREFIX_LEN.min(c.content_hash.len())];
        recomputed == prefix
    };

    if body_matches_pin && hash_ok {
        return base(RepairAction::AlreadyOk, None);
    }

    // Case 1: valid pin, wrong hash — recompute in place.
    if body_matches_pin {
        let mut new = old.clone();
        new.content_hash = compute_section_hash(&c.body);
        return base(RepairAction::Rehashed, Some(new));
    }

    // Case 2: re-pin to HEAD by finding the body in the current blob.
    let head_blob = match git::show_file_at(repo.root(), head, &repo.to_repo(&c.source_path)) {
        Ok(b) => b,
        Err(e) => {
            return base(
                RepairAction::Unrecoverable {
                    reason: format!(
                        "source not readable at HEAD ({e}); re-scaffold or fix the path by hand"
                    ),
                },
                None,
            );
        }
    };
    let lines: Vec<&str> = head_blob.split('\n').collect();
    match find_body(&lines, &c.body, c.line_start) {
        Some(found) => {
            let new = ProtectedSection {
                source_path: c.source_path.clone(),
                line_start: found.start,
                line_end: found.end,
                commit_sha: head_short.to_string(),
                content_hash: compute_section_hash(&found.body),
                body: found.body,
            };
            base(
                RepairAction::Repinned {
                    matches: found.candidates,
                },
                Some(new),
            )
        }
        None => base(
            RepairAction::Unrecoverable {
                reason: "body not found in source at HEAD; if the callout was hand-edited, \
                         `ft synth reslice` restores the canonical text from a valid pin, \
                         or re-scaffold the section"
                    .to_string(),
            },
            None,
        ),
    }
}

struct FoundBody {
    /// 1-indexed inclusive match range in the HEAD blob.
    start: u32,
    end: u32,
    /// The blob's verbatim lines for the match — identical to the
    /// needle for exact matches, whitespace-normalized source text for
    /// trailing-whitespace-insensitive matches.
    body: String,
    /// How many candidate locations matched (before nearest-wins).
    candidates: usize,
}

/// Find the callout body as a contiguous run of lines in `lines`.
/// Exact comparison first; if that yields nothing, retry comparing with
/// trailing whitespace stripped (adopting the blob's verbatim lines as
/// the new body so the section still verifies byte-for-byte). Among
/// multiple candidates the one whose start is nearest `prefer_near`
/// (the old `line_start`) wins.
fn find_body(lines: &[&str], body: &str, prefer_near: u32) -> Option<FoundBody> {
    let needle: Vec<&str> = body.split('\n').collect();
    if needle.is_empty() || lines.len() < needle.len() {
        return None;
    }

    let scan = |eq: &dyn Fn(&str, &str) -> bool| -> Vec<usize> {
        (0..=lines.len() - needle.len())
            .filter(|&i| needle.iter().enumerate().all(|(j, n)| eq(lines[i + j], n)))
            .collect()
    };

    let exact: Vec<usize> = scan(&|a, b| a == b);
    let (starts, exact_match) = if exact.is_empty() {
        (scan(&|a, b| a.trim_end() == b.trim_end()), false)
    } else {
        (exact, true)
    };
    if starts.is_empty() {
        return None;
    }

    let candidates = starts.len();
    let best = *starts
        .iter()
        .min_by_key(|&&i| (i as i64 + 1 - prefer_near as i64).unsigned_abs())
        .expect("starts is non-empty");
    let matched_body = if exact_match {
        body.to_string()
    } else {
        lines[best..best + needle.len()].join("\n")
    };
    Some(FoundBody {
        start: (best + 1) as u32,
        end: (best + needle.len()) as u32,
        body: matched_body,
        candidates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gather::GatherEntry;
    use crate::synth::scaffold::{apply_synth_scaffold, plan_synth_scaffold};
    use crate::synth::verify::{verify_synth_note, SectionStatus};
    use assert_fs::prelude::*;
    use chrono::NaiveDate;
    use std::process::Command;

    /// Repo with one committed source note and one scaffolded synth
    /// note whose single section verifies `ok`. Mirrors the verifier's
    /// test fixture.
    fn setup() -> (assert_fs::TempDir, Vault, PathBuf) {
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("notes/source.md")
            .write_str("Original paragraph line 1.\nOriginal paragraph line 2.\n\nSecond para.\n")
            .unwrap();
        run_git(tmp.path(), &["init", "-b", "main"]);
        run_git(tmp.path(), &["config", "user.name", "T"]);
        run_git(tmp.path(), &["config", "user.email", "t@e.com"]);
        run_git(tmp.path(), &["config", "commit.gpgsign", "false"]);
        run_git(tmp.path(), &["add", "."]);
        run_git(tmp.path(), &["commit", "-m", "c1"]);

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let entry = GatherEntry {
            source_title: "source".into(),
            source_path: PathBuf::from("notes/source.md"),
            line_start: 1,
            line_end: 2,
            section_text: "Original paragraph line 1.\nOriginal paragraph line 2.".into(),
            date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
            matched: vec![],
        };
        let target = PathBuf::from("Synthesis/topic.md");
        let plan =
            plan_synth_scaffold(&vault, &target, std::slice::from_ref(&entry)).expect("plan");
        apply_synth_scaffold(&vault, &plan).expect("apply");
        (tmp, vault, target)
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}");
    }

    fn mangle(vault: &Vault, note: &Path, pattern: &str, replacement: &str) {
        let abs = vault.path.join(note);
        let content = std::fs::read_to_string(&abs).unwrap();
        let mangled = regex::Regex::new(pattern)
            .unwrap()
            .replace(&content, replacement)
            .into_owned();
        assert_ne!(content, mangled, "mangle pattern must match: {pattern}");
        std::fs::write(&abs, mangled).unwrap();
    }

    fn assert_verifies_ok(vault: &Vault, note: &Path) {
        let results = verify_synth_note(vault, note).unwrap();
        assert!(
            results.iter().all(|r| r.status == SectionStatus::Ok),
            "expected all sections ok, got {results:#?}"
        );
    }

    #[test]
    fn already_ok_plan_changes_nothing() {
        let (_tmp, vault, note) = setup();
        let plan = plan_synth_repair(&vault, &note).unwrap();
        assert_eq!(plan.sections.len(), 1);
        assert_eq!(plan.sections[0].action, RepairAction::AlreadyOk);
        assert_eq!(apply_synth_repair(&vault, &plan).unwrap(), 0);
        assert_verifies_ok(&vault, &note);
    }

    #[test]
    fn stranded_sha_repins_to_head() {
        let (_tmp, vault, note) = setup();
        // Simulate a gc'd / rewritten pin: the SHA no longer resolves.
        mangle(&vault, &note, r"@[0-9a-f]{7}", "@deadbe1");
        assert_eq!(
            verify_synth_note(&vault, &note).unwrap()[0].status,
            SectionStatus::SourceMissing
        );

        let plan = plan_synth_repair(&vault, &note).unwrap();
        assert!(
            matches!(
                plan.sections[0].action,
                RepairAction::Repinned { matches: 1 }
            ),
            "expected Repinned, got {:?}",
            plan.sections[0].action
        );
        assert_eq!(apply_synth_repair(&vault, &plan).unwrap(), 1);
        assert_verifies_ok(&vault, &note);

        // The repaired pin is HEAD's short SHA.
        let content = std::fs::read_to_string(vault.path.join(&note)).unwrap();
        assert!(content.contains(&format!("@{}", plan.head_sha)));
    }

    #[test]
    fn repin_finds_shifted_paragraph() {
        let (tmp, vault, note) = setup();
        mangle(&vault, &note, r"@[0-9a-f]{7}", "@deadbe1");
        // The paragraph moved down two lines in a later commit.
        tmp.child("notes/source.md")
            .write_str(
                "New intro.\n\nOriginal paragraph line 1.\nOriginal paragraph line 2.\n\nSecond para.\n",
            )
            .unwrap();
        run_git(tmp.path(), &["add", "."]);
        run_git(tmp.path(), &["commit", "-m", "c2"]);

        let plan = plan_synth_repair(&vault, &note).unwrap();
        let s = &plan.sections[0];
        assert!(matches!(s.action, RepairAction::Repinned { .. }));
        let new = s.new.as_ref().unwrap();
        assert_eq!((new.line_start, new.line_end), (3, 4));
        apply_synth_repair(&vault, &plan).unwrap();
        assert_verifies_ok(&vault, &note);
    }

    #[test]
    fn hash_only_mismatch_rehashes_at_pin() {
        let (_tmp, vault, note) = setup();
        mangle(&vault, &note, r"#[0-9a-f]{6}", "#aaaaaa");
        assert_eq!(
            verify_synth_note(&vault, &note).unwrap()[0].status,
            SectionStatus::Drifted
        );

        let plan = plan_synth_repair(&vault, &note).unwrap();
        let s = &plan.sections[0];
        assert_eq!(s.action, RepairAction::Rehashed);
        // Pin (SHA + range) unchanged; only the hash moves.
        let new = s.new.as_ref().unwrap();
        assert_eq!(new.commit_sha, s.old.commit_sha);
        assert_eq!((new.line_start, new.line_end), (1, 2));
        apply_synth_repair(&vault, &plan).unwrap();
        assert_verifies_ok(&vault, &note);
    }

    #[test]
    fn hand_edited_body_matching_head_repins() {
        let (tmp, vault, note) = setup();
        // The source gets reworded and committed…
        tmp.child("notes/source.md")
            .write_str("Reworded line 1.\nOriginal paragraph line 2.\n\nSecond para.\n")
            .unwrap();
        run_git(tmp.path(), &["add", "."]);
        run_git(tmp.path(), &["commit", "-m", "reword"]);
        // …and the user hand-updated the callout body to match, which
        // drifts it (body no longer matches the old pin).
        mangle(
            &vault,
            &note,
            r"> Original paragraph line 1\.",
            "> Reworded line 1.",
        );
        assert_eq!(
            verify_synth_note(&vault, &note).unwrap()[0].status,
            SectionStatus::Drifted
        );

        let plan = plan_synth_repair(&vault, &note).unwrap();
        assert!(matches!(
            plan.sections[0].action,
            RepairAction::Repinned { .. }
        ));
        apply_synth_repair(&vault, &plan).unwrap();
        assert_verifies_ok(&vault, &note);
    }

    #[test]
    fn garbage_body_is_unrecoverable_and_untouched() {
        let (_tmp, vault, note) = setup();
        mangle(
            &vault,
            &note,
            r"> Original paragraph line 1\.",
            "> Nothing like the source.",
        );
        let before = std::fs::read_to_string(vault.path.join(&note)).unwrap();

        let plan = plan_synth_repair(&vault, &note).unwrap();
        assert!(
            matches!(plan.sections[0].action, RepairAction::Unrecoverable { .. }),
            "got {:?}",
            plan.sections[0].action
        );
        assert_eq!(apply_synth_repair(&vault, &plan).unwrap(), 0);
        let after = std::fs::read_to_string(vault.path.join(&note)).unwrap();
        assert_eq!(before, after, "unrecoverable sections must not be touched");
    }

    #[test]
    fn nearest_candidate_wins_on_duplicate_paragraphs() {
        let (tmp, vault, note) = setup();
        mangle(&vault, &note, r"@[0-9a-f]{7}", "@deadbe1");
        // Duplicate the paragraph far from its original position; the
        // original location (lines 1-2) must win over the copy.
        tmp.child("notes/source.md")
            .write_str(
                "Original paragraph line 1.\nOriginal paragraph line 2.\n\nSecond para.\n\n\
                 Original paragraph line 1.\nOriginal paragraph line 2.\n",
            )
            .unwrap();
        run_git(tmp.path(), &["add", "."]);
        run_git(tmp.path(), &["commit", "-m", "dup"]);

        let plan = plan_synth_repair(&vault, &note).unwrap();
        let s = &plan.sections[0];
        assert!(matches!(s.action, RepairAction::Repinned { matches: 2 }));
        let new = s.new.as_ref().unwrap();
        assert_eq!((new.line_start, new.line_end), (1, 2));
        apply_synth_repair(&vault, &plan).unwrap();
        assert_verifies_ok(&vault, &note);
    }

    #[test]
    fn plan_repair_all_walks_synth_notes() {
        let (tmp, vault, note) = setup();
        tmp.child("not-synth.md").write_str("# regular\n").unwrap();
        let plans = plan_repair_all(&vault).unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].note, note);
    }
}

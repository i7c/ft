//! Plan + apply for re-slicing a protected section's line range.
//!
//! A protected section pins a verbatim source paragraph to a specific
//! git commit (see [`crate::synth`]). Reslicing grows or shrinks the
//! captured line range **at the same pinned commit** — important
//! because that commit is usually no longer HEAD by the time a synth
//! note is edited. The body and content-hash are recomputed from the
//! source blob at the pinned commit, so the resliced section verifies
//! `ok` by construction.
//!
//! Because the new body always comes from the committed blob, reslicing
//! also *heals* a section whose on-disk body was hand-edited (drifted):
//! the canonical slice overwrites the edit. [`ReslicePlan::healed_drift`]
//! records when that happened so callers can surface it.
//!
//! Same plan/apply split as [`crate::synth::scaffold`]: `plan_reslice`
//! is a pure read-only planner; `apply_reslice` writes via
//! [`crate::fs::write_atomic`].

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::git;
use crate::synth::callout::{
    compute_section_hash, parse as parse_callouts, serialize, ParsedCallout, ProtectedSection,
};
use crate::vault::Vault;

/// How the new line range is expressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewRange {
    /// Replace the range outright (1-indexed inclusive).
    Absolute { start: u32, end: u32 },
    /// Adjust each edge relative to the current range. `up` lines are
    /// added above the start (negative shrinks from the top); `down`
    /// lines are added below the end (negative shrinks from the bottom).
    Delta { up: i32, down: i32 },
}

/// A planned reslice of one protected section. `apply_reslice` splices
/// the serialized [`new`](Self::new) section over `byte_range` in the
/// target note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReslicePlan {
    /// Vault-relative path of the synth note.
    pub target: PathBuf,
    /// Byte span of the old callout in the note (no trailing newline),
    /// taken from the [`ParsedCallout`].
    pub byte_range: Range<usize>,
    /// The section as it currently stands on disk.
    pub old: ProtectedSection,
    /// The resliced section: same `source_path` and `commit_sha`, new
    /// range / body / content-hash.
    pub new: ProtectedSection,
    /// `true` when the old on-disk body differed from the blob slice —
    /// i.e. the section had drifted and this reslice overwrote the edit.
    pub healed_drift: bool,
}

/// Build a [`ReslicePlan`] for the section in `note` (vault-relative).
///
/// `header_line` selects which `[!ft-source]` callout to reslice (its
/// 1-indexed header line, as printed by `ft synth verify`). When `None`
/// and the note has exactly one callout, that one is chosen; otherwise
/// an [`Error::ResliceAmbiguous`] / [`Error::ResliceSectionNotFound`] is
/// returned.
pub fn plan_reslice(
    vault: &Vault,
    note: &Path,
    header_line: Option<u32>,
    range: NewRange,
) -> Result<ReslicePlan> {
    let absolute = vault.path.join(note);
    let content = std::fs::read_to_string(&absolute).map_err(|e| Error::Io {
        path: absolute,
        source: e,
    })?;
    let callouts = parse_callouts(&content);
    let target = select_callout(&callouts, header_line, note)?;

    // Fetch the source blob at the section's own pinned commit — the
    // same call the verifier uses, so any commit that verifies also
    // reslices (HEAD or not).
    let repo = git::RepoMap::discover(&vault.path)?;
    let blob = git::show_file_at(
        repo.root(),
        &target.commit_sha,
        &repo.to_repo(&target.source_path),
    )
    .map_err(|e| Error::ResliceSourceMissing(e.to_string()))?;
    let lines: Vec<&str> = blob.split('\n').collect();
    let file_lines = lines.len() as u32;

    let (start, end) = resolve_range(range, target.line_start, target.line_end, file_lines)?;

    let body = lines[(start as usize - 1)..(end as usize)].join("\n");
    let content_hash = compute_section_hash(&body);

    // Drift = the old on-disk body no longer matched the blob at the
    // *old* range. Reslicing re-pins from the blob, healing it.
    let old_start = target.line_start as usize;
    let old_end = target.line_end as usize;
    let healed_drift = if old_start >= 1 && old_end >= old_start && old_end <= lines.len() {
        lines[(old_start - 1)..old_end].join("\n") != target.body
    } else {
        // Old range itself is out of bounds at the pin → already broken.
        true
    };

    let old = ProtectedSection {
        source_path: target.source_path.clone(),
        line_start: target.line_start,
        line_end: target.line_end,
        commit_sha: target.commit_sha.clone(),
        content_hash: target.content_hash.clone(),
        body: target.body.clone(),
    };
    let new = ProtectedSection {
        source_path: target.source_path.clone(),
        line_start: start,
        line_end: end,
        commit_sha: target.commit_sha.clone(),
        content_hash,
        body,
    };

    Ok(ReslicePlan {
        target: note.to_path_buf(),
        byte_range: target.byte_range.clone(),
        old,
        new,
        healed_drift,
    })
}

/// Apply a [`ReslicePlan`]: splice the serialized new section over the
/// old callout's byte range and rewrite the note atomically. Returns the
/// absolute path written.
pub fn apply_reslice(vault: &Vault, plan: &ReslicePlan) -> Result<PathBuf> {
    let absolute = vault.path.join(&plan.target);
    let content = std::fs::read_to_string(&absolute).map_err(|e| Error::Io {
        path: absolute.clone(),
        source: e,
    })?;
    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..plan.byte_range.start]);
    out.push_str(&serialize(&plan.new));
    out.push_str(&content[plan.byte_range.end..]);

    crate::fs::write_atomic(&absolute, &out)?;
    Ok(absolute)
}

/// Pick the target callout by header line, defaulting to the sole
/// callout when `header_line` is `None`.
fn select_callout<'a>(
    callouts: &'a [ParsedCallout],
    header_line: Option<u32>,
    note: &Path,
) -> Result<&'a ParsedCallout> {
    match header_line {
        Some(hl) => callouts
            .iter()
            .find(|c| c.header_line == hl)
            .ok_or_else(|| Error::ResliceSectionNotFound {
                note: note.to_path_buf(),
                header_line: hl,
            }),
        None => match callouts {
            [one] => Ok(one),
            [] => Err(Error::ResliceSectionNotFound {
                note: note.to_path_buf(),
                header_line: 0,
            }),
            many => Err(Error::ResliceAmbiguous {
                header_lines: many.iter().map(|c| c.header_line).collect(),
            }),
        },
    }
}

/// Resolve a [`NewRange`] against the current range and validate it
/// against the source's line count at the pinned commit.
fn resolve_range(
    range: NewRange,
    cur_start: u32,
    cur_end: u32,
    file_lines: u32,
) -> Result<(u32, u32)> {
    let (start, end) = match range {
        NewRange::Absolute { start, end } => (start as i64, end as i64),
        NewRange::Delta { up, down } => {
            (cur_start as i64 - up as i64, cur_end as i64 + down as i64)
        }
    };
    let in_bounds = start >= 1 && end >= start && end <= file_lines as i64;
    if !in_bounds {
        return Err(Error::ResliceOutOfBounds {
            start: start.max(0) as u32,
            end: end.max(0) as u32,
            file_lines,
        });
    }
    Ok((start as u32, end as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::JournalEntry;
    use crate::synth::scaffold::{apply_synth_scaffold, plan_synth_scaffold};
    use crate::synth::verify::{verify_synth_note, SectionStatus};
    use assert_fs::prelude::*;
    use chrono::NaiveDate;
    use std::process::Command;

    fn run_git(repo: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}");
    }

    fn git_init(repo: &Path) {
        run_git(repo, &["init", "-b", "main"]);
        run_git(repo, &["config", "user.name", "T"]);
        run_git(repo, &["config", "user.email", "t@e.com"]);
        run_git(repo, &["config", "commit.gpgsign", "false"]);
    }

    /// Source file with five single-line paragraphs; scaffold a section
    /// over lines 2-3, commit, then add a second unrelated commit so the
    /// pinned commit is no longer HEAD.
    fn setup() -> (assert_fs::TempDir, Vault, PathBuf) {
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("notes/source.md")
            .write_str("line one\nline two\nline three\nline four\nline five\n")
            .unwrap();
        let repo = tmp.path().to_path_buf();
        git_init(&repo);
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "c1"]);

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let entry = JournalEntry {
            source_title: "source".into(),
            source_path: PathBuf::from("notes/source.md"),
            line_start: 2,
            line_end: 3,
            section_text: "line two\nline three".into(),
            date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
            matched: vec![],
        };
        let target = PathBuf::from("Synth/topic.md");
        let plan =
            plan_synth_scaffold(&vault, &target, std::slice::from_ref(&entry)).expect("plan");
        apply_synth_scaffold(&vault, &plan).expect("apply");

        // A second commit unrelated to the source so HEAD != pinned.
        tmp.child("other.md").write_str("unrelated\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "c2"]);

        (tmp, vault, target)
    }

    fn header(vault: &Vault, note: &Path) -> u32 {
        let content = std::fs::read_to_string(vault.path.join(note)).unwrap();
        parse_callouts(&content)[0].header_line
    }

    #[test]
    fn extend_grows_range_and_verifies() {
        let (_tmp, vault, target) = setup();
        let hl = header(&vault, &target);
        let plan = plan_reslice(
            &vault,
            &target,
            Some(hl),
            NewRange::Delta { up: 1, down: 1 },
        )
        .expect("plan");
        assert_eq!((plan.new.line_start, plan.new.line_end), (1, 4));
        assert_eq!(plan.new.body, "line one\nline two\nline three\nline four");
        assert!(!plan.healed_drift);
        apply_reslice(&vault, &plan).unwrap();
        let results = verify_synth_note(&vault, &target).unwrap();
        assert_eq!(results[0].status, SectionStatus::Ok);
    }

    #[test]
    fn reduce_shrinks_range() {
        let (_tmp, vault, target) = setup();
        let hl = header(&vault, &target);
        // Drop the bottom line: 2-3 → 2-2.
        let plan = plan_reslice(
            &vault,
            &target,
            Some(hl),
            NewRange::Delta { up: 0, down: -1 },
        )
        .expect("plan");
        assert_eq!((plan.new.line_start, plan.new.line_end), (2, 2));
        assert_eq!(plan.new.body, "line two");
    }

    #[test]
    fn absolute_replaces_range() {
        let (_tmp, vault, target) = setup();
        let hl = header(&vault, &target);
        let plan = plan_reslice(
            &vault,
            &target,
            Some(hl),
            NewRange::Absolute { start: 4, end: 5 },
        )
        .expect("plan");
        assert_eq!((plan.new.line_start, plan.new.line_end), (4, 5));
        assert_eq!(plan.new.body, "line four\nline five");
    }

    #[test]
    fn out_of_bounds_is_rejected() {
        let (_tmp, vault, target) = setup();
        let hl = header(&vault, &target);
        let err = plan_reslice(
            &vault,
            &target,
            Some(hl),
            NewRange::Delta { up: 0, down: 100 },
        )
        .unwrap_err();
        assert!(
            matches!(err, Error::ResliceOutOfBounds { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn reslice_resolves_blob_at_pinned_commit_not_head() {
        // The pinned commit is the first commit; HEAD has moved on. A
        // successful reslice proves we fetch at the pin, not HEAD.
        let (_tmp, vault, target) = setup();
        let head = git::head_hash(&vault.path).unwrap();
        let content = std::fs::read_to_string(vault.path.join(&target)).unwrap();
        let pinned = &parse_callouts(&content)[0].commit_sha;
        assert!(
            !head.starts_with(pinned.as_str()),
            "pin should predate HEAD"
        );
        let hl = header(&vault, &target);
        plan_reslice(
            &vault,
            &target,
            Some(hl),
            NewRange::Delta { up: 1, down: 0 },
        )
        .expect("reslice resolves at the pinned commit");
    }

    #[test]
    fn reslice_heals_drifted_body() {
        let (_tmp, vault, target) = setup();
        // Hand-edit inside the callout to introduce drift.
        let abs = vault.path.join(&target);
        let content = std::fs::read_to_string(&abs).unwrap();
        std::fs::write(&abs, content.replace("> line two", "> EDITED two")).unwrap();
        assert_eq!(
            verify_synth_note(&vault, &target).unwrap()[0].status,
            SectionStatus::Drifted
        );

        // Reslice with no net change still re-pins from the blob.
        let hl = header(&vault, &target);
        let plan = plan_reslice(
            &vault,
            &target,
            Some(hl),
            NewRange::Delta { up: 0, down: 0 },
        )
        .expect("plan");
        assert!(plan.healed_drift);
        apply_reslice(&vault, &plan).unwrap();
        assert_eq!(
            verify_synth_note(&vault, &target).unwrap()[0].status,
            SectionStatus::Ok
        );
    }

    #[test]
    fn missing_section_errors() {
        let (_tmp, vault, target) = setup();
        let err = plan_reslice(
            &vault,
            &target,
            Some(999),
            NewRange::Delta { up: 1, down: 0 },
        )
        .unwrap_err();
        assert!(
            matches!(err, Error::ResliceSectionNotFound { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn ambiguous_without_at_errors() {
        let (tmp, vault, target) = setup();
        // Append a second section to the same note.
        let entry = JournalEntry {
            source_title: "source".into(),
            source_path: PathBuf::from("notes/source.md"),
            line_start: 5,
            line_end: 5,
            section_text: "line five".into(),
            date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
            matched: vec![],
        };
        // Source must be clean to scaffold; it is (committed in setup).
        let plan =
            plan_synth_scaffold(&vault, &target, std::slice::from_ref(&entry)).expect("plan2");
        apply_synth_scaffold(&vault, &plan).expect("apply2");
        let _ = tmp;

        let err =
            plan_reslice(&vault, &target, None, NewRange::Delta { up: 1, down: 0 }).unwrap_err();
        match err {
            Error::ResliceAmbiguous { header_lines } => assert_eq!(header_lines.len(), 2),
            other => panic!("expected ResliceAmbiguous, got {other:?}"),
        }
    }
}

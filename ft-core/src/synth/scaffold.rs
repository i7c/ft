//! Plan + apply for synth-note scaffolding.
//!
//! `plan_synth_scaffold` is a pure function: given a list of
//! [`crate::journal::JournalEntry`] values and a target path, it returns
//! a [`SynthScaffoldPlan`] describing the file mutation without
//! performing any I/O writes. `apply_synth_scaffold` consumes a plan
//! and writes the file atomically via [`crate::fs::write_atomic`].
//!
//! This split mirrors the plan/apply pattern used elsewhere in
//! `ft-core` (`task::ops::plan_move`, `graph::rename::plan_rename`,
//! etc.) so callers can preview/test mutations independently.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::git;
use crate::journal::JournalEntry;
use crate::synth::callout::{compute_section_hash, serialize, ProtectedSection, SHORT_SHA_LEN};
use crate::vault::Vault;

/// Frontmatter block prepended to a freshly-created synth note.
pub const SYNTH_FRONTMATTER: &str = "---\nft-synth: true\n---\n\n";

/// A planned mutation of a synth note. `create == true` means the target
/// does not exist and the applier will create it with `frontmatter`
/// followed by the serialized sections. `create == false` means append:
/// the applier reads the existing file, joins it to the serialized
/// sections with a separator newline, and rewrites atomically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthScaffoldPlan {
    /// Vault-relative path of the target synth note.
    pub target: PathBuf,
    /// `true` when the target file does not exist yet.
    pub create: bool,
    /// Frontmatter content to write at the top of a newly-created file.
    /// `None` when `create == false`.
    pub frontmatter: Option<String>,
    /// Sections to write, in scaffold order (journal date desc).
    pub sections: Vec<ProtectedSection>,
}

impl SynthScaffoldPlan {
    /// Render the full file contents for a `create` plan: frontmatter +
    /// sections separated by blank lines. Returns `None` for append
    /// plans (use [`render_append_block`] instead).
    pub fn render_create(&self) -> Option<String> {
        if !self.create {
            return None;
        }
        let mut out = self.frontmatter.clone().unwrap_or_default();
        out.push_str(&render_sections(&self.sections));
        Some(out)
    }

    /// Render only the new sections (no frontmatter, no leading blank
    /// line). Used by both create (joined after frontmatter) and append
    /// (joined after existing content).
    pub fn render_append_block(&self) -> String {
        render_sections(&self.sections)
    }
}

fn render_sections(sections: &[ProtectedSection]) -> String {
    let parts: Vec<String> = sections.iter().map(serialize).collect();
    parts.join("\n\n")
}

/// Build a [`SynthScaffoldPlan`] from a list of journal entries.
///
/// For each entry:
/// 1. The source paragraph text is taken verbatim from `entry.section_text`.
/// 2. The pinned commit SHA is `repo`'s HEAD. The entry's `source_path`,
///    line range, and body all come from the working-tree scan (= HEAD),
///    so HEAD is the only commit where `git show <sha>:<source_path>`
///    sliced at `line_start..line_end` is guaranteed to reproduce the
///    captured body. The blame commit (most recent change to those lines)
///    can predate a rename or a line-offset shift, which would make
///    `git show` resolve the wrong path or wrong lines.
/// 3. The content hash is computed via blake3 over the entry text.
///
/// Sections are emitted in the order of `entries` — the caller is
/// responsible for sorting them (journal already sorts by date desc).
pub fn plan_synth_scaffold(
    vault: &Vault,
    repo: &Path,
    target: &Path,
    entries: &[JournalEntry],
) -> Result<SynthScaffoldPlan> {
    // A section is pinned to HEAD, so each source file's working-tree
    // content must already match HEAD — otherwise verify would report
    // drift on a freshly created note. Refuse if any source is dirty.
    let status = git::status(repo)?;
    let dirty: HashSet<&Path> = status
        .modified
        .iter()
        .chain(&status.deleted)
        .chain(&status.conflicted)
        .chain(&status.untracked)
        .map(PathBuf::as_path)
        .collect();
    let mut offending: Vec<PathBuf> = entries
        .iter()
        .map(|e| &e.source_path)
        .filter(|p| dirty.contains(p.as_path()))
        .cloned()
        .collect();
    offending.sort();
    offending.dedup();
    if !offending.is_empty() {
        return Err(Error::SynthDirtySources(offending));
    }

    let head_sha = git::head_hash(repo)?;
    let short_sha = head_sha[..SHORT_SHA_LEN.min(head_sha.len())].to_string();

    let mut sections = Vec::with_capacity(entries.len());
    for entry in entries {
        let hash = compute_section_hash(&entry.section_text);

        sections.push(ProtectedSection {
            source_path: entry.source_path.clone(),
            line_start: entry.line_start,
            line_end: entry.line_end,
            commit_sha: short_sha.clone(),
            content_hash: hash,
            body: entry.section_text.clone(),
        });
    }

    let absolute = vault.path.join(target);
    let exists = absolute.exists();
    Ok(SynthScaffoldPlan {
        target: target.to_path_buf(),
        create: !exists,
        frontmatter: if exists {
            None
        } else {
            Some(SYNTH_FRONTMATTER.to_string())
        },
        sections,
    })
}

/// Apply a [`SynthScaffoldPlan`] to disk. Returns the absolute path
/// callers should open in `$EDITOR`. Uses [`crate::fs::write_atomic`]
/// for both create and append (append = read existing → join → atomic
/// rewrite).
pub fn apply_synth_scaffold(vault: &Vault, plan: &SynthScaffoldPlan) -> Result<PathBuf> {
    use crate::error::Error;

    let absolute = vault.path.join(&plan.target);
    let final_content = if plan.create {
        plan.render_create()
            .expect("create plan should render successfully")
    } else {
        let existing = std::fs::read_to_string(&absolute).map_err(|e| Error::Io {
            path: absolute.clone(),
            source: e,
        })?;
        let mut out = existing;
        if !out.ends_with('\n') {
            out.push('\n');
        }
        // One blank line separator between existing content and new block.
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&plan.render_append_block());
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out
    };

    crate::fs::write_atomic(&absolute, &final_content)?;
    Ok(absolute)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use chrono::NaiveDate;
    use std::process::Command;

    /// Build a repo with one note + one commit, return a journal entry
    /// referencing the first paragraph.
    fn make_repo_with_entry() -> (assert_fs::TempDir, Vault, std::path::PathBuf, JournalEntry) {
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("notes/source.md")
            .write_str("First paragraph here.\nLine two of first.\n\nSecond paragraph.\n")
            .unwrap();
        let repo = tmp.path().to_path_buf();
        let run_git = |args: &[&str]| {
            let out = Command::new("git")
                .current_dir(&repo)
                .env("GIT_TERMINAL_PROMPT", "0")
                .args(args)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}");
        };
        run_git(&["init", "-b", "main"]);
        run_git(&["config", "user.name", "T"]);
        run_git(&["config", "user.email", "t@e.com"]);
        run_git(&["config", "commit.gpgsign", "false"]);
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "c1"]);

        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let entry = JournalEntry {
            source_title: "source".into(),
            source_path: PathBuf::from("notes/source.md"),
            line_start: 1,
            line_end: 2,
            section_text: "First paragraph here.\nLine two of first.".into(),
            date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
            matched: vec![],
        };
        (tmp, vault, repo, entry)
    }

    #[test]
    fn plan_does_no_io_writes() {
        let (tmp, vault, repo, entry) = make_repo_with_entry();
        let listing_before = collect_files(tmp.path());
        let _plan = plan_synth_scaffold(
            &vault,
            &repo,
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .unwrap();
        let listing_after = collect_files(tmp.path());
        assert_eq!(listing_before, listing_after, "planner must not touch fs");
    }

    fn collect_files(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        fn rec(dir: &Path, out: &mut Vec<PathBuf>) {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for entry in rd.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        rec(&p, out);
                    } else {
                        out.push(p);
                    }
                }
            }
        }
        rec(root, &mut out);
        out.sort();
        out
    }

    #[test]
    fn create_plan_writes_frontmatter_and_sections() {
        let (tmp, vault, repo, entry) = make_repo_with_entry();
        let plan = plan_synth_scaffold(
            &vault,
            &repo,
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .unwrap();
        assert!(plan.create);
        assert!(plan.frontmatter.is_some());

        let abs = apply_synth_scaffold(&vault, &plan).unwrap();
        assert!(abs.exists());
        let content = std::fs::read_to_string(&abs).unwrap();
        assert!(content.starts_with("---\nft-synth: true\n---\n"));
        assert!(content.contains("> [!ft-source] \"notes/source.md\" L1-2 @"));
        assert!(content.contains("> First paragraph here.\n> Line two of first."));
        let _ = tmp;
    }

    #[test]
    fn append_plan_preserves_existing_content() {
        let (tmp, vault, repo, entry) = make_repo_with_entry();
        // Pre-create a synth note.
        tmp.child("Synthesis/topic.md")
            .write_str("---\nft-synth: true\n---\n\nUser prose already here.\n")
            .unwrap();

        let plan = plan_synth_scaffold(
            &vault,
            &repo,
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .unwrap();
        assert!(!plan.create);

        let abs = apply_synth_scaffold(&vault, &plan).unwrap();
        let content = std::fs::read_to_string(&abs).unwrap();
        // Existing prose preserved at the top.
        assert!(content.contains("User prose already here."));
        // New section appended.
        assert!(content.contains("> [!ft-source] \"notes/source.md\" L1-2 @"));
        assert!(content.contains("> First paragraph here.\n> Line two of first."));
    }

    #[test]
    fn scaffold_hash_matches_entry_text() {
        let (_tmp, vault, repo, entry) = make_repo_with_entry();
        let plan = plan_synth_scaffold(
            &vault,
            &repo,
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .unwrap();
        let expected_hash = compute_section_hash(&entry.section_text);
        assert_eq!(plan.sections[0].content_hash, expected_hash);
    }

    #[test]
    fn scaffold_pins_to_head() {
        let (_tmp, vault, repo, entry) = make_repo_with_entry();
        let plan = plan_synth_scaffold(
            &vault,
            &repo,
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .unwrap();
        // Pinned to HEAD's short SHA so verify can resolve the current
        // path/line range; blame commits could predate a rename.
        let head = git::head_hash(&repo).unwrap();
        let sha = &plan.sections[0].commit_sha;
        assert_eq!(sha, &head[..SHORT_SHA_LEN]);
    }

    #[test]
    fn scaffold_rejects_dirty_source() {
        let (tmp, vault, repo, entry) = make_repo_with_entry();
        // Uncommitted edit to the source the entry pins → working tree
        // no longer matches HEAD, so the HEAD-pinned section can't be
        // verified. Scaffolding must refuse.
        tmp.child("notes/source.md")
            .write_str("Edited first paragraph.\nLine two of first.\n\nSecond paragraph.\n")
            .unwrap();
        let err = plan_synth_scaffold(
            &vault,
            &repo,
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .unwrap_err();
        match err {
            Error::SynthDirtySources(paths) => {
                assert_eq!(paths, vec![PathBuf::from("notes/source.md")]);
            }
            other => panic!("expected SynthDirtySources, got {other:?}"),
        }
    }

    #[test]
    fn scaffold_rejects_untracked_source() {
        let (tmp, vault, repo, _entry) = make_repo_with_entry();
        tmp.child("notes/new.md")
            .write_str("Fresh untracked paragraph.\n")
            .unwrap();
        let entry = JournalEntry {
            source_title: "new".into(),
            source_path: PathBuf::from("notes/new.md"),
            line_start: 1,
            line_end: 1,
            section_text: "Fresh untracked paragraph.".into(),
            date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
            matched: vec![],
        };
        let err = plan_synth_scaffold(
            &vault,
            &repo,
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .unwrap_err();
        assert!(matches!(err, Error::SynthDirtySources(_)));
    }

    #[test]
    fn scaffold_succeeds_when_sources_clean() {
        let (_tmp, vault, repo, entry) = make_repo_with_entry();
        plan_synth_scaffold(
            &vault,
            &repo,
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .expect("clean source scaffolds");
    }
}

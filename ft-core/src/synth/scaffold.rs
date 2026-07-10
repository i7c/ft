//! Plan + apply for synth-note scaffolding.
//!
//! `plan_synth_scaffold` is a pure function: given a list of
//! [`crate::gather::GatherEntry`] values and a target path, it returns
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
use crate::gather::GatherEntry;
use crate::git;
use crate::synth::callout::{compute_section_hash, serialize, ProtectedSection, SHORT_SHA_LEN};
use crate::vault::Vault;

/// Frontmatter block prepended to a freshly-created synth note.
pub const SYNTH_FRONTMATTER: &str = "---\nft:\n  synth:\n    enabled: true\n---\n\n";

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
    /// Number of input entries that were dropped because they were
    /// already pinned in the target note (append path only; `0` for
    /// create). Surfaced by callers as "N already pinned, skipped".
    pub dedup_skipped: usize,
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
/// 2. The pinned commit SHA is the vault's enclosing-repo HEAD. The entry's `source_path`,
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
///
/// **Append dedup invariant.** When the target exists (append path),
/// entries whose `(source_path, body)` is already pinned in the note
/// are dropped via [`crate::synth::accrete::filter_missing`] before
/// section construction, so re-running scaffold/grow with the same
/// target is idempotent. The count of dropped entries is reported in
/// [`SynthScaffoldPlan::dedup_skipped`]. The create path never dedups
/// (`dedup_skipped == 0`).
pub fn plan_synth_scaffold(
    vault: &Vault,
    target: &Path,
    entries: &[GatherEntry],
) -> Result<SynthScaffoldPlan> {
    let repo = git::RepoMap::discover(&vault.path)?;

    // A section is pinned to HEAD, so each source file's working-tree
    // content must already match HEAD — otherwise verify would report
    // drift on a freshly created note. Refuse if any source is dirty.
    // `git status` reports repo-root-relative paths; translate the
    // entries' vault-relative source paths into the same coordinate
    // system before intersecting.
    let status = git::status(repo.root())?;
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
        .filter(|p| dirty.contains(repo.to_repo(p).as_path()))
        .cloned()
        .collect();
    offending.sort();
    offending.dedup();
    if !offending.is_empty() {
        return Err(Error::SynthDirtySources(offending));
    }

    let head_sha = git::head_hash(repo.root())?;
    let short_sha = head_sha[..SHORT_SHA_LEN.min(head_sha.len())].to_string();

    let absolute = vault.path.join(target);
    let exists = absolute.exists();

    // Append dedup: when the target exists, drop entries already
    // pinned in the note. Read + parse the existing callouts, then run
    // the pure filter. The create path skips this (no existing note to
    // dedup against).
    let (entries_owned, dedup_skipped): (Vec<GatherEntry>, usize) = if exists {
        let existing_content = std::fs::read_to_string(&absolute).map_err(|e| Error::Io {
            path: absolute.clone(),
            source: e,
        })?;
        let existing_callouts = crate::synth::callout::parse(&existing_content);
        let before = entries.len();
        let filtered = crate::synth::accrete::filter_missing(&existing_callouts, entries.to_vec());
        let after = filtered.len();
        (filtered, before.saturating_sub(after))
    } else {
        (entries.to_vec(), 0)
    };

    let mut sections = Vec::with_capacity(entries_owned.len());
    for entry in entries_owned {
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

    Ok(SynthScaffoldPlan {
        target: target.to_path_buf(),
        create: !exists,
        frontmatter: if exists {
            None
        } else {
            Some(SYNTH_FRONTMATTER.to_string())
        },
        sections,
        dedup_skipped,
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
        // No new sections to append → leave the file byte-identical.
        // (Re-planning an unchanged note is idempotent at the file level.)
        if plan.sections.is_empty() {
            crate::fs::write_atomic(&absolute, &existing)?;
            return Ok(absolute);
        }
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
    fn make_repo_with_entry() -> (assert_fs::TempDir, Vault, GatherEntry) {
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
        let entry = GatherEntry {
            source_title: "source".into(),
            source_path: PathBuf::from("notes/source.md"),
            line_start: 1,
            line_end: 2,
            section_text: "First paragraph here.\nLine two of first.".into(),
            date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
            matched: vec![],
        };
        (tmp, vault, entry)
    }

    #[test]
    fn plan_does_no_io_writes() {
        let (tmp, vault, entry) = make_repo_with_entry();
        let listing_before = collect_files(tmp.path());
        let _plan = plan_synth_scaffold(
            &vault,
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
        let (tmp, vault, entry) = make_repo_with_entry();
        let plan = plan_synth_scaffold(
            &vault,
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .unwrap();
        assert!(plan.create);
        assert!(plan.frontmatter.is_some());

        let abs = apply_synth_scaffold(&vault, &plan).unwrap();
        assert!(abs.exists());
        let content = std::fs::read_to_string(&abs).unwrap();
        assert!(content.starts_with("---\nft:\n  synth:\n    enabled: true\n---\n"));
        assert!(content.contains("> [!ft-source] \"notes/source.md\" L1-2 @"));
        assert!(content.contains("> First paragraph here.\n> Line two of first."));
        let _ = tmp;
    }

    #[test]
    fn append_plan_preserves_existing_content() {
        let (tmp, vault, entry) = make_repo_with_entry();
        // Pre-create a synth note.
        tmp.child("Synthesis/topic.md")
            .write_str("---\nft:\n  synth:\n    enabled: true\n---\n\nUser prose already here.\n")
            .unwrap();

        let plan = plan_synth_scaffold(
            &vault,
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
        let (_tmp, vault, entry) = make_repo_with_entry();
        let plan = plan_synth_scaffold(
            &vault,
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .unwrap();
        let expected_hash = compute_section_hash(&entry.section_text);
        assert_eq!(plan.sections[0].content_hash, expected_hash);
    }

    #[test]
    fn scaffold_pins_to_head() {
        let (_tmp, vault, entry) = make_repo_with_entry();
        let plan = plan_synth_scaffold(
            &vault,
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .unwrap();
        // Pinned to HEAD's short SHA so verify can resolve the current
        // path/line range; blame commits could predate a rename.
        let head = git::head_hash(&vault.path).unwrap();
        let sha = &plan.sections[0].commit_sha;
        assert_eq!(sha, &head[..SHORT_SHA_LEN]);
    }

    #[test]
    fn scaffold_rejects_dirty_source() {
        let (tmp, vault, entry) = make_repo_with_entry();
        // Uncommitted edit to the source the entry pins → working tree
        // no longer matches HEAD, so the HEAD-pinned section can't be
        // verified. Scaffolding must refuse.
        tmp.child("notes/source.md")
            .write_str("Edited first paragraph.\nLine two of first.\n\nSecond paragraph.\n")
            .unwrap();
        let err = plan_synth_scaffold(
            &vault,
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
        let (tmp, vault, _entry) = make_repo_with_entry();
        tmp.child("notes/new.md")
            .write_str("Fresh untracked paragraph.\n")
            .unwrap();
        let entry = GatherEntry {
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
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .unwrap_err();
        assert!(matches!(err, Error::SynthDirtySources(_)));
    }

    #[test]
    fn scaffold_succeeds_when_sources_clean() {
        let (_tmp, vault, entry) = make_repo_with_entry();
        plan_synth_scaffold(
            &vault,
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .expect("clean source scaffolds");
    }

    #[test]
    fn append_dedup_drops_already_pinned_entry() {
        // Scaffold once, then re-plan the same entry on the now-existing
        // note: the planner must report dedup_skipped == 1 and zero new
        // sections (idempotent append).
        let (_tmp, vault, entry) = make_repo_with_entry();
        let target = Path::new("Synthesis/topic.md");
        let plan1 = plan_synth_scaffold(&vault, target, std::slice::from_ref(&entry)).unwrap();
        assert_eq!(plan1.dedup_skipped, 0, "create path never dedups");
        let _ = apply_synth_scaffold(&vault, &plan1).unwrap();

        let plan2 = plan_synth_scaffold(&vault, target, std::slice::from_ref(&entry)).unwrap();
        assert!(!plan2.create, "second plan is an append");
        assert_eq!(plan2.dedup_skipped, 1, "already-pinned entry counted");
        assert!(plan2.sections.is_empty(), "no new sections emitted");
    }

    #[test]
    fn append_dedup_keeps_updated_entry_drops_unchanged() {
        // A note pins entry A (old body). Re-planning with [A_old, B_new]
        // keeps B and drops A.
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("notes/source.md")
            .write_str("Original paragraph.\n\nSecond para.\n")
            .unwrap();
        let repo = tmp.path().to_path_buf();
        let run_git = |args: &[&str]| {
            let out = std::process::Command::new("git")
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
        let entry_a = GatherEntry {
            source_title: "source".into(),
            source_path: PathBuf::from("notes/source.md"),
            line_start: 1,
            line_end: 1,
            section_text: "Original paragraph.".into(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
            matched: vec![],
        };
        let entry_b = GatherEntry {
            source_title: "source".into(),
            source_path: PathBuf::from("notes/source.md"),
            line_start: 3,
            line_end: 3,
            section_text: "Second para.".into(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
            matched: vec![],
        };
        let target = Path::new("Synthesis/topic.md");
        // Create with A, then append [A, B].
        let plan1 = plan_synth_scaffold(&vault, target, std::slice::from_ref(&entry_a)).unwrap();
        let _ = apply_synth_scaffold(&vault, &plan1).unwrap();

        let plan2 = plan_synth_scaffold(&vault, target, &[entry_a.clone(), entry_b]).unwrap();
        assert_eq!(plan2.dedup_skipped, 1, "A is already pinned");
        assert_eq!(plan2.sections.len(), 1, "only B (new) emitted");
        assert_eq!(plan2.sections[0].body, "Second para.");
    }

    #[test]
    fn append_dedup_idempotent_replan_no_write_needed() {
        // Re-planning an unchanged note yields zero sections; applying
        // is a no-op (the append path joins nothing new).
        let (tmp, vault, entry) = make_repo_with_entry();
        let target = Path::new("Synthesis/topic.md");
        let plan1 = plan_synth_scaffold(&vault, target, std::slice::from_ref(&entry)).unwrap();
        let abs = apply_synth_scaffold(&vault, &plan1).unwrap();
        let content_before = std::fs::read_to_string(&abs).unwrap();

        let plan2 = plan_synth_scaffold(&vault, target, std::slice::from_ref(&entry)).unwrap();
        let _ = apply_synth_scaffold(&vault, &plan2).unwrap();
        let content_after = std::fs::read_to_string(&abs).unwrap();
        // No new sections → content unchanged (no trailing blank block added).
        assert_eq!(
            content_before, content_after,
            "idempotent replan must not change the file"
        );
        let _ = tmp;
    }
}

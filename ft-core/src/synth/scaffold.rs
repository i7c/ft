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

use std::path::{Path, PathBuf};

use crate::blame_cache::BlameCache;
use crate::error::Result;
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
/// 2. The pinned commit SHA is determined via blame at the entry's
///    `(source_path, line_start)`; the most recent commit touching the
///    paragraph is used. Blame failures fall back to `repo`'s HEAD SHA
///    so verification still pins to a real commit (and the body still
///    needs to match HEAD content, which it does since the entry was
///    sourced from HEAD).
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
    let head_sha = git::head_hash(repo)?;
    let mut cache = BlameCache::default();

    let mut sections = Vec::with_capacity(entries.len());
    for entry in entries {
        let path_str = entry.source_path.to_string_lossy().into_owned();

        // Look up the commit SHA covering the paragraph's lines.
        let commit_sha = pinning_sha(repo, &mut cache, &path_str, entry, &head_sha)
            .unwrap_or_else(|| head_sha.clone());
        let short_sha = commit_sha[..SHORT_SHA_LEN.min(commit_sha.len())].to_string();
        let hash = compute_section_hash(&entry.section_text);

        sections.push(ProtectedSection {
            source_path: entry.source_path.clone(),
            line_start: entry.line_start,
            line_end: entry.line_end,
            commit_sha: short_sha,
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

/// Pick the commit SHA to pin a section to. Strategy: take the most
/// recent commit touching any line in the paragraph (via blame). Any
/// of those lines maps to a real commit at HEAD time, which is what
/// verification will compare against.
fn pinning_sha(
    repo: &Path,
    cache: &mut BlameCache,
    path_str: &str,
    entry: &JournalEntry,
    head_sha: &str,
) -> Option<String> {
    if cache.get(path_str, head_sha).is_none() {
        let blame = git::blame_file(repo, &entry.source_path).ok()?;
        cache.insert(path_str.to_string(), head_sha.to_string(), blame);
    }
    let blame = cache.get(path_str, head_sha)?;
    // Most recent commit (max timestamp) covering any line in the range.
    blame
        .iter()
        .filter(|lb| lb.line >= entry.line_start && lb.line <= entry.line_end)
        .max_by_key(|lb| lb.timestamp)
        .map(|lb| lb.commit_hash.clone())
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
        assert!(content.contains("> [!ft-source] notes/source.md L1-2 @"));
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
        assert!(content.contains("> [!ft-source] notes/source.md L1-2 @"));
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
    fn scaffold_pins_to_blame_commit() {
        let (_tmp, vault, repo, entry) = make_repo_with_entry();
        let plan = plan_synth_scaffold(
            &vault,
            &repo,
            Path::new("Synthesis/topic.md"),
            std::slice::from_ref(&entry),
        )
        .unwrap();
        // Commit hash should be 7 hex chars (the canonical short form).
        let sha = &plan.sections[0].commit_sha;
        assert_eq!(sha.len(), SHORT_SHA_LEN);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

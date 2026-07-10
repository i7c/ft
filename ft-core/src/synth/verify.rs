//! Verify protected sections in synth notes.
//!
//! For each `[!ft-source]` callout in a synth note, fetch the pinned
//! git blob, slice the line range, and compare against the callout
//! body byte-for-byte. Independently re-compute blake3 and check the
//! header's `#hash6` prefix.

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::git;
use crate::synth::callout::{
    compute_section_hash, is_synth_note, parse as parse_callouts, ParsedCallout,
    CONTENT_HASH_PREFIX_LEN,
};
use crate::vault::Vault;

/// Per-section verification status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionStatus {
    /// Body and hash both match the pinned source.
    Ok,
    /// Body content differs from the git blob at the pinned commit.
    /// Common cause: user edited inside a protected section.
    Drifted,
    /// Path or commit cannot be resolved (file moved before commit was
    /// made, or commit unreachable in local history).
    SourceMissing,
    /// Header itself didn't parse (this variant is currently produced
    /// only by `verify_synth_note` when [`parse_callouts`] skipped a
    /// header — captured indirectly via the on-disk text. v1: never
    /// emitted because the parser is lenient and silently skips
    /// malformed headers).
    Malformed,
}

/// One row of verification output.
#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub note_path: PathBuf,
    pub header_line: u32,
    pub source_path: PathBuf,
    pub line_start: u32,
    pub line_end: u32,
    pub commit_sha: String,
    pub status: SectionStatus,
    /// Human-readable detail. Empty when `status == Ok`.
    pub detail: String,
}

/// Verify every `[!ft-source]` callout in the synth note at `note_path`
/// (vault-relative). Returns one [`VerificationResult`] per callout in
/// document order. If the file does not contain any callouts, returns
/// an empty vector — the caller decides if "no callouts" is OK or an
/// issue.
pub fn verify_synth_note(vault: &Vault, note_path: &Path) -> Result<Vec<VerificationResult>> {
    use crate::error::Error;
    let repo = git::RepoMap::discover(&vault.path)?;
    let absolute = vault.path.join(note_path);
    let content = std::fs::read_to_string(&absolute).map_err(|e| Error::Io {
        path: absolute,
        source: e,
    })?;
    let callouts = parse_callouts(&content);
    let mut results = Vec::with_capacity(callouts.len());
    for c in &callouts {
        results.push(verify_one(&repo, note_path, c));
    }
    Ok(results)
}

/// Sweep every `.md` file in the vault, identify those with the
/// `ft.synth.enabled: true` frontmatter marker, and verify each. Returns one
/// `(note_path, results)` tuple per synth note in vault-walk order.
///
/// Files that fail to read are silently skipped (transient I/O races,
/// permission issues) — production users running this on a vault they
/// own typically don't encounter that case.
pub fn verify_all(vault: &Vault) -> Result<Vec<(PathBuf, Vec<VerificationResult>)>> {
    let repo = git::RepoMap::discover(&vault.path)?;
    let mut out = Vec::new();
    for note_rel in walk_markdown_files(&vault.path) {
        let absolute = vault.path.join(&note_rel);
        let content = match std::fs::read_to_string(&absolute) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if !is_synth_note(&content) {
            continue;
        }
        let callouts = parse_callouts(&content);
        let results: Vec<_> = callouts
            .iter()
            .map(|c| verify_one(&repo, &note_rel, c))
            .collect();
        out.push((note_rel, results));
    }
    Ok(out)
}

/// Inner verifier for one callout. `c.source_path` is vault-relative;
/// `repo` translates it to the repo-root-relative path `git show` needs.
fn verify_one(repo: &git::RepoMap, note_path: &Path, c: &ParsedCallout) -> VerificationResult {
    // Fetch the source blob.
    let blob = match git::show_file_at(repo.root(), &c.commit_sha, &repo.to_repo(&c.source_path)) {
        Ok(s) => s,
        Err(e) => {
            return VerificationResult {
                note_path: note_path.to_path_buf(),
                header_line: c.header_line,
                source_path: c.source_path.clone(),
                line_start: c.line_start,
                line_end: c.line_end,
                commit_sha: c.commit_sha.clone(),
                status: SectionStatus::SourceMissing,
                detail: format!("git show failed: {e}"),
            };
        }
    };

    // Slice lines (1-indexed inclusive).
    let lines: Vec<&str> = blob.split('\n').collect();
    let (start, end) = (c.line_start as usize, c.line_end as usize);
    if start == 0 || start > lines.len() || end < start || end > lines.len() {
        return VerificationResult {
            note_path: note_path.to_path_buf(),
            header_line: c.header_line,
            source_path: c.source_path.clone(),
            line_start: c.line_start,
            line_end: c.line_end,
            commit_sha: c.commit_sha.clone(),
            status: SectionStatus::SourceMissing,
            detail: format!(
                "line range L{}-{} outside file (file has {} lines)",
                c.line_start,
                c.line_end,
                lines.len()
            ),
        };
    }
    let expected = lines[(start - 1)..end].join("\n");

    if expected != c.body {
        return VerificationResult {
            note_path: note_path.to_path_buf(),
            header_line: c.header_line,
            source_path: c.source_path.clone(),
            line_start: c.line_start,
            line_end: c.line_end,
            commit_sha: c.commit_sha.clone(),
            status: SectionStatus::Drifted,
            detail: "body differs from source".to_string(),
        };
    }

    // Independent hash check — guards against the body being mutated in
    // ways that round-trip through the file but break the hash, and
    // catches grossly broken hashes typed by hand.
    let recomputed = compute_section_hash(&c.body);
    let header_hash_prefix = &c.content_hash[..CONTENT_HASH_PREFIX_LEN.min(c.content_hash.len())];
    if recomputed != header_hash_prefix {
        return VerificationResult {
            note_path: note_path.to_path_buf(),
            header_line: c.header_line,
            source_path: c.source_path.clone(),
            line_start: c.line_start,
            line_end: c.line_end,
            commit_sha: c.commit_sha.clone(),
            status: SectionStatus::Drifted,
            detail: format!(
                "content hash mismatch (header #{}, recomputed #{})",
                header_hash_prefix, recomputed
            ),
        };
    }

    VerificationResult {
        note_path: note_path.to_path_buf(),
        header_line: c.header_line,
        source_path: c.source_path.clone(),
        line_start: c.line_start,
        line_end: c.line_end,
        commit_sha: c.commit_sha.clone(),
        status: SectionStatus::Ok,
        detail: String::new(),
    }
}

/// Walk every `.md` file under `vault_root`, returning vault-relative
/// paths. Skips dot-prefixed entries (`.obsidian/`, `.git/`, etc.).
/// Shared with [`crate::synth::repair`]'s all-notes sweep.
pub(crate) fn walk_markdown_files(vault_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn rec(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let p = entry.path();
            if p.is_dir() {
                rec(&p, root, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("md") {
                if let Ok(rel) = p.strip_prefix(root) {
                    out.push(rel.to_path_buf());
                }
            }
        }
    }
    rec(vault_root, vault_root, &mut out);
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gather::GatherEntry;
    use crate::synth::scaffold::{apply_synth_scaffold, plan_synth_scaffold};
    use assert_fs::prelude::*;
    use chrono::NaiveDate;
    use std::process::Command;

    fn setup_repo_with_synth_note() -> (assert_fs::TempDir, Vault, PathBuf) {
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("notes/source.md")
            .write_str("Original paragraph line 1.\nOriginal paragraph line 2.\n\nSecond para.\n")
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

    #[test]
    fn verify_ok_after_scaffold() {
        let (_tmp, vault, target) = setup_repo_with_synth_note();
        let results = verify_synth_note(&vault, &target).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SectionStatus::Ok);
    }

    #[test]
    fn verify_drift_when_body_edited() {
        let (tmp, vault, target) = setup_repo_with_synth_note();
        // Hand-edit the synth note: corrupt the protected body.
        let abs = vault.path.join(&target);
        let mut content = std::fs::read_to_string(&abs).unwrap();
        content = content.replace("Original paragraph line 1.", "EDITED line 1.");
        std::fs::write(&abs, content).unwrap();

        let results = verify_synth_note(&vault, &target).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SectionStatus::Drifted);
        let _ = tmp;
    }

    #[test]
    fn verify_source_missing_for_unreachable_commit() {
        let (tmp, vault, target) = setup_repo_with_synth_note();
        // Replace the pinned SHA with one that doesn't exist.
        let abs = vault.path.join(&target);
        let content = std::fs::read_to_string(&abs).unwrap();
        let mangled = regex::Regex::new(r"@[0-9a-f]{7}")
            .unwrap()
            .replace(&content, "@deadbe1")
            .into_owned();
        std::fs::write(&abs, mangled).unwrap();

        let results = verify_synth_note(&vault, &target).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SectionStatus::SourceMissing);
        let _ = tmp;
    }

    #[test]
    fn verify_all_walks_synth_notes() {
        let (tmp, vault, _target) = setup_repo_with_synth_note();
        // Add a non-synth note that should be ignored.
        tmp.child("not-synth.md").write_str("# regular\n").unwrap();
        let all = verify_all(&vault).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].0, PathBuf::from("Synthesis/topic.md"));
        assert_eq!(all[0].1.len(), 1);
        assert_eq!(all[0].1[0].status, SectionStatus::Ok);
    }
}

//! Git integration — discovery, working-tree status, `sync`
//! (commit + pull + push), and `commit` (commit only, no network).
//! All operations shell out to `git -C <repo>` so we inherit the
//! user's full git configuration: credential helper, SSH agent, GPG
//! signing, `~/.gitconfig`. (Plan 012.)
//!
//! `sync()` is the single orchestrator. It does, in order:
//!
//! 1. Pre-check the upstream of the current branch — return a hard
//!    error before any tree mutation if no upstream is configured.
//! 2. Snapshot the working tree. Refuse to proceed if the repo is
//!    already in a half-merged / mid-rebase state.
//! 3. If dirty, `git add -A` then `git commit -m <message>`. The
//!    default message is `"ft sync <iso8601-utc>"`; callers can
//!    override via [`SyncOptions::message`].
//! 4. `git pull --no-rebase` (merge) or `git pull --rebase` per
//!    [`SyncOptions::strategy`]. On conflict, leave the working tree
//!    in its conflicted state (markers in files, merge/rebase in
//!    progress) and return the matching [`SyncOutcome`] variant.
//! 5. `git push`. If there is nothing to push, the call is still
//!    made but is a no-op as far as the remote is concerned.
//!
//! [`commit`] is the lightweight variant: the conflict pre-check and
//! the stage+commit step of `sync` only — no upstream check, no pull,
//! no push. Safe on a branch with no remote-tracking config, and fast
//! because there's no network round-trip. Use it for local iteration
//! when a full sync isn't wanted.
//!
//! Authentication uses whatever the user has configured globally; we
//! set `GIT_TERMINAL_PROMPT=0` on every spawn so the subprocess fails
//! fast instead of hanging on an interactive `Username:` prompt the
//! TUI cannot service.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

// ── Public types ─────────────────────────────────────────────────────────

/// Pull strategy used by [`sync`]. Conflict handling is identical for
/// both variants — markers stay in place, sync aborts before push.
#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PullStrategy {
    /// `git pull --no-rebase`. Diverged histories produce a merge
    /// commit; fast-forward when possible.
    #[default]
    Merge,
    /// `git pull --rebase`. Local commits are replayed on top of the
    /// fetched upstream.
    Rebase,
}

/// Per-call inputs for [`sync`].
#[derive(Debug, Default, Clone)]
pub struct SyncOptions {
    pub strategy: PullStrategy,
    /// Override the auto-generated commit message. `None` means
    /// `format!("ft sync {iso8601-utc}")`.
    pub message: Option<String>,
}

/// Per-call inputs for [`commit`].
#[derive(Debug, Default, Clone)]
pub struct CommitOptions {
    /// Override the auto-generated commit message. `None` means
    /// `format!("ft sync {iso8601-utc}")` — the same default as
    /// [`SyncOptions`] so local-only commits blend into the same
    /// history shape as full-sync commits.
    pub message: Option<String>,
}

/// Snapshot of working-tree state from `git status --porcelain=v1`.
/// Honors `.gitignore` automatically because git already filters
/// ignored entries from porcelain output.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WorkingTreeStatus {
    pub modified: Vec<PathBuf>,
    pub untracked: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
    pub conflicted: Vec<PathBuf>,
}

impl WorkingTreeStatus {
    pub fn is_clean(&self) -> bool {
        self.modified.is_empty()
            && self.untracked.is_empty()
            && self.deleted.is_empty()
            && self.conflicted.is_empty()
    }

    pub fn has_conflicts(&self) -> bool {
        !self.conflicted.is_empty()
    }
}

/// Defined end state of a [`sync`] call.
///
/// The conflict variants are *not* errors — they are normal outcomes
/// the CLI / TUI surfaces with extra detail. Hard errors (no upstream,
/// network failure, push rejected) come back as [`Error::Git`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// Tree was already clean. Pull may have fast-forwarded, and a
    /// pre-existing local-ahead state may have been pushed.
    Clean { pushed: bool },
    /// Local changes were staged and committed before pull/push.
    Synced {
        committed: usize,
        pulled: bool,
        pushed: bool,
    },
    /// `git pull` (merge) surfaced conflicts. Repo is in
    /// merge-in-progress state; `files` lists the paths with markers.
    MergeConflict { files: Vec<PathBuf> },
    /// `git pull --rebase` surfaced conflicts. Repo is mid-rebase;
    /// `files` lists the paths with markers.
    RebaseConflict { files: Vec<PathBuf> },
}

/// Defined end state of a [`commit`] call. Conflict states can't
/// arise from `commit` itself (it never pulls), so — unlike
/// [`SyncOutcome`] — there are no conflict variants; a pre-existing
/// conflicted tree surfaces as a hard [`Error::Git`] before staging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    /// Tree was already clean — nothing staged or committed.
    Clean,
    /// Local changes were staged and committed. `committed` is the
    /// file count reported by `git diff --cached --name-only`.
    Committed { committed: usize },
}

// ── Discovery ────────────────────────────────────────────────────────────

/// Walk up from `start`, returning the first ancestor that contains a
/// `.git` entry (directory or file — the file form is used by linked
/// worktrees and submodules). `None` if no enclosing repo exists.
pub fn discover_repo(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        if ancestor.join(".git").exists() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

/// Translates between vault-relative paths (what `ft` shows users and
/// stores in synth notes) and repo-root-relative paths — the coordinate
/// system git speaks in pathspecs, `<rev>:<path>`, and `--porcelain`
/// output.
///
/// When the vault root *is* the repo root the prefix is empty and both
/// conversions are the identity (the common case). But a vault nested
/// below the repo root — e.g. the repo keeps the vault under `cerebro/` —
/// must have the prefix applied, or `git show <sha>:<path>` and the
/// `git status` dirty-check resolve the wrong path entirely.
#[derive(Debug, Clone)]
pub struct RepoMap {
    root: PathBuf,
    prefix: PathBuf,
}

impl RepoMap {
    /// Discover the git repo enclosing `vault_root` and capture the
    /// vault's path prefix within it. Err if `vault_root` is not inside a
    /// git repository.
    pub fn discover(vault_root: &Path) -> Result<Self> {
        let root = discover_repo(vault_root).ok_or_else(|| {
            Error::Git(format!(
                "{} is not inside a git repository",
                vault_root.display()
            ))
        })?;
        let prefix = vault_root
            .strip_prefix(&root)
            .unwrap_or_else(|_| Path::new(""))
            .to_path_buf();
        Ok(Self { root, prefix })
    }

    /// The git repository root (absolute). Pass this as the `repo`
    /// argument to the rest of this module.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Vault-relative → repo-root-relative.
    pub fn to_repo(&self, vault_rel: &Path) -> PathBuf {
        self.prefix.join(vault_rel)
    }

    /// Repo-root-relative → vault-relative. `None` if the path lies
    /// outside the vault subtree (a sibling directory in the same repo).
    pub fn to_vault(&self, repo_rel: &Path) -> Option<PathBuf> {
        repo_rel
            .strip_prefix(&self.prefix)
            .ok()
            .map(Path::to_path_buf)
    }
}

// ── Status ───────────────────────────────────────────────────────────────

/// Snapshot the working tree via `git status --porcelain=v1
/// --untracked-files=normal -z`. The `-z` flag is load-bearing: it emits
/// NUL-terminated records and disables git's C-style path quoting, so
/// paths with spaces or non-ASCII bytes (common in vault note titles)
/// come through verbatim instead of wrapped in `"…"` with escapes.
/// Honors `.gitignore` automatically — git filters ignored entries.
pub fn status(repo: &Path) -> Result<WorkingTreeStatus> {
    let output = git(repo)
        .args(["status", "--porcelain=v1", "--untracked-files=normal", "-z"])
        .output()
        .map_err(spawn_err)?;
    if !output.status.success() {
        return Err(cmd_err("status --porcelain=v1 -z", &output.stderr));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut s = WorkingTreeStatus::default();
    // -z records are NUL-terminated. A rename/copy record is followed by
    // a second NUL-terminated field carrying the original path; we want
    // the destination (which comes first under -z) and must consume the
    // original field so it isn't parsed as a fresh record.
    let mut records = text.split('\0').filter(|r| !r.is_empty());
    while let Some(record) = records.next() {
        if record.len() < 3 {
            continue;
        }
        let (codes, rest) = record.split_at(2);
        let path = rest.strip_prefix(' ').unwrap_or(rest);
        let bytes = codes.as_bytes();
        let x = bytes[0] as char;
        let y = bytes[1] as char;

        if x == 'R' || x == 'C' || y == 'R' || y == 'C' {
            records.next(); // discard the original-path field
        }

        // U in either column, AA, DD → unresolved conflict
        if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
            s.conflicted.push(PathBuf::from(path));
            continue;
        }

        if x == '?' && y == '?' {
            s.untracked.push(PathBuf::from(path));
            continue;
        }

        if x == 'D' || y == 'D' {
            s.deleted.push(PathBuf::from(path));
            continue;
        }

        // M, A, R, C, T in either column → modified for our purposes
        s.modified.push(PathBuf::from(path));
    }
    Ok(s)
}

// ── Upstream ─────────────────────────────────────────────────────────────

/// Upstream of the current branch (e.g. `"origin/main"`), or `None` if
/// no upstream is configured (also covers detached HEAD).
pub fn upstream(repo: &Path) -> Result<Option<String>> {
    let output = git(repo)
        .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
        .output()
        .map_err(spawn_err)?;
    if output.status.success() {
        Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    } else {
        Ok(None)
    }
}

fn current_branch(repo: &Path) -> Result<String> {
    let output = git(repo)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .map_err(spawn_err)?;
    if !output.status.success() {
        return Err(cmd_err("rev-parse HEAD", &output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// ── Blame ───────────────────────────────────────────────────────────────

/// One line's git-blame record. `timestamp` is the author-time unix
/// epoch seconds; `commit_hash` is the full 40-char SHA.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LineBlame {
    pub line: u32,
    pub commit_hash: String,
    pub timestamp: i64,
}

/// Return the current HEAD commit hash for `repo` (40-char SHA).
pub fn head_hash(repo: &Path) -> Result<String> {
    let out = git(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err("rev-parse HEAD", &out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Per-line blame for `rel_path` (vault-relative or relative to `repo`).
///
/// Shells out to `git blame --porcelain`. The porcelain header for each
/// hunk has the form:
///
/// ```text
/// <40-char sha> <orig-line> <cur-line> <hunk-len>
/// author Name
/// author-time 1736000000
/// ...
/// \t<line content>
/// ```
///
/// We parse the SHA and author-time off the header lines and emit one
/// [`LineBlame`] per source line (1-indexed). Returns `Err` when `git`
/// fails (file not tracked, repo missing, etc.).
pub fn blame_file(repo: &Path, rel_path: &Path) -> Result<Vec<LineBlame>> {
    let out = git(repo)
        .args(["blame", "--porcelain"])
        .arg(rel_path)
        .output()
        .map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err(
            &format!("blame --porcelain {}", rel_path.display()),
            &out.stderr,
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);

    // Per-commit metadata is given once per SHA: we cache by SHA so
    // subsequent hunks from the same commit can reuse the timestamp
    // without git re-emitting the header lines.
    let mut sha_time: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut out_blame: Vec<LineBlame> = Vec::new();

    let mut current_sha: Option<String> = None;
    let mut pending_time: Option<i64> = None;

    for line in text.lines() {
        if line.starts_with('\t') {
            // Content line — emit the LineBlame for the most recent
            // header tuple.
            let sha = current_sha
                .as_ref()
                .expect("porcelain content line preceded by header");
            let ts = pending_time
                .or_else(|| sha_time.get(sha).copied())
                .expect("author-time recorded before content line");
            sha_time.insert(sha.clone(), ts);
            let lineno = (out_blame.len() as u32) + 1;
            out_blame.push(LineBlame {
                line: lineno,
                commit_hash: sha.clone(),
                timestamp: ts,
            });
            pending_time = None;
            continue;
        }
        // Header / metadata line.
        if let Some((head_sha, _rest)) = parse_porcelain_header(line) {
            current_sha = Some(head_sha);
            pending_time = None;
            continue;
        }
        if let Some(ts) = line.strip_prefix("author-time ") {
            if let Ok(n) = ts.trim().parse::<i64>() {
                pending_time = Some(n);
            }
        }
    }

    Ok(out_blame)
}

/// A porcelain header looks like `<sha> <orig> <cur> <hunk-len>` —
/// 40-char hex SHA followed by space-separated integers. Returns
/// `Some((sha, rest))` when the line begins with a 40-char hex SHA
/// followed by a space.
fn parse_porcelain_header(line: &str) -> Option<(String, &str)> {
    let sha = line.get(0..40)?;
    if !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let rest = line.get(40..)?;
    if !rest.starts_with(' ') {
        return None;
    }
    Some((sha.to_string(), rest))
}

// ── Diff helpers (used by pulse) ──────────────────────────────────

/// Resolve a ref (branch, tag, partial SHA) to its full 40-char commit SHA.
pub fn rev_parse(repo: &Path, refspec: &str) -> Result<String> {
    let out = git(repo)
        .args(["rev-parse", refspec])
        .output()
        .map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err(&format!("rev-parse {refspec}"), &out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Find the most recent commit at-or-before `iso_date` on HEAD. Returns
/// the full 40-char SHA, or `Err` when no commit exists at-or-before
/// that date.
pub fn commit_before(repo: &Path, iso_date: &str) -> Result<String> {
    let out = git(repo)
        .args([
            "log",
            "-1",
            &format!("--before={iso_date}"),
            "--format=%H",
            "HEAD",
        ])
        .output()
        .map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err(&format!("log -1 --before={iso_date}"), &out.stderr));
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.is_empty() {
        return Err(Error::Git(format!(
            "no commit at or before {iso_date} on HEAD"
        )));
    }
    Ok(sha)
}

/// List vault-relative paths changed between two refs (`git diff
/// --name-only <from>..<to>`).
pub fn diff_changed_paths(repo: &Path, from: &str, to: &str) -> Result<Vec<PathBuf>> {
    let range = format!("{from}..{to}");
    let out = git(repo)
        .args(["diff", "--name-only", &range])
        .output()
        .map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err(&format!("diff --name-only {range}"), &out.stderr));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text
        .lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect())
}

/// Parse `git diff <from>..<to> -- <path>` and return the set of
/// line numbers in `<to>`'s post-state that are ADDED (lines that
/// exist in `<to>` but not `<from>`, per the diff). Line numbers are
/// 1-indexed.
///
/// Returns an empty set when the file was not added/modified in this
/// range (e.g., only deletions).
pub fn diff_added_lines(repo: &Path, from: &str, to: &str, path: &Path) -> Result<Vec<u32>> {
    let range = format!("{from}..{to}");
    let out = git(repo)
        .args(["diff", "--unified=0", &range, "--"])
        .arg(path)
        .output()
        .map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err(
            &format!("diff --unified=0 {range} -- {}", path.display()),
            &out.stderr,
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(parse_added_lines_from_diff(&text))
}

/// Parse the "+ line numbers" out of unified-diff text. The hunk header
/// `@@ -<a>,<b> +<c>,<d> @@` declares post-state range; each subsequent
/// `+<content>` line (not `+++`) advances the post-state cursor.
fn parse_added_lines_from_diff(diff: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let mut cur: u32 = 0;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("@@ ") {
            // Parse the `+<start>[,<count>]` part.
            if let Some(plus_idx) = rest.find('+') {
                let after = &rest[plus_idx + 1..];
                let end = after
                    .find(|c: char| !c.is_ascii_digit() && c != ',')
                    .unwrap_or(after.len());
                let nums = &after[..end];
                let start: u32 = nums
                    .split(',')
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                cur = start;
            }
            continue;
        }
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if let Some(_added) = line.strip_prefix('+') {
            if cur > 0 {
                out.push(cur);
                cur += 1;
            }
            continue;
        }
        if line.starts_with(' ') {
            // context line — advances cursor (would not appear with
            // --unified=0 in practice, but be safe).
            cur += 1;
        }
        // `-` lines do not advance post-state cursor.
    }
    out
}

/// Return file content at a given commit. `path` is repo-relative.
/// Equivalent to `git show <sha>:<path>`.
pub fn show_file_at(repo: &Path, sha: &str, path: &Path) -> Result<String> {
    let spec = format!("{sha}:{}", path.display());
    let out = git(repo)
        .args(["show", &spec])
        .output()
        .map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err(&format!("show {spec}"), &out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Check whether a commit object exists in the local repo (`git
/// cat-file -e <sha>^{commit}`). Returns `false` (not an error) when the
/// object is unreachable — e.g. a shallow clone or a branch switch that
/// dropped the commit. Used by the synth accrete watermark to skip
/// unreachable pinned SHAs.
pub fn object_exists(repo: &Path, sha: &str) -> bool {
    let spec = format!("{sha}^{{commit}}");
    git(repo)
        .args(["cat-file", "-e", &spec])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The topological tip among a set of commits — the descendant
/// reachable from all of them — via `git rev-list --max-count=1 <sha...>`.
/// Returns the full 40-char SHA. Errors when a SHA is ambiguous or no
/// SHA resolves. Used by the synth accrete last-synth watermark.
pub fn rev_list_tip(repo: &Path, shas: &[&str]) -> Result<String> {
    let mut cmd = git(repo);
    cmd.args(["rev-list", "--max-count=1"]);
    for s in shas {
        cmd.arg(s);
    }
    let out = cmd.output().map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err("rev-list --max-count=1", &out.stderr));
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.is_empty() {
        return Err(Error::Git(format!("rev-list produced no tip for {shas:?}")));
    }
    Ok(sha)
}

/// The committer date (ISO 8601, e.g. `2026-06-01T12:34:56+00:00`) of a
/// single commit via `git log -1 --format=%cI <sha>`. Used by the synth
/// accrete watermark to scope `--new-only` entries.
pub fn commit_committer_date_iso(repo: &Path, sha: &str) -> Result<String> {
    let out = git(repo)
        .args(["log", "-1", "--format=%cI", sha])
        .output()
        .map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err(&format!("log -1 --format=%cI {sha}"), &out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

// ── Sync ─────────────────────────────────────────────────────────────────

/// Orchestrate the full sync sequence. See the module docs for the
/// step-by-step behavior.
pub fn sync(repo: &Path, opts: &SyncOptions) -> Result<SyncOutcome> {
    if upstream(repo)?.is_none() {
        let branch = current_branch(repo).unwrap_or_else(|_| "<detached>".to_string());
        return Err(Error::Git(format!(
            "branch '{branch}' has no upstream — run 'git push -u origin {branch}' once to set it"
        )));
    }

    let initial = status(repo)?;
    if initial.has_conflicts() {
        return Err(Error::Git(format!(
            "repository has unresolved conflicts in {} file(s); resolve and commit before syncing",
            initial.conflicted.len()
        )));
    }

    let tree_was_clean = initial.is_clean();
    let mut committed_count = 0usize;
    if !tree_was_clean {
        let out = git(repo).args(["add", "-A"]).output().map_err(spawn_err)?;
        if !out.status.success() {
            return Err(cmd_err("add -A", &out.stderr));
        }
        committed_count = staged_file_count(repo)?;

        let message = opts
            .message
            .clone()
            .unwrap_or_else(|| format!("ft sync {}", Utc::now().format("%Y-%m-%dT%H:%M:%SZ")));
        let out = git(repo)
            .args(["commit", "-m", &message])
            .output()
            .map_err(spawn_err)?;
        if !out.status.success() {
            return Err(cmd_err("commit", &out.stderr));
        }
    }

    let head_before_pull = head(repo)?;
    let pull_flag = match opts.strategy {
        PullStrategy::Merge => "--no-rebase",
        PullStrategy::Rebase => "--rebase",
    };
    let pull_out = git(repo)
        .args(["pull", pull_flag])
        .output()
        .map_err(spawn_err)?;
    if !pull_out.status.success() {
        let post = status(repo)?;
        if post.has_conflicts() {
            return Ok(match opts.strategy {
                PullStrategy::Merge => SyncOutcome::MergeConflict {
                    files: post.conflicted,
                },
                PullStrategy::Rebase => SyncOutcome::RebaseConflict {
                    files: post.conflicted,
                },
            });
        }
        return Err(cmd_err(&format!("pull {pull_flag}"), &pull_out.stderr));
    }
    let head_after_pull = head(repo)?;
    let pulled = head_before_pull != head_after_pull;

    let ahead_before_push = ahead_count(repo)?;
    let push_out = git(repo).arg("push").output().map_err(spawn_err)?;
    if !push_out.status.success() {
        return Err(cmd_err("push", &push_out.stderr));
    }
    let pushed = ahead_before_push > 0;

    Ok(if tree_was_clean {
        SyncOutcome::Clean { pushed }
    } else {
        SyncOutcome::Synced {
            committed: committed_count,
            pulled,
            pushed,
        }
    })
}

// ── Commit (lightweight sync) ───────────────────────────────────────────

/// Lightweight commit-only variant of [`sync`]: snapshot the working
/// tree, refuse on unresolved conflicts, `git add -A`, `git commit`,
/// return. No upstream check, no pull, no push — so it's safe on a
/// branch with no remote-tracking config and fast (no network
/// round-trip). See the module docs for the contrast with [`sync`].
pub fn commit(repo: &Path, opts: &CommitOptions) -> Result<CommitOutcome> {
    let initial = status(repo)?;
    if initial.has_conflicts() {
        return Err(Error::Git(format!(
            "repository has unresolved conflicts in {} file(s); resolve before committing",
            initial.conflicted.len()
        )));
    }

    if initial.is_clean() {
        return Ok(CommitOutcome::Clean);
    }

    let out = git(repo).args(["add", "-A"]).output().map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err("add -A", &out.stderr));
    }
    let committed_count = staged_file_count(repo)?;

    let message = opts
        .message
        .clone()
        .unwrap_or_else(|| format!("ft sync {}", Utc::now().format("%Y-%m-%dT%H:%M:%SZ")));
    let out = git(repo)
        .args(["commit", "-m", &message])
        .output()
        .map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err("commit", &out.stderr));
    }

    Ok(CommitOutcome::Committed {
        committed: committed_count,
    })
}

fn staged_file_count(repo: &Path) -> Result<usize> {
    let out = git(repo)
        .args(["diff", "--cached", "--name-only", "--no-renames"])
        .output()
        .map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err("diff --cached", &out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count())
}

fn head(repo: &Path) -> Result<String> {
    let out = git(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err("rev-parse HEAD", &out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn ahead_count(repo: &Path) -> Result<u32> {
    let out = git(repo)
        .args(["rev-list", "--count", "@{u}..HEAD"])
        .output()
        .map_err(spawn_err)?;
    if !out.status.success() {
        return Err(cmd_err("rev-list", &out.stderr));
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u32>()
        .map_err(|e| Error::Git(format!("rev-list: unparseable count: {e}")))
}

// ── Internals ────────────────────────────────────────────────────────────

fn git(repo: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo);
    // Prevent any interactive credential prompt — the TUI has no stdin
    // for the subprocess and would hang forever. Credential helpers
    // (osxkeychain, libsecret, etc.) are non-interactive and still work.
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

fn spawn_err(e: std::io::Error) -> Error {
    Error::Git(format!("spawn git: {e}"))
}

fn cmd_err(label: &str, stderr: &[u8]) -> Error {
    let s = String::from_utf8_lossy(stderr).trim().to_string();
    if s.is_empty() {
        Error::Git(format!("git {label} failed"))
    } else {
        Error::Git(format!("git {label}: {s}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── Test helpers ────────────────────────────────────────────────

    fn run_git(dir: &Path, args: &[&str]) -> std::process::Output {
        let out = Command::new("git")
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .output()
            .expect("git exec failed (is git on PATH?)");
        assert!(
            out.status.success(),
            "git {args:?} failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        out
    }

    /// `git init` a fresh repo with a known identity and signing
    /// disabled, so commits succeed even when the dev's global
    /// `~/.gitconfig` has `commit.gpgsign = true` with no key.
    fn init_repo(dir: &Path) {
        run_git(dir, &["init", "-b", "main"]);
        run_git(dir, &["config", "user.name", "Test"]);
        run_git(dir, &["config", "user.email", "test@example.com"]);
        run_git(dir, &["config", "commit.gpgsign", "false"]);
    }

    /// Create a bare origin and a clone that already has one commit
    /// pushed (so `main` is tracking `origin/main`). Returns
    /// `(origin_path, clone_path)`.
    fn setup_origin_and_clone(tmp: &Path) -> (PathBuf, PathBuf) {
        let origin = tmp.join("origin.git");
        fs::create_dir(&origin).unwrap();
        run_git(&origin, &["init", "--bare", "-b", "main"]);

        let clone = tmp.join("a");
        let out = Command::new("git")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["clone", origin.to_str().unwrap(), clone.to_str().unwrap()])
            .output()
            .expect("git clone");
        assert!(
            out.status.success(),
            "clone failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        run_git(&clone, &["config", "user.name", "A"]);
        run_git(&clone, &["config", "user.email", "a@example.com"]);
        run_git(&clone, &["config", "commit.gpgsign", "false"]);

        fs::write(clone.join("seed.md"), "seed\n").unwrap();
        run_git(&clone, &["add", "."]);
        run_git(&clone, &["commit", "-m", "seed"]);
        run_git(&clone, &["push", "-u", "origin", "main"]);

        (origin, clone)
    }

    /// Make a second clone of the same origin, push one new file, and
    /// return the clone path. Used to set the stage for conflict tests.
    fn second_clone_pushes(
        origin: &Path,
        tmp: &Path,
        name: &str,
        file: &str,
        content: &str,
    ) -> PathBuf {
        let dir = tmp.join(name);
        let out = Command::new("git")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["clone", origin.to_str().unwrap(), dir.to_str().unwrap()])
            .output()
            .expect("git clone");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        run_git(&dir, &["config", "user.name", name]);
        run_git(
            &dir,
            &["config", "user.email", &format!("{name}@example.com")],
        );
        run_git(&dir, &["config", "commit.gpgsign", "false"]);
        fs::write(dir.join(file), content).unwrap();
        run_git(&dir, &["add", "."]);
        run_git(&dir, &["commit", "-m", &format!("from {name}")]);
        run_git(&dir, &["push"]);
        dir
    }

    // ── discover_repo ───────────────────────────────────────────────

    #[test]
    fn discover_finds_git_at_repo_root() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        init_repo(&repo);
        let found = discover_repo(&repo).unwrap();
        assert_eq!(
            fs::canonicalize(&found).unwrap(),
            fs::canonicalize(&repo).unwrap()
        );
    }

    #[test]
    fn discover_finds_git_at_ancestor() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        let nested = repo.join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        init_repo(&repo);
        let found = discover_repo(&nested).unwrap();
        assert_eq!(
            fs::canonicalize(&found).unwrap(),
            fs::canonicalize(&repo).unwrap()
        );
    }

    #[test]
    fn discover_returns_none_when_no_git_in_tree() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("no-git");
        fs::create_dir(&path).unwrap();
        // /tmp (or platform equivalent) is not a git repo; safe to
        // assert None on a freshly-created subdir.
        assert!(discover_repo(&path).is_none());
    }

    #[test]
    fn discover_handles_git_as_file_for_worktrees() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("worktree-like");
        fs::create_dir(&dir).unwrap();
        // Linked worktrees use a `.git` file, not a directory.
        fs::write(dir.join(".git"), "gitdir: /elsewhere\n").unwrap();
        let found = discover_repo(&dir).unwrap();
        assert_eq!(
            fs::canonicalize(&found).unwrap(),
            fs::canonicalize(&dir).unwrap()
        );
    }

    // ── status ──────────────────────────────────────────────────────

    #[test]
    fn status_clean_tree_reports_clean() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        init_repo(repo);
        fs::write(repo.join("f.md"), "x\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "x"]);
        let s = status(repo).unwrap();
        assert!(s.is_clean(), "{s:?}");
    }

    #[test]
    fn status_reports_modified_file() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        init_repo(repo);
        fs::write(repo.join("f.md"), "v1\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "v1"]);
        fs::write(repo.join("f.md"), "v2\n").unwrap();
        let s = status(repo).unwrap();
        assert_eq!(s.modified, vec![PathBuf::from("f.md")]);
        assert!(s.untracked.is_empty());
        assert!(s.deleted.is_empty());
        assert!(s.conflicted.is_empty());
    }

    #[test]
    fn status_reports_untracked_file() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        init_repo(repo);
        fs::write(repo.join("seed.md"), "s\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "seed"]);
        fs::write(repo.join("u.md"), "new\n").unwrap();
        let s = status(repo).unwrap();
        assert_eq!(s.untracked, vec![PathBuf::from("u.md")]);
    }

    #[test]
    fn status_reports_deleted_file() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        init_repo(repo);
        fs::write(repo.join("f.md"), "x\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "x"]);
        fs::remove_file(repo.join("f.md")).unwrap();
        let s = status(repo).unwrap();
        assert_eq!(s.deleted, vec![PathBuf::from("f.md")]);
    }

    #[test]
    fn status_reports_path_with_spaces_unquoted() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        init_repo(repo);
        fs::write(repo.join("Life Strategy.md"), "v1\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "v1"]);
        fs::write(repo.join("Life Strategy.md"), "v2\n").unwrap();
        let s = status(repo).unwrap();
        // Verbatim path, not git's `"Life Strategy.md"` quoted form.
        assert_eq!(s.modified, vec![PathBuf::from("Life Strategy.md")]);
    }

    #[test]
    fn status_rename_reports_destination_only() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        init_repo(repo);
        fs::write(repo.join("old name.md"), "x\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "x"]);
        run_git(repo, &["mv", "old name.md", "new name.md"]);
        let s = status(repo).unwrap();
        // Destination captured; the original-path field must not leak in
        // as a separate entry.
        assert_eq!(s.modified, vec![PathBuf::from("new name.md")]);
        assert!(s.untracked.is_empty(), "{s:?}");
    }

    #[test]
    fn repomap_identity_when_vault_is_repo_root() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        init_repo(repo);
        let map = RepoMap::discover(repo).unwrap();
        assert_eq!(map.root(), repo);
        assert_eq!(
            map.to_repo(Path::new("notes/a.md")),
            PathBuf::from("notes/a.md")
        );
        assert_eq!(
            map.to_vault(Path::new("notes/a.md")),
            Some(PathBuf::from("notes/a.md"))
        );
    }

    #[test]
    fn repomap_applies_prefix_for_nested_vault() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        init_repo(repo);
        let vault = repo.join("cerebro");
        fs::create_dir_all(&vault).unwrap();
        let map = RepoMap::discover(&vault).unwrap();
        assert_eq!(map.root(), repo);
        // Vault-relative ↔ repo-relative round-trip through the prefix.
        assert_eq!(
            map.to_repo(Path::new("areas/life/Note.md")),
            PathBuf::from("cerebro/areas/life/Note.md")
        );
        assert_eq!(
            map.to_vault(Path::new("cerebro/areas/life/Note.md")),
            Some(PathBuf::from("areas/life/Note.md"))
        );
        // A sibling outside the vault subtree maps to None.
        assert_eq!(map.to_vault(Path::new("other/x.md")), None);
    }

    #[test]
    fn status_respects_gitignore() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        init_repo(repo);
        fs::write(repo.join(".gitignore"), "ignored.md\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "init"]);
        fs::write(repo.join("ignored.md"), "secret\n").unwrap();
        let s = status(repo).unwrap();
        assert!(s.is_clean(), "ignored file leaked into status: {s:?}");
    }

    // ── upstream ────────────────────────────────────────────────────

    #[test]
    fn upstream_returns_some_when_set() {
        let tmp = TempDir::new().unwrap();
        let (_origin, a) = setup_origin_and_clone(tmp.path());
        let u = upstream(&a).unwrap();
        assert_eq!(u.as_deref(), Some("origin/main"));
    }

    #[test]
    fn upstream_returns_none_when_unset() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        init_repo(repo);
        fs::write(repo.join("f.md"), "x\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "x"]);
        assert!(upstream(repo).unwrap().is_none());
    }

    // ── sync ────────────────────────────────────────────────────────

    #[test]
    fn sync_clean_tree_is_noop() {
        let tmp = TempDir::new().unwrap();
        let (_origin, a) = setup_origin_and_clone(tmp.path());
        let outcome = sync(&a, &SyncOptions::default()).unwrap();
        assert!(matches!(outcome, SyncOutcome::Clean { pushed: false }));
    }

    #[test]
    fn sync_commits_and_pushes_new_file() {
        let tmp = TempDir::new().unwrap();
        let (_origin, a) = setup_origin_and_clone(tmp.path());
        fs::write(a.join("new.md"), "new\n").unwrap();
        let outcome = sync(&a, &SyncOptions::default()).unwrap();
        match outcome {
            SyncOutcome::Synced {
                committed,
                pulled,
                pushed,
            } => {
                assert_eq!(committed, 1);
                assert!(!pulled);
                assert!(pushed);
            }
            other => panic!("expected Synced, got {other:?}"),
        }
    }

    #[test]
    fn sync_clean_tree_fast_forwards_when_upstream_ahead() {
        let tmp = TempDir::new().unwrap();
        let (origin, a) = setup_origin_and_clone(tmp.path());
        let _b = second_clone_pushes(&origin, tmp.path(), "b", "from-b.md", "x\n");

        let outcome = sync(&a, &SyncOptions::default()).unwrap();
        assert!(matches!(outcome, SyncOutcome::Clean { pushed: false }));
        assert!(a.join("from-b.md").exists());
    }

    #[test]
    fn sync_no_upstream_errors_before_committing() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        init_repo(repo);
        fs::write(repo.join("f.md"), "x\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "init"]);
        // New file: would be committed if sync didn't bail early.
        fs::write(repo.join("new.md"), "new\n").unwrap();

        let err = sync(repo, &SyncOptions::default()).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("no upstream"), "got: {msg}");

        // Pre-check ran before staging — untracked file is still untracked.
        let s = status(repo).unwrap();
        assert_eq!(s.untracked, vec![PathBuf::from("new.md")]);
    }

    #[test]
    fn sync_merge_conflict_leaves_markers_in_files() {
        let tmp = TempDir::new().unwrap();
        let (origin, a) = setup_origin_and_clone(tmp.path());
        let _b = second_clone_pushes(&origin, tmp.path(), "b", "seed.md", "from b\n");

        // A modifies the same file to a conflicting line.
        fs::write(a.join("seed.md"), "from a\n").unwrap();

        let outcome = sync(&a, &SyncOptions::default()).unwrap();
        match outcome {
            SyncOutcome::MergeConflict { files } => {
                assert_eq!(files, vec![PathBuf::from("seed.md")]);
                let content = fs::read_to_string(a.join("seed.md")).unwrap();
                assert!(content.contains("<<<<<<<"), "no markers: {content}");
                assert!(content.contains(">>>>>>>"), "no markers: {content}");
            }
            other => panic!("expected MergeConflict, got {other:?}"),
        }
    }

    #[test]
    fn sync_rebase_conflict_leaves_markers_in_files() {
        let tmp = TempDir::new().unwrap();
        let (origin, a) = setup_origin_and_clone(tmp.path());
        let _b = second_clone_pushes(&origin, tmp.path(), "b", "seed.md", "from b\n");

        fs::write(a.join("seed.md"), "from a\n").unwrap();

        let opts = SyncOptions {
            strategy: PullStrategy::Rebase,
            message: None,
        };
        let outcome = sync(&a, &opts).unwrap();
        match outcome {
            SyncOutcome::RebaseConflict { files } => {
                assert_eq!(files, vec![PathBuf::from("seed.md")]);
                let content = fs::read_to_string(a.join("seed.md")).unwrap();
                assert!(content.contains("<<<<<<<"), "no markers: {content}");
            }
            other => panic!("expected RebaseConflict, got {other:?}"),
        }
    }

    // ── blame_file ──────────────────────────────────────────────────

    #[test]
    fn blame_file_single_commit() {
        let tmp = TempDir::new().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("a.md"), "one\ntwo\nthree\n").unwrap();
        run_git(tmp.path(), &["add", "."]);
        run_git(tmp.path(), &["commit", "-m", "init"]);

        let blame = blame_file(tmp.path(), Path::new("a.md")).unwrap();
        assert_eq!(blame.len(), 3);
        let first_sha = &blame[0].commit_hash;
        assert_eq!(first_sha.len(), 40);
        for (i, b) in blame.iter().enumerate() {
            assert_eq!(b.line as usize, i + 1);
            assert_eq!(&b.commit_hash, first_sha, "single-commit file");
            assert!(b.timestamp > 0);
        }
    }

    #[test]
    fn blame_file_two_commits_have_different_shas() {
        let tmp = TempDir::new().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("a.md"), "one\ntwo\n").unwrap();
        run_git(tmp.path(), &["add", "."]);
        run_git(tmp.path(), &["commit", "-m", "first"]);

        // Wait a second so author-time differs.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(tmp.path().join("a.md"), "one\nALTERED\nthree\n").unwrap();
        run_git(tmp.path(), &["add", "."]);
        run_git(tmp.path(), &["commit", "-m", "second"]);

        let blame = blame_file(tmp.path(), Path::new("a.md")).unwrap();
        assert_eq!(blame.len(), 3);
        // Line 1 is unchanged (first commit). Lines 2 and 3 are from
        // the second commit.
        assert_eq!(blame[1].commit_hash, blame[2].commit_hash);
        assert_ne!(blame[0].commit_hash, blame[1].commit_hash);
    }

    #[test]
    fn blame_file_untracked_returns_error() {
        let tmp = TempDir::new().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("untracked.md"), "x\n").unwrap();
        let err = blame_file(tmp.path(), Path::new("untracked.md"));
        assert!(err.is_err(), "blame of untracked file should fail");
    }

    #[test]
    fn head_hash_returns_sha_after_commit() {
        let tmp = TempDir::new().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("a.md"), "x\n").unwrap();
        run_git(tmp.path(), &["add", "."]);
        run_git(tmp.path(), &["commit", "-m", "init"]);
        let sha = head_hash(tmp.path()).unwrap();
        assert_eq!(sha.len(), 40);
    }

    #[test]
    fn sync_uses_custom_message_when_provided() {
        let tmp = TempDir::new().unwrap();
        let (_origin, a) = setup_origin_and_clone(tmp.path());
        fs::write(a.join("new.md"), "v\n").unwrap();
        let opts = SyncOptions {
            strategy: PullStrategy::Merge,
            message: Some("my custom msg".to_string()),
        };
        sync(&a, &opts).unwrap();
        let out = Command::new("git")
            .current_dir(&a)
            .args(["log", "-1", "--format=%s"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "my custom msg");
    }

    // ── commit ──────────────────────────────────────────────────────
    //
    // `commit` is the lightweight variant of `sync`: stage + commit
    // only — no upstream check, no pull, no push. The first test
    // mirrors `sync_commits_and_pushes_new_file` but checks for the
    // absence of a push; the rest cover the clean path, the no-upstream
    // case (which `sync` rejects), the conflict pre-check, the custom
    // message, and the auto-message shape.

    #[test]
    fn commit_commits_new_file_without_pushing() {
        let tmp = TempDir::new().unwrap();
        let (_origin, a) = setup_origin_and_clone(tmp.path());
        fs::write(a.join("new.md"), "new\n").unwrap();

        let outcome = commit(&a, &CommitOptions::default()).unwrap();
        match outcome {
            CommitOutcome::Committed { committed } => assert_eq!(committed, 1),
            other => panic!("expected Committed, got {other:?}"),
        }

        // The new file is now committed locally.
        let s = status(&a).unwrap();
        assert!(s.is_clean(), "tree not clean after commit: {s:?}");
        // But it was NOT pushed — origin still lacks the file.
        let bare = Command::new("git")
            .current_dir(_origin)
            .args(["show", "main:new.md"])
            .output()
            .unwrap();
        assert!(
            !bare.status.success(),
            "commit must not push; origin should not have new.md"
        );
    }

    #[test]
    fn commit_clean_tree_is_noop() {
        let tmp = TempDir::new().unwrap();
        let (_origin, a) = setup_origin_and_clone(tmp.path());
        let outcome = commit(&a, &CommitOptions::default()).unwrap();
        assert!(matches!(outcome, CommitOutcome::Clean));
    }

    #[test]
    fn commit_works_on_branch_with_no_upstream() {
        // `sync` rejects a branch with no upstream; `commit` must not,
        // because it never pulls or pushes.
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        init_repo(repo);
        fs::write(repo.join("seed.md"), "x\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "seed"]);
        assert!(upstream(repo).unwrap().is_none());

        fs::write(repo.join("new.md"), "new\n").unwrap();
        let outcome = commit(repo, &CommitOptions::default()).unwrap();
        assert!(matches!(outcome, CommitOutcome::Committed { committed: 1 }));

        let s = status(repo).unwrap();
        assert!(s.is_clean(), "tree not clean after commit: {s:?}");
    }

    #[test]
    fn commit_refuses_on_preexisting_conflicts() {
        // Manufacture a real conflict via a divergent side-branch merge
        // (pulling into a clean tree would just fast-forward).
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        init_repo(repo);
        fs::write(repo.join("seed.md"), "base\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "base"]);

        run_git(repo, &["checkout", "-b", "feature"]);
        fs::write(repo.join("seed.md"), "from feature\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "feature"]);

        run_git(repo, &["checkout", "main"]);
        fs::write(repo.join("seed.md"), "from main\n").unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "main"]);

        // `git merge feature` conflicts — `run_git` asserts success, so
        // shell out directly and check for the expected failure.
        let merge = Command::new("git")
            .current_dir(repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["merge", "feature"])
            .output()
            .unwrap();
        assert!(
            !merge.status.success(),
            "precondition: merge should have conflicted"
        );
        let porcelain = Command::new("git")
            .current_dir(repo)
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&porcelain.stdout).contains("UU"),
            "precondition: repo should be conflicted"
        );

        let err = commit(repo, &CommitOptions::default()).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("unresolved conflicts"),
            "expected conflict error, got: {msg}"
        );
    }

    #[test]
    fn commit_uses_custom_message_when_provided() {
        let tmp = TempDir::new().unwrap();
        let (_origin, a) = setup_origin_and_clone(tmp.path());
        fs::write(a.join("new.md"), "v\n").unwrap();
        let opts = CommitOptions {
            message: Some("my commit msg".to_string()),
        };
        commit(&a, &opts).unwrap();
        let out = Command::new("git")
            .current_dir(&a)
            .args(["log", "-1", "--format=%s"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "my commit msg");
    }

    #[test]
    fn commit_auto_message_has_ft_sync_prefix() {
        // `commit` shares `sync`'s default message shape so local-only
        // commits blend into the same history as full-sync commits.
        let tmp = TempDir::new().unwrap();
        let (_origin, a) = setup_origin_and_clone(tmp.path());
        fs::write(a.join("new.md"), "v\n").unwrap();
        commit(&a, &CommitOptions::default()).unwrap();
        let out = Command::new("git")
            .current_dir(&a)
            .args(["log", "-1", "--format=%s"])
            .output()
            .unwrap();
        let subject = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert!(
            subject.starts_with("ft sync "),
            "expected `ft sync <iso8601>` subject, got: {subject}"
        );
    }
}

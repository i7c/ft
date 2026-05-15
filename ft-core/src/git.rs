//! Git integration — discovery, working-tree status, and `sync`
//! (commit + pull + push). All operations shell out to `git -C <repo>`
//! so we inherit the user's full git configuration: credential helper,
//! SSH agent, GPG signing, `~/.gitconfig`. (Plan 012.)
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

// ── Status ───────────────────────────────────────────────────────────────

/// Snapshot the working tree via `git status --porcelain=v1
/// --untracked-files=normal`. `core.quotePath=false` is set so non-ASCII
/// paths come through verbatim instead of as `\xNN`-quoted octals.
pub fn status(repo: &Path) -> Result<WorkingTreeStatus> {
    let output = git(repo)
        .args([
            "-c",
            "core.quotePath=false",
            "status",
            "--porcelain=v1",
            "--untracked-files=normal",
        ])
        .output()
        .map_err(spawn_err)?;
    if !output.status.success() {
        return Err(cmd_err("status --porcelain=v1", &output.stderr));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut s = WorkingTreeStatus::default();
    for line in text.lines() {
        if line.len() < 3 {
            continue;
        }
        let (codes, rest) = line.split_at(2);
        let path = rest.trim_start();
        let path = rename_destination(path);
        let bytes = codes.as_bytes();
        let x = bytes[0] as char;
        let y = bytes[1] as char;

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

/// For rename/copy entries (`R<X> old -> new`), the destination path
/// is what callers care about.
fn rename_destination(path: &str) -> &str {
    if let Some(idx) = path.find(" -> ") {
        &path[idx + 4..]
    } else {
        path
    }
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
}

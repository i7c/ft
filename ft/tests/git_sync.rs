//! Integration tests for `ft git sync` and `ft git commit` — the CLI
//! surfaces added in plan 012 session 2 and its lightweight sibling.
//!
//! Each test builds a fresh `TempDir` vault (with `.obsidian/` marker)
//! that is also a git repository, then drives `ft` via `assert_cmd` and
//! checks stdout/stderr + exit code. Pull/push tests set up a bare
//! origin in a sibling temp dir to give the clone a real upstream.

use std::path::Path;
use std::process::Command as ProcCommand;

use assert_cmd::Command;
use predicates::prelude::*;

fn run_git(dir: &Path, args: &[&str]) {
    let out = ProcCommand::new("git")
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
}

/// Init a fresh repo with a local identity and no signing.
fn init_repo(dir: &Path) {
    run_git(dir, &["init", "-b", "main"]);
    run_git(dir, &["config", "user.name", "Test"]);
    run_git(dir, &["config", "user.email", "test@example.com"]);
    run_git(dir, &["config", "commit.gpgsign", "false"]);
}

/// Make `dir` a vault by adding `.obsidian/`.
fn make_vault(dir: &Path) {
    std::fs::create_dir_all(dir.join(".obsidian")).unwrap();
}

/// Create `<tmp>/origin.git` (bare) and `<tmp>/vault` (clone + vault
/// marker + one seed commit pushed). Returns the vault path.
fn setup_origin_and_vault(tmp: &Path) -> std::path::PathBuf {
    let origin = tmp.join("origin.git");
    std::fs::create_dir(&origin).unwrap();
    run_git(&origin, &["init", "--bare", "-b", "main"]);

    let vault = tmp.join("vault");
    let out = ProcCommand::new("git")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["clone", origin.to_str().unwrap(), vault.to_str().unwrap()])
        .output()
        .expect("git clone");
    assert!(
        out.status.success(),
        "clone failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    run_git(&vault, &["config", "user.name", "Local"]);
    run_git(&vault, &["config", "user.email", "local@example.com"]);
    run_git(&vault, &["config", "commit.gpgsign", "false"]);
    make_vault(&vault);
    std::fs::write(vault.join("seed.md"), "seed\n").unwrap();
    run_git(&vault, &["add", "."]);
    run_git(&vault, &["commit", "-m", "seed"]);
    run_git(&vault, &["push", "-u", "origin", "main"]);
    vault
}

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

// ── happy path: dirty tree commits, pulls, pushes ──────────────────────────

#[test]
fn sync_commits_and_pushes_dirty_tree() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let vault = setup_origin_and_vault(tmp.path());
    std::fs::write(vault.join("new.md"), "x\n").unwrap();

    ft().args(["--vault", vault.to_str().unwrap(), "git", "sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("committed 1 file(s)"))
        .stdout(predicate::str::contains("pushed"));
}

#[test]
fn sync_uses_custom_message_with_dash_m() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let vault = setup_origin_and_vault(tmp.path());
    std::fs::write(vault.join("new.md"), "x\n").unwrap();

    ft().args([
        "--vault",
        vault.to_str().unwrap(),
        "git",
        "sync",
        "-m",
        "manual subject",
    ])
    .assert()
    .success();

    let out = ProcCommand::new("git")
        .current_dir(&vault)
        .args(["log", "-1", "--format=%s"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "manual subject"
    );
}

#[test]
fn sync_clean_tree_reports_already_in_sync() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let vault = setup_origin_and_vault(tmp.path());

    ft().args(["--vault", vault.to_str().unwrap(), "git", "sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("already in sync"));
}

// ── conflict: exit 2, files listed on stderr, markers stay ─────────────────

#[test]
fn sync_merge_conflict_exits_two_with_files_in_stderr() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let vault = setup_origin_and_vault(tmp.path());

    // Side clone pushes a conflicting change to seed.md.
    let side = tmp.path().join("side");
    let out = ProcCommand::new("git")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args([
            "clone",
            tmp.path().join("origin.git").to_str().unwrap(),
            side.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    run_git(&side, &["config", "user.name", "Side"]);
    run_git(&side, &["config", "user.email", "side@example.com"]);
    run_git(&side, &["config", "commit.gpgsign", "false"]);
    std::fs::write(side.join("seed.md"), "from side\n").unwrap();
    run_git(&side, &["add", "."]);
    run_git(&side, &["commit", "-m", "from side"]);
    run_git(&side, &["push"]);

    // Local conflicts.
    std::fs::write(vault.join("seed.md"), "from local\n").unwrap();

    ft().args(["--vault", vault.to_str().unwrap(), "git", "sync"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("merge conflict"))
        .stderr(predicate::str::contains("seed.md"));

    let content = std::fs::read_to_string(vault.join("seed.md")).unwrap();
    assert!(content.contains("<<<<<<<"), "markers missing: {content}");
}

// ── no enclosing git repo → exit 1 with documented message ─────────────────

#[test]
fn sync_with_no_git_repo_errors_with_documented_message() {
    let tmp = assert_fs::TempDir::new().unwrap();
    make_vault(tmp.path());
    // No `git init` anywhere up the tree.

    ft().args(["--vault", tmp.path().to_str().unwrap(), "git", "sync"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no git repository"));
}

// ── --dry-run does not touch the tree ──────────────────────────────────────

#[test]
fn sync_dry_run_does_not_commit_or_push() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let vault = setup_origin_and_vault(tmp.path());
    std::fs::write(vault.join("untracked.md"), "x\n").unwrap();

    let head_before = ProcCommand::new("git")
        .current_dir(&vault)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let head_before = String::from_utf8_lossy(&head_before.stdout)
        .trim()
        .to_string();

    ft().args([
        "--vault",
        vault.to_str().unwrap(),
        "git",
        "sync",
        "--dry-run",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("upstream: origin/main (merge)"))
    .stdout(predicate::str::contains("would commit 1 file(s)"))
    .stdout(predicate::str::contains("would pull origin/main"))
    .stdout(predicate::str::contains("would push"));

    let head_after = ProcCommand::new("git")
        .current_dir(&vault)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let head_after = String::from_utf8_lossy(&head_after.stdout)
        .trim()
        .to_string();
    assert_eq!(head_before, head_after, "dry-run moved HEAD");

    // Untracked file is still untracked, not staged or committed.
    assert!(vault.join("untracked.md").exists());
    let porcelain = ProcCommand::new("git")
        .current_dir(&vault)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&porcelain.stdout);
    assert!(out.contains("?? untracked.md"), "got: {out}");
}

// ── no upstream → exit 1 with hint ─────────────────────────────────────────

#[test]
fn sync_no_upstream_errors_before_committing() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let vault = tmp.path();
    init_repo(vault);
    make_vault(vault);
    std::fs::write(vault.join("seed.md"), "x\n").unwrap();
    run_git(vault, &["add", "."]);
    run_git(vault, &["commit", "-m", "seed"]);
    // No upstream configured.
    std::fs::write(vault.join("new.md"), "y\n").unwrap();

    ft().args(["--vault", vault.to_str().unwrap(), "git", "sync"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no upstream"));

    // Pre-check kept the untracked file out of the index.
    let porcelain = ProcCommand::new("git")
        .current_dir(vault)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&porcelain.stdout);
    assert!(out.contains("?? new.md"), "got: {out}");
}

// ── ft git commit: lightweight, local-only ───────────────────────────────
//
// `commit` shares `sync`'s stage+commit step but skips the upstream
// check, pull, and push. These tests mirror the sync happy-paths and
// add the two differentiators: it commits on a branch with no
// upstream, and it never pushes.

#[test]
fn commit_commits_dirty_tree_without_pushing() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let vault = setup_origin_and_vault(tmp.path());
    std::fs::write(vault.join("new.md"), "x\n").unwrap();

    ft().args(["--vault", vault.to_str().unwrap(), "git", "commit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("committed 1 file(s)"))
        // No "pushed" line — commit never touches the network.
        .stdout(predicate::str::contains("pushed").not());

    // Origin does not have the new file: commit didn't push.
    let origin = tmp.path().join("origin.git");
    let bare = ProcCommand::new("git")
        .current_dir(&origin)
        .args(["show", "main:new.md"])
        .output()
        .unwrap();
    assert!(
        !bare.status.success(),
        "commit must not push; origin should not have new.md"
    );
    // Working tree is clean locally.
    let porcelain = ProcCommand::new("git")
        .current_dir(&vault)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&porcelain.stdout).trim().is_empty(),
        "tree not clean after commit"
    );
}

#[test]
fn commit_clean_tree_reports_nothing_to_commit() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let vault = setup_origin_and_vault(tmp.path());

    ft().args(["--vault", vault.to_str().unwrap(), "git", "commit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("nothing to commit"));
}

#[test]
fn commit_works_on_branch_with_no_upstream() {
    // `sync` rejects this setup; `commit` must succeed because it never
    // pulls or pushes.
    let tmp = assert_fs::TempDir::new().unwrap();
    let vault = tmp.path();
    init_repo(vault);
    make_vault(vault);
    std::fs::write(vault.join("seed.md"), "x\n").unwrap();
    run_git(vault, &["add", "."]);
    run_git(vault, &["commit", "-m", "seed"]);
    std::fs::write(vault.join("new.md"), "y\n").unwrap();

    ft().args(["--vault", vault.to_str().unwrap(), "git", "commit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("committed 1 file(s)"));
}

#[test]
fn commit_uses_custom_message_with_dash_m() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let vault = setup_origin_and_vault(tmp.path());
    std::fs::write(vault.join("new.md"), "x\n").unwrap();

    ft().args([
        "--vault",
        vault.to_str().unwrap(),
        "git",
        "commit",
        "-m",
        "manual subject",
    ])
    .assert()
    .success();

    let out = ProcCommand::new("git")
        .current_dir(&vault)
        .args(["log", "-1", "--format=%s"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "manual subject"
    );
}

#[test]
fn commit_with_no_git_repo_errors_with_documented_message() {
    let tmp = assert_fs::TempDir::new().unwrap();
    make_vault(tmp.path());

    ft().args(["--vault", tmp.path().to_str().unwrap(), "git", "commit"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no git repository"));
}

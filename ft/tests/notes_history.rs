//! Integration tests for `ft notes history`.
//!
//! Each test builds a tiny git-backed vault so blame data and the window
//! filter have meaningful, deterministic dates.

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::process::Command as StdCommand;

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

fn run_git(dir: &std::path::Path, args: &[&str]) {
    let out = StdCommand::new("git")
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()
        .expect("git binary on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Commit everything currently staged/unstaged with a fixed author/commit
/// date so window resolution is deterministic.
fn commit_all_dated(dir: &std::path::Path, msg: &str, date: &str) {
    run_git(dir, &["add", "."]);
    let out = StdCommand::new("git")
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .args(["commit", "-m", msg])
        .output()
        .expect("git commit");
    assert!(out.status.success(), "commit failed");
}

/// Vault with an old paragraph (committed long ago) and a fresh one added
/// in the most recent commit. Returns the tempdir.
fn make_history_vault() -> assert_fs::TempDir {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();

    tmp.child("Old.md")
        .write_str("# Old\n\nOld paragraph.\n")
        .unwrap();
    let repo = tmp.path();
    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.name", "T"]);
    run_git(repo, &["config", "user.email", "t@e.com"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    commit_all_dated(repo, "c1", "2025-01-01T00:00:00");

    // Newest commit adds a fresh note; leave its date to "now" so the
    // default 7d window covers it.
    tmp.child("New.md")
        .write_str("# New\n\nFresh paragraph.\n")
        .unwrap();
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "c2"]);
    tmp
}

#[test]
fn history_default_window_includes_recent_excludes_old() {
    let tmp = make_history_vault();
    let out = ft()
        .env("NO_COLOR", "1")
        .args(["--vault", tmp.path().to_str().unwrap(), "notes", "recent"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("Fresh paragraph."), "recent shown:\n{text}");
    assert!(text.contains("New"), "recent title shown:\n{text}");
    assert!(!text.contains("Old paragraph."), "old excluded:\n{text}");
}

#[test]
fn history_json_shape_omits_matched() {
    let tmp = make_history_vault();
    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "recent",
            "--range",
            "HEAD~1..HEAD",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<Value> = serde_json::from_slice(&out).expect("valid JSON");
    assert!(!rows.is_empty(), "expected entries");
    for r in &rows {
        assert!(r.get("date").is_some(), "date field present");
        assert!(r.get("source_title").is_some(), "source_title present");
        assert!(r.get("source_path").is_some(), "source_path present");
        assert!(r.get("section").is_some(), "section present");
        assert!(
            r.get("matched").is_none(),
            "history JSON must omit matched: {r}"
        );
    }
}

#[test]
fn history_window_flags_mutually_exclusive() {
    let tmp = make_history_vault();
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "notes",
        "recent",
        "--since",
        "7d",
        "--range",
        "HEAD~1..HEAD",
    ])
    .assert()
    .failure();
}

#[test]
fn history_excludes_synth_notes_by_default() {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    let repo = tmp.path();
    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.name", "T"]);
    run_git(repo, &["config", "user.email", "t@e.com"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    // Base commit so HEAD~1 exists and the window diffs against it.
    tmp.child("Seed.md").write_str("# Seed\n").unwrap();
    commit_all_dated(repo, "base", "2025-01-01T00:00:00");
    // Window commit adds both a plain and a synth note.
    tmp.child("Plain.md")
        .write_str("# Plain\n\nPlain body.\n")
        .unwrap();
    tmp.child("Synth.md")
        .write_str("---\nft-synth: true\n---\n\nSynth body.\n")
        .unwrap();
    commit_all_dated(repo, "c2", "2025-02-01T00:00:00");

    let default_out = ft()
        .env("NO_COLOR", "1")
        .args([
            "--vault",
            repo.to_str().unwrap(),
            "notes",
            "recent",
            "--range",
            "HEAD~1..HEAD",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let default_text = String::from_utf8(default_out).unwrap();
    assert!(default_text.contains("Plain body."), "{default_text}");
    assert!(
        !default_text.contains("Synth body."),
        "synth excluded by default:\n{default_text}"
    );

    let incl_out = ft()
        .env("NO_COLOR", "1")
        .args([
            "--vault",
            repo.to_str().unwrap(),
            "notes",
            "recent",
            "--range",
            "HEAD~1..HEAD",
            "--include-synth",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let incl_text = String::from_utf8(incl_out).unwrap();
    assert!(
        incl_text.contains("Synth body."),
        "--include-synth surfaces synth:\n{incl_text}"
    );
}

#[test]
fn history_no_color_has_no_ansi() {
    let tmp = make_history_vault();
    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "recent",
            "--no-color",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(!text.contains('\u{1b}'), "no ANSI escapes:\n{text}");
}

#[test]
fn history_non_git_vault_errors() {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("Note.md").write_str("# Note\n\nBody.\n").unwrap();
    ft().args(["--vault", tmp.path().to_str().unwrap(), "notes", "recent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not inside a git repository"));
}

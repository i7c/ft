//! Integration tests for `ft review`.

use assert_cmd::Command;
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

/// Two-commit fixture: baseline note in c1, two notes with wikilinks in c2.
fn make_review_vault() -> (assert_fs::TempDir, String) {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("baseline.md")
        .write_str("# Baseline\n\nNo links yet.\n")
        .unwrap();
    let repo = tmp.path();
    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.name", "T"]);
    run_git(repo, &["config", "user.email", "t@e.com"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "c1"]);

    let c1 = String::from_utf8(
        StdCommand::new("git")
            .current_dir(repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    tmp.child("note-a.md")
        .write_str("Para one mentions [[Foo]] and [[Bar]].\n\nPara two mentions [[Foo]] again.\n")
        .unwrap();
    tmp.child("note-b.md")
        .write_str("Only [[Bar]] here.\n")
        .unwrap();
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "c2"]);

    (tmp, c1)
}

#[test]
fn review_range_json_counts_paragraph_dedup() {
    let (tmp, c1) = make_review_vault();
    let range = format!("{c1}..HEAD");
    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "pulse",
            "--range",
            &range,
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<Value> = serde_json::from_slice(&out).expect("valid JSON");

    let mut by_target: std::collections::HashMap<String, (u64, bool)> = Default::default();
    for r in &rows {
        let target = r["target"].as_str().unwrap().to_string();
        let count = r["count"].as_u64().unwrap();
        let is_ghost = r["is_ghost"].as_bool().unwrap();
        by_target.insert(target, (count, is_ghost));
    }
    assert_eq!(by_target["Foo"], (2, true));
    assert_eq!(by_target["Bar"], (2, true));
}

#[test]
fn review_table_format_count_target_with_ghost_suffix() {
    let (tmp, c1) = make_review_vault();
    let range = format!("{c1}..HEAD");
    let out = ft()
        .env("NO_COLOR", "1")
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "pulse",
            "--range",
            &range,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    // Both are ghosts, count 2 each. Alphabetical tiebreak → Bar first.
    let first_line = text.lines().next().unwrap();
    assert!(first_line.contains("(2) [[Bar]]?"), "got: {first_line}");
    assert!(text.contains("(2) [[Foo]]?"));
}

#[test]
fn review_no_commits_in_window_prints_friendly_message() {
    let (tmp, _c1) = make_review_vault();
    // Range HEAD..HEAD has zero added lines.
    let out = ft()
        .env("NO_COLOR", "1")
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "pulse",
            "--range",
            "HEAD..HEAD",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("no new links in window"));
}

#[test]
fn review_mutually_exclusive_flags_rejected() {
    let (tmp, _c1) = make_review_vault();
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "notes",
        "pulse",
        "--since",
        "7d",
        "--range",
        "HEAD..HEAD",
    ])
    .assert()
    .failure();
}

#[test]
fn review_outside_git_repo_errors_clearly() {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("foo.md").write_str("just a note\n").unwrap();
    ft().args(["--vault", tmp.path().to_str().unwrap(), "notes", "pulse"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not inside a git repository"));
}

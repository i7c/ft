//! Integration tests for the multi-target `ft notes journal --link` flow.

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

fn make_multi_target_vault() -> assert_fs::TempDir {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("DailyA.md")
        .write_str("Some thought about [[Foo]].\n\nLater, [[Bar]] came up.\n")
        .unwrap();
    tmp.child("DailyB.md")
        .write_str("Cross-link: [[Foo]] and [[Bar]] in one paragraph.\n")
        .unwrap();
    let repo = tmp.path();
    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.name", "T"]);
    run_git(repo, &["config", "user.email", "t@e.com"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "init"]);
    tmp
}

#[test]
fn multi_link_json_includes_matched_field() {
    let tmp = make_multi_target_vault();
    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "journal",
            "--link",
            "[[Foo]]",
            "--link",
            "[[Bar]]",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<Value> = serde_json::from_slice(&out).expect("valid JSON");
    // Three paragraphs match overall: DailyA's two paragraphs (one Foo,
    // one Bar) and DailyB's single paragraph (both).
    assert_eq!(rows.len(), 3);
    let multi: Vec<&Value> = rows
        .iter()
        .filter(|r| r["matched"].as_array().unwrap().len() > 1)
        .collect();
    assert_eq!(multi.len(), 1, "exactly the DailyB paragraph matches both");
    let matched: Vec<&str> = multi[0]["matched"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(matched.contains(&"Foo") && matched.contains(&"Bar"));
}

#[test]
fn multi_link_table_shows_matched_badge_for_multi_match() {
    let tmp = make_multi_target_vault();
    let out = ft()
        .env("NO_COLOR", "1")
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "journal",
            "--link",
            "[[Foo]]",
            "--link",
            "[[Bar]]",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    // DailyB's paragraph matched both → badge present somewhere.
    assert!(
        text.contains("matched:") && text.contains("Foo") && text.contains("Bar"),
        "table missing `matched:` badge\n{text}"
    );
}

#[test]
fn note_and_link_are_mutually_exclusive() {
    let tmp = make_multi_target_vault();
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "notes",
        "journal",
        "DailyA",
        "--link",
        "[[Foo]]",
    ])
    .assert()
    .failure();
}

#[test]
fn in_window_requires_window_flag() {
    let tmp = make_multi_target_vault();
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "notes",
        "journal",
        "--link",
        "[[Foo]]",
        "--in-window",
    ])
    .assert()
    .failure()
    .stderr(predicates::str::contains("--in-window requires"));
}

#[test]
fn unknown_link_target_errors_with_clear_message() {
    let tmp = make_multi_target_vault();
    // [[Nonexistent]] is not even a ghost (no paragraph references it).
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "notes",
        "journal",
        "--link",
        "[[Nonexistent]]",
    ])
    .assert()
    .failure()
    .stderr(predicates::str::contains("did not resolve"));
}

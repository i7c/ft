//! Integration tests for `ft notes journal`.
//!
//! These tests build a tiny git-backed vault on the fly so blame data
//! has meaningful dates.

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::process::Command as StdCommand;

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

/// Run `git` against `dir`. Panics on non-zero exit.
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

/// Build a vault under a fresh temp dir with two commits.
/// Returns the tempdir so it lives for the duration of the test.
fn make_journal_vault() -> assert_fs::TempDir {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();

    tmp.child("Target.md")
        .write_str("# Target\n\n## Related\n- [[Bar]]\n")
        .unwrap();
    tmp.child("Bar.md").write_str("# Bar\n").unwrap();
    tmp.child("DailyA.md")
        .write_str("Note about [[Target]] today.\n")
        .unwrap();
    tmp.child("DailyB.md")
        .write_str("Followup about [[Bar]].\n")
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
fn journal_returns_expected_entries_in_json() {
    let tmp = make_journal_vault();
    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "journal",
            "Target",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Vec<Value> = serde_json::from_slice(&out).expect("valid JSON");
    let titles: Vec<&str> = json
        .iter()
        .map(|r| r["source_title"].as_str().unwrap())
        .collect();
    assert!(titles.contains(&"DailyA"), "got {titles:?}");
    assert!(titles.contains(&"DailyB"), "got {titles:?}");
    assert!(!titles.contains(&"Target"), "self-link excluded");
}

#[test]
fn journal_table_output_renders_entries() {
    let tmp = make_journal_vault();
    let out = ft()
        .env("NO_COLOR", "1")
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "journal",
            "Target",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("Note about [[Target]] today."));
    assert!(text.contains("Followup about [[Bar]]."));
    assert!(text.contains("DailyA"));
    assert!(text.contains("DailyB"));
}

#[test]
fn journal_unknown_note_exits_non_zero() {
    let tmp = make_journal_vault();
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "notes",
        "journal",
        "NoSuchNoteExists",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("no note found"));
}

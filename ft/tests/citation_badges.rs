//! Integration tests for citation badges on `ft notes journal` /
//! `ft notes history`: `cited:` / `cited*:` badge lines, the `cited_in`
//! JSON field, and the `--uncited` filter.
//!
//! Each test builds a git-backed vault, then creates the synth note
//! through the real `ft notes synth scaffold` flow so the callout pins
//! (commit SHA, content hash) are genuine.

use assert_cmd::Command;
use serde_json::Value;
use std::process::Command as StdCommand;

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

/// Run `git` against `dir` with a pinned author/committer date.
fn run_git(dir: &std::path::Path, date: &str, args: &[&str]) {
    let out = StdCommand::new("git")
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
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

/// Vault with two paragraphs mentioning `[[Topic]]` in `a.md`, committed
/// 2025-01-01, and a synth note citing only the first paragraph
/// (`a.md:1`), created via `ft synth scaffold` and committed 2025-01-02.
fn make_vault() -> assert_fs::TempDir {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();

    // Baseline commit predating any test window, so `--since` windows
    // can resolve a start commit.
    let repo = tmp.path();
    run_git(repo, "2024-12-01T00:00:00", &["init", "-b", "main"]);
    run_git(repo, "2024-12-01T00:00:00", &["config", "user.name", "T"]);
    run_git(
        repo,
        "2024-12-01T00:00:00",
        &["config", "user.email", "t@e.com"],
    );
    run_git(
        repo,
        "2024-12-01T00:00:00",
        &["config", "commit.gpgsign", "false"],
    );
    tmp.child("Topic.md").write_str("# Topic\n").unwrap();
    run_git(repo, "2024-12-01T00:00:00", &["add", "."]);
    run_git(repo, "2024-12-01T00:00:00", &["commit", "-m", "baseline"]);

    tmp.child("a.md")
        .write_str("Alpha paragraph about [[Topic]].\n\nBeta paragraph about [[Topic]].\n")
        .unwrap();
    run_git(repo, "2025-01-01T00:00:00", &["add", "."]);
    run_git(repo, "2025-01-01T00:00:00", &["commit", "-m", "init"]);

    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "notes",
        "synth",
        "scaffold",
        "Synth/topic.md",
        "--from",
        "a.md:1",
        "--no-edit",
    ])
    .assert()
    .success();
    run_git(repo, "2025-01-02T00:00:00", &["add", "."]);
    run_git(repo, "2025-01-02T00:00:00", &["commit", "-m", "synth"]);
    tmp
}

fn journal_json(tmp: &assert_fs::TempDir, extra: &[&str]) -> Vec<Value> {
    let mut args = vec![
        "--vault",
        tmp.path().to_str().unwrap(),
        "notes",
        "gather",
        "Topic",
        "--json",
    ];
    args.extend_from_slice(extra);
    let out = ft()
        .args(&args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).expect("valid JSON")
}

fn entry<'a>(rows: &'a [Value], needle: &str) -> &'a Value {
    rows.iter()
        .find(|r| r["section"].as_str().unwrap().contains(needle))
        .unwrap_or_else(|| panic!("no entry containing {needle:?} in {rows:?}"))
}

#[test]
fn journal_json_carries_cited_in() {
    let tmp = make_vault();
    let rows = journal_json(&tmp, &[]);

    let alpha = entry(&rows, "Alpha");
    let cited = alpha["cited_in"].as_array().unwrap();
    assert_eq!(cited.len(), 1, "{alpha:?}");
    assert_eq!(cited[0]["note"], "Synth/topic.md");
    assert_eq!(cited[0]["stale"], false);

    let beta = entry(&rows, "Beta");
    assert_eq!(beta["cited_in"].as_array().unwrap().len(), 0, "{beta:?}");
}

#[test]
fn journal_table_shows_cited_badge() {
    let tmp = make_vault();
    let out = ft()
        .env("NO_COLOR", "1")
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "gather",
            "Topic",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("cited: topic"), "{text}");
    insta::assert_snapshot!(text);
}

#[test]
fn journal_uncited_drops_cited_keeps_rest() {
    let tmp = make_vault();
    let rows = journal_json(&tmp, &["--uncited"]);
    let sections: Vec<&str> = rows
        .iter()
        .map(|r| r["section"].as_str().unwrap())
        .collect();
    assert!(
        sections.iter().all(|s| !s.contains("Alpha")),
        "{sections:?}"
    );
    assert!(sections.iter().any(|s| s.contains("Beta")), "{sections:?}");
}

#[test]
fn edited_since_cited_is_stale_and_survives_uncited() {
    use assert_fs::prelude::*;
    let tmp = make_vault();
    // Edit the cited paragraph and commit: exact match gone, line
    // overlap remains → stale.
    tmp.child("a.md")
        .write_str("Alpha paragraph about [[Topic]], edited.\n\nBeta paragraph about [[Topic]].\n")
        .unwrap();
    run_git(tmp.path(), "2025-01-03T00:00:00", &["add", "."]);
    run_git(tmp.path(), "2025-01-03T00:00:00", &["commit", "-m", "edit"]);

    let rows = journal_json(&tmp, &["--uncited"]);
    let alpha = entry(&rows, "Alpha");
    let cited = alpha["cited_in"].as_array().unwrap();
    assert_eq!(cited.len(), 1, "{alpha:?}");
    assert_eq!(cited[0]["stale"], true);

    let out = ft()
        .env("NO_COLOR", "1")
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "gather",
            "Topic",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("cited*: topic"), "{text}");
}

#[test]
fn history_json_and_uncited_match_journal_semantics() {
    let tmp = make_vault();
    let out = ft()
        .env("FT_TODAY", "2025-01-05")
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "recent",
            "--since",
            "30d",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<Value> = serde_json::from_slice(&out).expect("valid JSON");
    let alpha = entry(&rows, "Alpha");
    assert_eq!(alpha["cited_in"][0]["note"], "Synth/topic.md");
    let beta = entry(&rows, "Beta");
    assert_eq!(beta["cited_in"].as_array().unwrap().len(), 0);

    let out = ft()
        .env("FT_TODAY", "2025-01-05")
        .env("NO_COLOR", "1")
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "recent",
            "--since",
            "30d",
            "--uncited",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(!text.contains("Alpha"), "{text}");
    assert!(text.contains("Beta"), "{text}");
}

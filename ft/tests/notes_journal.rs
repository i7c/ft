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

// ── heading-section expansion ─────────────────────────────────────────

/// Build a vault where `Daily.md` has a heading-sited `[[Target]]` link
/// (`## Thoughts about [[Target]]`) followed by two sibling paragraphs
/// that do NOT repeat the link. Commits the heading paragraph on an
/// older date and the two siblings on a newer date so per-paragraph
/// dates are distinguishable and reverse-chronological order is
/// meaningful. Returns the tempdir.
fn make_heading_expansion_vault() -> assert_fs::TempDir {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("Target.md").write_str("# Target\n").unwrap();
    // Day 1: heading + Para A (merged via Fork A2 — no blank line).
    tmp.child("Daily.md")
        .write_str("# Day\n\n## Thoughts about [[Target]]\nPara A.\n")
        .unwrap();
    let repo = tmp.path();
    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.name", "T"]);
    run_git(repo, &["config", "user.email", "t@e.com"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    // Backdate the first commit so the heading paragraph is older.
    run_git_dated(repo, "2025-01-01T00:00:00", &["add", "."]);
    run_git_dated(repo, "2025-01-01T00:00:00", &["commit", "-m", "c1"]);
    // Day 2: append Para B and Para C (siblings under the same heading,
    // no link to Target).
    tmp.child("Daily.md")
        .write_str("# Day\n\n## Thoughts about [[Target]]\nPara A.\n\nPara B.\n\nPara C.\n")
        .unwrap();
    run_git_dated(repo, "2025-02-01T00:00:00", &["add", "."]);
    run_git_dated(repo, "2025-02-01T00:00:00", &["commit", "-m", "c2"]);
    tmp
}

/// Like [`run_git`] but pins the author/committer date so blame yields a
/// deterministic per-paragraph date.
fn run_git_dated(dir: &std::path::Path, date: &str, args: &[&str]) {
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
        "git {args:?} (date {date}) failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn journal_heading_link_expands_sibling_paragraphs_table() {
    let tmp = make_heading_expansion_vault();
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
    // All three sibling paragraphs appear (the heading paragraph Para A,
    // plus the link-less siblings Para B and Para C).
    assert!(text.contains("Para A"), "missing Para A\n{text}");
    assert!(text.contains("Para B"), "missing Para B\n{text}");
    assert!(text.contains("Para C"), "missing Para C\n{text}");
    // Reverse-chronological by paragraph date: Para B/C (2025-02-01)
    // come before Para A (2025-01-01).
    let b = text.find("Para B").unwrap();
    let a = text.find("Para A").unwrap();
    assert!(b < a, "expected newer Para B before older Para A\n{text}");
}

#[test]
fn journal_heading_link_expansion_json_one_entry_per_paragraph() {
    let tmp = make_heading_expansion_vault();
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
    let rows: Vec<Value> = serde_json::from_slice(&out).expect("valid JSON");
    // One entry per paragraph: the heading paragraph plus two link-less
    // siblings expanded in by the heading link.
    assert_eq!(rows.len(), 3, "got {:?}", rows);
    let sections: Vec<&str> = rows
        .iter()
        .map(|r| r["section"].as_str().unwrap())
        .collect();
    assert!(
        sections.iter().any(|s| s.contains("Para A")),
        "{sections:?}"
    );
    assert!(
        sections.iter().any(|s| s.contains("Para B")),
        "{sections:?}"
    );
    assert!(
        sections.iter().any(|s| s.contains("Para C")),
        "{sections:?}"
    );
    // Single-target mode: every entry's matched is ["Target"].
    for r in &rows {
        let matched: Vec<&str> = r["matched"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(matched, vec!["Target"], "got {matched:?}");
    }
    // Per-paragraph dates: Para B/C on 2025-02-01, Para A on 2025-01-01.
    let dates: Vec<&str> = rows.iter().map(|r| r["date"].as_str().unwrap()).collect();
    assert!(dates.contains(&"2025-02-01"), "{dates:?}");
    assert!(dates.contains(&"2025-01-01"), "{dates:?}");
}

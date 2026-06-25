//! Integration tests for `ft tasks cancel` and `ft tasks complete --query`
//! (graph-task-interaction §4).

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

fn vault() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir
}

fn run(vault: &std::path::Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut full = vec!["--vault", vault.to_str().unwrap(), "tasks"];
    full.extend(args);
    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-05-10")
        .args(&full)
        .assert()
}

#[test]
fn cancel_by_id_marks_cancelled() {
    let dir = vault();
    dir.child("notes.md")
        .write_str("- [ ] Buy milk 📅 2026-05-10 🆔 abc123\n")
        .unwrap();

    run(dir.path(), &["cancel", "abc123", "--yes"]).success();

    let content = std::fs::read_to_string(dir.path().join("notes.md")).unwrap();
    assert!(
        content.contains("- [-]"),
        "expected cancelled checkbox, got: {content}"
    );
    assert!(
        content.contains("❌ 2026-05-10"),
        "expected cancelled date, got: {content}"
    );
}

#[test]
fn cancel_by_file_line_marks_cancelled() {
    let dir = vault();
    dir.child("notes/inbox.md")
        .write_str("# Inbox\n- [ ] First\n- [ ] Second\n")
        .unwrap();

    run(dir.path(), &["cancel", "notes/inbox.md:3", "--yes"]).success();

    let content = std::fs::read_to_string(dir.path().join("notes/inbox.md")).unwrap();
    assert!(
        content.lines().nth(2).unwrap().starts_with("- [-]"),
        "expected line 3 cancelled, got: {content}"
    );
}

#[test]
fn cancel_with_on_date_overrides_today() {
    let dir = vault();
    dir.child("notes.md")
        .write_str("- [ ] Task 🆔 xyz\n")
        .unwrap();

    run(
        dir.path(),
        &["cancel", "xyz", "--on", "2026-06-01", "--yes"],
    )
    .success();

    let content = std::fs::read_to_string(dir.path().join("notes.md")).unwrap();
    assert!(
        content.contains("❌ 2026-06-01"),
        "expected overridden cancelled date, got: {content}"
    );
}

#[test]
fn cancel_already_cancelled_is_skipped_in_bulk() {
    let dir = vault();
    dir.child("notes.md")
        .write_str("- [-] Done ❌ 2026-05-01\n- [ ] Open 🆔 a1\n")
        .unwrap();

    // Bulk via --query: the already-cancelled task is skipped, not an error.
    run(
        dir.path(),
        &["cancel", "--query", "path includes \"notes\"", "--yes"],
    )
    .success()
    .stdout(
        predicate::str::contains("1 task(s)").or(predicate::str::contains("1 already cancelled")),
    );

    let content = std::fs::read_to_string(dir.path().join("notes.md")).unwrap();
    // The open one is now cancelled; the already-cancelled one is unchanged.
    assert!(
        content.lines().nth(1).unwrap().starts_with("- [-]"),
        "expected second line cancelled, got: {content}"
    );
}

#[test]
fn complete_query_bulk_marks_all_matching() {
    let dir = vault();
    dir.child("notes.md")
        .write_str("- [ ] A 📅 2026-05-10\n- [ ] B 📅 2026-05-10\n")
        .unwrap();

    run(
        dir.path(),
        &["complete", "--query", "due = 2026-05-10", "--yes"],
    )
    .success()
    .stdout(predicate::str::contains("Completed"));

    let content = std::fs::read_to_string(dir.path().join("notes.md")).unwrap();
    assert!(
        content.lines().all(|l| l.starts_with("- [x]")),
        "expected all tasks done, got: {content}"
    );
}

#[test]
fn complete_query_no_match_errors() {
    let dir = vault();
    dir.child("notes.md").write_str("- [ ] A\n").unwrap();

    run(
        dir.path(),
        &["complete", "--query", "due = 1999-01-01", "--yes"],
    )
    .failure()
    .stderr(predicate::str::contains("no tasks match query"));
}

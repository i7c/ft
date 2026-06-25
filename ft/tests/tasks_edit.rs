//! Integration tests for `ft tasks edit` (graph-task-interaction §4).

use assert_cmd::Command;
use assert_fs::prelude::*;

fn vault() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir
}

fn run(vault: &std::path::Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut full = vec!["--vault", vault.to_str().unwrap(), "tasks", "edit"];
    full.extend(args);
    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-05-10")
        .args(&full)
        .assert()
}

#[test]
fn edit_sets_due_date() {
    let dir = vault();
    dir.child("notes.md")
        .write_str("- [ ] Task 🆔 a1\n")
        .unwrap();

    run(dir.path(), &["a1", "--due", "2026-07-01"]).success();

    let content = std::fs::read_to_string(dir.path().join("notes.md")).unwrap();
    assert!(
        content.contains("📅 2026-07-01"),
        "expected due date set, got: {content}"
    );
}

#[test]
fn edit_clears_due_date() {
    let dir = vault();
    dir.child("notes.md")
        .write_str("- [ ] Task 📅 2026-05-10 🆔 a1\n")
        .unwrap();

    run(dir.path(), &["a1", "--due", "none"]).success();

    let content = std::fs::read_to_string(dir.path().join("notes.md")).unwrap();
    assert!(
        !content.contains("📅"),
        "expected due date cleared, got: {content}"
    );
}

#[test]
fn edit_sets_priority() {
    let dir = vault();
    dir.child("notes.md")
        .write_str("- [ ] Task 🆔 a1\n")
        .unwrap();

    run(dir.path(), &["a1", "--priority", "high"]).success();

    let content = std::fs::read_to_string(dir.path().join("notes.md")).unwrap();
    assert!(
        content.contains("⏫"),
        "expected high priority emoji, got: {content}"
    );
}

#[test]
fn edit_clears_priority() {
    let dir = vault();
    dir.child("notes.md")
        .write_str("- [ ] Task ⏫ 🆔 a1\n")
        .unwrap();

    run(dir.path(), &["a1", "--priority", "none"]).success();

    let content = std::fs::read_to_string(dir.path().join("notes.md")).unwrap();
    assert!(
        !content.contains("⏫"),
        "expected priority cleared, got: {content}"
    );
}

#[test]
fn edit_sets_description() {
    let dir = vault();
    dir.child("notes.md")
        .write_str("- [ ] old text 🆔 a1\n")
        .unwrap();

    run(dir.path(), &["a1", "--description", "new text"]).success();

    let content = std::fs::read_to_string(dir.path().join("notes.md")).unwrap();
    assert!(
        content.contains("new text"),
        "expected description updated, got: {content}"
    );
    assert!(
        !content.contains("old text"),
        "expected old description gone, got: {content}"
    );
}

#[test]
fn edit_no_fields_errors() {
    let dir = vault();
    dir.child("notes.md")
        .write_str("- [ ] Task 🆔 a1\n")
        .unwrap();

    run(dir.path(), &["a1"]).failure();
}

#[test]
fn edit_ambiguous_selector_errors() {
    let dir = vault();
    dir.child("notes.md")
        .write_str("- [ ] Task 🆔 a1\n- [ ] Task 🆔 a2\n")
        .unwrap();

    run(dir.path(), &["Task"]).failure();
}

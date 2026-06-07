//! Integration tests for `ft graph delete` — note and directory deletion.

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

fn vault() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir
}

fn ft() -> Command {
    let mut cmd = Command::cargo_bin("ft").unwrap();
    cmd.env("FT_TODAY", "2026-05-13");
    cmd
}

#[test]
fn delete_note() {
    let dir = vault();
    dir.child("notes/hello.md").write_str("# hello\n").unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "graph",
        "delete",
        "notes/hello.md",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("deleted note notes/hello.md"));

    assert!(!dir.child("notes/hello.md").path().exists());
}

#[test]
fn delete_directory() {
    let dir = vault();
    dir.child("archive/a.md").write_str("a").unwrap();
    dir.child("archive/sub/b.md").write_str("b").unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "graph",
        "delete",
        "archive",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("deleted archive"));

    assert!(!dir.child("archive").path().exists());
}

#[test]
fn delete_nonexistent_errors() {
    let dir = vault();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "graph",
        "delete",
        "nonexistent.md",
    ])
    .assert()
    .failure();
}

#[test]
fn delete_requires_path() {
    let dir = vault();

    ft().args(["--vault", dir.path().to_str().unwrap(), "graph", "delete"])
        .assert()
        .failure();
}

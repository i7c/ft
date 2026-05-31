//! Integration tests for `ft notes mv` (cli-rename-mv-clean-split).

use assert_cmd::Command;
use assert_fs::prelude::*;
use assert_fs::TempDir;
use predicates::prelude::*;

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

fn make_vault(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    for (rel, content) in files {
        dir.child(rel).write_str(content).unwrap();
    }
    dir
}

fn read(dir: &TempDir, rel: &str) -> String {
    std::fs::read_to_string(dir.child(rel).path()).unwrap()
}

// ── happy path ───────────────────────────────────────────────────────────────

#[test]
fn mv_single_note_to_directory() {
    let v = make_vault(&[("foo.md", "# Foo\n"), ("a.md", "see [[foo]]\n")]);
    std::fs::create_dir_all(v.path().join("archive")).unwrap();
    ft().args([
        "--vault",
        v.path().to_str().unwrap(),
        "notes",
        "mv",
        "foo.md",
        "archive",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("moved 1 note(s) to archive"));
    assert!(!v.child("foo.md").path().exists());
    assert!(v.child("archive/foo.md").path().exists());
    assert_eq!(read(&v, "a.md"), "see [[foo]]\n");
}

#[test]
fn mv_multiple_notes_to_directory() {
    let v = make_vault(&[("a.md", "# A\n"), ("b.md", "# B\n")]);
    std::fs::create_dir_all(v.path().join("target")).unwrap();
    ft().args([
        "--vault",
        v.path().to_str().unwrap(),
        "notes",
        "mv",
        "a.md",
        "b.md",
        "target",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("moved 2 note(s) to target"));
    assert!(!v.child("a.md").path().exists());
    assert!(!v.child("b.md").path().exists());
    assert!(v.child("target/a.md").path().exists());
    assert!(v.child("target/b.md").path().exists());
}

#[test]
fn mv_directory_moves_all_files() {
    let v = make_vault(&[
        ("projects/old/alpha.md", "# Alpha\n"),
        ("projects/old/beta.md", "# Beta\n"),
        ("external.md", "see [[projects/old/alpha]]\n"),
    ]);
    std::fs::create_dir_all(v.path().join("archive")).unwrap();
    ft().args([
        "--vault",
        v.path().to_str().unwrap(),
        "notes",
        "mv",
        "projects/old",
        "archive",
    ])
    .assert()
    .success();
    assert!(v.child("archive/old/alpha.md").path().exists());
    assert!(v.child("archive/old/beta.md").path().exists());
    // Old directory cleaned up.
    assert!(!v.child("projects/old").path().exists());
    // External reference updated.
    assert_eq!(read(&v, "external.md"), "see [[archive/old/alpha]]\n");
}

#[test]
fn mv_mixed_notes_and_directory() {
    let v = make_vault(&[("top.md", "# Top\n"), ("sub/deep.md", "# Deep\n")]);
    std::fs::create_dir_all(v.path().join("out")).unwrap();
    ft().args([
        "--vault",
        v.path().to_str().unwrap(),
        "notes",
        "mv",
        "top.md",
        "sub",
        "out",
    ])
    .assert()
    .success();
    assert!(v.child("out/top.md").path().exists());
    assert!(v.child("out/sub/deep.md").path().exists());
}

#[test]
fn mv_source_without_md_extension() {
    let v = make_vault(&[("foo.md", "# Foo\n")]);
    std::fs::create_dir_all(v.path().join("out")).unwrap();
    ft().args([
        "--vault",
        v.path().to_str().unwrap(),
        "notes",
        "mv",
        "foo", // no .md
        "out",
    ])
    .assert()
    .success();
    assert!(v.child("out/foo.md").path().exists());
}

// ── dry-run ──────────────────────────────────────────────────────────────────

#[test]
fn mv_dry_run_prints_plan_and_modifies_nothing() {
    let v = make_vault(&[("foo.md", "# Foo\n"), ("a.md", "[[foo]]\n")]);
    std::fs::create_dir_all(v.path().join("archive")).unwrap();
    ft().args([
        "--vault",
        v.path().to_str().unwrap(),
        "notes",
        "mv",
        "foo.md",
        "archive",
        "--dry-run",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("would move"));
    // File NOT moved.
    assert!(v.child("foo.md").path().exists());
    assert!(!v.child("archive/foo.md").path().exists());
}

// ── error paths ──────────────────────────────────────────────────────────────

#[test]
fn mv_source_not_found_errors() {
    let v = make_vault(&[("foo.md", "# Foo\n")]);
    std::fs::create_dir_all(v.path().join("target")).unwrap();
    ft().args([
        "--vault",
        v.path().to_str().unwrap(),
        "notes",
        "mv",
        "nonexistent.md",
        "target",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("source not found"));
}

#[test]
fn mv_target_not_a_directory_errors() {
    let v = make_vault(&[("foo.md", "# Foo\n"), ("bar.md", "# Bar\n")]);
    ft().args([
        "--vault",
        v.path().to_str().unwrap(),
        "notes",
        "mv",
        "foo.md",
        "bar.md", // file, not dir
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("is not a directory"));
}

#[test]
fn mv_target_does_not_exist_errors() {
    let v = make_vault(&[("foo.md", "# Foo\n")]);
    ft().args([
        "--vault",
        v.path().to_str().unwrap(),
        "notes",
        "mv",
        "foo.md",
        "newdir",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("target directory not found"));
}

#[test]
fn mv_missing_arguments_errors() {
    let v = make_vault(&[("foo.md", "# Foo\n")]);
    ft().args([
        "--vault",
        v.path().to_str().unwrap(),
        "notes",
        "mv",
        "foo.md",
        // no target
    ])
    .assert()
    .failure();
}

#[test]
fn mv_cross_reference_md_link_preserved() {
    let v = make_vault(&[("x.md", "# X\nsee [other note](y.md)\n"), ("y.md", "# Y\n")]);
    std::fs::create_dir_all(v.path().join("sub")).unwrap();
    ft().args([
        "--vault",
        v.path().to_str().unwrap(),
        "notes",
        "mv",
        "x.md",
        "y.md",
        "sub",
    ])
    .assert()
    .success();
    assert_eq!(read(&v, "sub/x.md"), "# X\nsee [other note](y.md)\n");
}

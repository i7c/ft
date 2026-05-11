//! Integration tests for `ft notes move-section`.
//!
//! Each test seeds an `assert_fs::TempDir` vault, then drives the CLI
//! with `--yes` so the confirm prompt is skipped (the non-TTY behavior
//! has its own dedicated test).

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

fn vault() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir
}

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

/// Read a file under the vault as a string.
fn read(vault: &std::path::Path, rel: &str) -> String {
    std::fs::read_to_string(vault.join(rel)).unwrap()
}

#[test]
fn move_single_heading_succeeds() {
    let dir = vault();
    dir.child("src.md")
        .write_str("## Keep\nA\n\n## Move\nB\n")
        .unwrap();
    dir.child("dst.md")
        .write_str("# Target\n\n## Existing\nX\n")
        .unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "move-section",
        "--from",
        "src.md",
        "--to",
        "dst.md",
        "--heading",
        "Move",
        "--yes",
    ])
    .assert()
    .success();

    let src = read(dir.path(), "src.md");
    let dst = read(dir.path(), "dst.md");
    assert!(!src.contains("## Move"), "source still has section:\n{src}");
    assert!(src.contains("## Keep"), "source lost Keep:\n{src}");
    assert!(dst.contains("## Move"), "target missing section:\n{dst}");
    assert!(dst.contains("## Existing"), "target lost Existing:\n{dst}");
}

#[test]
fn ambiguous_heading_default_policy_errors_with_line_numbers() {
    let dir = vault();
    dir.child("src.md")
        .write_str("## Notes\nA\n\n## Other\nX\n\n## Notes\nB\n")
        .unwrap();
    dir.child("dst.md").write_str("# Target\n").unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "move-section",
        "--from",
        "src.md",
        "--to",
        "dst.md",
        "--heading",
        "Notes",
        "--yes",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("matched 2 headings"))
    .stderr(predicate::str::contains("lines 1, 7"));
}

#[test]
fn match_policy_first_resolves_ambiguity() {
    let dir = vault();
    dir.child("src.md")
        .write_str("## Notes\nA\n\n## Notes\nB\n")
        .unwrap();
    dir.child("dst.md").write_str("# Target\n").unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "move-section",
        "--from",
        "src.md",
        "--to",
        "dst.md",
        "--heading",
        "Notes",
        "--match-policy",
        "first",
        "--yes",
    ])
    .assert()
    .success();

    let src = read(dir.path(), "src.md");
    let dst = read(dir.path(), "dst.md");
    // Only the FIRST `## Notes` (with body "A") should have moved.
    assert!(
        dst.contains("## Notes\nA\n"),
        "target should contain first section:\n{dst}"
    );
    assert!(
        src.contains("## Notes\nB\n"),
        "source should still have second section:\n{src}"
    );
}

#[test]
fn match_policy_all_moves_every_match() {
    let dir = vault();
    dir.child("src.md")
        .write_str("## Notes\nA\n\n## Notes\nB\n")
        .unwrap();
    dir.child("dst.md").write_str("# Target\n").unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "move-section",
        "--from",
        "src.md",
        "--to",
        "dst.md",
        "--heading",
        "Notes",
        "--match-policy",
        "all",
        "--yes",
    ])
    .assert()
    .success();

    let src = read(dir.path(), "src.md");
    let dst = read(dir.path(), "dst.md");
    assert!(
        !src.contains("## Notes"),
        "source should be empty of Notes:\n{src}"
    );
    assert_eq!(
        dst.matches("## Notes").count(),
        2,
        "target should have two Notes sections:\n{dst}"
    );
}

#[test]
fn heading_regex_matches_multiple() {
    let dir = vault();
    dir.child("src.md")
        .write_str("## Meeting A\nA\n\n## Other\nO\n\n## Meeting B\nB\n")
        .unwrap();
    dir.child("dst.md").write_str("# Target\n").unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "move-section",
        "--from",
        "src.md",
        "--to",
        "dst.md",
        "--heading-regex",
        "^Meeting",
        "--match-policy",
        "all",
        "--yes",
    ])
    .assert()
    .success();

    let dst = read(dir.path(), "dst.md");
    let src = read(dir.path(), "src.md");
    assert!(dst.contains("## Meeting A"), "{dst}");
    assert!(dst.contains("## Meeting B"), "{dst}");
    assert!(src.contains("## Other"), "Other should stay:\n{src}");
    assert!(
        !src.contains("## Meeting"),
        "Meetings should be gone:\n{src}"
    );
}

#[test]
fn at_level_shifts_target_heading() {
    let dir = vault();
    dir.child("src.md")
        .write_str("## Move\nbody\n\n### Child\nnested\n")
        .unwrap();
    dir.child("dst.md").write_str("# Target\n").unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "move-section",
        "--from",
        "src.md",
        "--to",
        "dst.md",
        "--heading",
        "Move",
        "--at-level",
        "3",
        "--yes",
    ])
    .assert()
    .success();

    let dst = read(dir.path(), "dst.md");
    assert!(dst.contains("### Move"), "expected H2→H3 shift:\n{dst}");
    assert!(dst.contains("#### Child"), "expected cascade H3→H4:\n{dst}");
}

#[test]
fn at_level_cascade_overflow_exits_nonzero() {
    let dir = vault();
    dir.child("src.md")
        .write_str("## Move\nbody\n\n##### Deep\nnested\n")
        .unwrap();
    dir.child("dst.md").write_str("# Target\n").unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "move-section",
        "--from",
        "src.md",
        "--to",
        "dst.md",
        "--heading",
        "Move",
        "--at-level",
        "4", // shift +2 → ##### (5) becomes ####### which exceeds 6
        "--yes",
    ])
    .assert()
    .failure();

    // Files unchanged.
    assert!(read(dir.path(), "src.md").contains("## Move"));
    assert!(!read(dir.path(), "dst.md").contains("Move"));
}

#[test]
fn from_query_resolves_via_fuzzy() {
    let dir = vault();
    dir.child("daily-2026-05-11.md")
        .write_str("## Standup\nx\n\n## Notes\ny\n")
        .unwrap();
    dir.child("dst.md").write_str("# Target\n").unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "move-section",
        "--from-query",
        "daily#notes",
        "--to",
        "dst.md",
        "--yes",
    ])
    .assert()
    .success();

    let src = read(dir.path(), "daily-2026-05-11.md");
    let dst = read(dir.path(), "dst.md");
    assert!(
        dst.contains("## Notes"),
        "Notes should have moved to target:\n{dst}"
    );
    assert!(
        !src.contains("## Notes"),
        "source should no longer have Notes:\n{src}"
    );
    assert!(src.contains("## Standup"), "Standup should remain:\n{src}");
}

#[test]
fn after_places_section_in_target_position() {
    let dir = vault();
    dir.child("src.md").write_str("## Move\nMOVED\n").unwrap();
    dir.child("dst.md")
        .write_str("# Target\n\n## Background\nbg\n\n## Tasks\nt\n")
        .unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "move-section",
        "--from",
        "src.md",
        "--to",
        "dst.md",
        "--heading",
        "Move",
        "--after",
        "Background",
        "--yes",
    ])
    .assert()
    .success();

    let dst = read(dir.path(), "dst.md");
    let bg = dst.find("## Background").unwrap();
    let mv = dst.find("## Move").unwrap();
    let tasks = dst.find("## Tasks").unwrap();
    assert!(
        bg < mv && mv < tasks,
        "expected Background < Move < Tasks order:\n{dst}"
    );
}

#[test]
fn missing_after_inserts_at_top_of_target() {
    let dir = vault();
    dir.child("src.md").write_str("## Move\nMOVED\n").unwrap();
    dir.child("dst.md")
        .write_str("Some prose.\n\n## Existing\nx\n")
        .unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "move-section",
        "--from",
        "src.md",
        "--to",
        "dst.md",
        "--heading",
        "Move",
        "--yes",
    ])
    .assert()
    .success();

    let dst = read(dir.path(), "dst.md");
    let mv = dst.find("## Move").unwrap();
    let existing = dst.find("## Existing").unwrap();
    assert!(
        mv < existing,
        "Move should be placed above Existing:\n{dst}"
    );
}

#[test]
fn non_tty_without_yes_exits_two() {
    let dir = vault();
    dir.child("src.md").write_str("## Move\nMOVED\n").unwrap();
    dir.child("dst.md").write_str("# Target\n").unwrap();

    // assert_cmd uses piped stdin by default → non-TTY. Without --yes
    // this should error rather than block.
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "move-section",
        "--from",
        "src.md",
        "--to",
        "dst.md",
        "--heading",
        "Move",
        // no --yes
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("non-TTY"));

    // Source file unchanged.
    assert!(read(dir.path(), "src.md").contains("## Move"));
}

#[test]
fn same_file_move_exits_nonzero() {
    let dir = vault();
    dir.child("only.md")
        .write_str("## Move\nx\n\n## Keep\ny\n")
        .unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "move-section",
        "--from",
        "only.md",
        "--to",
        "only.md",
        "--heading",
        "Move",
        "--yes",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("same file"));
}

#[test]
fn no_match_exits_one() {
    let dir = vault();
    dir.child("src.md").write_str("## Real\nx\n").unwrap();
    dir.child("dst.md").write_str("# Target\n").unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "move-section",
        "--from",
        "src.md",
        "--to",
        "dst.md",
        "--heading",
        "Nonexistent",
        "--yes",
    ])
    .assert()
    .code(1);
}

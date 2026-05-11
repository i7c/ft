//! Integration tests for `ft notes open` — fuzzy-find a note (or
//! heading) and dispatch to `$EDITOR` or Obsidian.

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

#[test]
fn open_top_hit_invokes_editor_with_line_and_path() {
    // Seed a vault with two notes; the top hit for "finance" should be
    // `finance.md`. We use `echo` as the editor: it echos the assembled
    // args (`+<line> <path>`) to stdout, which assert_cmd captures.
    let dir = vault();
    dir.child("finance.md")
        .write_str("# Finance\n\nSome notes.\n")
        .unwrap();
    dir.child("travel.md")
        .write_str("# Travel\n\nUnrelated.\n")
        .unwrap();

    let out = ft()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "open",
            "finance",
            "--editor",
            "echo",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(
        stdout.contains("+1"),
        "no heading in query → line defaults to 1: {stdout:?}"
    );
    assert!(
        stdout.contains("finance.md"),
        "expected path in editor invocation: {stdout:?}"
    );
}

#[test]
fn open_with_heading_part_jumps_to_heading_line() {
    let dir = vault();
    dir.child("project.md")
        .write_str("# Project\n\n## Background\n\nIntro.\n\n## Tasks\n\n- Do thing\n")
        .unwrap();

    let out = ft()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "open",
            "project#tasks",
            "--editor",
            "echo",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    // `## Tasks` is on line 7 in the seeded file.
    assert!(
        stdout.contains("+7"),
        "expected editor to jump to heading line 7: {stdout:?}"
    );
    assert!(stdout.contains("project.md"), "stdout: {stdout:?}");
}

#[test]
fn open_obsidian_dry_run_emits_url_on_stdout() {
    let dir = vault();
    dir.child("finance.md").write_str("# Finance\n").unwrap();

    let out = ft()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "open",
            "finance",
            "--obsidian",
        ])
        .env("FT_OBSIDIAN_DRY_RUN", "1")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(
        stdout.starts_with("obsidian://open?vault="),
        "expected obsidian URL: {stdout:?}"
    );
    assert!(
        stdout.contains("file=finance.md"),
        "expected file param: {stdout:?}"
    );
}

#[test]
fn open_obsidian_with_heading_appends_heading_param() {
    let dir = vault();
    dir.child("project.md")
        .write_str("# Project\n\n## Tasks\n")
        .unwrap();

    let out = ft()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "open",
            "project#tasks",
            "--obsidian",
        ])
        .env("FT_OBSIDIAN_DRY_RUN", "1")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(
        stdout.contains("&heading=Tasks"),
        "expected heading param in URL: {stdout:?}"
    );
}

#[test]
fn open_no_match_exits_one() {
    let dir = vault();
    dir.child("only.md").write_str("# Only\n").unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "open",
        "zzzzzzzzzz",
        "--editor",
        "echo",
    ])
    .assert()
    .code(1)
    .stderr(predicate::str::contains("no match"));
}

#[test]
fn open_missing_query_exits_two() {
    let dir = vault();
    // clap's `required = true` rejects an empty positional, so the
    // process exits 2 with usage text on stderr.
    ft().args(["--vault", dir.path().to_str().unwrap(), "notes", "open"])
        .assert()
        .code(2);
}

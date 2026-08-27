//! Integration tests for `ft notes search`.

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;
use serde_json::Value;

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

/// Fixture vault with a spread of paragraphs exercising every mode.
fn make_vault() -> assert_fs::TempDir {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("notes/a.md")
        .write_str(
            "The eigen decomposition is central.\n\
             \n\
             We mention [[Eigen Decomposition]] in passing.\n\
             \n\
             Memoization strategy pays off here.\n\
             \n\
             prose about memoization only\n",
        )
        .unwrap();
    tmp.child("notes/b.md")
        .write_str("Eigenvalue bounds and trigram candidate filtering.\n")
        .unwrap();
    tmp.child("journal/2026-05-08.md")
        .write_str("Eigen daily note, noisy.\n")
        .unwrap();
    tmp
}

#[test]
fn substring_default_matches_fragments() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "eigen", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Eigenvalue bounds"));
    assert!(stdout.contains("eigen decomposition is central"));
    // The `[[Eigen Decomposition]]` mention contains the substring too.
    assert!(stdout.contains("in passing"));
}

#[test]
fn word_mode_requires_whole_token() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "=eigen", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("eigen decomposition is central"));
    assert!(
        !stdout.contains("Eigenvalue"),
        "=eigen must not match eigenvalue"
    );
}

#[test]
fn link_clause_with_spaces() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "[[Eigen Decomposition]]", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("in passing"));
    assert!(!stdout.contains("Memoization"));
    // One result only: the paragraph carrying the link.
    assert_eq!(stdout.lines().count(), 1);
}

#[test]
fn and_requires_all_terms_in_same_paragraph() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "eigen memoization", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.is_empty(), "no paragraph contains both: {stdout}");
}

#[test]
fn any_mode_unions() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "eigen memoization", "--any", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Memoization strategy"));
    assert!(stdout.contains("eigen decomposition is central"));
}

#[test]
fn fuzzy_tolerates_typos() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "~memoizaton", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Memoization strategy"));
}

#[test]
fn phrase_requires_contiguous() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "\"trigram candidate\"", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Eigenvalue bounds"));
}

#[test]
fn exclude_filters_after_matching() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "eigen -decomposition", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Eigenvalue bounds"));
    assert!(!stdout.contains("eigen decomposition is central"));
}

#[test]
fn exclude_prefixes_skip_noisy_folders() {
    let tmp = make_vault();
    tmp.child(".ft/config.toml")
        .write_str("[synth]\nexclude_prefixes = [\"journal/\"]\n")
        .unwrap();
    let out = ft()
        .args(["notes", "search", "eigen", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(!stdout.contains("daily note"), "journal/ must be excluded");
}

#[test]
fn limit_caps_results() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "eigen", "--limit", "1", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(stdout.lines().count(), 1);
}

#[test]
fn json_shape() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "eigen", "--json", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty());
    let first = &arr[0];
    assert!(first.get("path").unwrap().is_string());
    assert_eq!(first.get("line_start").unwrap().as_u64(), Some(1));
    assert!(first.get("body").unwrap().is_string());
    assert!(first.get("matched").unwrap().is_array());
    assert!(first.get("score").unwrap().is_number());
    assert!(first.get("date").is_none(), "date only with --sort date");
}

#[test]
fn json_includes_date_with_sort_date() {
    let tmp = make_vault();
    // git repo so blame can resolve dates.
    let repo = tmp.path();
    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .current_dir(repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}");
    };
    run(&["init", "-b", "main"]);
    run(&["config", "user.name", "T"]);
    run(&["config", "user.email", "t@e.com"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["config", "maintenance.auto", "false"]);
    run(&["add", "."]);
    run(&["commit", "-m", "c1"]);

    let out = ft()
        .args([
            "notes", "search", "eigen", "--sort", "date", "--json", "--vault",
        ])
        .arg(tmp.path())
        .assert()
        .success();
    let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    for row in v.as_array().unwrap() {
        assert!(row.get("date").unwrap().is_string(), "date present per row");
    }
}

#[test]
fn empty_query_prints_nothing() {
    let tmp = make_vault();
    ft().args(["notes", "search", "", "--vault"])
        .arg(tmp.path())
        .assert()
        .success()
        .stdout("");
}

#[test]
fn no_match_prints_nothing_exit_zero() {
    let tmp = make_vault();
    ft().args(["notes", "search", "zebra", "--vault"])
        .arg(tmp.path())
        .assert()
        .success()
        .stdout("");
}

#[test]
fn removed_gather_subcommand_is_unknown() {
    // `ft notes gather` (and its `journal` alias) were removed with the
    // deprecated gather feed; the subcommand must not exist anymore.
    let tmp = make_vault();
    ft().args(["notes", "gather", "a", "--vault"])
        .arg(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
    ft().args(["notes", "journal", "a", "--vault"])
        .arg(tmp.path())
        .assert()
        .failure();
}

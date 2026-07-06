//! Integration tests for `ft notes ghosts` — the vault-wide ranked
//! ghost list. Pure graph: fixtures need no git repository.

use assert_cmd::Command;
use assert_fs::prelude::*;
use serde_json::Value;

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

/// Ghosts with 5, 3, and 1 distinct-paragraph mentions (`five`,
/// `three`, `one`), plus a resolved link that must not rank.
fn make_vault() -> assert_fs::TempDir {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("real.md").write_str("# real\n").unwrap();
    let mut a = String::from("see [[real]]\n");
    for i in 0..5 {
        a.push_str(&format!("\npara {i} about [[five]]\n"));
    }
    for i in 0..3 {
        a.push_str(&format!("\npara {i} about [[three]]\n"));
    }
    // Multiple mentions in ONE paragraph — must count once.
    a.push_str("\n[[one]] and [[one]] again in the same paragraph\n");
    tmp.child("a.md").write_str(&a).unwrap();
    tmp
}

#[test]
fn ranked_table_output() {
    let tmp = make_vault();
    let out = ft()
        .env("NO_COLOR", "1")
        .args(["--vault", tmp.path().to_str().unwrap(), "notes", "ghosts"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        vec!["(5) [[five]]", "(3) [[three]]", "(1) [[one]]"],
        "{text}"
    );
}

#[test]
fn filters_compose() {
    let tmp = make_vault();
    let out = ft()
        .env("NO_COLOR", "1")
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "ghosts",
            "--min-mentions",
            "2",
            "--limit",
            "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.trim(), "(5) [[five]]", "{text}");
}

#[test]
fn json_shape() {
    let tmp = make_vault();
    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "ghosts",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<Value> = serde_json::from_slice(&out).expect("valid JSON");
    let shape: Vec<(String, u64)> = rows
        .iter()
        .map(|r| {
            (
                r["target"].as_str().unwrap().to_string(),
                r["mentions"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        vec![("five".into(), 5), ("three".into(), 3), ("one".into(), 1)]
    );
}

#[test]
fn empty_vault_exits_zero() {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("a.md").write_str("no links here\n").unwrap();
    ft().env("NO_COLOR", "1")
        .args(["--vault", tmp.path().to_str().unwrap(), "notes", "ghosts"])
        .assert()
        .success()
        .stdout(predicates::str::contains("no ghosts in the vault"));
}

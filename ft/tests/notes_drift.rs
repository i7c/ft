//! Integration tests for `ft notes drift` — the read-only concept-name
//! drift report. Pure graph: fixtures need no git repository.

use assert_cmd::Command;
use assert_fs::prelude::*;
use serde_json::Value;

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

/// A note `onboarding.md` plus a ghost `onboarding-flow`, mentioned in
/// separate paragraphs that share a `[[activation]]` neighbor — the
/// canonical drift shape. A second unrelated concept keeps the report
/// from being trivially single-pair.
fn make_vault() -> assert_fs::TempDir {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("onboarding.md")
        .write_str("# onboarding\n")
        .unwrap();
    tmp.child("a.md")
        .write_str(
            "[[onboarding]] with [[activation]] here.\n\n\
             [[onboarding]] again with [[activation]].\n\n\
             [[onboarding-flow]] with [[activation]] there.\n\n\
             [[timeline]] is unrelated.\n",
        )
        .unwrap();
    tmp
}

#[test]
fn report_shape_ghost_marker_and_merge_suggestion() {
    let tmp = make_vault();
    let out = ft()
        .env("NO_COLOR", "1")
        .args(["--vault", tmp.path().to_str().unwrap(), "notes", "drift"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("[[onboarding]] (2) ↔ [[onboarding-flow]]? (1)"),
        "{text}"
    );
    assert!(
        text.contains("merge: ft notes rename \"[[onboarding-flow]]\" \"onboarding\""),
        "{text}"
    );
}

#[test]
fn note_pair_gets_alias_line() {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("onboarding.md")
        .write_str("# onboarding\n")
        .unwrap();
    tmp.child("onboarding-flow.md")
        .write_str("# onboarding-flow\n")
        .unwrap();
    tmp.child("a.md")
        .write_str(
            "[[onboarding]] with [[activation]].\n\n\
             [[onboarding]] more with [[activation]].\n\n\
             [[onboarding-flow]] with [[activation]].\n",
        )
        .unwrap();
    let out = ft()
        .env("NO_COLOR", "1")
        .args(["--vault", tmp.path().to_str().unwrap(), "notes", "drift"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("alias: list [[onboarding-flow]] under onboarding.md"),
        "{text}"
    );
    assert!(!text.contains("merge:"), "{text}");
}

#[test]
fn json_and_limit() {
    let tmp = make_vault();
    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "drift",
            "--json",
            "--limit",
            "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<Value> = serde_json::from_slice(&out).expect("valid JSON");
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r["keeper"]["target"], "onboarding");
    assert_eq!(r["keeper"]["is_ghost"], false);
    assert_eq!(r["lesser"]["target"], "onboarding-flow");
    assert_eq!(r["lesser"]["is_ghost"], true);
    for key in ["name_similarity", "neighborhood_overlap", "score"] {
        assert!(r[key].is_f64() || r[key].is_u64(), "{key} missing: {r}");
    }
    assert!(r["direct_cooccurrence"].is_u64(), "{r}");
    assert!(
        r["suggestion"].as_str().unwrap().starts_with("merge:"),
        "{r}"
    );
}

#[test]
fn clean_vault_exits_zero() {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("a.md")
        .write_str("[[onboarding]] and [[activation]] only.\n")
        .unwrap();
    ft().env("NO_COLOR", "1")
        .args(["--vault", tmp.path().to_str().unwrap(), "notes", "drift"])
        .assert()
        .success()
        .stdout(predicates::str::contains("no drift candidates found"));
}

/// Round-trip sanity (spec: the report points at real resolution
/// machinery): run the exact rename the report suggests, then assert
/// the pair is gone from the next report.
#[test]
fn suggested_merge_resolves_the_pair() {
    let tmp = make_vault();
    let vault = tmp.path().to_str().unwrap().to_string();

    let out = ft()
        .args([
            "--vault", &vault, "notes", "drift", "--json", "--limit", "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<Value> = serde_json::from_slice(&out).expect("valid JSON");
    let suggestion = rows[0]["suggestion"].as_str().unwrap();
    // "merge: ft notes rename "[[onboarding-flow]]" "onboarding""
    let lesser = rows[0]["lesser"]["target"].as_str().unwrap();
    let keeper = rows[0]["keeper"]["target"].as_str().unwrap();
    assert!(
        suggestion.starts_with("merge: ft notes rename"),
        "{suggestion}"
    );

    ft().args([
        "--vault",
        &vault,
        "notes",
        "rename",
        &format!("[[{lesser}]]"),
        keeper,
    ])
    .assert()
    .success();

    ft().env("NO_COLOR", "1")
        .args(["--vault", &vault, "notes", "drift"])
        .assert()
        .success()
        .stdout(predicates::str::contains("no drift candidates found"));
}

/// `[drift].exclude` keeps linked attachments out of the report while
/// real concept drift still surfaces.
#[test]
fn config_exclude_patterns_drop_attachments() {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child(".ft/config.toml")
        .write_str("[drift]\nexclude = [\"*.png\", \"*.pdf\"]\n")
        .unwrap();
    tmp.child("a.md")
        .write_str(
            "see ![[diagram-v1.png]] with [[zebra]].\n\n\
             see ![[diagram-v2.png]] with [[zebra]].\n\n\
             [[onboarding]] with [[zebra]].\n\n\
             [[onboarding-flow]] with [[zebra]].\n",
        )
        .unwrap();
    let out = ft()
        .env("NO_COLOR", "1")
        .args(["--vault", tmp.path().to_str().unwrap(), "notes", "drift"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(!text.contains("diagram-v1.png"), "{text}");
    assert!(
        text.contains("[[onboarding]]") && text.contains("[[onboarding-flow]]"),
        "{text}"
    );
}

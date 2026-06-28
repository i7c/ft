//! Integration tests for `ft notes related`.
//!
//! Unlike `notes_journal.rs`, these need no git history — related
//! scoring is pure graph (co-occurrence via ParagraphLink edges), so a
//! plain `.obsidian/`-marked vault suffices. This also verifies the
//! "no git dependency" guarantee of the spec.

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

/// Build a vault under a fresh temp dir with a target note N whose
/// `## Related` section already declares `[[Alias]]`, plus co-occurring
/// concepts C (same-paragraph, score 3) and D (cross-paragraph, score 1),
/// and a phantom target mentioned alongside C.
fn make_related_vault() -> assert_fs::TempDir {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();

    tmp.child("N.md")
        .write_str("# N\n\n## Related\n- [[Alias]]\n")
        .unwrap();
    tmp.child("Alias.md").write_str("# Alias\n").unwrap();
    tmp.child("C.md").write_str("# C\n").unwrap();
    tmp.child("D.md").write_str("# D\n").unwrap();
    // Same paragraph links N + C (+3 for C); a later paragraph links D
    // alone (+1 cross-paragraph for D).
    tmp.child("Notes.md")
        .write_str(
            "Mentions [[N]] and [[C]] in the same paragraph.\n\nLater, [[D]] gets mentioned alone.\n",
        )
        .unwrap();
    // A phantom target (no Phantom.md) co-occurring with C in one
    // paragraph — exercises ghost-target scoring.
    tmp.child("Ghost.md")
        .write_str("Phantom mention [[Phantom]] alongside [[C]].\n")
        .unwrap();
    tmp
}

#[test]
fn related_note_target_prints_scored_concepts_table() {
    let tmp = make_related_vault();
    let out = ft()
        .env("NO_COLOR", "1")
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "related",
            "N",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    // Alias is already in N's Related section → marked ✓.
    assert!(text.contains("[[Alias]]"), "alias shown:\n{text}");
    assert!(text.contains('✓'), "already-in-related row marked:\n{text}");
    assert!(text.contains("[[C]]"), "candidate C shown:\n{text}");
    assert!(text.contains("[[D]]"), "candidate D shown:\n{text}");
    // Scores appear in the table.
    assert!(text.contains("3"), "score column present:\n{text}");
}

#[test]
fn related_note_target_json_structure() {
    let tmp = make_related_vault();
    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "related",
            "N",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Vec<Value> = serde_json::from_slice(&out).expect("valid JSON array");
    let by_title: std::collections::HashMap<&str, &Value> = json
        .iter()
        .map(|r| (r["title"].as_str().unwrap(), r))
        .collect();

    // Alias: already-in-related, score > 0, resolved path.
    let alias = by_title.get("Alias").expect("Alias present");
    assert_eq!(alias["already_in_related"], true);
    assert_eq!(alias["score"], 3);
    assert_eq!(alias["target"]["kind"], "resolved");
    assert_eq!(alias["target"]["path"], "Alias.md");

    // C: candidate, score 3.
    let c = by_title.get("C").expect("C present");
    assert_eq!(c["already_in_related"], false);
    assert_eq!(c["score"], 3);

    // D: candidate, score 1 (cross-paragraph).
    let d = by_title.get("D").expect("D present");
    assert_eq!(d["score"], 1);
    assert_eq!(d["already_in_related"], false);
}

#[test]
fn related_ghost_target_prints_scored_concepts() {
    let tmp = make_related_vault();
    let out = ft()
        .env("NO_COLOR", "1")
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "related",
            "[[Phantom]]",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    // Phantom co-occurs with C in Ghost.md → C scored.
    assert!(text.contains("[[C]]"), "ghost target yields C:\n{text}");
}

#[test]
fn related_ghost_target_json_no_already_in_related_rows() {
    let tmp = make_related_vault();
    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "related",
            "[[Phantom]]",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Vec<Value> = serde_json::from_slice(&out).expect("valid JSON array");
    assert!(!json.is_empty(), "ghost target yields scored concepts");
    assert!(
        json.iter().all(|r| r["already_in_related"] == false),
        "no already_in_related rows for a ghost target: {json:#?}"
    );
    // C is the scored concept.
    assert!(json.iter().any(|r| r["title"] == "C"));
}

#[test]
fn related_empty_result_exits_non_zero_by_default() {
    let tmp = make_related_vault();
    // Alias has no co-occurring concepts (it only appears in N's
    // Related section, never alongside another concept in a paragraph).
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "notes",
        "related",
        "Alias",
    ])
    .assert()
    .failure()
    .stderr(predicates::str::contains("no related concepts").not())
    .stdout(predicates::str::contains("no related concepts"));
}

#[test]
fn related_allow_empty_succeeds() {
    let tmp = make_related_vault();
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "notes",
        "related",
        "Alias",
        "--allow-empty",
    ])
    .assert()
    .success()
    .stdout(predicates::str::contains("no related concepts"));
}

#[test]
fn related_unknown_note_exits_non_zero() {
    let tmp = make_related_vault();
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "notes",
        "related",
        "DoesNotExist",
    ])
    .assert()
    .failure();
}

#[test]
fn related_works_without_git_repository() {
    // The make_related_vault vault is never `git init`-ed. Success here
    // is the spec's "no git/blame dependency" guarantee: `ft notes
    // journal` would fail on this vault; `ft notes related` must not.
    let tmp = make_related_vault();
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "notes",
        "related",
        "N",
    ])
    .assert()
    .success();
}

#[test]
fn related_already_in_related_marked_in_all_formats() {
    let tmp = make_related_vault();
    // Markdown: already-in-related rows are prefixed with ✓.
    let md = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "related",
            "N",
            "--format",
            "markdown",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let md = String::from_utf8(md).unwrap();
    assert!(md.contains("- ✓ [[Alias]]"), "markdown marks alias:\n{md}");
}

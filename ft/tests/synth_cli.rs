//! Integration tests for `ft synth scaffold` and `ft synth verify`.

use assert_cmd::Command;
use serde_json::Value;
use std::process::Command as StdCommand;

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

fn run_git(dir: &std::path::Path, args: &[&str]) {
    let out = StdCommand::new("git")
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()
        .expect("git binary on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

fn make_source_vault() -> assert_fs::TempDir {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("notes/source.md")
        .write_str(
            "First paragraph mentions [[Foo]].\n\
             Continues on a second line.\n\n\
             Second paragraph mentions [[Foo]] again.\n",
        )
        .unwrap();
    let repo = tmp.path();
    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.name", "T"]);
    run_git(repo, &["config", "user.email", "t@e.com"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "init"]);
    tmp
}

#[test]
fn scaffold_create_writes_frontmatter_and_callouts() {
    use assert_fs::prelude::*;
    let tmp = make_source_vault();
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "scaffold",
        "Synthesis/topic.md",
        "--link",
        "[[Foo]]",
        "--no-edit",
    ])
    .assert()
    .success();
    let written = tmp.child("Synthesis/topic.md");
    written.assert(predicates::path::is_file());
    let body = std::fs::read_to_string(written.path()).unwrap();
    assert!(body.starts_with("---\nft-synth: true\n---\n"));
    assert!(body.contains("> [!ft-source] notes/source.md L1-2 @"));
    assert!(body.contains("> First paragraph mentions [[Foo]]."));
    assert!(body.contains("> [!ft-source] notes/source.md L4-4 @"));
}

#[test]
fn scaffold_append_preserves_existing_content() {
    use assert_fs::prelude::*;
    let tmp = make_source_vault();
    // Pre-create the synth note with user prose.
    tmp.child("Synthesis/topic.md")
        .write_str("---\nft-synth: true\n---\n\nUser prose written earlier.\n")
        .unwrap();

    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "scaffold",
        "Synthesis/topic.md",
        "--link",
        "[[Foo]]",
        "--no-edit",
    ])
    .assert()
    .success();

    let body = std::fs::read_to_string(tmp.child("Synthesis/topic.md").path()).unwrap();
    assert!(body.contains("User prose written earlier."));
    assert!(body.contains("> [!ft-source] notes/source.md L"));
}

#[test]
fn scaffold_from_picks_specific_paragraph() {
    use assert_fs::prelude::*;
    let tmp = make_source_vault();
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "scaffold",
        "Synthesis/picked.md",
        "--from",
        "notes/source.md:4",
        "--no-edit",
    ])
    .assert()
    .success();
    let body = std::fs::read_to_string(tmp.child("Synthesis/picked.md").path()).unwrap();
    // Exactly one section, from the second paragraph (line 4).
    assert!(body.contains("> [!ft-source] notes/source.md L4-4 @"));
    assert!(body.contains("> Second paragraph mentions [[Foo]] again."));
    // First-paragraph content should NOT appear.
    assert!(!body.contains("> First paragraph mentions"));
}

#[test]
fn scaffold_requires_link_or_from() {
    let tmp = make_source_vault();
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "scaffold",
        "Synthesis/topic.md",
        "--no-edit",
    ])
    .assert()
    .failure()
    .stderr(predicates::str::contains("one of --link or --from"));
}

#[test]
fn verify_single_note_passes_after_scaffold() {
    let tmp = make_source_vault();
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "scaffold",
        "Synthesis/topic.md",
        "--link",
        "[[Foo]]",
        "--no-edit",
    ])
    .assert()
    .success();

    ft().env("NO_COLOR", "1")
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "synth",
            "verify",
            "Synthesis/topic.md",
        ])
        .assert()
        .success();
}

#[test]
fn verify_all_json_reports_drift_after_edit() {
    use assert_fs::prelude::*;
    let tmp = make_source_vault();
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "scaffold",
        "Synthesis/topic.md",
        "--link",
        "[[Foo]]",
        "--no-edit",
    ])
    .assert()
    .success();

    // Hand-corrupt the body of a protected section.
    let p = tmp.child("Synthesis/topic.md");
    let body = std::fs::read_to_string(p.path()).unwrap();
    let corrupted = body.replace("First paragraph mentions", "EDITED mentions");
    std::fs::write(p.path(), corrupted).unwrap();

    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "synth",
            "verify",
            "--all",
            "--json",
        ])
        .assert()
        .failure() // exit 1 because at least one section drifted
        .get_output()
        .stdout
        .clone();

    let rows: Vec<Value> = serde_json::from_slice(&out).expect("valid JSON");
    assert!(!rows.is_empty(), "verify --all --json must emit rows");
    assert!(
        rows.iter().any(|r| r["status"].as_str() == Some("drifted")),
        "expected at least one drifted row, got {rows:?}"
    );
}

#[test]
fn verify_requires_note_or_all() {
    let tmp = make_source_vault();
    ft().args(["--vault", tmp.path().to_str().unwrap(), "synth", "verify"])
        .assert()
        .failure();
}

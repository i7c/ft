use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

fn vault() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir
}

fn ft(vault: &std::path::Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut full = vec!["--vault", vault.to_str().unwrap(), "tasks"];
    full.extend(args);
    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-05-10")
        .args(&full)
        .assert()
}

fn read(dir: &assert_fs::TempDir, rel: &str) -> String {
    std::fs::read_to_string(dir.path().join(rel)).unwrap()
}

fn fixture() -> assert_fs::TempDir {
    let dir = vault();
    dir.child("inbox.md")
        .write_str("- [ ] keep\n- [ ] call vendor\n  - [ ] find number\n")
        .unwrap();
    dir.child("proj.md")
        .write_str("- [ ] Vendor onboarding\n    - [ ] sign NDA\n- [ ] other\n")
        .unwrap();
    dir
}

#[test]
fn move_under_task_in_other_file() {
    let dir = fixture();
    ft(
        dir.path(),
        &["move", "inbox.md:2", "--under", "Vendor onboarding"],
    )
    .success()
    .stdout(predicate::str::contains("Moved 1 task(s) → proj.md:1"));
    assert_eq!(read(&dir, "inbox.md"), "- [ ] keep\n");
    assert_eq!(
        read(&dir, "proj.md"),
        "- [ ] Vendor onboarding\n    - [ ] sign NDA\n    - [ ] call vendor\n        - [ ] find number\n- [ ] other\n"
    );
}

#[test]
fn move_under_sibling() {
    let dir = fixture();
    ft(
        dir.path(),
        &["move", "inbox.md:1", "--under", "proj.md:1", "--sibling"],
    )
    .success();
    assert_eq!(
        read(&dir, "proj.md"),
        "- [ ] Vendor onboarding\n    - [ ] sign NDA\n- [ ] keep\n- [ ] other\n"
    );
}

#[test]
fn move_under_ambiguous_lists_candidates() {
    let dir = fixture();
    ft(dir.path(), &["move", "inbox.md:1", "--under", "o"])
        .failure()
        .stderr(predicate::str::contains("--under:").and(predicate::str::contains("candidates")));
    assert_eq!(
        read(&dir, "inbox.md"),
        "- [ ] keep\n- [ ] call vendor\n  - [ ] find number\n"
    );
}

#[test]
fn move_under_own_subtree_is_rejected_with_relative_path() {
    let dir = fixture();
    ft(dir.path(), &["move", "inbox.md:2", "--under", "inbox.md:3"])
        .failure()
        .stderr(
            predicate::str::contains("under itself").and(predicate::str::contains("inbox.md:3")),
        );
}

#[test]
fn to_and_under_conflict() {
    let dir = fixture();
    ft(
        dir.path(),
        &["move", "keep", "--to", "x.md", "--under", "other"],
    )
    .failure()
    .stderr(predicate::str::contains("cannot be used with"));
    ft(dir.path(), &["move", "keep", "--to", "x.md", "--sibling"]).failure();
    ft(dir.path(), &["move", "keep"]).failure();
}

#[test]
fn indent_and_outdent() {
    let dir = fixture();
    ft(dir.path(), &["indent", "proj.md:3"]).success();
    assert_eq!(
        read(&dir, "proj.md"),
        "- [ ] Vendor onboarding\n    - [ ] sign NDA\n    - [ ] other\n"
    );
    ft(dir.path(), &["outdent", "proj.md:2"]).success();
    assert_eq!(
        read(&dir, "proj.md"),
        "- [ ] Vendor onboarding\n    - [ ] other\n- [ ] sign NDA\n"
    );
}

#[test]
fn indent_errors_are_readable() {
    let dir = fixture();
    ft(dir.path(), &["indent", "proj.md:1"])
        .failure()
        .stderr(predicate::str::contains(
            "proj.md:1 has no previous sibling to indent under",
        ));
    ft(dir.path(), &["outdent", "proj.md:3"])
        .failure()
        .stderr(predicate::str::contains("proj.md:3 is already top-level"));
}

#[test]
fn outdent_dry_run_prints_diff_without_writing() {
    let dir = fixture();
    ft(dir.path(), &["outdent", "proj.md:2", "--dry-run"])
        .success()
        .stdout(predicate::str::contains("+- [ ] sign NDA"));
    assert_eq!(
        read(&dir, "proj.md"),
        "- [ ] Vendor onboarding\n    - [ ] sign NDA\n- [ ] other\n"
    );
}

#[test]
fn move_under_json_output() {
    let dir = fixture();
    let out = ft(
        dir.path(),
        &[
            "move",
            "inbox.md:2",
            "--under",
            "proj.md:1",
            "--format",
            "json",
        ],
    )
    .success()
    .get_output()
    .stdout
    .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    insta::assert_snapshot!(serde_json::to_string_pretty(&v).unwrap());
}

#[test]
fn indent_json_output_reports_new_location() {
    let dir = fixture();
    let out = ft(dir.path(), &["indent", "proj.md:3", "--format", "json"])
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v[0]["from"]["line"], 3);
    assert_eq!(v[0]["to"]["path"], "proj.md");
    assert_eq!(v[0]["to"]["line"], 3);
}

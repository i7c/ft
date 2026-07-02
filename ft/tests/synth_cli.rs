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
    assert!(body.contains("> [!ft-source] \"notes/source.md\" L1-2 @"));
    assert!(body.contains("> First paragraph mentions [[Foo]]."));
    assert!(body.contains("> [!ft-source] \"notes/source.md\" L4-4 @"));
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
    assert!(body.contains("> [!ft-source] \"notes/source.md\" L"));
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
    assert!(body.contains("> [!ft-source] \"notes/source.md\" L4-4 @"));
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

/// Regression: when the vault is a subdirectory of the git repo, the
/// scaffold's HEAD pin and verify's `git show <sha>:<path>` must apply
/// the vault→repo path prefix. Before the RepoMap fix, verify reported
/// `source-missing` because it looked up the vault-relative path against
/// the repo root.
#[test]
fn nested_vault_scaffold_verify_roundtrip() {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    let repo = tmp.path();
    // Git repo at the top; the vault lives under `brain/`.
    tmp.child("brain/.obsidian").create_dir_all().unwrap();
    tmp.child("brain/notes/source.md")
        .write_str("First paragraph mentions [[Foo]].\nContinues here.\n")
        .unwrap();
    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.name", "T"]);
    run_git(repo, &["config", "user.email", "t@e.com"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "init"]);

    let vault = tmp.child("brain");
    ft().args([
        "--vault",
        vault.path().to_str().unwrap(),
        "synth",
        "scaffold",
        "Synthesis/topic.md",
        "--link",
        "[[Foo]]",
        "--no-edit",
    ])
    .assert()
    .success();

    // Pinned section refers to the vault-relative `notes/source.md`.
    let body = std::fs::read_to_string(vault.path().join("Synthesis/topic.md")).unwrap();
    assert!(
        body.contains("> [!ft-source] \"notes/source.md\" L"),
        "got:\n{body}"
    );

    ft().env("NO_COLOR", "1")
        .args([
            "--vault",
            vault.path().to_str().unwrap(),
            "synth",
            "verify",
            "--all",
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

// ── reslice ──────────────────────────────────────────────────────────────

/// Scaffold a single-section synth note (sourcing `[[Foo]]`, which the
/// fixture mentions only in the first paragraph at L1-2 when we keep just
/// that one via `--from`). Returns the temp vault.
fn scaffold_single_section(tmp: &assert_fs::TempDir) {
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "scaffold",
        "Synthesis/topic.md",
        "--from",
        "notes/source.md:1",
        "--no-edit",
    ])
    .assert()
    .success();
}

#[test]
fn reslice_extends_range_and_verifies() {
    use assert_fs::prelude::*;
    let tmp = make_source_vault();
    scaffold_single_section(&tmp);
    // Add an unrelated commit so HEAD is no longer the pinned commit.
    tmp.child("other.md").write_str("unrelated\n").unwrap();
    run_git(tmp.path(), &["add", "."]);
    run_git(tmp.path(), &["commit", "-m", "c2"]);

    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "reslice",
        "Synthesis/topic.md",
        "--down",
        "1",
    ])
    .assert()
    .success()
    .stdout(predicates::str::contains("L1-3"));

    let body = std::fs::read_to_string(tmp.child("Synthesis/topic.md").path()).unwrap();
    assert!(body.contains("> [!ft-source] \"notes/source.md\" L1-3 @"));

    ft().args([
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
fn reslice_absolute_lines() {
    use assert_fs::prelude::*;
    let tmp = make_source_vault();
    scaffold_single_section(&tmp);

    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "reslice",
        "Synthesis/topic.md",
        "--lines",
        "4-4",
    ])
    .assert()
    .success();

    let body = std::fs::read_to_string(tmp.child("Synthesis/topic.md").path()).unwrap();
    assert!(body.contains("> [!ft-source] \"notes/source.md\" L4-4 @"));
    assert!(body.contains("> Second paragraph mentions [[Foo]] again."));
}

#[test]
fn reslice_ambiguous_without_at_errors() {
    let tmp = make_source_vault();
    // Two sections via two --from picks.
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "scaffold",
        "Synthesis/topic.md",
        "--from",
        "notes/source.md:1",
        "--from",
        "notes/source.md:4",
        "--no-edit",
    ])
    .assert()
    .success();

    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "reslice",
        "Synthesis/topic.md",
        "--down",
        "1",
    ])
    .assert()
    .failure()
    .stderr(predicates::str::contains("--at"));
}

// ── repair ───────────────────────────────────────────────────────────────

/// Scaffold a synth note, then mangle its pinned SHA so verify fails.
/// Returns the vault tempdir; the note lives at `Synthesis/topic.md`.
fn make_vault_with_stranded_pin() -> assert_fs::TempDir {
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
    let note = tmp.path().join("Synthesis/topic.md");
    let content = std::fs::read_to_string(&note).unwrap();
    let mangled = regex_replace_all_sha(&content);
    std::fs::write(&note, mangled).unwrap();
    tmp
}

/// Replace every pinned `@<sha7>` with an unresolvable placeholder,
/// simulating a rewritten/gc'd history.
fn regex_replace_all_sha(content: &str) -> String {
    let re = regex::Regex::new(r"@[0-9a-f]{7}").unwrap();
    re.replace_all(content, "@deadbe1").into_owned()
}

#[test]
fn repair_repins_stranded_sections_and_verify_passes() {
    let tmp = make_vault_with_stranded_pin();
    // Precondition: verify fails on the stranded pins.
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "verify",
        "Synthesis/topic.md",
    ])
    .assert()
    .failure();

    let assert = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "synth",
            "repair",
            "Synthesis/topic.md",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        stdout.contains("repinned"),
        "repair should report repinned sections:\n{stdout}"
    );

    // Postcondition: verify is clean again.
    ft().args([
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
fn repair_dry_run_reports_but_writes_nothing() {
    let tmp = make_vault_with_stranded_pin();
    let note = tmp.path().join("Synthesis/topic.md");
    let before = std::fs::read_to_string(&note).unwrap();

    let assert = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "synth",
            "repair",
            "--all",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        stdout.contains("would repair"),
        "dry-run should announce itself:\n{stdout}"
    );

    let after = std::fs::read_to_string(&note).unwrap();
    assert_eq!(before, after, "--dry-run must not write");

    // Verify still fails — nothing was repaired.
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "verify",
        "--all",
    ])
    .assert()
    .failure();
}

#[test]
fn repair_unrecoverable_body_exits_failure_with_json_detail() {
    let tmp = make_vault_with_stranded_pin();
    // Also mangle the quoted body so it can't be found at HEAD.
    let note = tmp.path().join("Synthesis/topic.md");
    let content = std::fs::read_to_string(&note).unwrap();
    let mangled = content.replace(
        "> First paragraph mentions [[Foo]].",
        "> Text that never existed in the source.",
    );
    assert_ne!(content, mangled);
    std::fs::write(&note, mangled).unwrap();

    let assert = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "synth",
            "repair",
            "Synthesis/topic.md",
            "--json",
        ])
        .assert()
        .failure();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let rows: Value = serde_json::from_str(&stdout).expect("repair --json emits JSON");
    let actions: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["action"].as_str().unwrap())
        .collect();
    assert!(
        actions.contains(&"unrecoverable"),
        "mangled body must be unrecoverable: {actions:?}"
    );
    // The second, untouched section still repairs.
    assert!(
        actions.contains(&"repinned"),
        "intact section must still repair: {actions:?}"
    );
}

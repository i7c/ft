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
    assert!(body.starts_with("---\nft-synth: true\n"));
    assert!(body.contains("ft-synth-targets: [\"[[Foo]]\"]"));
    assert!(body.contains("---\n"));
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

// ── ft synth grow ──────────────────────────────────────────────────────

/// A vault with a Foo-target note whose two paragraphs are committed on
/// different days, so the watermark date filter is exercisable.
fn make_grow_vault() -> assert_fs::TempDir {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    // Day 1: one Foo-mentioning paragraph.
    tmp.child("notes/daily.md")
        .write_str("First mention of [[Foo]] here.\n")
        .unwrap();
    let repo = tmp.path();
    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.name", "T"]);
    run_git(repo, &["config", "user.email", "t@e.com"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["add", "."]);
    // Backdate the first commit so the watermark is distinguishable.
    let out = StdCommand::new("git")
        .current_dir(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00")
        .args(["commit", "-m", "c1"])
        .output()
        .expect("git commit");
    assert!(out.status.success());
    // Day 2: a second Foo-mentioning paragraph, newer date.
    tmp.child("notes/daily.md")
        .write_str(
            "First mention of [[Foo]] here.\n\n\
             Second mention of [[Foo]] committed later.\n",
        )
        .unwrap();
    let out = StdCommand::new("git")
        .current_dir(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_DATE", "2026-06-01T00:00:00")
        .env("GIT_COMMITTER_DATE", "2026-06-01T00:00:00")
        .args(["add", "."])
        .output()
        .expect("git add");
    assert!(out.status.success());
    let out = StdCommand::new("git")
        .current_dir(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_DATE", "2026-06-01T00:00:00")
        .env("GIT_COMMITTER_DATE", "2026-06-01T00:00:00")
        .args(["commit", "-m", "c2"])
        .output()
        .expect("git commit");
    assert!(out.status.success());
    tmp
}

#[test]
fn grow_appends_only_missing_entries() {
    use assert_fs::prelude::*;
    let tmp = make_grow_vault();
    // Scaffold first (captures both Foo paragraphs, pinned at HEAD=c2).
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
    let note = tmp.child("Synthesis/topic.md");
    let before = std::fs::read_to_string(note.path()).unwrap();
    let section_count_before = before.matches("[!ft-source]").count();

    // Grow with the same link: dedup-on-append means nothing new.
    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "synth",
        "grow",
        "Synthesis/topic.md",
        "--link",
        "[[Foo]]",
        "--no-edit",
    ])
    .assert()
    .success();
    let after = std::fs::read_to_string(note.path()).unwrap();
    let section_count_after = after.matches("[!ft-source]").count();
    assert_eq!(
        section_count_before, section_count_after,
        "grow with all-pinned entries must append nothing"
    );
}

#[test]
fn grow_new_only_scopes_to_entries_newer_than_watermark() {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    // Day 1: first Foo paragraph.
    tmp.child("notes/daily.md")
        .write_str("First mention of [[Foo]] here.\n")
        .unwrap();
    let repo = tmp.path();
    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.name", "T"]);
    run_git(repo, &["config", "user.email", "t@e.com"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    let commit_env = |date: &str| {
        vec![
            ("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()),
            ("GIT_AUTHOR_DATE".to_string(), date.to_string()),
            ("GIT_COMMITTER_DATE".to_string(), date.to_string()),
        ]
    };
    run_git_env(repo, &["add", "."], &commit_env("2026-01-01T00:00:00"));
    run_git_env(
        repo,
        &["commit", "-m", "c1"],
        &commit_env("2026-01-01T00:00:00"),
    );

    // Scaffold the note at c1 → watermark = 2026-01-01.
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
    let note = tmp.child("Synthesis/topic.md");
    let after_scaffold = std::fs::read_to_string(note.path()).unwrap();
    assert_eq!(
        after_scaffold.matches("[!ft-source]").count(),
        1,
        "scaffold captured the one paragraph"
    );

    // Day 2: add a second Foo paragraph (date 2026-06-01 > watermark).
    tmp.child("notes/daily.md")
        .write_str(
            "First mention of [[Foo]] here.\n\n\
             Second mention of [[Foo]] committed later.\n",
        )
        .unwrap();
    run_git_env(repo, &["add", "."], &commit_env("2026-06-01T00:00:00"));
    run_git_env(
        repo,
        &["commit", "-m", "c2"],
        &commit_env("2026-06-01T00:00:00"),
    );

    // grow --new-only should append exactly the new (second) paragraph.
    let assert = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "synth",
            "grow",
            "Synthesis/topic.md",
            "--link",
            "[[Foo]]",
            "--new-only",
            "--no-edit",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("appended 1 section"),
        "expected 1 new section, got stdout: {stdout}"
    );
    let after_grow = std::fs::read_to_string(note.path()).unwrap();
    assert_eq!(
        after_grow.matches("[!ft-source]").count(),
        2,
        "note now has both sections"
    );
    assert!(
        after_grow.contains("Second mention of [[Foo]] committed later."),
        "new paragraph body must be present"
    );
}

#[test]
fn grow_new_only_on_brand_new_note_falls_back_with_warning() {
    use assert_fs::prelude::*;
    let tmp = make_grow_vault();
    // Create an empty synth note (no callouts → no watermark).
    tmp.child("Synthesis/topic.md")
        .write_str("---\nft-synth: true\nft-synth-targets: [\"[[Foo]]\"]\n---\n\n")
        .unwrap();
    let assert = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "synth",
            "grow",
            "Synthesis/topic.md",
            "--new-only",
            "--no-edit",
        ])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("could not determine a last-synth watermark"),
        "expected fallback warning, got stderr: {stderr}"
    );
    let body = std::fs::read_to_string(tmp.child("Synthesis/topic.md").path()).unwrap();
    // Both paragraphs appended (all missing, watermark unavailable).
    assert_eq!(body.matches("[!ft-source]").count(), 2);
}

#[test]
fn grow_reads_targets_from_frontmatter() {
    use assert_fs::prelude::*;
    let tmp = make_grow_vault();
    // Pre-create a synth note with frontmatter targets and one already-pinned section.
    // Scaffold first to get a real pinned section + targets, then grow with no --link.
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
    // grow with NO --link → reads targets from frontmatter. All already
    // pinned → appends nothing, succeeds.
    let assert = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "synth",
            "grow",
            "Synthesis/topic.md",
            "--no-edit",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    // Both paragraphs are already pinned → either "appended 0" or the
    // all-pinned message. Accept success + no new sections.
    let body = std::fs::read_to_string(tmp.child("Synthesis/topic.md").path()).unwrap();
    let _ = stdout;
    assert_eq!(
        body.matches("[!ft-source]").count(),
        2,
        "frontmatter-driven grow must not duplicate pinned sections"
    );
}

#[test]
fn grow_no_targets_errors_clearly() {
    use assert_fs::prelude::*;
    let tmp = make_grow_vault();
    // A synth note with NO ft-synth-targets frontmatter.
    tmp.child("Synthesis/topic.md")
        .write_str("---\nft-synth: true\n---\n\nUser prose.\n")
        .unwrap();
    let assert = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "synth",
            "grow",
            "Synthesis/topic.md",
            "--no-edit",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("no targets") && stderr.contains("--link"),
        "expected a clear no-targets error, got stderr: {stderr}"
    );
}

#[test]
fn grow_nonexistent_target_errors() {
    let tmp = make_grow_vault();
    let assert = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "synth",
            "grow",
            "Synthesis/missing.md",
            "--link",
            "[[Foo]]",
            "--no-edit",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("does not exist") && stderr.contains("scaffold"),
        "expected a 'use scaffold' hint, got stderr: {stderr}"
    );
}

#[test]
fn grow_limit_caps_appended_sections() {
    use assert_fs::prelude::*;
    let tmp = make_grow_vault();
    // Empty synth note with frontmatter target → all missing, then limit to 1.
    tmp.child("Synthesis/topic.md")
        .write_str("---\nft-synth: true\nft-synth-targets: [\"[[Foo]]\"]\n---\n\n")
        .unwrap();
    let assert = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "synth",
            "grow",
            "Synthesis/topic.md",
            "--link",
            "[[Foo]]",
            "--limit",
            "1",
            "--no-edit",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("appended 1 section"),
        "expected exactly 1 section with --limit 1, got stdout: {stdout}"
    );
    let body = std::fs::read_to_string(tmp.child("Synthesis/topic.md").path()).unwrap();
    assert_eq!(
        body.matches("[!ft-source]").count(),
        1,
        "only 1 section should be on disk"
    );
}

#[test]
fn grow_no_edit_suppresses_editor() {
    // Smoke: --no-edit exits 0 without trying to spawn an editor.
    // (Setting EDITATOR=true would make a non-no-edit run hang; this
    // test just confirms --no-edit short-circuits.)
    use assert_fs::prelude::*;
    let tmp = make_grow_vault();
    tmp.child("Synthesis/topic.md")
        .write_str("---\nft-synth: true\nft-synth-targets: [\"[[Foo]]\"]\n---\n\n")
        .unwrap();
    ft().env("EDITOR", "false")
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "synth",
            "grow",
            "Synthesis/topic.md",
            "--no-edit",
        ])
        .assert()
        .success();
}

/// Run git with extra environment variables (for backdated commits).
fn run_git_env(dir: &std::path::Path, args: &[&str], env: &[(String, String)]) {
    let mut cmd = StdCommand::new("git");
    cmd.current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.args(args).output().expect("git binary on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

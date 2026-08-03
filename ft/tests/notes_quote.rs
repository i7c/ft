//! Integration tests for `ft notes quote` — the read-only protected-
//! section plumbing command.

use assert_cmd::Command;
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

/// Vault with one committed source file:
/// `notes/source.md` = "First paragraph mentions [[Foo]].\nContinues on a second line.\n\nSecond paragraph.\n"
fn make_source_vault() -> assert_fs::TempDir {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("notes/source.md")
        .write_str(
            "First paragraph mentions [[Foo]].\n\
             Continues on a second line.\n\n\
             Second paragraph.\n",
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

fn quote(tmp: &assert_fs::TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
    ft().args(["--vault", tmp.path().to_str().unwrap(), "notes", "quote"])
        .args(args)
        .assert()
}

#[test]
fn emits_exact_canonical_callout() {
    let tmp = make_source_vault();
    let head_short = {
        let out = StdCommand::new("git")
            .current_dir(tmp.path())
            .args(["rev-parse", "--short=7", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    // The whole stdout must be exactly one callout: pinned header
    // (path, range, 7-hex HEAD short SHA, 6-hex blake3 prefix) +
    // `> `-prefixed body + a single trailing newline.
    let re = format!(
        r#"^> \[!ft-source\] "notes/source\.md" L1-2 @{} #[0-9a-f]{{6}}\n> First paragraph mentions \[\[Foo\]\]\.\n> Continues on a second line\.\n$"#,
        head_short
    );
    quote(&tmp, &["notes/source.md", "--lines", "1-2"])
        .success()
        .stdout(predicates::str::is_match(re).unwrap());
}

#[test]
fn short_flag_is_byte_identical_to_long() {
    let tmp = make_source_vault();
    let long = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "quote",
            "notes/source.md",
            "--lines",
            "2-4",
        ])
        .output()
        .unwrap();
    let short = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "quote",
            "notes/source.md",
            "-l",
            "2-4",
        ])
        .output()
        .unwrap();
    assert!(long.status.success());
    assert!(short.status.success());
    assert_eq!(long.stdout, short.stdout);
    assert_eq!(String::from_utf8(short.stdout).unwrap().lines().count(), 4);
}

#[test]
fn missing_file_errors_and_nothing_on_stdout() {
    let tmp = make_source_vault();
    quote(&tmp, &["does-not-exist.md", "-l", "1-1"])
        .failure()
        .stderr(predicates::str::contains(
            "cannot read source file `does-not-exist.md`",
        ))
        .stdout("");
}

#[test]
fn non_utf8_file_errors_naming_file() {
    let tmp = make_source_vault();
    std::fs::write(tmp.path().join("notes/bad.md"), [0xFF, 0xFE, 0x00]).unwrap();
    run_git(tmp.path(), &["add", "notes/bad.md"]);
    run_git(tmp.path(), &["commit", "-m", "bad"]);
    quote(&tmp, &["notes/bad.md", "-l", "1-1"])
        .failure()
        .stderr(predicates::str::contains(
            "cannot read source file `notes/bad.md`",
        ))
        .stdout("");
}

#[test]
fn dirty_source_modified_blocks() {
    use assert_fs::prelude::*;
    let tmp = make_source_vault();
    tmp.child("notes/source.md")
        .write_str("Edited first paragraph.\n")
        .unwrap();
    quote(&tmp, &["notes/source.md", "-l", "1-1"])
        .failure()
        .stderr(predicates::str::contains("uncommitted changes"))
        .stderr(predicates::str::contains("notes/source.md"))
        .stdout("");
}

#[test]
fn dirty_source_untracked_blocks() {
    use assert_fs::prelude::*;
    let tmp = make_source_vault();
    tmp.child("notes/new.md")
        .write_str("Fresh paragraph.\n")
        .unwrap();
    quote(&tmp, &["notes/new.md", "-l", "1-1"])
        .failure()
        .stderr(predicates::str::contains("uncommitted changes"))
        .stderr(predicates::str::contains("notes/new.md"))
        .stdout("");
}

#[test]
fn dirty_source_staged_blocks() {
    let tmp = make_source_vault();
    std::fs::write(tmp.path().join("notes/source.md"), "Staged edit.\n").unwrap();
    run_git(tmp.path(), &["add", "notes/source.md"]);
    quote(&tmp, &["notes/source.md", "-l", "1-1"])
        .failure()
        .stderr(predicates::str::contains("uncommitted changes"))
        .stdout("");
}

#[test]
fn unrelated_dirty_file_does_not_block() {
    use assert_fs::prelude::*;
    let tmp = make_source_vault();
    tmp.child("notes/other.md").write_str("v1\n").unwrap();
    run_git(tmp.path(), &["add", "notes/other.md"]);
    run_git(tmp.path(), &["commit", "-m", "other"]);
    tmp.child("notes/other.md").write_str("v2\n").unwrap();
    // The quoted source is clean; the unrelated dirty file is fine.
    quote(&tmp, &["notes/source.md", "-l", "1-1"]).success();
}

#[test]
fn out_of_range_errors_with_actual_line_count() {
    let tmp = make_source_vault();
    quote(&tmp, &["notes/source.md", "-l", "1-99"])
        .failure()
        .stderr(predicates::str::contains(
            "line range L1-99 outside file `notes/source.md` (file has 4 lines)",
        ))
        .stdout("");
}

#[test]
fn trailing_newline_is_not_a_line() {
    use assert_fs::prelude::*;
    let tmp = make_source_vault();
    tmp.child("notes/tiny.md").write_str("a\nb\n").unwrap();
    run_git(tmp.path(), &["add", "notes/tiny.md"]);
    run_git(tmp.path(), &["commit", "-m", "tiny"]);
    // L1-2 is the whole 2-line file, no phantom third line.
    quote(&tmp, &["notes/tiny.md", "-l", "1-2"])
        .success()
        .stdout(predicates::str::contains("> a\n> b\n"));
    quote(&tmp, &["notes/tiny.md", "-l", "1-3"])
        .failure()
        .stderr(predicates::str::contains("file has 2 lines"));
}

#[test]
fn absolute_path_is_relativized_in_header() {
    let tmp = make_source_vault();
    let abs = tmp.path().join("notes/source.md");
    quote(&tmp, &[abs.to_str().unwrap(), "-l", "1-1"])
        .success()
        .stdout(predicates::str::contains(
            "> [!ft-source] \"notes/source.md\" L1-1 @",
        ));
}

#[test]
fn no_md_auto_append() {
    let tmp = make_source_vault();
    // Only notes/source.md exists; the bare stem must not be resolved.
    quote(&tmp, &["notes/source", "-l", "1-1"])
        .failure()
        .stderr(predicates::str::contains(
            "cannot read source file `notes/source`",
        ))
        .stdout("");
}

#[test]
fn invalid_lines_argument_errors() {
    let tmp = make_source_vault();
    for bad in ["1", "a-b", "0-1", "2-1", "1-0", "-1-2"] {
        quote(&tmp, &["notes/source.md", "-l", bad])
            .failure()
            .stdout("");
    }
}

#[test]
fn vault_without_git_repo_errors() {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("notes/source.md").write_str("x\n").unwrap();
    quote(&tmp, &["notes/source.md", "-l", "1-1"])
        .failure()
        .stderr(predicates::str::contains(
            "vault is not inside a git repository",
        ))
        .stdout("");
}

#[test]
fn no_vault_errors() {
    // A nonexistent --vault path is deterministic regardless of the
    // developer's default-vault config.
    quote_nonexistent_vault().failure().stdout("");
}

#[test]
fn quoted_callout_verifies_ok_round_trip() {
    // Emit a callout via `ft notes quote`, place it in a synth note,
    // and confirm `ft notes synth verify` reports the section ok — the
    // pin (path, range, HEAD SHA, blake3 hash) must be self-consistent.
    let tmp = make_source_vault();
    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "quote",
            "notes/source.md",
            "-l",
            "1-2",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let callout = String::from_utf8(out.stdout).unwrap();

    use assert_fs::prelude::*;
    tmp.child("Synthesis/topic.md")
        .write_str(&format!(
            "---\nft:\n  synth:\n    enabled: true\n---\n\n{callout}\n"
        ))
        .unwrap();

    ft().args([
        "--vault",
        tmp.path().to_str().unwrap(),
        "notes",
        "synth",
        "verify",
        "Synthesis/topic.md",
    ])
    .assert()
    .success()
    .stdout(predicates::str::contains("ok"));
}

fn quote_nonexistent_vault() -> assert_cmd::assert::Assert {
    ft().args([
        "--vault",
        "/nonexistent/quote-test-vault",
        "notes",
        "quote",
        "notes/source.md",
        "-l",
        "1-1",
    ])
    .assert()
}

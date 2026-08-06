//! Integration tests for `ft notes export` — the read-only plumbing
//! command that renders a vault file as clean CommonMark.
//!
//! Unlike `ft notes quote`, export has no git dependency: the working
//! tree is the source of truth, so the fixture vault needs only the
//! `.obsidian` marker.

use assert_cmd::Command;

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

/// Vault with one source file. Line layout (frontmatter lines 1-6,
/// closing fence at 6, body lines 7+):
///   1  ---
///   2  title: Sample
///   3  ft:
///   4    synth:
///   5      enabled: true
///   6  ---
///   7  # Heading [[Foo]]
///   8  (blank)
///   9  See [[Bar|bee]] and ![[img.png]] and [[#Anchor]].
///  10  (blank)
///  11  > [!note] Keep me
///  12  > see [[Baz]]
///  13  (blank)
///  14  > [!ft-source] "notes/source.md" L1-2 @aaaaaaa #bbbbbb
///  15  > quoted [[Quoted]]
///  16  (blank)
///  17  - [ ] ⏫ 📅 2026-08-05 task with [md link](foo.md)
///  18  ```
///  19  [[InsideFence]]
///  20  ```
fn make_vault() -> assert_fs::TempDir {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("notes/sample.md")
        .write_str(
            "---\ntitle: Sample\nft:\n  synth:\n    enabled: true\n---\n\
             # Heading [[Foo]]\n\n\
             See [[Bar|bee]] and ![[img.png]] and [[#Anchor]].\n\n\
             > [!note] Keep me\n> see [[Baz]]\n\n\
             > [!ft-source] \"notes/source.md\" L1-2 @aaaaaaa #bbbbbb\n> quoted [[Quoted]]\n\n\
             - [ ] ⏫ 📅 2026-08-05 task with [md link](foo.md)\n\
             ```\n[[InsideFence]]\n```\n",
        )
        .unwrap();
    tmp
}

fn export(tmp: &assert_fs::TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
    ft().args(["--vault", tmp.path().to_str().unwrap(), "notes", "export"])
        .args(args)
        .assert()
}

#[test]
fn whole_file_export_is_exact_commonmark() {
    let tmp = make_vault();
    export(&tmp, &["notes/sample.md"]).success().stdout(
        "# Heading Foo\n\
         \n\
         See bee and ![img.png](img.png) and #Anchor.\n\
         \n\
         > [!note] Keep me\n> see Baz\n\
         \n\
         > quoted Quoted\n\
         \n\
         - [ ] ⏫ 📅 2026-08-05 task with [md link](foo.md)\n\
         ```\n[[InsideFence]]\n```\n",
    );
}

#[test]
fn output_has_single_trailing_newline() {
    let tmp = make_vault();
    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "export",
            "notes/sample.md",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.ends_with(b"\n"));
    assert!(!out.stdout.ends_with(b"\n\n"));
}

#[test]
fn short_flag_is_byte_identical_to_long() {
    let tmp = make_vault();
    let long = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "export",
            "notes/sample.md",
            "--lines",
            "7-9",
        ])
        .output()
        .unwrap();
    let short = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "export",
            "notes/sample.md",
            "-l",
            "7-9",
        ])
        .output()
        .unwrap();
    assert!(long.status.success());
    assert!(short.status.success());
    assert_eq!(long.stdout, short.stdout);
    assert_eq!(
        String::from_utf8(short.stdout).unwrap(),
        "# Heading Foo\n\nSee bee and ![img.png](img.png) and #Anchor.\n"
    );
}

#[test]
fn format_flag_defaults_and_validates() {
    let tmp = make_vault();
    let default = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "export",
            "notes/sample.md",
        ])
        .output()
        .unwrap();
    let explicit = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "notes",
            "export",
            "notes/sample.md",
            "--format",
            "commonmark",
        ])
        .output()
        .unwrap();
    assert!(default.status.success());
    assert!(explicit.status.success());
    assert_eq!(default.stdout, explicit.stdout);
    // Unknown targets are rejected by clap.
    export(&tmp, &["notes/sample.md", "--format", "plaintext"])
        .failure()
        .stdout("");
}

#[test]
fn range_after_frontmatter_exports_those_lines() {
    let tmp = make_vault();
    export(&tmp, &["notes/sample.md", "-l", "7-8"])
        .success()
        .stdout("# Heading Foo\n\n");
}

#[test]
fn mixed_range_clamps_start_to_first_body_line() {
    let tmp = make_vault();
    export(&tmp, &["notes/sample.md", "-l", "1-7"])
        .success()
        .stdout("# Heading Foo\n");
}

#[test]
fn range_fully_inside_frontmatter_is_empty_exit_zero() {
    let tmp = make_vault();
    export(&tmp, &["notes/sample.md", "-l", "1-3"])
        .success()
        .stdout("");
}

#[test]
fn blank_line_after_frontmatter_respected() {
    use assert_fs::prelude::*;
    let tmp = make_vault();
    tmp.child("notes/blanky.md")
        .write_str("---\na: 1\n---\n\nL7\n")
        .unwrap();
    // frontmatter lines 1-3, line 4 blank, line 5 = L7.
    export(&tmp, &["notes/blanky.md", "-l", "4-5"])
        .success()
        .stdout("\nL7\n");
}

#[test]
fn no_frontmatter_no_clamp() {
    use assert_fs::prelude::*;
    let tmp = make_vault();
    tmp.child("notes/plain.md").write_str("a\nb\n").unwrap();
    export(&tmp, &["notes/plain.md", "-l", "1-2"])
        .success()
        .stdout("a\nb\n");
}

#[test]
fn callout_header_dropped_body_kept() {
    let tmp = make_vault();
    export(&tmp, &["notes/sample.md", "-l", "14-15"])
        .success()
        .stdout("> quoted Quoted\n");
}

#[test]
fn malformed_callout_header_kept() {
    use assert_fs::prelude::*;
    let tmp = make_vault();
    tmp.child("notes/malformed.md")
        .write_str("> [!ft-source] \"notes/foo.md\"\n> body\n")
        .unwrap();
    export(&tmp, &["notes/malformed.md"])
        .success()
        .stdout("> [!ft-source] \"notes/foo.md\"\n> body\n");
}

#[test]
fn code_spans_untouched() {
    use assert_fs::prelude::*;
    let tmp = make_vault();
    tmp.child("notes/code.md")
        .write_str("`[[Foo]]` real [[Bar]]\n")
        .unwrap();
    export(&tmp, &["notes/code.md"])
        .success()
        .stdout("`[[Foo]]` real Bar\n");
}

#[test]
fn blockquote_wikilinks_converted() {
    let tmp = make_vault();
    export(&tmp, &["notes/sample.md", "-l", "11-12"])
        .success()
        .stdout("> [!note] Keep me\n> see Baz\n");
}

#[test]
fn absolute_path_accepted() {
    let tmp = make_vault();
    let abs = tmp.path().join("notes/sample.md");
    export(&tmp, &[abs.to_str().unwrap(), "-l", "7-7"])
        .success()
        .stdout("# Heading Foo\n");
}

#[test]
fn no_md_auto_append() {
    let tmp = make_vault();
    export(&tmp, &["notes/sample", "-l", "7-7"])
        .failure()
        .stderr(predicates::str::contains(
            "cannot read source file `notes/sample`",
        ))
        .stdout("");
}

#[test]
fn missing_file_errors() {
    let tmp = make_vault();
    export(&tmp, &["does-not-exist.md"])
        .failure()
        .stderr(predicates::str::contains(
            "cannot read source file `does-not-exist.md`",
        ))
        .stdout("");
}

#[test]
fn non_utf8_file_errors() {
    use assert_fs::prelude::*;
    let tmp = make_vault();
    tmp.child("notes/bad.md")
        .write_binary(&[0xFF, 0xFE, 0x00])
        .unwrap();
    export(&tmp, &["notes/bad.md"])
        .failure()
        .stderr(predicates::str::contains(
            "cannot read source file `notes/bad.md`",
        ))
        .stdout("");
}

#[test]
fn range_past_end_errors_with_raw_count() {
    let tmp = make_vault();
    export(&tmp, &["notes/sample.md", "-l", "9-99"])
        .failure()
        .stderr(predicates::str::contains(
            "line range L9-99 outside file `notes/sample.md` (file has 20 lines)",
        ))
        .stdout("");
}

#[test]
fn trailing_newline_is_not_a_line() {
    use assert_fs::prelude::*;
    let tmp = make_vault();
    tmp.child("notes/tiny.md").write_str("a\nb\n").unwrap();
    export(&tmp, &["notes/tiny.md", "-l", "1-2"])
        .success()
        .stdout("a\nb\n");
}

#[test]
fn invalid_lines_argument_errors() {
    let tmp = make_vault();
    for bad in ["1", "a-b", "0-1", "2-1", "1-0", "-1-2"] {
        export(&tmp, &["notes/sample.md", "-l", bad])
            .failure()
            .stdout("");
    }
}

#[test]
fn no_vault_errors() {
    ft().args([
        "--vault",
        "/nonexistent/export-test-vault",
        "notes",
        "export",
        "notes/sample.md",
    ])
    .assert()
    .failure()
    .stdout("");
}

#[test]
fn read_only_guarantee() {
    let tmp = make_vault();
    let before: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    export(&tmp, &["notes/sample.md"]).success();
    let after: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(before.len(), after.len());
    // The source file's bytes are untouched.
    let content = std::fs::read_to_string(tmp.path().join("notes/sample.md")).unwrap();
    assert!(content.contains("[!ft-source]"));
    assert!(content.contains("[[Foo]]"));
}

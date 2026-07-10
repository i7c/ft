//! Integration tests for `ft notes append` — template-driven append to
//! existing notes with end-of-file and section targeting.

use assert_cmd::Command;
use assert_fs::prelude::*;

fn vault() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir
}

fn vault_with_template(name: &str, content: &str) -> assert_fs::TempDir {
    let dir = vault();
    dir.child("templates-ft").create_dir_all().unwrap();
    dir.child("templates-ft")
        .child(name)
        .write_str(content)
        .unwrap();
    dir
}

fn ft() -> Command {
    let mut cmd = Command::cargo_bin("ft").unwrap();
    cmd.env("FT_TODAY", "2026-05-13");
    cmd
}

#[test]
fn append_to_end_of_file() {
    let dir = vault_with_template("meeting.md", "## Meeting\n\n- Agenda\n");
    let target = dir.child("gather.md");
    target.write_str("# Journal\nentry 1\n").unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "append",
        "gather.md",
        "--template",
        "meeting.md",
        "--no-open",
    ])
    .assert()
    .success();

    let content = std::fs::read_to_string(target.path()).unwrap();
    assert!(content.contains("entry 1"), "content: {content:?}");
    assert!(content.contains("## Meeting"), "content: {content:?}");
}

#[test]
fn append_to_section_via_frontmatter() {
    let dir = vault_with_template("meeting.md", "## Meeting\n\n- Agenda\n");
    let target = dir.child("gather.md");
    target
        .write_str(
            "---\nft:\n  append:\n    section: Daily Log\n---\n# Journal\n## Daily Log\nentry\n",
        )
        .unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "append",
        "gather.md",
        "--template",
        "meeting.md",
        "--no-open",
    ])
    .assert()
    .success();

    let content = std::fs::read_to_string(target.path()).unwrap();
    // The "## Meeting" from the template should appear after the Daily Log section.
    assert!(
        content.contains("entry\n## Meeting"),
        "content: {content:?}"
    );
}

#[test]
fn append_with_explicit_section_override() {
    let dir = vault_with_template("meeting.md", "## Meeting\n\n- Agenda\n");
    let target = dir.child("gather.md");
    target
        .write_str("---\nft:\n  append:\n    section: Daily Log\n---\n# Journal\n## Daily Log\ndaily\n## Notes\nnotes\n")
        .unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "append",
        "gather.md",
        "--template",
        "meeting.md",
        "--section",
        "Notes",
        "--no-open",
    ])
    .assert()
    .success();

    let content = std::fs::read_to_string(target.path()).unwrap();
    // Template should appear after "Notes" section, ignoring frontmatter.
    assert!(
        content.contains("notes\n## Meeting"),
        "content: {content:?}"
    );
}

#[test]
fn append_target_not_found() {
    let dir = vault();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "append",
        "nonexistent.md",
        "--template",
        "meeting.md",
        "--no-open",
    ])
    .assert()
    .failure();
}

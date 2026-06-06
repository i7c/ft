use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

fn vault() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir
}

fn run(vault: &std::path::Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut full = vec!["--vault", vault.to_str().unwrap(), "tasks", "move"];
    full.extend(args);
    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-05-10")
        .args(&full)
        .assert()
}

#[test]
fn move_single_by_id_to_new_file() {
    let dir = vault();
    dir.child("inbox.md")
        .write_str("- [ ] keep\n- [ ] move me 🆔 mv1\n")
        .unwrap();

    run(dir.path(), &["mv1", "--to", "triage.md"]).success();

    assert_eq!(
        std::fs::read_to_string(dir.path().join("inbox.md")).unwrap(),
        "- [ ] keep\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("triage.md")).unwrap(),
        "- [ ] move me 🆔 mv1\n"
    );
}

#[test]
fn move_with_heading_creates_section() {
    let dir = vault();
    dir.child("src.md")
        .write_str("- [ ] move me 🆔 hd1\n")
        .unwrap();
    dir.child("triage.md")
        .write_str("# Triage\n\nProse here.\n")
        .unwrap();

    run(dir.path(), &["hd1", "--to", "triage.md#Inbox"]).success();

    assert_eq!(
        std::fs::read_to_string(dir.path().join("triage.md")).unwrap(),
        "# Triage\n\nProse here.\n\n## Inbox\n- [ ] move me 🆔 hd1\n"
    );
}

#[test]
fn move_subtree_takes_children() {
    let dir = vault();
    dir.child("src.md")
        .write_str(
            "- [ ] keep top\n\
             - [ ] parent 🆔 par1\n  - [ ] child A\n  - [ ] child B\n\
             - [ ] keep bottom\n",
        )
        .unwrap();

    run(dir.path(), &["par1", "--to", "out.md"]).success();

    assert_eq!(
        std::fs::read_to_string(dir.path().join("src.md")).unwrap(),
        "- [ ] keep top\n- [ ] keep bottom\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("out.md")).unwrap(),
        "- [ ] parent 🆔 par1\n  - [ ] child A\n  - [ ] child B\n",
    );
}

#[test]
fn move_dry_run_does_not_modify_files() {
    let dir = vault();
    let inbox = dir.child("inbox.md");
    inbox.write_str("- [ ] move me 🆔 dryid\n").unwrap();

    let mtime_before = std::fs::metadata(inbox.path()).unwrap().modified().unwrap();
    // Sleep a tick so any rewrite would be visible.
    std::thread::sleep(std::time::Duration::from_millis(10));

    let assert = run(dir.path(), &["dryid", "--to", "triage.md", "--dry-run"]).success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("--- inbox.md"),
        "expected diff header; got:\n{stdout}"
    );
    assert!(stdout.contains("+++ triage.md"));
    assert!(
        stdout.contains("-- [ ] move me 🆔 dryid") || stdout.contains("- [ ] move me 🆔 dryid"),
        "diff body should reference the moved task; got:\n{stdout}"
    );

    let mtime_after = std::fs::metadata(inbox.path()).unwrap().modified().unwrap();
    assert_eq!(mtime_before, mtime_after, "dry-run must not touch files");
    assert!(
        !dir.path().join("triage.md").exists(),
        "dry-run must not create the target"
    );
}

#[test]
fn move_bulk_query_with_yes() {
    let dir = vault();
    dir.child("a.md")
        .write_str("- [ ] task1 #legacy\n- [ ] keepme\n")
        .unwrap();
    dir.child("b.md")
        .write_str("- [ ] task2 #legacy\n")
        .unwrap();

    run(
        dir.path(),
        &[
            "--query",
            "tags includes \"legacy\"",
            "--to",
            "triage.md",
            "--yes",
        ],
    )
    .success();

    // Both tagged tasks moved, untagged kept.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.md")).unwrap(),
        "- [ ] keepme\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.md")).unwrap(),
        ""
    );
    let triage = std::fs::read_to_string(dir.path().join("triage.md")).unwrap();
    assert!(triage.contains("task1 #legacy"));
    assert!(triage.contains("task2 #legacy"));
}

#[test]
fn move_bulk_without_yes_in_non_tty_errors() {
    let dir = vault();
    dir.child("a.md")
        .write_str("- [ ] task1 #legacy\n- [ ] task2 #legacy\n")
        .unwrap();

    run(
        dir.path(),
        &["--query", "tags includes \"legacy\"", "--to", "out.md"],
    )
    .failure()
    .stderr(predicate::str::contains("--yes"));
}

#[test]
fn move_no_match_errors() {
    let dir = vault();
    dir.child("a.md").write_str("- [ ] real task\n").unwrap();
    run(dir.path(), &["nope", "--to", "out.md"])
        .failure()
        .stderr(predicate::str::contains("no tasks match"));
}

#[test]
fn move_idempotent_no_op_on_second_run() {
    let dir = vault();
    dir.child("src.md")
        .write_str("- [ ] move 🆔 once\n")
        .unwrap();

    run(dir.path(), &["once", "--to", "out.md"]).success();
    let out_after_first = std::fs::read_to_string(dir.path().join("out.md")).unwrap();

    // Second run: the task is no longer at src.md (it's in out.md). The
    // selector resolves to the moved task, which is now at out.md → moving
    // it to out.md is a no-op.
    run(dir.path(), &["once", "--to", "out.md"]).success();
    let out_after_second = std::fs::read_to_string(dir.path().join("out.md")).unwrap();
    assert_eq!(out_after_first, out_after_second);
}

#[test]
fn move_within_same_file_under_heading() {
    let dir = vault();
    dir.child("f.md")
        .write_str("## Inbox\n- [ ] move me 🆔 same\n\n## Done\n")
        .unwrap();

    run(dir.path(), &["same", "--to", "f.md#Done"]).success();

    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.md")).unwrap(),
        "## Inbox\n\n## Done\n- [ ] move me 🆔 same\n"
    );
}

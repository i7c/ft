use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

fn vault() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child("proj.md")
        .write_str("- [ ] Vendor onboarding ^vo1\n    - [ ] sign NDA\n")
        .unwrap();
    dir.child("journal/2026-10-21.md")
        .write_str("# Day\n- [ ] reproduce 📅 2026-10-23\n- [ ] call legal\n")
        .unwrap();
    dir.child("jira/ABC-123.md")
        .write_str("---\nft:\n  tasks:\n    anchor: ABC-123\n---\n- [ ] Fix login 🆔 ABC-123\n")
        .unwrap();
    dir
}

fn ft(vault: &std::path::Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut full = vec!["--vault", vault.to_str().unwrap(), "tasks"];
    full.extend(args);
    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-10-21")
        .args(&full)
        .assert()
}

fn read(dir: &assert_fs::TempDir, rel: &str) -> String {
    std::fs::read_to_string(dir.path().join(rel)).unwrap()
}

const JOURNAL: &str = "journal/2026-10-21.md";

#[test]
fn link_two_children_and_show_tree() {
    let dir = vault();
    ft(
        dir.path(),
        &["link", "reproduce", "call legal", "--under", "proj.md:1"],
    )
    .success()
    .stdout("Linked 2 task(s) under proj.md:1\n");
    assert_eq!(
        read(&dir, JOURNAL),
        "# Day\n- [ ] reproduce [[proj#^vo1]] 📅 2026-10-23\n- [ ] call legal [[proj#^vo1]]\n"
    );
    ft(
        dir.path(),
        &[
            "list",
            "--tree",
            "--query",
            "status = Open",
            "--format",
            "json",
        ],
    )
    .success();
    let out = ft(
        dir.path(),
        &[
            "list",
            "--tree",
            "--format",
            "markdown",
            "--query",
            "description includes \"Vendor\"",
        ],
    )
    .success()
    .get_output()
    .stdout
    .clone();
    let out = String::from_utf8(out).unwrap();
    assert!(
        out.contains("reproduce") && out.contains("call legal") && out.contains("sign NDA"),
        "{out}"
    );
}

#[test]
fn link_json_and_dry_run() {
    let dir = vault();
    ft(
        dir.path(),
        &["link", "reproduce", "--under", "proj.md:1", "--dry-run"],
    )
    .success()
    .stdout(predicate::str::contains("+- [ ] reproduce [[proj#^vo1]]"));
    assert_eq!(
        read(&dir, JOURNAL),
        "# Day\n- [ ] reproduce 📅 2026-10-23\n- [ ] call legal\n"
    );
    let out = ft(
        dir.path(),
        &[
            "link",
            "reproduce",
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
fn link_errors() {
    let dir = vault();
    ft(dir.path(), &["link", "reproduce", "--under", "o"])
        .failure()
        .stderr(predicate::str::contains("--under:"));
    ft(dir.path(), &["link", "sign NDA", "--under", "Fix login"])
        .failure()
        .stderr(predicate::str::contains("nested under another task"));
    ft(dir.path(), &["link", "Vendor", "--under", "Vendor"])
        .failure()
        .stderr(predicate::str::contains("own subtask (proj.md:1)"));
}

#[test]
fn unlink_round_trip() {
    let dir = vault();
    ft(dir.path(), &["link", "reproduce", "--under", "proj.md:1"]).success();
    ft(dir.path(), &["unlink", "reproduce"])
        .success()
        .stdout(format!("Unlinked {JOURNAL}:2\n"));
    assert_eq!(
        read(&dir, JOURNAL),
        "# Day\n- [ ] reproduce 📅 2026-10-23\n- [ ] call legal\n"
    );
    ft(dir.path(), &["unlink", "reproduce"])
        .failure()
        .stderr(predicate::str::contains(format!(
            "{JOURNAL}:2 has no parent link"
        )));
}

#[test]
fn create_with_link_parent() {
    let dir = vault();
    ft(
        dir.path(),
        &[
            "create",
            "write regression test",
            "--link-parent",
            "ABC-123",
            "--file",
            JOURNAL,
        ],
    )
    .success();
    assert!(read(&dir, JOURNAL).contains("- [ ] write regression test [[ABC-123]] ➕ 2026-10-21\n"));
    // Block-ref parent in another file gets the link; parent unchanged.
    ft(
        dir.path(),
        &[
            "create",
            "book room",
            "--link-parent",
            "proj.md:1",
            "--file",
            JOURNAL,
        ],
    )
    .success();
    assert!(read(&dir, JOURNAL).contains("- [ ] book room [[proj#^vo1]] ➕ 2026-10-21\n"));
    ft(
        dir.path(),
        &["create", "x", "--link-parent", "a", "--parent", "b"],
    )
    .failure();
}

#[test]
fn check_reports_issues_and_exit_code() {
    let dir = vault();
    ft(dir.path(), &["check"]).success().stdout("no issues\n");
    dir.child("bad.md")
        .write_str("- [ ] gone [[proj#^nope]]\n- [ ] two [[proj#^vo1]] [[ABC-123]]\n- [ ] P\n  - [ ] nested [[proj#^vo1]]\n")
        .unwrap();
    let out = ft(dir.path(), &["check", "--format", "json"])
        .code(1)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let kinds: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["dangling", "extra", "ignored"]);
    assert_eq!(v[0]["path"], "bad.md");
    assert_eq!(v[0]["link"], "[[proj#^nope]]");
}

#[test]
fn move_retargets_links_and_reports_them() {
    let dir = vault();
    ft(dir.path(), &["link", "reproduce", "--under", "proj.md:1"]).success();
    ft(dir.path(), &["move", "proj.md:1", "--to", "archive.md"])
        .success()
        .stdout("Moved 1 task(s) → archive.md (retargeted 1 link(s))\n");
    assert!(read(&dir, JOURNAL).contains("reproduce [[archive#^vo1]]"));
    ft(dir.path(), &["check"]).success();
}

#[test]
fn paste_like_move_under_strips_link() {
    let dir = vault();
    ft(dir.path(), &["link", "reproduce", "--under", "proj.md:1"]).success();
    ft(dir.path(), &["move", "reproduce", "--under", "Fix login"]).success();
    assert_eq!(
        read(&dir, "jira/ABC-123.md"),
        "---\nft:\n  tasks:\n    anchor: ABC-123\n---\n- [ ] Fix login 🆔 ABC-123\n  - [ ] reproduce 📅 2026-10-23\n"
    );
}

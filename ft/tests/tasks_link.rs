use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

fn vault() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child("proj.md")
        .write_str("- [ ] Vendor onboarding 🆔 vo1 ^vb\n    - [ ] sign NDA\n")
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

fn stdout(a: assert_cmd::assert::Assert) -> String {
    String::from_utf8(a.get_output().stdout.clone()).unwrap()
}

fn json(a: assert_cmd::assert::Assert) -> serde_json::Value {
    serde_json::from_slice(&a.get_output().stdout).unwrap()
}

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
        "# Day\n- [ ] reproduce ⤴ vo1 📅 2026-10-23\n- [ ] call legal ⤴ vo1\n"
    );
    let out = stdout(
        ft(
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
        .success(),
    );
    assert!(
        out.contains("reproduce") && out.contains("call legal") && out.contains("sign NDA"),
        "{out}"
    );
}

#[test]
fn second_parent_repeats_the_task_in_trees() {
    let dir = vault();
    ft(dir.path(), &["link", "reproduce", "--under", "proj.md:1"]).success();
    ft(dir.path(), &["link", "reproduce", "--under", "ABC-123"])
        .success()
        .stdout("Linked 1 task(s) under jira/ABC-123.md:6\n");
    assert!(read(&dir, JOURNAL).contains("- [ ] reproduce ⤴ vo1,ABC-123 📅 2026-10-23\n"));
    // Linking again changes nothing.
    ft(dir.path(), &["link", "reproduce", "--under", "ABC-123"])
        .success()
        .stdout("Linked 0 task(s) under jira/ABC-123.md:6 (1 already linked)\n");
    let out = stdout(
        ft(
            dir.path(),
            &[
                "list",
                "--tree",
                "--format",
                "markdown",
                "--query",
                "description includes \"Vendor\" or description includes \"Fix login\"",
            ],
        )
        .success(),
    );
    assert_eq!(out.matches("reproduce").count(), 2, "{out}");
    let v = json(
        ft(
            dir.path(),
            &[
                "list",
                "--query",
                "description includes \"reproduce\"",
                "--format",
                "json",
            ],
        )
        .success(),
    );
    let parents = v[0]["parents"].as_array().unwrap();
    assert_eq!(parents.len(), 2, "{v}");
    assert_eq!(parents[1]["id"], "ABC-123");
    assert_eq!(parents[1]["path"], "jira/ABC-123.md");
    assert_eq!(parents[1]["state"], "resolved");
}

#[test]
fn link_assigns_an_id_to_a_parent_without_one() {
    let dir = vault();
    ft(dir.path(), &["link", "reproduce", "--under", "call legal"]).success();
    let journal = read(&dir, JOURNAL);
    let id = journal
        .split("🆔 ")
        .nth(1)
        .and_then(|r| r.split_whitespace().next())
        .expect("call legal got an id")
        .to_string();
    assert_eq!(id.len(), 6);
    assert_eq!(
        journal,
        format!("# Day\n- [ ] reproduce ⤴ {id} 📅 2026-10-23\n- [ ] call legal 🆔 {id}\n")
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
    .stdout(predicate::str::contains("+- [ ] reproduce ⤴ vo1"));
    assert_eq!(
        read(&dir, JOURNAL),
        "# Day\n- [ ] reproduce 📅 2026-10-23\n- [ ] call legal\n"
    );
    let dry = json(
        ft(
            dir.path(),
            &[
                "link",
                "reproduce",
                "--under",
                "proj.md:1",
                "--dry-run",
                "--format",
                "json",
            ],
        )
        .success(),
    );
    let v = json(
        ft(
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
        .success(),
    );
    assert_eq!(dry, v);
    insta::assert_snapshot!(serde_json::to_string_pretty(&v).unwrap());
}

#[test]
fn link_errors() {
    let dir = vault();
    ft(dir.path(), &["link", "reproduce", "--under", "o"])
        .failure()
        .stderr(predicate::str::contains("--under:"));
    ft(dir.path(), &["link", "sign NDA", "--under", "proj.md:1"])
        .failure()
        .stderr(predicate::str::contains(
            "sign NDA is already nested under it",
        ));
    ft(dir.path(), &["link", "Vendor", "--under", "Vendor"])
        .failure()
        .stderr(predicate::str::contains("own subtask (proj.md:1)"));
    // A nested task can still gain a logical parent elsewhere.
    ft(dir.path(), &["link", "sign NDA", "--under", "Fix login"]).success();
    assert!(read(&dir, "proj.md").contains("    - [ ] sign NDA ⤴ ABC-123\n"));
}

#[test]
fn unlink_one_of_several() {
    let dir = vault();
    ft(dir.path(), &["link", "reproduce", "--under", "proj.md:1"]).success();
    ft(dir.path(), &["link", "reproduce", "--under", "ABC-123"]).success();
    ft(dir.path(), &["unlink", "reproduce"])
        .code(2)
        .stderr(predicate::str::contains(
            "has several parents — pass --from SELECTOR, --id ID or --all",
        ))
        .stderr(predicate::str::contains(
            "ABC-123  jira/ABC-123.md:6  Fix login",
        ));
    let out = Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-10-21")
        .args(["--json-errors", "--vault", dir.path().to_str().unwrap()])
        .args(["tasks", "unlink", "reproduce"])
        .assert()
        .code(2)
        .get_output()
        .stderr
        .clone();
    let err: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(err["refs"][0]["id"], "vo1");
    assert_eq!(err["refs"][1]["path"], "jira/ABC-123.md");
    ft(dir.path(), &["unlink", "reproduce", "--from", "Fix login"])
        .success()
        .stdout(format!("Unlinked {JOURNAL}:2\n"));
    assert!(read(&dir, JOURNAL).contains("- [ ] reproduce ⤴ vo1 📅"));
    ft(dir.path(), &["unlink", "reproduce", "--from", "Fix login"])
        .failure()
        .stderr(predicate::str::contains("not linked to that parent"));
    ft(dir.path(), &["unlink", "reproduce"]).success();
    assert_eq!(
        read(&dir, JOURNAL),
        "# Day\n- [ ] reproduce 📅 2026-10-23\n- [ ] call legal\n"
    );
    ft(dir.path(), &["unlink", "reproduce"])
        .failure()
        .stderr(predicate::str::contains(format!(
            "{JOURNAL}:2 has no parent link"
        )));
    ft(dir.path(), &["link", "call legal", "--under", "proj.md:1"]).success();
    ft(dir.path(), &["link", "call legal", "--under", "ABC-123"]).success();
    std::fs::write(
        dir.path().join(JOURNAL),
        read(&dir, JOURNAL).replace("⤴ vo1,ABC-123", "⤴ vo1,ABC-123,gone"),
    )
    .unwrap();
    ft(dir.path(), &["unlink", "call legal", "--id", "gone"]).success();
    assert!(read(&dir, JOURNAL).ends_with("- [ ] call legal ⤴ vo1,ABC-123\n"));
    ft(dir.path(), &["unlink", "call legal", "--all"]).success();
    assert!(read(&dir, JOURNAL).ends_with("- [ ] call legal\n"));
}

#[test]
fn create_with_link_parents() {
    let dir = vault();
    ft(
        dir.path(),
        &[
            "create",
            "write regression test",
            "--link-parent",
            "ABC-123",
            "--link-parent",
            "proj.md:1",
            "--file",
            JOURNAL,
        ],
    )
    .success();
    assert!(
        read(&dir, JOURNAL).contains("- [ ] write regression test ⤴ ABC-123,vo1 ➕ 2026-10-21\n")
    );
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
        .write_str("- [ ] gone ⤴ nope\n- [ ] copy 🆔 vo1\n- [ ] amb ⤴ vo1\n- [ ] P 🆔 pp\n  - [ ] nested ⤴ pp\n- [ ] S 🆔 ss ⤴ ss\n")
        .unwrap();
    let v = json(ft(dir.path(), &["check", "--format", "json"]).code(1));
    let kinds: Vec<(&str, u64)> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["kind"].as_str().unwrap(), e["line"].as_u64().unwrap()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("dangling", 1),
            ("ambiguous", 3),
            ("redundant", 5),
            ("cycle", 6),
            ("duplicate-id", 1),
        ]
    );
    assert_eq!(v[0]["path"], "bad.md");
    assert_eq!(v[0]["id"], "nope");
    assert_eq!(v[4]["path"], "proj.md");
}

#[test]
fn move_keeps_parents_and_retargets_block_refs() {
    let dir = vault();
    dir.child("notes.md")
        .write_str("see [[proj#^vb]]\n")
        .unwrap();
    ft(dir.path(), &["link", "reproduce", "--under", "proj.md:1"]).success();
    ft(dir.path(), &["move", "proj.md:1", "--to", "archive.md"])
        .success()
        .stdout("Moved 1 task(s) → archive.md (retargeted 1 link(s))\n");
    assert_eq!(read(&dir, "notes.md"), "see [[archive#^vb]]\n");
    assert!(read(&dir, JOURNAL).contains("reproduce ⤴ vo1"));
    ft(dir.path(), &["check"]).success();
}

#[test]
fn move_under_strips_only_the_new_parent() {
    let dir = vault();
    ft(dir.path(), &["link", "reproduce", "--under", "proj.md:1"]).success();
    ft(dir.path(), &["link", "reproduce", "--under", "ABC-123"]).success();
    ft(dir.path(), &["move", "reproduce", "--under", "Fix login"]).success();
    assert_eq!(
        read(&dir, "jira/ABC-123.md"),
        "---\nft:\n  tasks:\n    anchor: ABC-123\n---\n- [ ] Fix login 🆔 ABC-123\n  - [ ] reproduce ⤴ vo1 📅 2026-10-23\n"
    );
}

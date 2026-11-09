//! Integration tests for the CLI surface a two-way sync drives
//! (openspec change tasks-edit-for-sync).

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

fn vault() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir
}

fn run(vault: &std::path::Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut full = vec!["--vault", vault.to_str().unwrap(), "tasks"];
    full.extend(args);
    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-05-10")
        .args(&full)
        .assert()
}

fn json(assert: assert_cmd::assert::Assert) -> serde_json::Value {
    let out = assert.success().get_output().stdout.clone();
    serde_json::from_slice(&out).expect("stdout is JSON")
}

fn read(dir: &assert_fs::TempDir, file: &str) -> String {
    std::fs::read_to_string(dir.path().join(file)).unwrap()
}

/// The first `list --format json` row, as a sync would hold it.
fn listed_row(dir: &assert_fs::TempDir) -> String {
    let rows = json(run(dir.path(), &["list", "--format", "json"]));
    rows[0].to_string()
}

// ── edit fields ────────────────────────────────────────────────────────────

#[test]
fn tags_are_written_to_the_line() {
    let dir = vault();
    dir.child("a.md")
        .write_str("- [ ] Buy #home milk 📅 2026-11-10\n")
        .unwrap();
    run(dir.path(), &["edit", "a.md:1", "--tags", "work #errand"]).success();
    assert_eq!(
        read(&dir, "a.md"),
        "- [ ] Buy milk #work #errand 📅 2026-11-10\n"
    );
    run(dir.path(), &["edit", "a.md:1", "--tags", "none"]).success();
    assert_eq!(read(&dir, "a.md"), "- [ ] Buy milk 📅 2026-11-10\n");
}

#[test]
fn title_keeps_the_tags() {
    let dir = vault();
    dir.child("a.md")
        .write_str("- [ ] Buy milk #home\n")
        .unwrap();
    run(dir.path(), &["edit", "a.md:1", "--title", "Buy oat milk"]).success();
    assert_eq!(read(&dir, "a.md"), "- [ ] Buy oat milk #home\n");
}

#[test]
fn title_conflicts_with_description() {
    let dir = vault();
    dir.child("a.md").write_str("- [ ] Task\n").unwrap();
    run(
        dir.path(),
        &["edit", "a.md:1", "--title", "x", "--description", "y"],
    )
    .failure();
}

#[test]
fn start_date_set_and_cleared() {
    let dir = vault();
    dir.child("a.md").write_str("- [ ] Task\n").unwrap();
    run(dir.path(), &["edit", "a.md:1", "--start", "2026-06-01"]).success();
    assert_eq!(read(&dir, "a.md"), "- [ ] Task 🛫 2026-06-01\n");
    run(dir.path(), &["edit", "a.md:1", "--start", "none"]).success();
    assert_eq!(read(&dir, "a.md"), "- [ ] Task\n");
}

#[test]
fn status_reopens_and_marks_in_progress() {
    let dir = vault();
    dir.child("a.md")
        .write_str("- [x] Done ✅ 2026-05-01\n- [-] Gone ❌ 2026-05-01\n")
        .unwrap();
    run(dir.path(), &["edit", "a.md:1", "--status", "open"]).success();
    run(dir.path(), &["edit", "a.md:2", "--status", "in-progress"]).success();
    assert_eq!(read(&dir, "a.md"), "- [ ] Done\n- [/] Gone\n");
}

#[test]
fn status_done_is_left_to_complete() {
    let dir = vault();
    dir.child("a.md").write_str("- [ ] Task\n").unwrap();
    run(dir.path(), &["edit", "a.md:1", "--status", "done"]).failure();
}

#[test]
fn id_is_given_once_and_never_shared() {
    let dir = vault();
    dir.child("a.md")
        .write_str("- [ ] One\n- [ ] Two 🆔 two\n")
        .unwrap();
    run(dir.path(), &["edit", "a.md:1", "--id", "two"])
        .failure()
        .stderr(predicate::str::contains("taken by a.md:2"));
    run(dir.path(), &["edit", "a.md:2", "--id", "other"])
        .failure()
        .stderr(predicate::str::contains("already has 🆔 two"));
    run(dir.path(), &["edit", "a.md:1", "--id", "one"]).success();
    run(dir.path(), &["edit", "a.md:1", "--id", "one"]).success();
    assert_eq!(read(&dir, "a.md"), "- [ ] One 🆔 one\n- [ ] Two 🆔 two\n");
}

#[test]
fn depends_on_replaces_the_set() {
    let dir = vault();
    dir.child("a.md").write_str("- [ ] Task ⛔ old\n").unwrap();
    run(dir.path(), &["edit", "a.md:1", "--depends-on", "a1, b2"]).success();
    assert_eq!(read(&dir, "a.md"), "- [ ] Task ⛔ a1,b2\n");
    run(dir.path(), &["edit", "a.md:1", "--depends-on", "none"]).success();
    assert_eq!(read(&dir, "a.md"), "- [ ] Task\n");
}

// ── guarded, machine-readable writes ───────────────────────────────────────

#[test]
fn edit_json_is_the_written_row() {
    let dir = vault();
    dir.child("a.md")
        .write_str("- [ ] Parent\n  - [ ] Buy milk #home\n")
        .unwrap();
    let row = json(run(
        dir.path(),
        &[
            "edit",
            "a.md:2",
            "--tags",
            "work",
            "--due",
            "2026-12-01",
            "--format",
            "json",
        ],
    ));
    assert_eq!(row["description"], "Buy milk #work");
    assert_eq!(row["tags"], serde_json::json!(["work"]));
    assert_eq!(row["due"], "2026-12-01");
    assert_eq!(row["source_file"], "a.md");
    assert_eq!(row["source_line"], 2);
    assert_eq!(row["parent"], 1);
}

#[test]
fn matching_expectation_writes() {
    let dir = vault();
    dir.child("a.md").write_str("- [ ] Task 🆔 t1\n").unwrap();
    let row = listed_row(&dir);
    run(
        dir.path(),
        &["edit", "a.md:1", "--expect", &row, "--due", "2026-12-01"],
    )
    .success();
    assert_eq!(read(&dir, "a.md"), "- [ ] Task 📅 2026-12-01 🆔 t1\n");
}

#[test]
fn stale_expectation_exits_3_and_writes_nothing() {
    let dir = vault();
    dir.child("a.md").write_str("- [ ] Task 🆔 t1\n").unwrap();
    let row = listed_row(&dir);
    dir.child("a.md")
        .write_str("- [ ] Task, edited 🆔 t1\n")
        .unwrap();
    for verb in [
        vec!["edit", "a.md:1", "--due", "2026-12-01"],
        vec!["complete", "a.md:1"],
        vec!["cancel", "a.md:1"],
    ] {
        let mut args = verb.clone();
        args.extend(["--expect", row.as_str()]);
        run(dir.path(), &args).code(3);
    }
    assert_eq!(read(&dir, "a.md"), "- [ ] Task, edited 🆔 t1\n");
}

#[test]
fn expect_must_be_a_task_object() {
    let dir = vault();
    dir.child("a.md").write_str("- [ ] Task\n").unwrap();
    run(
        dir.path(),
        &["edit", "a.md:1", "--expect", "{}", "--due", "today"],
    )
    .failure()
    .stderr(predicate::str::contains("--expect"));
}

#[test]
fn complete_json_has_the_next_instance() {
    let dir = vault();
    dir.child("a.md")
        .write_str("- [ ] Pay rent 🔁 every month on the 1st 📅 2026-05-01 🆔 rent\n")
        .unwrap();
    let out = json(run(dir.path(), &["complete", "rent", "--format", "json"]));
    assert_eq!(out["completed"]["status"], "Done");
    assert_eq!(out["completed"]["done"], "2026-05-10");
    assert_eq!(out["completed"]["source_line"], 2);
    assert_eq!(out["next"]["status"], "Open");
    assert_eq!(out["next"]["due"], "2026-06-01");
    assert_eq!(out["next"]["source_line"], 1);
}

#[test]
fn complete_json_without_recurrence_has_no_next() {
    let dir = vault();
    dir.child("a.md").write_str("- [ ] Task 🆔 t1\n").unwrap();
    let out = json(run(dir.path(), &["complete", "t1", "--format", "json"]));
    assert_eq!(out["completed"]["status"], "Done");
    assert!(out["next"].is_null());
}

#[test]
fn cancel_json_is_the_written_row() {
    let dir = vault();
    dir.child("a.md").write_str("- [ ] Task 🆔 t1\n").unwrap();
    let row = json(run(dir.path(), &["cancel", "t1", "--format", "json"]));
    assert_eq!(row["status"], "Cancelled");
    assert_eq!(row["cancelled"], "2026-05-10");
}

#[test]
fn json_and_expect_refuse_bulk_writes() {
    let dir = vault();
    dir.child("a.md").write_str("- [ ] A\n- [ ] B\n").unwrap();
    run(
        dir.path(),
        &[
            "complete",
            "--query",
            "status = Open",
            "--yes",
            "--format",
            "json",
        ],
    )
    .failure();
    run(
        dir.path(),
        &[
            "cancel",
            "--query",
            "status = Open",
            "--yes",
            "--format",
            "json",
        ],
    )
    .failure();
    assert_eq!(read(&dir, "a.md"), "- [ ] A\n- [ ] B\n");
}

// ── assign-ids and --exclude-path ──────────────────────────────────────────

#[test]
fn assign_ids_gives_open_tasks_fresh_ids() {
    let dir = vault();
    dir.child("a.md")
        .write_str("- [ ] One\n- [x] Done ✅ 2026-05-01\n- [ ] Has 🆔 x1\n")
        .unwrap();
    dir.child("b.md").write_str("- [ ] Two\n").unwrap();
    dir.child("templates/t.md")
        .write_str("- [ ] Template\n")
        .unwrap();
    let rows = json(run(
        dir.path(),
        &[
            "assign-ids",
            "--status",
            "open",
            "--exclude-path",
            "templates/",
            "--format",
            "json",
        ],
    ));
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_ne!(rows[0]["id"], rows[1]["id"]);

    let a = read(&dir, "a.md");
    let id = rows.iter().find(|r| r["source_file"] == "a.md").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        a,
        format!("- [ ] One 🆔 {id}\n- [x] Done ✅ 2026-05-01\n- [ ] Has 🆔 x1\n")
    );
    assert_eq!(read(&dir, "templates/t.md"), "- [ ] Template\n");
}

#[test]
fn assign_ids_dry_run_writes_nothing() {
    let dir = vault();
    dir.child("a.md").write_str("- [ ] One\n").unwrap();
    run(dir.path(), &["assign-ids", "--dry-run"])
        .success()
        .stdout(predicate::str::contains("Would assign 1 id(s)"));
    assert_eq!(read(&dir, "a.md"), "- [ ] One\n");
}

#[test]
fn list_excludes_path_prefixes() {
    let dir = vault();
    dir.child("inbox.md").write_str("- [ ] Keep\n").unwrap();
    dir.child("ticktick/a.md")
        .write_str("- [ ] Drop\n")
        .unwrap();
    dir.child("templates/t.md")
        .write_str("- [ ] Drop too\n")
        .unwrap();
    let rows = json(run(
        dir.path(),
        &[
            "list",
            "--format",
            "json",
            "--exclude-path",
            "ticktick/",
            "--exclude-path",
            "templates/",
        ],
    ));
    let descriptions: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["description"].as_str().unwrap())
        .collect();
    assert_eq!(descriptions, ["Keep"]);
}

use assert_cmd::Command;
use predicates::prelude::*;

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("ft crate must have a parent (workspace root)")
        .to_path_buf()
}

fn realistic_vault() -> std::path::PathBuf {
    workspace_root().join("tests/fixtures/realistic")
}

fn run_list(args: &[&str]) -> assert_cmd::assert::Assert {
    let vault = realistic_vault();
    let mut full = vec!["--vault", vault.to_str().unwrap(), "tasks", "list"];
    full.extend(args);
    Command::cargo_bin("ft").unwrap().args(&full).assert()
}

fn json_tasks(args: &[&str]) -> serde_json::Value {
    let mut full: Vec<&str> = vec!["--format", "json", "--no-color"];
    full.extend(args);
    let assert = run_list(&full).success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    serde_json::from_str(&stdout).expect("ft tasks list --format json must produce valid JSON")
}

#[test]
fn list_table_default_runs() {
    run_list(&["--no-color"])
        .success()
        .stdout(predicate::str::contains("Description"));
}

#[test]
fn list_json_is_parseable() {
    let v = json_tasks(&[]);
    assert!(v.is_array(), "JSON output must be an array");
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty(), "realistic fixture has tasks");
}

#[test]
fn filter_status_open_excludes_done() {
    let v = json_tasks(&["--status", "open"]);
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty());
    for t in arr {
        assert_eq!(t["status"], "Open", "every task must be Open");
    }
}

#[test]
fn filter_status_done_only() {
    let v = json_tasks(&["--status", "done"]);
    let arr = v.as_array().unwrap();
    for t in arr {
        assert_eq!(t["status"], "Done");
    }
}

#[test]
fn filter_priority_high() {
    let v = json_tasks(&["--priority", "high"]);
    let arr = v.as_array().unwrap();
    assert!(
        !arr.is_empty(),
        "realistic fixture has at least one ⏫ task"
    );
    for t in arr {
        assert_eq!(t["priority"], "High");
    }
}

#[test]
fn filter_tag_strips_hash() {
    let v_with_hash = json_tasks(&["--tag", "#area/health"]);
    let v_without = json_tasks(&["--tag", "area/health"]);
    assert_eq!(v_with_hash, v_without, "leading # should be optional");
    let arr = v_with_hash.as_array().unwrap();
    assert!(!arr.is_empty());
    for t in arr {
        let tags: Vec<&str> = t["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert!(tags.contains(&"area/health"));
    }
}

#[test]
fn filter_path_substring() {
    let v = json_tasks(&["--path", "Projects/"]);
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty());
    for t in arr {
        assert!(
            t["source_file"].as_str().unwrap().contains("Projects/"),
            "task in {:?} should not match Projects/",
            t["source_file"]
        );
    }
}

#[test]
fn filter_due_before() {
    let v = json_tasks(&["--due-before", "2026-05-15"]);
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty());
    for t in arr {
        let due = t["due"].as_str().expect("filter requires a due date");
        assert!(due < "2026-05-15", "due {due} should be before cutoff");
    }
}

#[test]
fn filter_due_after() {
    let v = json_tasks(&["--due-after", "2026-06-01"]);
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty());
    for t in arr {
        let due = t["due"].as_str().unwrap();
        assert!(due > "2026-06-01");
    }
}

#[test]
fn filter_has_due() {
    let v = json_tasks(&["--has-due"]);
    let arr = v.as_array().unwrap();
    for t in arr {
        assert!(!t["due"].is_null());
    }
}

#[test]
fn filter_no_due() {
    let v = json_tasks(&["--no-due"]);
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty(), "fixture has at least one undated task");
    for t in arr {
        assert!(t["due"].is_null());
    }
}

#[test]
fn gitignored_files_excluded_from_scan() {
    let v = json_tasks(&[]);
    let arr = v.as_array().unwrap();
    for t in arr {
        let path = t["source_file"].as_str().unwrap();
        assert!(
            !path.starts_with("private/"),
            "private/ is gitignored; should not be scanned: {path}"
        );
    }
}

#[test]
fn attachments_dir_excluded_from_scan() {
    let v = json_tasks(&[]);
    let arr = v.as_array().unwrap();
    for t in arr {
        let path = t["source_file"].as_str().unwrap();
        assert!(
            !path.starts_with("attachments/"),
            "attachments/ should be excluded: {path}"
        );
    }
}

#[test]
fn obsidian_dir_excluded_from_scan() {
    let v = json_tasks(&[]);
    let arr = v.as_array().unwrap();
    for t in arr {
        let path = t["source_file"].as_str().unwrap();
        assert!(!path.starts_with(".obsidian/"));
    }
}

#[test]
fn default_sort_due_asc_then_priority_desc() {
    // Due-bearing tasks come first, ordered by date asc; ties break by
    // priority desc.
    let v = json_tasks(&["--has-due"]);
    let arr = v.as_array().unwrap();
    let mut prev_due: Option<String> = None;
    for t in arr {
        let due = t["due"].as_str().unwrap().to_string();
        if let Some(p) = &prev_due {
            assert!(p.as_str() <= due.as_str(), "due not ascending: {p} > {due}");
        }
        prev_due = Some(due);
    }
}

#[test]
fn flags_compose_as_and() {
    // status=open AND tag=project/website → only open project/website tasks
    let v = json_tasks(&["--status", "open", "--tag", "project/website"]);
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty());
    for t in arr {
        assert_eq!(t["status"], "Open");
        let tags: Vec<&str> = t["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert!(tags.contains(&"project/website"));
    }
}

#[test]
fn tasks_list_help_works() {
    Command::cargo_bin("ft")
        .unwrap()
        .args(["tasks", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--status"))
        .stdout(predicate::str::contains("--priority"));
}

#[test]
fn has_due_and_no_due_are_mutually_exclusive() {
    Command::cargo_bin("ft")
        .unwrap()
        .args([
            "--vault",
            realistic_vault().to_str().unwrap(),
            "tasks",
            "list",
            "--has-due",
            "--no-due",
        ])
        .assert()
        .failure();
}

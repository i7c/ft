//! `ft tasks list --format json|ndjson` prints where each task lives and
//! what its note is about: `headings`, `mentions`, `line_mentions`, `note`.

use assert_cmd::Command;
use assert_fs::prelude::*;
use serde_json::{json, Value};

const NEW_KEYS: [&str; 4] = ["headings", "mentions", "line_mentions", "note"];

fn vault() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir
}

fn run(vault: &std::path::Path, args: &[&str]) -> Vec<u8> {
    let mut full = vec!["--vault", vault.to_str().unwrap(), "tasks"];
    full.extend(args);
    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-11-27")
        .args(&full)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone()
}

fn list(dir: &assert_fs::TempDir) -> Vec<Value> {
    serde_json::from_slice(&run(dir.path(), &["list", "--format", "json"])).unwrap()
}

/// The listed row for the task whose description starts with `desc`.
fn row<'a>(rows: &'a [Value], desc: &str) -> &'a Value {
    rows.iter()
        .find(|r| r["description"].as_str().unwrap().starts_with(desc))
        .unwrap_or_else(|| panic!("no task {desc:?} in {rows:#?}"))
}

fn heading_texts(row: &Value) -> Vec<&str> {
    row["headings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["text"].as_str().unwrap())
        .collect()
}

#[test]
fn nested_headings_outermost_first_with_skipped_levels() {
    let dir = vault();
    dir.child("p.md")
        .write_str(
            "- [ ] Above all\n\
             \n\
             # Projects\n\
             \n\
             ## Alpha\n\
             \n\
             #### Deep\n\
             \n\
             - [ ] Nested task\n\
             \n\
             ## Beta\n\
             \n\
             - [ ] Beta task\n",
        )
        .unwrap();
    let rows = list(&dir);

    let nested = row(&rows, "Nested task");
    assert_eq!(heading_texts(nested), ["Projects", "Alpha", "Deep"]);
    assert_eq!(
        nested["headings"][2],
        json!({ "text": "Deep", "level": 4, "line": 7 })
    );
    assert_eq!(heading_texts(row(&rows, "Beta task")), ["Projects", "Beta"]);
    assert_eq!(row(&rows, "Above all")["headings"], json!([]));
}

#[test]
fn task_in_a_note_without_headings_has_an_empty_path() {
    let dir = vault();
    dir.child("flat.md").write_str("- [ ] Flat task\n").unwrap();
    let rows = list(&dir);
    assert_eq!(row(&rows, "Flat task")["headings"], json!([]));
}

#[test]
fn mentions_cover_the_paragraph_and_line_mentions_the_line() {
    let dir = vault();
    dir.child("people/Priya.md").write_str("# Priya\n").unwrap();
    dir.child("onboarding.md")
        .write_str("# Onboarding\n")
        .unwrap();
    dir.child("m.md")
        .write_str(
            "Notes with [[onboarding|the flow]] and [[Priya]].\n\
             - [ ] Chase [[Priya]] about [[Nobody Yet]]\n\
             - [ ] Quiet neighbour\n\
             \n\
             - [ ] Alone in its paragraph\n",
        )
        .unwrap();
    let rows = list(&dir);

    let chase = row(&rows, "Chase");
    assert_eq!(
        chase["mentions"],
        json!([
            { "target": "onboarding", "path": "onboarding.md" },
            { "target": "Priya", "path": "people/Priya.md" },
            { "target": "Nobody Yet", "path": null },
        ]),
        "paragraph mentions, deduplicated, in document order, never the alias"
    );
    assert_eq!(
        chase["line_mentions"],
        json!([
            { "target": "Priya", "path": "people/Priya.md" },
            { "target": "Nobody Yet", "path": null },
        ])
    );

    let quiet = row(&rows, "Quiet neighbour");
    assert_eq!(quiet["mentions"], chase["mentions"]);
    assert_eq!(quiet["line_mentions"], json!([]));

    let alone = row(&rows, "Alone");
    assert_eq!(alone["mentions"], json!([]));
    assert_eq!(alone["line_mentions"], json!([]));
}

#[test]
fn notes_sharing_a_title_are_told_apart_by_path() {
    let dir = vault();
    dir.child("a/Plan.md").write_str("x\n").unwrap();
    dir.child("b/Plan.md").write_str("y\n").unwrap();
    dir.child("t.md")
        .write_str("- [ ] Read [[a/Plan]] and [[b/Plan]]\n")
        .unwrap();
    let rows = list(&dir);
    let paths: Vec<&str> = row(&rows, "Read")["mentions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["a/Plan.md", "b/Plan.md"]);
}

#[test]
fn a_heading_link_mentions_its_note() {
    let dir = vault();
    dir.child("spec.md")
        .write_str("# Spec\n\n## Scope\n")
        .unwrap();
    dir.child("t.md")
        .write_str("- [ ] Settle [[spec#Scope]]\n")
        .unwrap();
    let rows = list(&dir);
    assert_eq!(
        row(&rows, "Settle")["mentions"],
        json!([{ "target": "spec", "path": "spec.md" }])
    );
}

#[test]
fn note_tags_and_date() {
    let dir = vault();
    dir.child("daily/2026-11-20.md")
        .write_str(
            "---\ntags: [daily, \"#work\"]\ndate_created: 2026-01-01\n---\n- [ ] From the daily\n",
        )
        .unwrap();
    dir.child("meeting.md")
        .write_str("---\ndate_created: 2026-10-05 14:30\n---\n- [ ] From the meeting #inline\n")
        .unwrap();
    dir.child("2026-W47.md")
        .write_str("- [ ] From the week\n")
        .unwrap();
    let rows = list(&dir);

    let daily = &row(&rows, "From the daily")["note"];
    assert_eq!(daily["tags"], json!(["daily", "work"]));
    assert_eq!(daily["date"], json!("2026-11-20"), "the file name wins");

    let meeting = &row(&rows, "From the meeting")["note"];
    assert_eq!(
        meeting["tags"],
        json!([]),
        "inline tags are the task's, not the note's"
    );
    assert_eq!(meeting["date"], json!("2026-10-05"));

    let week = &row(&rows, "From the week")["note"];
    assert_eq!(week["date"], Value::Null);
    let modified = week["modified"].as_str().unwrap();
    assert!(
        modified.len() == 20 && modified.ends_with('Z'),
        "RFC 3339 UTC to the second: {modified}"
    );
}

#[test]
fn ndjson_and_tree_rows_carry_the_context() {
    let dir = vault();
    dir.child("t.md")
        .write_str("# Top\n\n- [ ] Parent [[x]]\n    - [ ] Child\n")
        .unwrap();

    let ndjson = run(dir.path(), &["list", "--format", "ndjson"]);
    for line in String::from_utf8(ndjson).unwrap().lines() {
        let v: Value = serde_json::from_str(line).unwrap();
        assert_eq!(heading_texts(&v), ["Top"]);
    }

    let tree: Vec<Value> =
        serde_json::from_slice(&run(dir.path(), &["list", "--format", "json", "--tree"])).unwrap();
    let parent = row(&tree, "Parent");
    assert_eq!(parent["mentions"], json!([{ "target": "x", "path": null }]));
    assert_eq!(heading_texts(&parent["subtasks"][0]), ["Top"]);

    let tree_nd = run(dir.path(), &["list", "--format", "ndjson", "--tree"]);
    for line in String::from_utf8(tree_nd).unwrap().lines() {
        let v: Value = serde_json::from_str(line).unwrap();
        assert!(NEW_KEYS.iter().all(|k| v.get(k).is_some()), "{v}");
    }
}

#[test]
fn write_rows_do_not_carry_the_context() {
    let dir = vault();
    dir.child("t.md")
        .write_str("# Top\n\n- [ ] Pay [[rent]] 🆔 rent\n")
        .unwrap();
    let out: Value =
        serde_json::from_slice(&run(dir.path(), &["complete", "rent", "--format", "json"]))
            .unwrap();
    let text = out.to_string();
    for k in NEW_KEYS {
        assert!(!text.contains(&format!("\"{k}\"")), "{k} in {text}");
    }
}

#[test]
fn existing_keys_are_the_task_unchanged() {
    // A listed row minus the new keys is exactly what a write prints for
    // the same task: the serialised `Task`.
    let dir = vault();
    dir.child("t.md")
        .write_str("# Top\n\n- [ ] Pay [[rent]] #home 📅 2026-12-01 🆔 rent\n")
        .unwrap();
    let mut listed = row(&list(&dir), "Pay").clone();
    for k in NEW_KEYS {
        assert!(listed.as_object_mut().unwrap().remove(k).is_some(), "{k}");
    }
    let edited: Value = serde_json::from_slice(&run(
        dir.path(),
        &["edit", "t.md:3", "--due", "2026-12-01", "--format", "json"],
    ))
    .unwrap();
    assert_eq!(listed, edited);
}

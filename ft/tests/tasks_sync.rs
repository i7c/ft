//! `ft tasks sync` / `ft sources` against a local stub Jira.

use std::io::{Read, Write};
use std::net::TcpListener;

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;
use serde_json::{json, Value};

/// Serve forever on a local port; `route` maps the raw request to a
/// response body (status 200) or `Err(status)`.
fn stub(route: impl Fn(&str) -> Result<String, u16> + Send + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let resp = match route(&req) {
                Ok(body) => format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                ),
                Err(code) => format!("HTTP/1.1 {code} X\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"),
            };
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    url
}

fn issue(key: &str, status: &str, category: &str, parent: Option<&str>) -> Value {
    json!({
        "key": key,
        "fields": {
            "summary": format!("Summary of {key}"),
            "status": {"name": status, "statusCategory": {"key": category}},
            "priority": {"name": "High"},
            "duedate": "2026-10-30",
            "resolution": null, "resolutiondate": null,
            "assignee": {"displayName": "Jane Doe"},
            "parent": parent.map(|p| json!({"key": p})),
            "updated": "2026-10-21T09:12:44.000+0000",
            "description": {"type": "doc", "content": [{"type": "taskList", "content": [
                {"type": "taskItem", "attrs": {"state": "TODO"}, "content": [{"type": "text", "text": "remote checklist item"}]}]}]},
            "comment": {"comments": []}
        }
    })
}

/// A Jira stub: the default query returns `ABC-123` (child of `EPIC-7`)
/// with `status`; key lookups return the epic.
fn jira(status: &'static str, category: &'static str) -> String {
    stub(move |req| {
        let line = req.lines().next().unwrap_or("");
        if !line.contains("/rest/api/3/search/jql") {
            return Err(404);
        }
        let issues = if line.contains("key%20in") {
            vec![issue("EPIC-7", "In Progress", "indeterminate", None)]
        } else {
            vec![issue("ABC-123", status, category, Some("EPIC-7"))]
        };
        Ok(json!({"issues": issues, "isLast": true}).to_string())
    })
}

fn vault(base_url: &str) -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml")
        .write_str(&format!(
            "[sources.jira]\nkind = \"jira\"\nemail = \"fake-user@example.test\"\nbase_url = \"{base_url}\"\ncomments = 0\n"
        ))
        .unwrap();
    dir
}

fn ft(dir: &assert_fs::TempDir, args: &[&str], token: bool) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("ft").unwrap();
    cmd.env("FT_TODAY", "2026-10-21")
        .env_remove("FT_JIRA_TOKEN");
    if token {
        // NOT VALID — test data only.
        cmd.env("FT_JIRA_TOKEN", "FAKE_TEST_token");
    }
    let mut full = vec!["--vault", dir.path().to_str().unwrap()];
    full.extend(args);
    cmd.args(&full).assert()
}

fn read(dir: &assert_fs::TempDir, rel: &str) -> String {
    std::fs::read_to_string(dir.path().join(rel)).unwrap()
}

#[test]
fn sync_creates_notes_and_tree_shows_epic_story_local() {
    let dir = vault(&jira("In Review", "indeterminate"));
    ft(&dir, &["tasks", "sync"], true)
        .success()
        .stdout("jira: 2 new · 0 updated · 0 pending push · 0 conflicts\n");
    let note = read(&dir, "jira/ABC-123.md");
    assert!(
        note.contains("- [/] [ABC-123](") && note.contains("🆔 ABC-123"),
        "{note}"
    );
    assert!(note.contains("- ☐ remote checklist item"));
    assert!(read(&dir, "jira/EPIC-7.md").contains("state: context"));

    dir.child("journal.md")
        .write_str("- [ ] write regression test [[ABC-123]]\n")
        .unwrap();
    let out = ft(
        &dir,
        &["tasks", "list", "remote", "--tree", "--format", "markdown"],
        false,
    )
    .success()
    .get_output()
    .stdout
    .clone();
    let out = String::from_utf8(out).unwrap();
    let pos = |s: &str| {
        out.find(s)
            .unwrap_or_else(|| panic!("{s} missing in:\n{out}"))
    };
    assert!(pos("EPIC-7") < pos("ABC-123") && pos("ABC-123") < pos("write regression test"));
    assert!(!out.contains("remote checklist item"));

    // Idempotent.
    ft(&dir, &["tasks", "sync"], true)
        .success()
        .stdout("jira: 0 new · 0 updated · 0 pending push · 0 conflicts\n");
}

#[test]
fn local_tick_is_pending_and_owned_edit_is_refused() {
    let url = jira("In Review", "indeterminate");
    let dir = vault(&url);
    ft(&dir, &["tasks", "sync"], true).success();
    ft(&dir, &["tasks", "complete", "ABC-123"], false).success();
    ft(&dir, &["tasks", "sync"], true)
        .success()
        .stdout(predicate::str::contains("1 pending push"));
    ft(&dir, &["tasks", "edit", "ABC-123", "--due", "none"], false)
        .failure()
        .stderr(predicate::str::contains(
            "due is owned by jira — change it in jira",
        ));
}

#[test]
fn offline_and_missing_credentials_are_skipped_with_exit_0() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dir = vault(&format!("http://127.0.0.1:{port}"));
    ft(&dir, &["tasks", "sync"], true)
        .success()
        .stdout("jira: skipped (offline)\n");
    ft(&dir, &["tasks", "sync"], false)
        .success()
        .stdout(predicate::str::contains(
            "skipped (no credentials — run ft sources login jira)",
        ));
    assert!(!dir.path().join("jira").exists());
}

#[test]
fn unauthorized_exits_1_with_json_status() {
    let dir = vault(&stub(|_| Err(401)));
    let out = ft(&dir, &["tasks", "sync", "--format", "json"], true)
        .code(1)
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v[0]["status"], "unauthorized");
    assert!(!String::from_utf8_lossy(&out).contains("FAKE_TEST"));
}

#[test]
fn dry_run_diffs_without_writing_and_json_summary() {
    let url = jira("To Do", "new");
    let dir = vault(&url);
    ft(&dir, &["tasks", "sync", "--dry-run"], true)
        .success()
        .stdout(predicate::str::contains("+- [ ] [ABC-123]("));
    assert!(!dir.path().join("jira").exists());
    let out = ft(&dir, &["tasks", "sync", "--format", "json"], true)
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    insta::assert_snapshot!(serde_json::to_string_pretty(&v).unwrap());
}

#[test]
fn sources_list_and_unknown_source() {
    let dir = vault("https://example.test");
    let out = ft(&dir, &["sources", "list", "--format", "json"], true)
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v[0]["name"], "jira");
    assert_eq!(v[0]["credentials"], "env");
    ft(&dir, &["tasks", "sync", "--source", "nope"], true)
        .failure()
        .stderr(predicate::str::contains(
            "no source `nope` configured (known: jira)",
        ));
}

#[test]
fn login_rejects_bad_token_without_storing() {
    let dir = vault(&stub(|_| Err(401)));
    let mut cmd = Command::cargo_bin("ft").unwrap();
    cmd.args([
        "--vault",
        dir.path().to_str().unwrap(),
        "sources",
        "login",
        "jira",
    ])
    // NOT VALID — test data only.
    .write_stdin("FAKE_TEST_bad\n")
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("token rejected by http://127.0.0.1:")
            .and(predicate::str::contains("(401)")),
    );
}

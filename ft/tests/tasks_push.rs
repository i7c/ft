//! Phase 4 against a stateful stub Jira: status push, conflicts,
//! `--take`, promote.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;
use serde_json::{json, Value};

#[derive(Clone)]
struct Issue {
    status: &'static str,
    category: &'static str,
    resolution: Option<&'static str>,
    parent: Option<&'static str>,
    summary: String,
}

#[derive(Default)]
struct Jira {
    issues: BTreeMap<String, Issue>,
    /// Keys whose transitions POST answers 403.
    forbidden: Vec<String>,
    /// Requests seen: "METHOD path".
    log: Vec<String>,
    created_bodies: Vec<Value>,
}

fn raw(key: &str, i: &Issue) -> Value {
    json!({
        "key": key,
        "fields": {
            "summary": i.summary,
            "status": {"name": i.status, "statusCategory": {"key": i.category}},
            "priority": null, "duedate": null,
            "resolution": i.resolution.map(|r| json!({"name": r})),
            "resolutiondate": i.resolution.map(|_| "2026-10-21T08:00:00.000+0000"),
            "assignee": {"displayName": "Me"},
            "parent": i.parent.map(|p| json!({"key": p})),
            "updated": "2026-10-21T09:00:00.000+0000",
            "description": null,
            "comment": {"comments": []}
        }
    })
}

fn read_request(stream: &mut std::net::TcpStream) -> String {
    let mut data = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = stream.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&data).into_owned();
        if let Some(end) = text.find("\r\n\r\n") {
            let len = text[..end]
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if data.len() >= end + 4 + len {
                break;
            }
        }
    }
    String::from_utf8_lossy(&data).into_owned()
}

fn route(state: &mut Jira, req: &str) -> Result<String, u16> {
    let line = req.lines().next().unwrap_or("");
    let mut parts = line.split(' ');
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    let path = target.split('?').next().unwrap_or("");
    let body: Value = req
        .split_once("\r\n\r\n")
        .and_then(|(_, b)| serde_json::from_str(b).ok())
        .unwrap_or(Value::Null);
    state.log.push(format!("{method} {path}"));
    let seg: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method, seg.as_slice()) {
        ("GET", ["rest", "api", "3", "search", "jql"]) => {
            let issues: Vec<Value> = if target.contains("key%20in") {
                state
                    .issues
                    .iter()
                    .filter(|(k, _)| target.contains(&format!("%22{k}%22")))
                    .map(|(k, i)| raw(k, i))
                    .collect()
            } else {
                state
                    .issues
                    .iter()
                    .filter(|(_, i)| i.category != "done")
                    .map(|(k, i)| raw(k, i))
                    .collect()
            };
            Ok(json!({"issues": issues, "isLast": true}).to_string())
        }
        ("GET", ["rest", "api", "3", "myself"]) => {
            Ok(json!({"accountId": "acc-me", "displayName": "Me"}).to_string())
        }
        ("GET", ["rest", "api", "3", "issue", "createmeta", _, "issuetypes"]) => {
            Ok(json!({"issueTypes": [
                {"id": "10001", "name": "Task", "subtask": false, "hierarchyLevel": 0},
                {"id": "10003", "name": "Sub-task", "subtask": true, "hierarchyLevel": -1}
            ]})
            .to_string())
        }
        ("GET", ["rest", "api", "3", "issue", key, "transitions"]) => {
            let i = state.issues.get(*key).ok_or(404u16)?;
            let mut ts = vec![];
            if i.category != "indeterminate" {
                ts.push(json!({"id": "11", "name": "Start Progress", "to": {"name": "In Progress", "statusCategory": {"key": "indeterminate"}}}));
            }
            if i.category != "done" {
                ts.push(json!({"id": "31", "name": "Resolve", "to": {"name": "Done", "statusCategory": {"key": "done"}}}));
                ts.push(json!({"id": "41", "name": "Won't Do", "to": {"name": "Closed", "statusCategory": {"key": "done"}}}));
            }
            Ok(json!({"transitions": ts}).to_string())
        }
        ("POST", ["rest", "api", "3", "issue", key, "transitions"]) => {
            if state.forbidden.contains(&key.to_string()) {
                return Err(403);
            }
            let i = state.issues.get_mut(*key).ok_or(404u16)?;
            match body["transition"]["id"].as_str() {
                Some("11") => {
                    (i.status, i.category, i.resolution) = ("In Progress", "indeterminate", None)
                }
                Some("31") => (i.status, i.category, i.resolution) = ("Done", "done", Some("Done")),
                Some("41") => {
                    (i.status, i.category, i.resolution) = ("Closed", "done", Some("Won't Do"))
                }
                _ => return Err(400),
            }
            Ok(String::new())
        }
        ("GET", ["rest", "api", "3", "issue", key]) => {
            let i = state.issues.get(*key).ok_or(404u16)?;
            if target.contains("fields=issuetype,project") {
                let sub = key.starts_with("SUB");
                return Ok(json!({"fields": {
                    "issuetype": {"subtask": sub, "hierarchyLevel": if sub { -1 } else { 0 }},
                    "project": {"key": "ABC"}
                }})
                .to_string());
            }
            Ok(raw(key, i).to_string())
        }
        ("POST", ["rest", "api", "3", "issue"]) => {
            state.created_bodies.push(body.clone());
            let parent: &'static str = Box::leak(
                body["fields"]["parent"]["key"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
                    .into_boxed_str(),
            );
            state.issues.insert(
                "ABC-124".into(),
                Issue {
                    status: "To Do",
                    category: "new",
                    resolution: None,
                    parent: Some(parent),
                    summary: body["fields"]["summary"].as_str().unwrap_or("").to_string(),
                },
            );
            Ok(json!({"key": "ABC-124"}).to_string())
        }
        _ => Err(404),
    }
}

fn stub(state: Arc<Mutex<Jira>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let req = read_request(&mut stream);
            let resp = match route(&mut state.lock().unwrap(), &req) {
                Ok(b) if b.is_empty() => "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_string(),
                Ok(b) => format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
                    b.len()
                ),
                Err(code) => format!("HTTP/1.1 {code} X\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"),
            };
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    url
}

fn story() -> Issue {
    Issue {
        status: "In Review",
        category: "indeterminate",
        resolution: None,
        parent: None,
        summary: "Fix login".into(),
    }
}

fn setup(issues: &[(&str, Issue)], extra_cfg: &str) -> (assert_fs::TempDir, Arc<Mutex<Jira>>) {
    let state = Arc::new(Mutex::new(Jira {
        issues: issues
            .iter()
            .map(|(k, i)| (k.to_string(), i.clone()))
            .collect(),
        ..Default::default()
    }));
    let url = stub(Arc::clone(&state));
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml")
        .write_str(&format!(
            "[sources.jira]\nkind = \"jira\"\nemail = \"fake-user@example.test\"\nbase_url = \"{url}\"\ncomments = 0\nfetch_parents = false\n{extra_cfg}"
        ))
        .unwrap();
    (dir, state)
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

fn set_checkbox(dir: &assert_fs::TempDir, key: &str, from: char, to: char) {
    let rel = format!("jira/{key}.md");
    let note =
        read(dir, &rel).replace(&format!("- [{from}] [{key}]"), &format!("- [{to}] [{key}]"));
    std::fs::write(dir.path().join(rel), note).unwrap();
}

#[test]
fn ticked_anchor_is_pushed_via_transition() {
    let (dir, state) = setup(&[("ABC-123", story())], "");
    ft(&dir, &["tasks", "sync"], true).success();
    set_checkbox(&dir, "ABC-123", '/', 'x');
    ft(&dir, &["tasks", "sync"], true)
        .success()
        .stdout(predicate::str::contains("· 1 pushed"));
    assert_eq!(state.lock().unwrap().issues["ABC-123"].status, "Done");
    assert!(state
        .lock()
        .unwrap()
        .log
        .contains(&"POST /rest/api/3/issue/ABC-123/transitions".to_string()));
    let note = read(&dir, "jira/ABC-123.md");
    assert!(
        note.contains("      status: Done\n") && note.contains("- [x] [ABC-123]"),
        "{note}"
    );
    // Nothing pending any more.
    ft(&dir, &["tasks", "sync"], true)
        .success()
        .stdout("jira: 0 new · 0 updated · 0 pending push · 0 conflicts\n");
}

#[test]
fn dry_run_and_no_push_never_write_remotely() {
    let (dir, state) = setup(&[("ABC-123", story())], "");
    ft(&dir, &["tasks", "sync"], true).success();
    set_checkbox(&dir, "ABC-123", '/', 'x');
    ft(&dir, &["tasks", "sync", "--dry-run"], true).success();
    ft(&dir, &["tasks", "sync", "--no-push"], true)
        .success()
        .stdout(predicate::str::contains("1 pending push"));
    assert!(!state
        .lock()
        .unwrap()
        .log
        .iter()
        .any(|l| l.starts_with("POST")));
}

#[test]
fn cancel_needs_configured_transition() {
    let (dir, state) = setup(&[("ABC-123", story())], "");
    ft(&dir, &["tasks", "sync"], true).success();
    set_checkbox(&dir, "ABC-123", '/', '-');
    ft(&dir, &["tasks", "sync"], true)
        .success()
        .stdout(predicate::str::contains(
            "push failed ABC-123: cancel isn't pushed — set transitions.Cancelled",
        ));
    ft(&dir, &["tasks", "check"], false)
        .code(1)
        .stdout(predicate::str::contains("push-failed"));
    assert_eq!(state.lock().unwrap().issues["ABC-123"].status, "In Review");

    let (dir, state) = setup(
        &[("ABC-123", story())],
        "[sources.jira.transitions]\nCancelled = \"Won't Do\"\n",
    );
    ft(&dir, &["tasks", "sync"], true).success();
    set_checkbox(&dir, "ABC-123", '/', '-');
    ft(&dir, &["tasks", "sync"], true)
        .success()
        .stdout(predicate::str::contains("1 pushed"));
    assert_eq!(
        state.lock().unwrap().issues["ABC-123"].resolution,
        Some("Won't Do")
    );
}

#[test]
fn forbidden_push_stays_pending_and_is_reported() {
    let (dir, state) = setup(&[("ABC-123", story())], "");
    ft(&dir, &["tasks", "sync"], true).success();
    state.lock().unwrap().forbidden.push("ABC-123".into());
    set_checkbox(&dir, "ABC-123", '/', 'x');
    ft(&dir, &["tasks", "sync"], true).success().stdout(
        predicate::str::contains("push failed ABC-123: forbidden (403)")
            .and(predicate::str::contains("1 pending push")),
    );
    let out = ft(&dir, &["tasks", "check", "--format", "json"], false)
        .code(1)
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert!(v
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "push-failed" && e["link"] == "forbidden (403)"));
    assert!(read(&dir, "jira/ABC-123.md").contains("- [x] [ABC-123]"));
}

#[test]
fn already_there_is_not_pushed() {
    let (dir, state) = setup(&[("ABC-123", story())], "");
    ft(&dir, &["tasks", "sync"], true).success();
    set_checkbox(&dir, "ABC-123", '/', 'x');
    {
        let mut s = state.lock().unwrap();
        let i = s.issues.get_mut("ABC-123").unwrap();
        (i.status, i.category, i.resolution) = ("Done", "done", Some("Done"));
    }
    ft(&dir, &["tasks", "sync"], true).success();
    assert!(!state
        .lock()
        .unwrap()
        .log
        .iter()
        .any(|l| l.starts_with("POST")));
    assert!(read(&dir, "jira/ABC-123.md").contains("      status: Done\n"));
}

#[test]
fn conflict_is_stored_and_resolved_offline_with_take() {
    let (dir, state) = setup(&[("ABC-123", story())], "");
    ft(&dir, &["tasks", "sync"], true).success();
    set_checkbox(&dir, "ABC-123", '/', 'x');
    {
        let mut s = state.lock().unwrap();
        let i = s.issues.get_mut("ABC-123").unwrap();
        (i.status, i.category, i.resolution) = ("Closed", "done", Some("Won't Do"));
    }
    ft(&dir, &["tasks", "sync", "--no-push"], true)
        .success()
        .stdout(predicate::str::contains(
            "conflict ABC-123 local=Done remote=Cancelled",
        ));
    assert!(read(&dir, "jira/ABC-123.md").contains("    conflict: Cancelled\n"));
    // A conflicted note is never pushed.
    ft(&dir, &["tasks", "sync"], true).success();
    assert!(!state
        .lock()
        .unwrap()
        .log
        .iter()
        .any(|l| l.starts_with("POST")));
    ft(&dir, &["tasks", "check"], false)
        .code(1)
        .stdout(predicate::str::contains("conflict"));
    // Resolve without credentials or network.
    ft(
        &dir,
        &["tasks", "sync", "--take", "remote", "ABC-123"],
        false,
    )
    .success()
    .stdout(predicate::str::contains("ABC-123: took remote"));
    let note = read(&dir, "jira/ABC-123.md");
    assert!(
        note.contains("- [-] [ABC-123]") && !note.contains("conflict:"),
        "{note}"
    );
    ft(
        &dir,
        &["tasks", "sync", "--take", "remote", "ABC-123"],
        false,
    )
    .failure()
    .stderr(predicate::str::contains("ABC-123 has no stored conflict"));
    ft(&dir, &["tasks", "sync", "--resolve"], false)
        .success()
        .stdout("no conflicts\n");
}

#[test]
fn promote_creates_subtask_and_restructures_vault() {
    let (dir, state) = setup(&[("ABC-123", story())], "");
    ft(&dir, &["tasks", "sync"], true).success();
    dir.child("journal.md")
        .write_str("# Day\n- [/] write regression test [[ABC-123]] #work ^wr1\n  - [ ] cover Safari\n- [ ] other\n")
        .unwrap();
    dir.child("notes.md")
        .write_str("see [[journal#^wr1]]\n")
        .unwrap();
    let out = ft(
        &dir,
        &["tasks", "promote", "write regression", "--format", "json"],
        true,
    )
    .success()
    .get_output()
    .stdout
    .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["key"], "ABC-124");
    assert_eq!(v["note"], "jira/ABC-124.md");
    let s = state.lock().unwrap();
    let body = &s.created_bodies[0]["fields"];
    assert_eq!(body["summary"], "write regression test");
    assert_eq!(body["parent"]["key"], "ABC-123");
    assert_eq!(body["issuetype"]["id"], "10003");
    assert_eq!(body["assignee"]["accountId"], "acc-me");
    // In progress locally → pushed right after creation.
    assert_eq!(s.issues["ABC-124"].status, "In Progress");
    drop(s);
    assert_eq!(read(&dir, "journal.md"), "# Day\n- [ ] other\n");
    let n = read(&dir, "jira/ABC-124.md");
    assert!(
        n.contains("- [/] [ABC-124](") && n.contains("🆔 ABC-124 ^wr1\n  - [ ] cover Safari\n"),
        "{n}"
    );
    assert_eq!(read(&dir, "notes.md"), "see [[ABC-124#^wr1]]\n");
    // The next sync is a no-op for the new note.
    ft(&dir, &["tasks", "sync"], true)
        .success()
        .stdout(predicate::str::contains("0 new"));
}

#[test]
fn promote_preconditions() {
    let (dir, _state) = setup(&[("ABC-123", story())], "");
    ft(&dir, &["tasks", "sync"], true).success();
    dir.child("j.md")
        .write_str("- [ ] P\n  - [ ] local child\n- [ ] linked [[ABC-123]]\n")
        .unwrap();
    ft(&dir, &["tasks", "promote", "local child"], true)
        .failure()
        .stderr(predicate::str::contains(
            "promote needs a parent that is a remote issue",
        ));
    ft(&dir, &["tasks", "promote", "linked"], false)
        .failure()
        .stderr(predicate::str::contains("no credentials"));
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let cfg = read(&dir, ".ft/config.toml");
    let cfg = cfg
        .lines()
        .map(|l| {
            if l.starts_with("base_url") {
                format!("base_url = \"http://127.0.0.1:{port}\"")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(dir.path().join(".ft/config.toml"), cfg).unwrap();
    ft(&dir, &["tasks", "promote", "linked"], true)
        .failure()
        .stderr(predicate::str::contains(
            "jira: offline — promote needs a connection",
        ));
    assert_eq!(
        read(&dir, "j.md"),
        "- [ ] P\n  - [ ] local child\n- [ ] linked [[ABC-123]]\n"
    );
}

//! `ft tasks sync` / `ft sources` against a local stub TickTick.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;
use serde_json::{json, Value};

const PROJECT: &str = "6aec746604183a21c6cf0001";
const PARENT: &str = "6aec746604183a21c6cffe41";
const CHILD: &str = "6aec746604183a21c6cffe42";
const RECURRING: &str = "6aec746604183a21c6cffe50";

#[derive(Default)]
struct State {
    /// task id → status code
    status: std::collections::HashMap<String, i64>,
    /// task id → due date (ISO day); recurring tasks advance on complete
    due: std::collections::HashMap<String, String>,
    log: Vec<String>,
}

fn task(id: &str, status: i64, parent: Option<&str>, due: &str) -> Value {
    json!({
        "id": id, "projectId": PROJECT, "title": format!("Task {}", &id[20..]),
        "content": "", "timeZone": "America/Sao_Paulo", "isAllDay": true,
        "dueDate": format!("{due}T03:00:00.000+0000"),
        "repeatFlag": if id == RECURRING { "RRULE:FREQ=DAILY;INTERVAL=1" } else { "" }, "priority": 5, "status": status,
        "etag": "e1", "kind": "TEXT", "parentId": parent,
        "modifiedTime": "2026-11-05T12:19:57.000+0000",
    })
}

/// Read one request, including a `Content-Length` body.
fn read_request(stream: &mut std::net::TcpStream) -> String {
    let mut data = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = stream.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&data);
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

/// A TickTick stub: one list `Work` holding an open child under an open
/// parent; an empty inbox; single-task lookups; complete.
fn ticktick() -> (String, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State::default()));
    {
        let mut s = state.lock().unwrap();
        s.status.insert(PARENT.into(), 0);
        s.status.insert(CHILD.into(), 0);
        for id in [PARENT, CHILD] {
            s.due.insert(id.into(), "2026-11-05".into());
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let shared = state.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let req = read_request(&mut stream);
            let line = req.lines().next().unwrap_or("").to_string();
            let mut parts = line.split(' ');
            let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let mut s = shared.lock().unwrap();
            s.log.push(format!("{method} {path}"));
            let parent_of = |id: &str| (id == CHILD).then_some(PARENT);
            let body = match (method, path) {
                ("GET", "/open/v1/project") => {
                    Some(json!([{"id": PROJECT, "name": "Work", "kind": "TASK"}]))
                }
                ("GET", "/open/v1/project/inbox/data") => Some(json!({"tasks": []})),
                ("GET", p) if p == format!("/open/v1/project/{PROJECT}/data") => {
                    let mut ids: Vec<&String> = s.status.keys().collect();
                    ids.sort();
                    let tasks: Vec<Value> = ids
                        .into_iter()
                        .filter(|id| s.status[*id] == 0)
                        .map(|id| task(id, 0, parent_of(id), &s.due[id]))
                        .collect();
                    Some(json!({"tasks": tasks}))
                }
                ("GET", p) => p
                    .strip_prefix(&format!("/open/v1/project/{PROJECT}/task/"))
                    .and_then(|id| {
                        s.status
                            .get(id)
                            .map(|st| task(id, *st, parent_of(id), &s.due[id]))
                    }),
                ("POST", p) if p.ends_with("/complete") => {
                    let id = p.rsplit('/').nth(1).unwrap().to_string();
                    if id == RECURRING {
                        // Like the real API: same id, next occurrence.
                        s.due.insert(id, "2026-11-07".into());
                    } else {
                        s.status.insert(id, 2);
                    }
                    Some(Value::Null)
                }
                _ => None,
            };
            let resp = match body {
                Some(b) => {
                    let b = if b.is_null() {
                        String::new()
                    } else {
                        b.to_string()
                    };
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
                        b.len()
                    )
                }
                None => {
                    "HTTP/1.1 404 X\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_string()
                }
            };
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    (url, state)
}

fn vault(base_url: &str) -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml")
        .write_str(&format!(
            "[sources.ticktick]\nkind = \"ticktick\"\nbase_url = \"{base_url}\"\n"
        ))
        .unwrap();
    dir
}

fn ft(dir: &assert_fs::TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("ft").unwrap();
    // NOT VALID — test data only.
    cmd.env("FT_TODAY", "2026-11-06")
        .env("FT_TICKTICK_TOKEN", "FAKE_TEST_token");
    let mut full = vec!["--vault", dir.path().to_str().unwrap()];
    full.extend(args);
    cmd.args(&full).assert()
}

fn read(dir: &assert_fs::TempDir, rel: &str) -> String {
    std::fs::read_to_string(dir.path().join(rel)).unwrap()
}

#[test]
fn sync_creates_task_notes_and_is_idempotent() {
    let (url, _state) = ticktick();
    let dir = vault(&url);
    ft(&dir, &["tasks", "sync"])
        .success()
        .stdout("ticktick: 2 new · 0 updated · 0 pending push · 0 conflicts\n");
    let note = read(&dir, &format!("ticktick/tt-{CHILD}.md"));
    assert!(
        note.contains(&format!(
            "- [ ] [tt-{CHILD}]({url}/webapp/#p/{PROJECT}/tasks/{CHILD}) Task fe42 ⏫ 📅 2026-11-05 🆔 tt-{CHILD}"
        )),
        "{note}"
    );
    assert!(
        note.contains(&format!("    parent: tt-{PARENT}\n")),
        "{note}"
    );
    assert!(
        !note.contains("## Comments"),
        "comments default off: {note}"
    );
    ft(&dir, &["tasks", "sync"])
        .success()
        .stdout("ticktick: 0 new · 0 updated · 0 pending push · 0 conflicts\n");
}

#[test]
fn ticked_anchor_completes_the_task() {
    let (url, state) = ticktick();
    let dir = vault(&url);
    ft(&dir, &["tasks", "sync"]).success();
    let rel = format!("ticktick/tt-{CHILD}.md");
    let note = read(&dir, &rel).replace(
        &format!("- [ ] [tt-{CHILD}]"),
        &format!("- [x] [tt-{CHILD}]"),
    );
    std::fs::write(dir.path().join(&rel), note).unwrap();
    ft(&dir, &["tasks", "sync"])
        .success()
        .stdout(predicate::str::contains("· 1 pushed"));
    assert!(state.lock().unwrap().log.contains(&format!(
        "POST /open/v1/project/{PROJECT}/task/{CHILD}/complete"
    )));
    let note = read(&dir, &rel);
    assert!(
        note.contains("      status: Done\n") && note.contains(&format!("- [x] [tt-{CHILD}]")),
        "{note}"
    );
    ft(&dir, &["tasks", "sync"])
        .success()
        .stdout("ticktick: 0 new · 0 updated · 0 pending push · 0 conflicts\n");
}

#[test]
fn sources_list_shows_kind_and_default_base_url() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml")
        .write_str("[sources.ticktick]\nkind = \"ticktick\"\n")
        .unwrap();
    let out = ft(&dir, &["sources", "list", "--format", "json"])
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(rows[0]["kind"], "ticktick");
    assert_eq!(rows[0]["base_url"], "https://api.ticktick.com");
    assert_eq!(rows[0]["credentials"], "env");
}

#[test]
fn unknown_list_fails_the_source() {
    let (url, _state) = ticktick();
    let dir = vault(&url);
    let mut cfg = read(&dir, ".ft/config.toml");
    cfg.push_str("[sources.ticktick.queries]\nwork = \"list:Wrok\"\n");
    std::fs::write(dir.path().join(".ft/config.toml"), cfg).unwrap();
    ft(&dir, &["tasks", "sync"])
        .code(1)
        .stdout("ticktick: error: unknown TickTick list \"Wrok\"\n");
}

#[test]
fn completing_a_recurring_task_reopens_with_the_next_due_date() {
    let (url, state) = ticktick();
    {
        let mut s = state.lock().unwrap();
        s.status.insert(RECURRING.into(), 0);
        s.due.insert(RECURRING.into(), "2026-11-06".into());
    }
    let dir = vault(&url);
    ft(&dir, &["tasks", "sync"]).success();
    let rel = format!("ticktick/tt-{RECURRING}.md");
    let note = read(&dir, &rel).replace(
        &format!("- [ ] [tt-{RECURRING}]"),
        &format!("- [x] [tt-{RECURRING}]"),
    );
    std::fs::write(dir.path().join(&rel), note).unwrap();
    ft(&dir, &["tasks", "sync"]).success().stdout(
        predicate::str::contains("· 1 pushed").and(predicate::str::contains("0 conflicts")),
    );
    let note = read(&dir, &rel);
    assert!(
        note.contains(&format!("- [ ] [tt-{RECURRING}]"))
            && note.contains("📅 2026-11-07")
            && note.contains("      status: Open\n"),
        "{note}"
    );
    ft(&dir, &["tasks", "sync"])
        .success()
        .stdout("ticktick: 0 new · 0 updated · 0 pending push · 0 conflicts\n");
}

//! Remote source sync in the TUI: startup sync via `Tick`, `g r`,
//! toasts, and the owned-field guard on Jira anchors.

use super::*;
use ft_core::sources::run::FetchFailure;
use ft_core::sources::{FetchResult, RemoteIssue};
use ft_core::task::{Priority, Status};
use std::sync::{Arc, Mutex};

fn sources_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("test-vault");
    std::fs::create_dir_all(root.join(".obsidian")).unwrap();
    std::fs::create_dir_all(root.join(".ft")).unwrap();
    std::fs::write(
        root.join(".ft/config.toml"),
        "[tui]\ntasks_tab = true\n\n[sources.jira]\nkind = \"jira\"\nemail = \"fake-user@example.test\"\nbase_url = \"https://example.test\"\ncomments = 0\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(root)).unwrap();
    (dir, vault)
}

fn issue(status: Status) -> RemoteIssue {
    RemoteIssue {
        key: "ABC-123".into(),
        url: "https://example.test/browse/ABC-123".into(),
        title: "Fix login".into(),
        status_name: "In Review".into(),
        status,
        priority: Some(Priority::High),
        due: chrono::NaiveDate::from_ymd_opt(2026, 5, 12),
        updated: "2026-05-10T09:00:00.000+0000".into(),
        ..Default::default()
    }
}

/// A fake fetcher whose next outcome the test can swap.
fn fetcher(app: &mut App) -> Arc<Mutex<Result<FetchResult, FetchFailure>>> {
    let next = Arc::new(Mutex::new(Ok(FetchResult {
        active: vec![issue(Status::InProgress)],
        ..Default::default()
    })));
    let shared = Arc::clone(&next);
    app.set_source_fetcher_for_test(Arc::new(move |_, _, _, _| {
        shared
            .lock()
            .unwrap()
            .clone()
            .map(|fetched| ft_core::sources::run::SourceOutcome {
                fetched,
                ..Default::default()
            })
    }));
    next
}

fn toast(app: &App) -> String {
    app.current_toast().map(|t| t.text).unwrap_or_default()
}

fn note(dir: &TempDir) -> String {
    std::fs::read_to_string(dir.path().join("test-vault/jira/ABC-123.md")).unwrap_or_default()
}

#[test]
fn startup_sync_creates_anchor_visible_in_tasks_tab() -> Result<()> {
    let (dir, vault) = sources_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    fetcher(&mut app);
    app.tick_for_test()?;
    assert!(note(&dir).contains("🆔 ABC-123"), "startup tick syncs");
    assert_eq!(toast(&app), "jira: 1 new · 0 updated");
    app.switch_to(3)?;
    app.pump_graph_rebuild_for_test();
    let frame = render(&mut app, 100, 14);
    assert!(
        frame.contains("[ABC-123](https://example.test/browse/ABC-123) Fix login"),
        "{frame}"
    );

    // Not due again until the interval elapses: a second tick is a no-op.
    std::fs::remove_file(dir.path().join("test-vault/jira/ABC-123.md")).unwrap();
    app.tick_for_test()?;
    assert_eq!(note(&dir), "");
    Ok(())
}

#[test]
fn manual_sync_offline_and_pending_toasts() -> Result<()> {
    let (_dir, vault) = sources_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    let next = fetcher(&mut app);
    app.tick_for_test()?;
    app.switch_to(3)?;
    *next.lock().unwrap() = Err(FetchFailure::Offline("connection refused".into()));
    app.dispatch(key('g'))?;
    app.dispatch(key('r'))?;
    assert_eq!(toast(&app), "jira: offline — nothing synced");

    // Tick the anchor locally, then sync with the remote unchanged.
    *next.lock().unwrap() = Ok(FetchResult {
        active: vec![issue(Status::InProgress)],
        ..Default::default()
    });
    app.switch_to(3)?;
    app.pump_graph_rebuild_for_test();
    app.dispatch(key('x'))?;
    app.pump_graph_rebuild_for_test();
    app.dispatch(key('g'))?;
    app.dispatch(key('r'))?;
    assert_eq!(toast(&app), "jira: no changes · 1 pending push");
    Ok(())
}

#[test]
fn background_sync_stays_quiet_without_changes_but_reports_auth_errors() -> Result<()> {
    let (_dir, vault) = sources_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    let next = fetcher(&mut app);
    *next.lock().unwrap() = Err(FetchFailure::NoCredentials);
    app.tick_for_test()?;
    assert_eq!(
        toast(&app),
        "",
        "no credentials is silent in the background"
    );
    app.switch_to(3)?;
    *next.lock().unwrap() = Err(FetchFailure::Unauthorized(401));
    app.dispatch(key('g'))?;
    app.dispatch(key('r'))?;
    let t = app.current_toast().unwrap();
    assert_eq!(
        t.text,
        "jira: unauthorized (401) — run ft sources login jira"
    );
    assert_eq!(t.style, crate::tui::tab::ToastStyle::Error);
    Ok(())
}

#[test]
fn owned_field_bump_on_anchor_is_refused() -> Result<()> {
    let (dir, vault) = sources_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    fetcher(&mut app);
    app.tick_for_test()?;
    let before = note(&dir);
    app.switch_to(3)?;
    app.pump_graph_rebuild_for_test();
    app.dispatch(key(']'))?;
    app.service_pending_requests()?;
    assert_eq!(toast(&app), "due is owned by jira — change it in jira");
    assert_eq!(note(&dir), before);
    Ok(())
}

#[test]
fn edits_made_during_a_sync_are_not_overwritten() -> Result<()> {
    use crate::tui::source_sync::{apply_plan, SourcesJob};
    let (dir, app_vault) = sources_vault();
    let vault = Arc::new(Vault::discover(Some(dir.path().join("test-vault"))).unwrap());
    let mut app = App::for_test_with_clock(app_vault, fixed_clock);
    fetcher(&mut app);
    app.tick_for_test()?;
    // Plan a sync with a new remote title, then edit the note before
    // applying: the planned write is skipped.
    let job = SourcesJob {
        vault: Arc::clone(&vault),
        sources: vec!["jira".into()],
        fetcher: Arc::new(|_, _, _, _| {
            let mut i = issue(Status::InProgress);
            i.title = "Renamed".into();
            Ok(ft_core::sources::run::SourceOutcome {
                fetched: FetchResult {
                    active: vec![i],
                    ..Default::default()
                },
                ..Default::default()
            })
        }),
        today: chrono::NaiveDate::from_ymd_opt(2026, 5, 10).unwrap(),
        manual: true,
    };
    let result = job.run();
    let path = dir.path().join("test-vault/jira/ABC-123.md");
    let edited = std::fs::read_to_string(&path).unwrap() + "\nmy note\n";
    std::fs::write(&path, &edited).unwrap();
    let plan = result.runs[0].outcome.as_ref().unwrap();
    assert_eq!(apply_plan(plan).unwrap(), 1);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
    Ok(())
}

// ── Phase 4: push, conflicts, promote ───────────────────────────────

#[test]
fn background_push_is_summarised() -> Result<()> {
    let (dir, vault) = sources_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    let next = fetcher(&mut app);
    app.tick_for_test()?;
    // The fake reports a push and the remote now Done.
    *next.lock().unwrap() = Ok(FetchResult {
        active: vec![issue(Status::Done)],
        ..Default::default()
    });
    let pushed = Arc::new(Mutex::new(vec!["ABC-123".to_string()]));
    let shared = Arc::clone(&next);
    let p = Arc::clone(&pushed);
    app.set_source_fetcher_for_test(Arc::new(move |_, _, _, _| {
        shared
            .lock()
            .unwrap()
            .clone()
            .map(|fetched| ft_core::sources::run::SourceOutcome {
                fetched,
                push: ft_core::sources::push::PushReport {
                    pushed: p.lock().unwrap().clone(),
                    failures: Vec::new(),
                },
            })
    }));
    app.switch_to(3)?;
    app.dispatch(key('g'))?;
    app.dispatch(key('r'))?;
    assert_eq!(toast(&app), "jira: 0 new · 1 updated · 1 pushed");
    assert!(note(&dir).contains("      status: Done\n"));
    Ok(())
}

#[test]
fn conflict_modal_opens_and_takes_remote() -> Result<()> {
    let (dir, vault) = sources_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    let next = fetcher(&mut app);
    app.tick_for_test()?;
    // Tick locally; the remote meanwhile cancelled the issue.
    let ticked = note(&dir).replace("- [/] [ABC-123]", "- [x] [ABC-123]");
    std::fs::write(dir.path().join("test-vault/jira/ABC-123.md"), ticked).unwrap();
    *next.lock().unwrap() = Ok(FetchResult {
        active: vec![issue(Status::Cancelled)],
        ..Default::default()
    });
    app.switch_to(3)?;
    app.dispatch(key('g'))?;
    app.dispatch(key('r'))?;
    assert_eq!(app.active_modal_name(), Some("conflict-resolve"));
    let frame = render(&mut app, 100, 20);
    assert!(
        frame.contains("ABC-123")
            && frame.contains("local=Done")
            && frame.contains("remote=Cancelled"),
        "{frame}"
    );
    assert_tui_snapshot!("conflict_resolve_modal_100x20", frame);
    app.dispatch(key('r'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    assert_eq!(app.active_modal_name(), None);
    app.service_pending_requests()?;
    assert_eq!(toast(&app), "resolved 1 conflict(s)");
    let n = note(&dir);
    assert!(
        n.contains("- [-] [ABC-123]") && !n.contains("conflict:"),
        "{n}"
    );
    Ok(())
}

#[test]
fn promote_from_tasks_tab() -> Result<()> {
    let (dir, vault) = sources_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    fetcher(&mut app);
    app.tick_for_test()?;
    std::fs::write(
        dir.path().join("test-vault/today.md"),
        "- [ ] write regression test ⤴ ABC-123 📅 2026-05-11\n  - [ ] cover Safari\n",
    )
    .unwrap();
    app.set_promoter_for_test(Arc::new(|_, _, parent, new, _| {
        Ok((
            RemoteIssue {
                key: "ABC-124".into(),
                url: "https://example.test/browse/ABC-124".into(),
                title: new.title.clone(),
                status_name: "To Do".into(),
                parent: Some(parent.key.clone()),
                due: new.due,
                updated: "2026-05-10T10:00:00.000+0000".into(),
                ..Default::default()
            },
            None,
        ))
    }));
    app.switch_to(3)?;
    app.dispatch(key('R'))?;
    app.pump_graph_rebuild_for_test();
    // Rows: ABC-123 (due 05-12) with its linked child collapsed; the
    // linked task is nested, so expand the anchor and step onto it.
    let frame = render(&mut app, 100, 14);
    assert!(frame.contains("Fix login"), "{frame}");
    app.dispatch(key('l'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('I'))?;
    app.service_pending_requests()?;
    assert_eq!(toast(&app), "promoted to ABC-124");
    app.pump_graph_rebuild_for_test();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("test-vault/today.md")).unwrap(),
        ""
    );
    let n = std::fs::read_to_string(dir.path().join("test-vault/jira/ABC-124.md")).unwrap();
    assert!(n.contains("🆔 ABC-124\n  - [ ] cover Safari\n"), "{n}");
    let frame = render(&mut app, 100, 14);
    let selected = frame
        .lines()
        .find(|l| l.contains('▶'))
        .unwrap_or_default()
        .to_string();
    assert!(
        selected.contains("ABC-124"),
        "cursor on the new anchor:\n{frame}"
    );
    Ok(())
}

#[test]
fn promote_precondition_toast_without_network() -> Result<()> {
    let (dir, vault) = sources_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    fetcher(&mut app);
    app.tick_for_test()?;
    std::fs::write(
        dir.path().join("test-vault/today.md"),
        "- [ ] plain 📅 2026-05-11\n",
    )
    .unwrap();
    app.set_promoter_for_test(Arc::new(|_, _, _, _, _| panic!("no network call expected")));
    app.switch_to(3)?;
    app.dispatch(key('R'))?;
    app.pump_graph_rebuild_for_test();
    app.dispatch(key('I'))?;
    app.service_pending_requests()?;
    assert_eq!(
        toast(&app),
        "promote needs a parent that is a remote issue — move or link it under one first"
    );
    Ok(())
}

#[test]
fn promote_with_two_ticket_parents_asks_which() -> Result<()> {
    let (dir, vault) = sources_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    fetcher(&mut app);
    app.tick_for_test()?;
    let root = dir.path().join("test-vault");
    let story = std::fs::read_to_string(root.join("jira/ABC-123.md")).unwrap();
    std::fs::write(
        root.join("jira/ABC-200.md"),
        story.replace("ABC-123", "ABC-200"),
    )
    .unwrap();
    std::fs::write(
        root.join("today.md"),
        "- [ ] both ⤴ ABC-123,ABC-200 📅 2026-05-11\n",
    )
    .unwrap();
    app.set_promoter_for_test(Arc::new(|_, _, _, _, _| panic!("no network call expected")));
    app.switch_to(3)?;
    app.dispatch(key('R'))?;
    app.pump_graph_rebuild_for_test();
    // Rows: the two anchors, each with `both` collapsed beneath it.
    app.dispatch(key('l'))?;
    app.dispatch(key('j'))?;
    let frame = render(&mut app, 100, 30);
    let selected = frame.lines().find(|l| l.contains('▶')).unwrap_or_default();
    assert!(selected.contains("both ⤴2"), "{frame}");
    app.dispatch(key('I'))?;
    app.service_pending_requests()?;
    let frame = render(&mut app, 100, 40);
    assert!(
        frame.contains("ABC-123") && frame.contains("ABC-200") && frame.contains("task-parent-p"),
        "{frame}"
    );
    Ok(())
}

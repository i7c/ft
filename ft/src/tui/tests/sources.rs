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
    app.switch_to(5)?;
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
    app.switch_to(5)?;
    *next.lock().unwrap() = Err(FetchFailure::Offline("connection refused".into()));
    app.dispatch(key('g'))?;
    app.dispatch(key('r'))?;
    assert_eq!(toast(&app), "jira: offline — nothing synced");

    // Tick the anchor locally, then sync with the remote unchanged.
    *next.lock().unwrap() = Ok(FetchResult {
        active: vec![issue(Status::InProgress)],
        ..Default::default()
    });
    app.switch_to(5)?;
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
    app.switch_to(5)?;
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
    app.switch_to(5)?;
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

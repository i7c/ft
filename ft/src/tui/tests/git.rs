//! Git integration: `g s`/`g c` chords, background sync/commit
//! workers, conflict surfacing, end-to-end worker thread.

use super::*;

// ── plan 012 session 3: TUI `g s` chord ──────────────────────────────────

const TASKS_TAB_INDEX: usize = 1;

fn key_with_mods(c: char, mods: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), mods))
}

fn esc_key() -> Event {
    Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
}

#[test]
fn git_chord_g_from_normal_enters_git_leader_mode() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    // The graph tab shadows `g` (cursor-first); use Notes, which doesn't.
    app.switch_to(NOTES_TAB_INDEX)?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    app.dispatch(key('g'))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::GitLeader);
    Ok(())
}

#[test]
fn git_leader_s_queues_sync_request_and_returns_to_normal() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('g'))?;
    app.dispatch(key('s'))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    let req = app
        .take_pending_request()
        .expect("`g s` should queue an AppRequest::SyncGit");
    match req {
        AppRequest::SyncGit { message } => {
            assert!(message.is_none(), "TUI never overrides the commit message");
        }
        other => panic!("expected SyncGit, got {other:?}"),
    }
    Ok(())
}

#[test]
fn git_leader_esc_dismisses_without_queueing() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.dispatch(key('g'))?;
    app.dispatch(esc_key())?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    assert!(app.take_pending_request().is_none());
    Ok(())
}

#[test]
fn git_leader_unknown_letter_dismisses_without_queueing() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.dispatch(key('g'))?;
    app.dispatch(key('x'))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    assert!(app.take_pending_request().is_none());
    Ok(())
}

#[test]
fn git_leader_q_does_not_quit_via_global_handler() -> Result<()> {
    // Bug-guard: while the leader is open we must not fall through to
    // the global keymap. `q` would otherwise quit the app.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('g'))?;
    app.dispatch(key('q'))?;
    assert!(!app.is_quit(), "q from git-leader should dismiss, not quit");
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    Ok(())
}

#[test]
fn g_chord_works_from_tasks_tab() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(TASKS_TAB_INDEX)?;
    app.dispatch(key('g'))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::GitLeader);
    app.dispatch(key('s'))?;
    let req = app.take_pending_request().expect("SyncGit queued");
    assert!(matches!(req, AppRequest::SyncGit { .. }));
    Ok(())
}

#[test]
fn g_chord_works_from_notes_tab() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('g'))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::GitLeader);
    app.dispatch(key('s'))?;
    let req = app.take_pending_request().expect("SyncGit queued");
    assert!(matches!(req, AppRequest::SyncGit { .. }));
    Ok(())
}

#[test]
fn git_leader_modal_snapshot() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('g'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("git_leader_80x24", frame);
    Ok(())
}

#[test]
fn git_chord_does_not_trigger_inside_help_overlay() -> Result<()> {
    // Help mode swallows everything except its own dismiss keys, so
    // `g` while help is open must not enter the git leader.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.enter_help();
    app.dispatch(key('g'))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::Help);
    Ok(())
}

#[test]
fn shift_g_does_not_trigger_leader() -> Result<()> {
    // Only bare `g` enters the leader — Shift+G (an unrelated capital
    // letter) should fall through to the active tab or be a no-op.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.dispatch(key_with_mods('G', KeyModifiers::SHIFT))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    Ok(())
}

// ── plan 014: background git sync ────────────────────────────────────────

use crate::tui::event::{BgEvent, CommitJobResult, EventStream, SyncJobResult};
use crate::tui::jobs::JobKind;
use crate::tui::tab::ToastStyle;
use ft_core::git::SyncOutcome;

fn bg_event(outcome: Result<SyncOutcome, String>) -> Event {
    Event::Background(BgEvent::SyncCompleted(SyncJobResult {
        outcome,
        repo: std::path::PathBuf::from("/tmp/test-repo"),
    }))
}

#[test]
fn sync_completed_clean_pushed_renders_toast_and_clears_job() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);
    assert_eq!(app.in_flight_job_for_test(), Some(JobKind::Sync));

    app.dispatch(bg_event(Ok(SyncOutcome::Clean { pushed: true })))?;

    assert!(app.in_flight_job_for_test().is_none());
    let toast = app.current_toast().expect("toast should be set");
    assert_eq!(toast.text, "pushed local commits");
    assert_eq!(toast.style, ToastStyle::Success);
    // No mode transition for clean outcomes.
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    Ok(())
}

#[test]
fn sync_completed_clean_no_push_renders_already_in_sync_toast() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);

    app.dispatch(bg_event(Ok(SyncOutcome::Clean { pushed: false })))?;

    let toast = app.current_toast().expect("toast should be set");
    assert_eq!(toast.text, "already in sync");
    assert_eq!(toast.style, ToastStyle::Success);
    assert!(app.in_flight_job_for_test().is_none());
    Ok(())
}

#[test]
fn sync_completed_synced_renders_compound_toast() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);

    app.dispatch(bg_event(Ok(SyncOutcome::Synced {
        committed: 3,
        pulled: true,
        pushed: true,
    })))?;

    let toast = app.current_toast().expect("toast should be set");
    assert_eq!(toast.text, "sync ok — committed 3, pulled, pushed");
    assert_eq!(toast.style, ToastStyle::Success);
    assert!(app.in_flight_job_for_test().is_none());
    Ok(())
}

#[test]
fn sync_completed_merge_conflict_enters_conflict_mode() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);

    app.dispatch(bg_event(Ok(SyncOutcome::MergeConflict {
        files: vec![std::path::PathBuf::from("seed.md")],
    })))?;

    assert_eq!(app.mode(), crate::tui::ui::Mode::SyncConflict);
    assert!(app.in_flight_job_for_test().is_none());
    Ok(())
}

#[test]
fn sync_completed_rebase_conflict_enters_conflict_mode() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);

    app.dispatch(bg_event(Ok(SyncOutcome::RebaseConflict {
        files: vec![std::path::PathBuf::from("seed.md")],
    })))?;

    assert_eq!(app.mode(), crate::tui::ui::Mode::SyncConflict);
    assert!(app.in_flight_job_for_test().is_none());
    Ok(())
}

#[test]
fn sync_completed_error_renders_red_toast() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);

    app.dispatch(bg_event(Err("branch 'main' has no upstream".to_string())))?;

    let toast = app.current_toast().expect("error toast should be set");
    assert!(
        toast.text.contains("git sync failed:"),
        "expected error prefix, got: {}",
        toast.text
    );
    assert!(
        toast.text.contains("no upstream"),
        "expected error body, got: {}",
        toast.text
    );
    assert_eq!(toast.style, ToastStyle::Error);
    assert!(app.in_flight_job_for_test().is_none());
    Ok(())
}

#[test]
fn sync_completed_while_in_help_mode_still_clears_job() -> Result<()> {
    // Background events bypass the mode short-circuits — a completion
    // that arrives while the help overlay is up must still update the
    // app's state (jobs cleared, toast set) so the user sees the
    // outcome the moment they close help.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);
    app.enter_help();

    app.dispatch(bg_event(Ok(SyncOutcome::Clean { pushed: false })))?;

    assert!(app.in_flight_job_for_test().is_none());
    assert!(app.current_toast().is_some());
    // Help mode untouched.
    assert_eq!(app.mode(), crate::tui::ui::Mode::Help);
    Ok(())
}

#[test]
fn g_s_while_job_in_flight_toasts_already_in_progress() -> Result<()> {
    // The re-entrancy guard fires inside `dispatch_sync_git`, which
    // service_request calls. Drive the full path via an EventStream
    // so the guard sees the in-flight slot we pre-populated.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);

    let events = EventStream::new(std::time::Duration::from_secs(60));
    app.submit_sync_for_test(&events, None)?;

    let toast = app.current_toast().expect("re-entrancy toast");
    assert_eq!(toast.text, "sync already in progress");
    assert_eq!(toast.style, ToastStyle::Info);
    // Indicator stays — the first job is still in flight.
    assert_eq!(app.in_flight_job_for_test(), Some(JobKind::Sync));
    Ok(())
}

#[test]
fn status_bar_shows_sync_indicator_while_job_in_flight() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.set_in_flight_for_test(JobKind::Sync);
    let frame = render(&mut app, 80, 24);
    // The right cell composes `⟳ sync · mode: normal `. Look for the
    // glyph + label to confirm the indicator landed.
    assert!(
        frame.contains("⟳ sync"),
        "expected sync indicator in status bar, got:\n{frame}"
    );
    Ok(())
}

#[test]
fn sync_indicator_in_status_bar_snapshot() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.set_in_flight_for_test(JobKind::Sync);
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("sync_indicator_in_status_bar_80x24", frame);
    Ok(())
}

#[test]
fn status_bar_no_indicator_when_no_job_in_flight() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("⟳ sync"),
        "indicator must only appear while a job is in flight"
    );
    Ok(())
}

#[test]
fn sync_indicator_persists_across_modes() -> Result<()> {
    // The indicator is orthogonal to mode — opening help or the git
    // leader while a sync runs must not hide it.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.set_in_flight_for_test(JobKind::Sync);

    app.enter_help();
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("⟳ sync"),
        "indicator should survive help mode, got:\n{frame}"
    );
    Ok(())
}

#[test]
fn dispatch_sync_git_with_no_git_repo_toasts_and_does_not_spawn() -> Result<()> {
    // Vault that is NOT inside a git repo (test_vault uses a fresh
    // temp dir with no .git/ anywhere up). The submission should
    // toast the "no git repository" error and leave the in-flight
    // slot empty (no worker thread).
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    let events = EventStream::new(std::time::Duration::from_secs(60));

    app.submit_sync_for_test(&events, None)?;

    let toast = app.current_toast().expect("no-repo toast");
    assert!(
        toast.text.contains("no git repository"),
        "expected no-repo error, got: {}",
        toast.text
    );
    assert_eq!(toast.style, ToastStyle::Error);
    assert!(
        app.in_flight_job_for_test().is_none(),
        "no worker should be spawned when discover_repo fails"
    );
    Ok(())
}

// ── e2e: real git worker thread, bare origin + clone ────────────────────

use std::path::Path;

fn run_git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()
        .expect("git exec failed (is git on PATH?)");
    assert!(
        out.status.success(),
        "git {args:?} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Bare origin + cloned vault + `.obsidian/` marker + one seed commit
/// pushed. Returns the vault path. Mirrors the helper used by
/// `ft/tests/git_sync.rs` (kept local here to avoid sharing fixture
/// code across the integration / unit boundary).
fn setup_origin_and_vault(tmp: &Path) -> std::path::PathBuf {
    let origin = tmp.join("origin.git");
    std::fs::create_dir(&origin).unwrap();
    run_git(&origin, &["init", "--bare", "-b", "main"]);

    let vault = tmp.join("vault");
    let out = std::process::Command::new("git")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["clone", origin.to_str().unwrap(), vault.to_str().unwrap()])
        .output()
        .expect("git clone");
    assert!(out.status.success());

    run_git(&vault, &["config", "user.name", "Local"]);
    run_git(&vault, &["config", "user.email", "local@example.com"]);
    run_git(&vault, &["config", "commit.gpgsign", "false"]);
    std::fs::create_dir_all(vault.join(".obsidian")).unwrap();
    std::fs::write(vault.join("seed.md"), "seed\n").unwrap();
    run_git(&vault, &["add", "."]);
    run_git(&vault, &["commit", "-m", "seed"]);
    run_git(&vault, &["push", "-u", "origin", "main"]);
    vault
}

#[test]
fn e2e_background_sync_dirty_tree_dispatches_event_and_renders_toast() -> Result<()> {
    // Drive the full background path against a real bare-origin /
    // clone handshake. Asserts (1) the in-flight slot lights up
    // immediately on submission, (2) the worker thread posts a
    // `Background(SyncCompleted)` event into the channel, (3)
    // dispatching that event clears the slot and sets a success toast.
    let tmp = assert_fs::TempDir::new().unwrap();
    let vault_path = setup_origin_and_vault(tmp.path());

    // Dirty the tree so the sync has something to commit.
    std::fs::write(vault_path.join("new.md"), "hello\n").unwrap();

    let vault = Vault::discover(Some(vault_path)).unwrap();
    let mut app = App::for_test(vault);
    let events = EventStream::new(std::time::Duration::from_millis(500));

    app.submit_sync_for_test(&events, None)?;
    // Indicator lights up before the worker finishes.
    assert_eq!(app.in_flight_job_for_test(), Some(JobKind::Sync));

    // Drain events from the channel until the `Background` arrives.
    // A 1 Hz `Tick` may come in first; ignore non-Background events.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let bg_event = loop {
        if std::time::Instant::now() > deadline {
            panic!("worker did not complete within 15s");
        }
        match events.next()? {
            Event::Background(b) => break b,
            _ => continue,
        }
    };

    app.dispatch(Event::Background(bg_event))?;

    assert!(app.in_flight_job_for_test().is_none());
    let toast = app.current_toast().expect("success toast after sync");
    assert_eq!(toast.style, ToastStyle::Success);
    assert!(
        toast.text.starts_with("sync ok"),
        "expected 'sync ok …' toast, got: {}",
        toast.text
    );
    Ok(())
}

// ── plan 014: background git commit (lightweight sync) ────────────────
//
// `g c` mirrors `g s` but fires the commit-only worker. The leader
// dispatches `CommitGit`, the worker posts `BgEvent::CommitCompleted`,
// and `apply_commit_result` toasts the outcome. These mirror the sync
// tests above minus the conflict branch (commit never pulls).

fn bg_commit_event(outcome: Result<ft_core::git::CommitOutcome, String>) -> Event {
    Event::Background(BgEvent::CommitCompleted(CommitJobResult {
        outcome,
        repo: std::path::PathBuf::from("/tmp/test-repo"),
    }))
}

#[test]
fn git_leader_c_queues_commit_request_and_returns_to_normal() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('g'))?;
    app.dispatch(key('c'))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    let req = app
        .take_pending_request()
        .expect("`g c` should queue an AppRequest::CommitGit");
    match req {
        AppRequest::CommitGit { message } => {
            assert!(message.is_none(), "TUI never overrides the commit message");
        }
        other => panic!("expected CommitGit, got {other:?}"),
    }
    Ok(())
}

#[test]
fn commit_completed_clean_renders_nothing_to_commit_toast_and_clears_job() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Commit);
    assert_eq!(app.in_flight_job_for_test(), Some(JobKind::Commit));

    app.dispatch(bg_commit_event(Ok(ft_core::git::CommitOutcome::Clean)))?;

    assert!(app.in_flight_job_for_test().is_none());
    let toast = app.current_toast().expect("toast should be set");
    assert_eq!(toast.text, "nothing to commit");
    assert_eq!(toast.style, ToastStyle::Success);
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    Ok(())
}

#[test]
fn commit_completed_committed_renders_toast_and_clears_job() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Commit);

    app.dispatch(bg_commit_event(Ok(
        ft_core::git::CommitOutcome::Committed { committed: 2 },
    )))?;

    assert!(app.in_flight_job_for_test().is_none());
    let toast = app.current_toast().expect("toast should be set");
    assert_eq!(toast.text, "committed 2 file(s)");
    assert_eq!(toast.style, ToastStyle::Success);
    Ok(())
}

#[test]
fn commit_completed_error_renders_red_toast() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Commit);

    app.dispatch(bg_commit_event(Err(
        "repository has unresolved conflicts in 1 file(s); resolve before committing".to_string(),
    )))?;

    let toast = app.current_toast().expect("error toast should be set");
    assert!(
        toast.text.contains("git commit failed:"),
        "expected error prefix, got: {}",
        toast.text
    );
    assert_eq!(toast.style, ToastStyle::Error);
    assert!(app.in_flight_job_for_test().is_none());
    Ok(())
}

#[test]
fn g_c_while_job_in_flight_toasts_already_in_progress() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Commit);

    let events = EventStream::new(std::time::Duration::from_secs(60));
    app.submit_commit_for_test(&events, None)?;

    let toast = app.current_toast().expect("re-entrancy toast");
    assert_eq!(toast.text, "commit already in progress");
    assert_eq!(toast.style, ToastStyle::Info);
    assert_eq!(app.in_flight_job_for_test(), Some(JobKind::Commit));
    Ok(())
}

#[test]
fn dispatch_commit_git_with_no_git_repo_toasts_and_does_not_spawn() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    let events = EventStream::new(std::time::Duration::from_secs(60));

    app.submit_commit_for_test(&events, None)?;

    let toast = app.current_toast().expect("no-repo toast");
    assert!(
        toast.text.contains("no git repository"),
        "expected no-repo error, got: {}",
        toast.text
    );
    assert_eq!(toast.style, ToastStyle::Error);
    assert!(
        app.in_flight_job_for_test().is_none(),
        "no worker should be spawned when discover_repo fails"
    );
    Ok(())
}

#[test]
fn status_bar_shows_commit_indicator_while_job_in_flight() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.set_in_flight_for_test(JobKind::Commit);
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("⟳ commit"),
        "expected commit indicator in status bar, got:\n{frame}"
    );
    Ok(())
}

#[test]
fn commit_indicator_persists_across_modes() -> Result<()> {
    // The commit indicator is orthogonal to mode — opening help while a
    // commit runs must not hide it.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.set_in_flight_for_test(JobKind::Commit);

    app.enter_help();
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("⟳ commit"),
        "indicator should survive help mode, got:\n{frame}"
    );
    Ok(())
}

#[test]
fn e2e_background_commit_dirty_tree_dispatches_event_and_renders_toast() -> Result<()> {
    // Commit-only e2e: the worker commits locally and posts a
    // `CommitCompleted` event with the Committed outcome. We also
    // assert the commit was NOT pushed by checking the bare origin.
    let tmp = assert_fs::TempDir::new().unwrap();
    let vault_path = setup_origin_and_vault(tmp.path());

    std::fs::write(vault_path.join("new.md"), "hello\n").unwrap();

    let vault = Vault::discover(Some(vault_path.clone())).unwrap();
    let mut app = App::for_test(vault);
    let events = EventStream::new(std::time::Duration::from_millis(500));

    app.submit_commit_for_test(&events, None)?;
    assert_eq!(app.in_flight_job_for_test(), Some(JobKind::Commit));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let bg_event = loop {
        if std::time::Instant::now() > deadline {
            panic!("worker did not complete within 15s");
        }
        match events.next()? {
            Event::Background(b) => break b,
            _ => continue,
        }
    };

    app.dispatch(Event::Background(bg_event))?;

    assert!(app.in_flight_job_for_test().is_none());
    let toast = app.current_toast().expect("success toast after commit");
    assert_eq!(toast.style, ToastStyle::Success);
    assert!(
        toast.text.starts_with("committed"),
        "expected 'committed …' toast, got: {}",
        toast.text
    );

    // Commit must not push — the bare origin must NOT have new.md.
    let origin = tmp.path().join("origin.git");
    let bare = std::process::Command::new("git")
        .current_dir(&origin)
        .args(["show", "main:new.md"])
        .output()
        .unwrap();
    assert!(
        !bare.status.success(),
        "commit must not push; origin should not have new.md"
    );
    Ok(())
}

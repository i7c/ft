//! History tab: windowed recent-edits feed, seeded section-move, and
//! the send-to-synth overlay. Uses `App::for_test` (production tab
//! layout — History sits at index 5, between Journal and Review).

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;
use std::process::Command as StdCommand;

/// Index of the History tab in the production tab layout.
fn history_tab_idx() -> usize {
    5
}

fn key(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

fn run_git(dir: &Path, args: &[&str]) {
    let out = StdCommand::new("git")
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()
        .expect("git binary on PATH");
    assert!(out.status.success(), "git {args:?}");
}

fn commit_dated(dir: &Path, msg: &str, date: &str) {
    run_git(dir, &["add", "."]);
    let out = StdCommand::new("git")
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .args(["commit", "-m", msg])
        .output()
        .expect("git commit");
    assert!(out.status.success());
}

/// Vault with an old base commit and a *recent* commit that adds a note
/// with a heading + body — so the default 7-day window includes it and
/// the note has a section for the move flow. The base commit is backdated
/// so the `Since(7d)` window has a commit to diff against.
fn recent_edit_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vp = dir.path().join("vault");
    std::fs::create_dir_all(vp.join(".obsidian")).unwrap();
    std::fs::write(vp.join("Seed.md"), "# Seed\n").unwrap();
    run_git(&vp, &["init", "-b", "main"]);
    run_git(&vp, &["config", "user.name", "T"]);
    run_git(&vp, &["config", "user.email", "t@e.com"]);
    run_git(&vp, &["config", "commit.gpgsign", "false"]);
    commit_dated(&vp, "base", "2025-01-01T00:00:00");
    // Recent commit (real "now") — inside the default 7d window.
    std::fs::write(
        vp.join("Daily.md"),
        "# Daily\n\n## Morning\n\nFixed the parser bug today.\n",
    )
    .unwrap();
    run_git(&vp, &["add", "."]);
    run_git(&vp, &["commit", "-m", "recent"]);
    let vault = Vault::discover(Some(vp)).unwrap();
    (dir, vault)
}

/// Vault whose only commits are all older than the 7-day window — the
/// feed should be empty.
fn stale_only_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vp = dir.path().join("vault");
    std::fs::create_dir_all(vp.join(".obsidian")).unwrap();
    std::fs::write(vp.join("Seed.md"), "# Seed\n").unwrap();
    run_git(&vp, &["init", "-b", "main"]);
    run_git(&vp, &["config", "user.name", "T"]);
    run_git(&vp, &["config", "user.email", "t@e.com"]);
    run_git(&vp, &["config", "commit.gpgsign", "false"]);
    commit_dated(&vp, "base", "2025-01-01T00:00:00");
    std::fs::write(vp.join("Old.md"), "# Old\n\nAncient prose.\n").unwrap();
    commit_dated(&vp, "old", "2025-02-01T00:00:00");
    let vault = Vault::discover(Some(vp)).unwrap();
    (dir, vault)
}

#[test]
fn history_tab_renders_recent_feed() -> Result<()> {
    let (_dir, vault) = recent_edit_vault();
    let mut app = App::for_test(vault);
    app.switch_to(history_tab_idx())?;
    assert_eq!(app.active_title(), "History");
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Fixed the parser bug today."),
        "history feed missing the recent paragraph:\n{frame}"
    );
    assert!(
        !frame.contains("Ancient"),
        "feed should be windowed:\n{frame}"
    );
    Ok(())
}

#[test]
fn history_tab_empty_state_when_nothing_recent() -> Result<()> {
    let (_dir, vault) = stale_only_vault();
    let mut app = App::for_test(vault);
    app.switch_to(history_tab_idx())?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("no paragraphs edited in the window"),
        "expected empty-state prompt:\n{frame}"
    );
    Ok(())
}

#[test]
fn history_move_opens_seeded_section_move_modal() -> Result<()> {
    let (_dir, vault) = recent_edit_vault();
    let mut app = App::for_test(vault);
    app.switch_to(history_tab_idx())?;
    // Precondition: the feed rendered at least one row.
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Fixed the parser bug"),
        "no feed row:\n{frame}"
    );
    // `m` opens the shared section-move modal seeded to the row's note.
    app.dispatch(key('m'))?;
    assert_eq!(
        app.active_modal_name(),
        Some("section-move"),
        "pressing m should open the section-move modal"
    );
    Ok(())
}

#[test]
fn history_send_to_synth_opens_existing_picker() -> Result<()> {
    let (_dir, vault) = recent_edit_vault();
    let mut app = App::for_test(vault);
    app.switch_to(history_tab_idx())?;
    let before = render(&mut app, 80, 24);
    assert!(
        !before.contains("Seed"),
        "Seed should not be in the feed pre-picker:\n{before}"
    );
    // `s` opens the existing-note fuzzy picker (lists every vault .md).
    app.dispatch(key('s'))?;
    let after = render(&mut app, 80, 24);
    assert!(
        after.contains("Seed"),
        "existing-note picker should list vault notes:\n{after}"
    );
    Ok(())
}

//! History tab: windowed recent-edits feed, seeded section-move, and
//! the send-to-synth overlay. The recent commit is backdated to a fixed
//! date inside the default 7d window, and the helpers pin `FT_TODAY` to
//! a matching fixed date so the pulse window (which reads
//! `dates::today()` directly, not the App's clock field) stays stable
//! and the snapshot doesn't drift as the calendar advances.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;
use std::process::Command as StdCommand;

/// Index of the History tab in the production tab layout.
fn recent_tab_idx() -> usize {
    3
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

/// Pin `FT_TODAY` so the recent-feed window (which resolves
/// `WindowRange::Since(7d)` against `dates::today()` directly, not the
/// App's clock field) lands on a fixed date matching the backdated
/// commits. Every vault helper in this module calls this so all tests
/// agree on `today = 2026-05-10`, regardless of the real calendar date
/// or test-thread scheduling. Safe under parallel test execution because
/// every caller sets the same value (idempotent).
fn pin_today() {
    std::env::set_var("FT_TODAY", "2026-05-10");
}

/// Vault with an old base commit and a *recent* commit that adds a note
/// with a heading + body — so the default 7-day window includes it and
/// the note has a section for the move flow. The base commit is backdated
/// so the `Since(7d)` window has a commit to diff against.
fn recent_edit_vault() -> (TempDir, Vault) {
    pin_today();
    let dir = TempDir::new().unwrap();
    let vp = dir.path().join("vault");
    std::fs::create_dir_all(vp.join(".obsidian")).unwrap();
    std::fs::write(vp.join("Seed.md"), "# Seed\n").unwrap();
    run_git(&vp, &["init", "-b", "main"]);
    run_git(&vp, &["config", "user.name", "T"]);
    run_git(&vp, &["config", "user.email", "t@e.com"]);
    run_git(&vp, &["config", "commit.gpgsign", "false"]);
    commit_dated(&vp, "base", "2025-01-01T00:00:00");
    // Recent commit, backdated to a fixed date inside the default 7d
    // window relative to the fixed test clock (2026-05-10). Keeping it
    // deterministic (rather than committing at real "now") stops the
    // recent-feed snapshot from drifting as the calendar advances.
    std::fs::write(
        vp.join("Daily.md"),
        "# Daily\n\n## Morning\n\nFixed the parser bug today.\n",
    )
    .unwrap();
    commit_dated(&vp, "recent", "2026-05-09T12:00:00");
    let vault = Vault::discover(Some(vp)).unwrap();
    (dir, vault)
}

/// Vault whose only commits are all older than the 7-day window — the
/// feed should be empty.
fn stale_only_vault() -> (TempDir, Vault) {
    pin_today();
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
    app.switch_to(recent_tab_idx())?;
    assert_eq!(app.active_title(), "Recent");
    let frame = render(&mut app, 80, 24);
    // The list pane renders one compact row per entry; the paragraph
    // body lives in the preview pane for the *selected* entry. The
    // fixture's first entry is the `# Daily` heading, so step down
    // until the preview shows the edited paragraph.
    let down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let mut frame = frame;
    for _ in 0..8 {
        if frame.contains("Fixed the parser bug today.") {
            break;
        }
        app.dispatch(down.clone())?;
        frame = render(&mut app, 80, 24);
    }
    assert!(
        frame.contains("Fixed the parser bug today."),
        "history feed missing the recent paragraph in the preview pane:\n{frame}"
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
    app.switch_to(recent_tab_idx())?;
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
    app.switch_to(recent_tab_idx())?;
    // Precondition: the feed rendered at least one row. Step down to a
    // non-heading entry so the section-move modal has a real section.
    let down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let mut frame = render(&mut app, 80, 24);
    for _ in 0..8 {
        if frame.contains("Fixed the parser bug") {
            break;
        }
        app.dispatch(down.clone())?;
        frame = render(&mut app, 80, 24);
    }
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
    app.switch_to(recent_tab_idx())?;
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

/// `recent_edit_vault` plus an (untracked) synth note citing the recent
/// paragraph byte-identically. Untracked is fine: the citation index
/// reads the working tree, and synth notes are excluded from the feed
/// itself by default.
fn cited_recent_vault() -> (TempDir, Vault) {
    let (dir, vault) = recent_edit_vault();
    let body = "Fixed the parser bug today.";
    let hash = ft_core::synth::callout::compute_section_hash(body);
    std::fs::write(
        vault.path.join("Synth.md"),
        format!(
            "---\nft-synth: true\n---\n\n\
             > [!ft-source] \"Daily.md\" L5-5 @abc1234 #{hash}\n> {body}\n"
        ),
    )
    .unwrap();
    // Re-discover so the App's initial snapshot sees the synth note.
    let vp = vault.path.clone();
    let vault = Vault::discover(Some(vp)).unwrap();
    (dir, vault)
}

#[test]
fn history_rows_show_citation_badge_and_uncited_toggle() -> Result<()> {
    let (_dir, vault) = cited_recent_vault();
    let mut app = App::for_test(vault);
    app.switch_to(recent_tab_idx())?;
    let frame = render(&mut app, 80, 24);
    // The compact list row carries the inline `cited:` badge for the
    // cited entry (the third row in this fixture).
    assert!(
        frame.contains("cited: Synth"),
        "cited paragraph missing badge:\n{frame}"
    );

    app.dispatch(key('u'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("[filter: uncited]"),
        "title missing filter flag:\n{frame}"
    );
    assert!(
        !frame.contains("Fixed the parser bug"),
        "cited paragraph should be filtered:\n{frame}"
    );

    app.dispatch(key('u'))?;
    // Toggle off restores the feed; navigate to the cited paragraph to
    // confirm its body is back in the preview pane.
    let down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let mut frame = render(&mut app, 80, 24);
    for _ in 0..8 {
        if frame.contains("Fixed the parser bug") {
            break;
        }
        app.dispatch(down.clone())?;
        frame = render(&mut app, 80, 24);
    }
    assert!(
        frame.contains("Fixed the parser bug"),
        "toggle off should restore the feed:\n{frame}"
    );
    Ok(())
}

#[test]
fn history_split_layout_snapshot() -> Result<()> {
    // Visual snapshot of the list/preview split: compact list pane on
    // top, preview pane (header + rule + body) on the bottom. The
    // cited entry is the third row; navigate to it so the preview
    // header shows the citation detail and the body shows the paragraph.
    let (_dir, vault) = cited_recent_vault();
    let mut app = App::for_test(vault);
    app.switch_to(recent_tab_idx())?;
    let down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    // Step to the cited paragraph (third entry).
    for _ in 0..2 {
        app.dispatch(down.clone())?;
    }
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("history_split_layout_80x24", frame);
    Ok(())
}

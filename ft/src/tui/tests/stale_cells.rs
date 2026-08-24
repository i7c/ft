//! Regression test: rendering must not leak stale cells across frames.
//!
//! ratatui renders into a persistent buffer and diffs against the
//! previous frame, so any area a widget doesn't fully overwrite keeps
//! the previous frame's characters — a paragraph that shrinks leaves
//! its old longer text visible, and a wide (CJK) character that moves
//! one column leaves the old glyph's second half behind. Two draws on
//! the SAME terminal reproduce it; a fresh terminal renders cleanly,
//! which is why single-frame snapshots never caught it.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use std::process::Command as StdCommand;

fn key(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

fn search_tab_idx() -> usize {
    4
}

fn unicode_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::write(vault_path.join("baseline.md"), "# Baseline\n").unwrap();
    let run_git = |date: Option<&str>, args: &[&str]| {
        let mut cmd = StdCommand::new("git");
        cmd.current_dir(&vault_path).env("GIT_TERMINAL_PROMPT", "0");
        if let Some(d) = date {
            cmd.env("GIT_AUTHOR_DATE", d).env("GIT_COMMITTER_DATE", d);
        }
        let out = cmd.args(args).output().expect("git");
        assert!(out.status.success(), "git {args:?}");
    };
    run_git(None, &["init", "-b", "main"]);
    run_git(None, &["config", "user.name", "T"]);
    run_git(None, &["config", "user.email", "t@e.com"]);
    run_git(None, &["config", "commit.gpgsign", "false"]);
    run_git(None, &["config", "maintenance.auto", "false"]);
    run_git(None, &["add", "."]);
    run_git(Some("2024-01-01T00:00:00"), &["commit", "-m", "c1"]);

    // A very long line with wide (CJK) + combining + emoji chars, and a
    // short paragraph — both matching the query term.
    let long = format!(
        "量子力学は物理学の基礎理論であり、{} さらに続く長い文章… e\u{301}moji 🚀 test ",
        "極めて重要な分野である。".repeat(20)
    );
    std::fs::write(
        vault_path.join("notes.md"),
        format!("# Notes\n\n{long}\n\nshort paragraph here 量 too.\n"),
    )
    .unwrap();
    run_git(None, &["add", "."]);
    run_git(None, &["commit", "-m", "c2"]);

    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn preview_and_query_box_do_not_leak_stale_cells_across_frames() -> Result<()> {
    let (_dir, vault) = unicode_vault();
    let mut app = App::for_test(vault);
    app.switch_to(search_tab_idx())?;
    app.dispatch(key('/'))?;
    for c in "量".chars() {
        app.dispatch(key(c))?;
    }
    // Leave the query editor so Down moves the result cursor.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;

    let backend = TestBackend::new(90, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let draw = |terminal: &mut Terminal<TestBackend>, app: &mut App| {
        terminal.draw(|f| app.render_to(f)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area().height {
            for x in 0..buf.area().width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    };

    // Frame 1: the long CJK paragraph is selected; its wrapped body
    // fills the whole preview pane.
    draw(&mut terminal, &mut app);

    // Frame 2: move to the short result. The preview pane shows a
    // one-line body; everything below it must be blank.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    let frame2 = draw(&mut terminal, &mut app);

    // Preview rows below the one-line body (rows 11..21 inside the tab
    // block) must not keep the previous frame's long paragraph.
    let buf = terminal.backend().buffer().clone();
    for y in 11..21 {
        for x in 1..89 {
            assert_eq!(
                buf[(x, y)].symbol(),
                " ",
                "stale cell at ({x},{y}) from frame 1:\n{frame2}"
            );
        }
    }
    Ok(())
}

#[test]
fn shortened_query_does_not_leave_a_stale_tail() -> Result<()> {
    let (_dir, vault) = unicode_vault();
    let mut app = App::for_test(vault);
    app.switch_to(search_tab_idx())?;
    app.dispatch(key('/'))?;
    // Type a long query, then delete back to a short one.
    for c in "量子力学は".chars() {
        app.dispatch(key(c))?;
    }
    for _ in 0..4 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }

    let backend = TestBackend::new(90, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render_to(f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let mut row3 = String::new();
    for x in 0..buf.area().width {
        row3.push_str(buf[(x, 3)].symbol());
    }
    assert!(
        !row3.contains('学'),
        "query row leaked the deleted tail:\n{row3}"
    );
    assert!(
        row3.contains('量'),
        "query row should still show the kept term:\n{row3}"
    );
    Ok(())
}

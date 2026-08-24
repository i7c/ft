//! Search tab: live as-you-type paragraph search, all/any toggle, sort
//! cycle, multi-select + send-to-synth handoff, and the Pulse handoff.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::process::Command as StdCommand;

/// Index of the Search tab in the production tab layout.
fn search_tab_idx() -> usize {
    4
}

fn key(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

/// Vault with a spread of paragraphs (prose + wikilinks), committed so
/// blame dates are resolvable.
fn search_vault() -> (TempDir, Vault) {
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
    std::fs::write(
        vault_path.join("notes.md"),
        "The eigen decomposition is central.\n\n\
         Memoization strategy pays off here.\n\n\
         Eigen and memoization together in one paragraph.\n\n\
         See [[Foo]] and [[Bar]] here.\n\n\
         Only [[Foo]] here.\n",
    )
    .unwrap();
    run_git(None, &["add", "."]);
    run_git(None, &["commit", "-m", "c2"]);
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn live_typing_updates_results_and_snapshot() -> Result<()> {
    let (_dir, vault) = search_vault();
    let mut app = App::for_test(vault);
    app.switch_to(search_tab_idx())?;
    app.dispatch(key('/'))?;
    for c in "eigen".chars() {
        app.dispatch(key(c))?;
    }
    let frame = render(&mut app, 90, 24);
    // The query bar echoes the typed term; substring matching surfaces
    // every paragraph containing "eigen".
    assert!(frame.contains("eigen"), "query bar missing term:\n{frame}");
    assert!(
        frame.contains("L5-5  eigen") && frame.contains("L1-1  eigen"),
        "result list missing path/lines/matched-label rows:\n{frame}"
    );
    assert!(
        frame.contains("(2 results"),
        "expected 2 eigen matches:\n{frame}"
    );
    // The feed-split preview pane echoes the selected paragraph under a
    // header band that carries the relevance score.
    assert!(
        frame.contains("· score "),
        "preview header missing relevance score:\n{frame}"
    );
    assert!(
        frame.contains("Eigen and memoization together in one paragraph."),
        "preview pane missing selected paragraph body:\n{frame}"
    );
    assert_tui_snapshot!("search_live_typing_80x24", frame);
    Ok(())
}

#[test]
fn any_toggle_unions_terms() -> Result<()> {
    let (_dir, vault) = search_vault();
    let mut app = App::for_test(vault);
    app.switch_to(search_tab_idx())?;
    app.dispatch(key('/'))?;
    for c in "eigen memoization".chars() {
        app.dispatch(key(c))?;
    }
    // AND: only the paragraph containing both.
    let frame = render(&mut app, 90, 24);
    assert!(frame.contains("(1 result"), "AND scope:\n{frame}");

    // Leave the query editor, then toggle to ANY.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    app.dispatch(key('a'))?;
    let frame = render(&mut app, 90, 24);
    assert!(
        frame.contains("ANY · sort: relevance") && frame.contains("(3 results"),
        "ANY scope:\n{frame}"
    );
    Ok(())
}

#[test]
fn sort_cycle_flips_status_and_order() -> Result<()> {
    let (_dir, vault) = search_vault();
    let mut app = App::for_test(vault);
    app.switch_to(search_tab_idx())?;
    app.dispatch(key('/'))?;
    for c in "eigen".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    app.dispatch(key('o'))?;
    let frame = render(&mut app, 90, 24);
    assert!(
        frame.contains("sort: date"),
        "date sort indicator:\n{frame}"
    );
    app.dispatch(key('o'))?;
    let frame = render(&mut app, 90, 24);
    assert!(
        frame.contains("sort: relevance"),
        "back to relevance:\n{frame}"
    );
    Ok(())
}

#[test]
fn clear_returns_to_empty_state() -> Result<()> {
    let (_dir, vault) = search_vault();
    let mut app = App::for_test(vault);
    app.switch_to(search_tab_idx())?;
    app.dispatch(key('/'))?;
    for c in "eigen".chars() {
        app.dispatch(key(c))?;
    }
    let frame = render(&mut app, 90, 24);
    assert!(
        frame.contains("(2 results"),
        "results before clear:\n{frame}"
    );

    // Leave the query editor, then clear with `c`.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    app.dispatch(key('c'))?;
    let frame = render(&mut app, 90, 24);
    assert!(
        frame.contains("press / to start typing"),
        "empty-state hint after clear:\n{frame}"
    );
    assert!(
        !frame.contains("(2 results"),
        "results must be gone after clear:\n{frame}"
    );
    assert!(
        frame.contains("(0 results, 0 selected)"),
        "status resets to zero counts:\n{frame}"
    );
    Ok(())
}

#[test]
fn pulse_handoff_opens_search_prefilled_in_any_mode() -> Result<()> {
    use crossterm::event::KeyModifiers;
    let (_dir, vault) = search_vault();
    let mut app = App::for_test(vault);
    // Pulse tab = index 2. The fixture's only commit adds [[Foo]] /
    // [[Bar]] links, so the pulse (any window that includes c1) ranks
    // them. Select the first row and hand off with Enter.
    app.switch_to(2)?;
    app.dispatch(key(' '))?; // select the cursor row
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.service_pending_requests()?;
    assert_eq!(app.active_title(), "Search", "handoff must land on Search");
    let frame = render(&mut app, 90, 24);
    assert!(
        frame.contains("[[") && frame.contains("ANY"),
        "prefilled query + any-mode:\n{frame}"
    );
    Ok(())
}

//! Search tab: live as-you-type paragraph search, all/any toggle, sort
//! cycle, multi-select, and the send-to-synth handoff.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::process::Command as StdCommand;

/// Index of the Search tab in the production tab layout.
fn search_tab_idx() -> usize {
    2
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
        frame.contains("notes.md (2)"),
        "grouped results carry a note header row:\n{frame}"
    );
    assert!(
        frame.contains("L5-5  eigen") && frame.contains("L1-1  eigen"),
        "result list missing lines/matched-label rows:\n{frame}"
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
    // The cursor starts on the note header, which previews its best
    // member: BM25 ranks the shorter single-mention paragraph first.
    assert!(
        frame.contains("The eigen decomposition is central."),
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

/// Type into the existing-note picker and press Enter to select the
/// first match (the shared synth-send flow's append path).
fn select_existing_note_in_picker(app: &mut App, query: &str) -> Result<()> {
    for ch in query.chars() {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char(ch),
            KeyModifiers::NONE,
        )))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    Ok(())
}

/// `s` on the Search tab opens the shared existing-note picker.
#[test]
fn search_send_to_synth_existing_opens_picker_on_s() -> Result<()> {
    let (_dir, vault) = search_vault();
    let mut app = App::for_test(vault);
    app.switch_to(search_tab_idx())?;
    app.dispatch(key('/'))?;
    for c in "Foo".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?; // apply the query (leaves edit mode)
          // `s` opens the existing-note fuzzy picker.
    app.dispatch(key('s'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("notes.md") || frame.contains("baseline.md"),
        "existing-note picker should list vault notes:\n{frame}"
    );
    Ok(())
}

/// Sending search results to a synth note applies append-dedup: an
/// already-pinned paragraph is not re-added.
#[test]
fn search_send_to_existing_dedups_already_pinned_entries() -> Result<()> {
    let (_dir, vault) = search_vault();
    let vault_path = vault.path.clone();

    // Pre-create a synth note that already pins the "See [[Foo]] and
    // [[Bar]] here." paragraph (notes.md line 7), leaving the second
    // Foo result ("Only [[Foo]] here.", line 9) unpinned.
    let abs = vault_path.join("Synth/topic.md");
    std::fs::create_dir_all(abs.parent().unwrap()).ok();
    let pinned = ft_core::synth::source::SynthSource {
        source_path: std::path::PathBuf::from("notes.md"),
        line_start: 7,
        line_end: 7,
        body: "See [[Foo]] and [[Bar]] here.".to_string(),
    };
    let plan = ft_core::synth::scaffold::plan_synth_scaffold(
        &vault,
        std::path::Path::new("Synth/topic.md"),
        std::slice::from_ref(&pinned),
    )?;
    ft_core::synth::scaffold::apply_synth_scaffold(&vault, &plan)?;

    let mut app = App::for_test(vault);
    app.switch_to(search_tab_idx())?;
    app.dispatch(key('/'))?;
    for c in "Foo".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?; // apply the query (leaves edit mode)
    app.pump_graph_rebuild_for_test();

    // `s` → type "topic" + Enter.
    app.dispatch(key('s'))?;
    select_existing_note_in_picker(&mut app, "topic")?;
    app.service_pending_requests()?;

    let body = std::fs::read_to_string(&abs).unwrap();
    let count = body.matches("[!ft-source]").count();
    // The pinned paragraph is skipped; only the second Foo result is
    // newly appended.
    assert_eq!(
        count, 2,
        "dedup should keep the existing section and add only the missing one:\n{body}"
    );
    Ok(())
}

fn esc() -> Event {
    Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
}

fn type_query(app: &mut App, q: &str) -> Result<()> {
    app.dispatch(key('/'))?;
    for c in q.chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(esc())?;
    Ok(())
}

/// `g` drops the note header rows for one row per paragraph and back.
#[test]
fn grouping_toggle_switches_to_flat_rows() -> Result<()> {
    let (_dir, vault) = search_vault();
    let mut app = App::for_test(vault);
    app.switch_to(search_tab_idx())?;
    type_query(&mut app, "eigen")?;
    app.dispatch(key('g'))?;
    let frame = render(&mut app, 90, 24);
    assert!(frame.contains("· flat"), "status shows flat:\n{frame}");
    assert!(!frame.contains("notes.md (2)"), "no header row:\n{frame}");
    assert!(
        frame.contains("notes.md L1-1  eigen") && frame.contains("notes.md L5-5  eigen"),
        "flat rows lead with the path:\n{frame}"
    );
    assert_tui_snapshot!("search_flat_rows_90x24", frame);
    app.dispatch(key('g'))?;
    let frame = render(&mut app, 90, 24);
    assert!(frame.contains("· grouped") && frame.contains("notes.md (2)"));
    Ok(())
}

/// `Space` on a note header selects every paragraph of that note.
#[test]
fn space_on_header_selects_the_whole_note() -> Result<()> {
    let (_dir, vault) = search_vault();
    let mut app = App::for_test(vault);
    app.switch_to(search_tab_idx())?;
    type_query(&mut app, "eigen")?;
    app.dispatch(key(' '))?;
    let frame = render(&mut app, 90, 24);
    assert!(
        frame.contains("2 selected"),
        "both members selected:\n{frame}"
    );
    app.dispatch(key(' '))?;
    let frame = render(&mut app, 90, 24);
    assert!(
        frame.contains("0 selected"),
        "second press clears:\n{frame}"
    );
    Ok(())
}

/// The preview pane styles every match of the query.
#[test]
fn preview_highlights_matches() -> Result<()> {
    let (_dir, vault) = search_vault();
    let mut app = App::for_test(vault);
    app.switch_to(search_tab_idx())?;
    type_query(&mut app, "eigen")?;
    let backend = TestBackend::new(90, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render_to(f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let text = buffer_to_string(&buf);
    let (y, line) = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("The eigen decomposition is central."))
        .expect("preview body line");
    let col = line.chars().position(|_| true).unwrap_or(0);
    let x = line[..line.find("eigen").unwrap()].chars().count() + col;
    let cell = &buf[(x as u16, y as u16)];
    assert_eq!(
        cell.style().bg,
        Some(crate::tui::palette::SECONDARY),
        "`eigen` is highlighted in the preview"
    );
    let plain = &buf[((x - 2) as u16, y as u16)];
    assert_ne!(plain.style().bg, Some(crate::tui::palette::SECONDARY));
    Ok(())
}

/// An unknown `scope:` preset reports in the tab instead of failing.
#[test]
fn unknown_scope_shows_error() -> Result<()> {
    let (_dir, vault) = search_vault();
    let mut app = App::for_test(vault);
    app.switch_to(search_tab_idx())?;
    type_query(&mut app, "eigen scope:nope")?;
    let frame = render(&mut app, 90, 24);
    assert!(
        frame.contains("unknown scope preset `nope`"),
        "error names the preset:\n{frame}"
    );
    assert!(
        frame.contains("1 filter"),
        "status counts the filter:\n{frame}"
    );
    assert_tui_snapshot!("search_unknown_scope_90x24", frame);
    Ok(())
}

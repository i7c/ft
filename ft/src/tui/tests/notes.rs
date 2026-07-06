//! Notes tab: browse, section-move flow, recents picker, create
//! flow, periodic notes.

use super::*;

// ── Notes tab (plan 003 · session 3) ─────────────────────────────────────

/// Notes-tab snapshot vault: a couple of files with headings so the
/// fuzzy picker has something to surface.
fn notes_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::write(
        vault_path.join("project.md"),
        "# Project\n\n## Background\n\nIntro.\n\n## Tasks\n\n- Do thing\n",
    )
    .unwrap();
    std::fs::write(vault_path.join("inbox.md"), "# Inbox\n\nNotes.\n").unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn notes_tab_idle_renders_keymap_panel() -> Result<()> {
    let (_dir, vault) = notes_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_idle_80x24", frame);
    Ok(())
}

#[test]
fn notes_tab_help_overlay_renders_over_idle() -> Result<()> {
    let (_dir, vault) = notes_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('?'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_help_overlay_80x24", frame);
    Ok(())
}

#[test]
fn timeblocks_tab_help_overlay_renders() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(6)?;
    app.enter_help();
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("timeblocks_help_overlay_80x24", frame);
    Ok(())
}

// --- help overlay scrolling --------------------------------------------------
//
// `?` on the Graph tab overflows an 80×24 terminal, so it exercises the
// scroll + scrollbar path. These tests assert the overlay actually
// moves its viewport in response to the mode-local scroll keys and
// clamps at the bounds, rather than silently clipping rows.

fn key_event(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn help_overlay_scrolls_on_graph_tab() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    // Graph is tab 0 (the default). Enter help; the content overflows
    // 80×24 so a scrollbar is rendered.
    app.enter_help();
    let top = render(&mut app, 80, 24);
    assert_tui_snapshot!("help_overlay_graph_scrolled_top_80x24", top);
    // The scrollbar thumb is rendered on the right edge on overflow.
    assert!(
        top.contains('█'),
        "scrollbar thumb should be visible on overflow"
    );
    // The header is visible at the top.
    assert!(top.contains("Keybindings — Graph"));

    // Scroll down several lines. The header should scroll out of view
    // and rows that were previously clipped become visible.
    for _ in 0..6 {
        app.dispatch(key_event(KeyCode::Down))?;
    }
    let down = render(&mut app, 80, 24);
    assert_tui_snapshot!("help_overlay_graph_scrolled_down_80x24", down);
    // Header no longer visible after scrolling past it.
    assert!(!down.contains("Keybindings — Graph"));
    Ok(())
}

#[test]
fn help_overlay_page_keys() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.enter_help();
    let top = render(&mut app, 80, 24);

    // PageDown advances by one viewport. The visible window must
    // differ from the top frame.
    app.dispatch(key_event(KeyCode::PageDown))?;
    let paged = render(&mut app, 80, 24);
    assert_ne!(top, paged, "PageDown should move the viewport");

    // PageUp returns to the top.
    app.dispatch(key_event(KeyCode::PageUp))?;
    let back = render(&mut app, 80, 24);
    assert_eq!(top, back, "PageUp should return to the top");
    Ok(())
}

#[test]
fn help_overlay_scroll_clamps_at_bottom() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.enter_help();
    // `G` jumps to the end; the render clamp bounds the offset to
    // `max_scroll` so no panic and the last row is visible.
    app.dispatch(key_event(KeyCode::Char('G')))?;
    let at_end = render(&mut app, 80, 24);
    // The footer hint is the last line; at the bottom of the content it
    // must be visible.
    assert!(at_end.contains("?/Esc/q close"));

    // Pressing `j` past the end must not advance further — the frame
    // is stable (clamped).
    app.dispatch(key_event(KeyCode::Char('j')))?;
    let still_end = render(&mut app, 80, 24);
    assert_eq!(at_end, still_end, "scroll should clamp at the bottom");
    Ok(())
}

#[test]
fn help_overlay_reopen_resets_scroll() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.enter_help();
    let top = render(&mut app, 80, 24);

    // Scroll down, close, reopen: the reopened overlay must show the
    // top again (offset reset to 0 on entry).
    for _ in 0..4 {
        app.dispatch(key_event(KeyCode::Down))?;
    }
    let _scrolled = render(&mut app, 80, 24);
    app.dispatch(key('?'))?; // close
    app.enter_help();
    let reopened = render(&mut app, 80, 24);
    assert_eq!(top, reopened, "reopening should reset scroll to the top");
    Ok(())
}

#[test]
fn notes_tab_open_picker_renders_results() -> Result<()> {
    let (_dir, vault) = notes_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('o'))?;
    for c in "project".chars() {
        app.dispatch(key(c))?;
    }
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_open_picker_80x24", frame);
    Ok(())
}

#[test]
fn notes_tab_open_picker_enter_queues_editor_open() -> Result<()> {
    let (dir, vault) = notes_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('o'))?;
    for c in "project".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let req = app
        .take_pending_request()
        .expect("Enter should queue OpenInEditor");
    match req {
        AppRequest::OpenInEditor { path, line: _ } => {
            let expected = dir
                .path()
                .join("test-vault/project.md")
                .canonicalize()
                .unwrap();
            assert_eq!(path.canonicalize().unwrap(), expected);
        }
        other => panic!("expected OpenInEditor, got {other:?}"),
    }
    Ok(())
}

#[test]
fn notes_tab_open_picker_ctrl_o_queues_obsidian_url() -> Result<()> {
    let (_dir, vault) = notes_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('o'))?;
    for c in "project".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL,
    )))?;
    let req = app
        .take_pending_request()
        .expect("Ctrl+O should queue OpenInObsidian");
    match req {
        AppRequest::OpenInObsidian { url } => {
            assert!(
                url.starts_with("obsidian://open?vault=") && url.contains("file=project.md"),
                "unexpected URL: {url}"
            );
        }
        other => panic!("expected OpenInObsidian, got {other:?}"),
    }
    Ok(())
}

#[test]
fn notes_tab_open_picker_esc_returns_to_idle() -> Result<()> {
    let (_dir, vault) = notes_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('o'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("pick file / heading"),
        "picker should be closed:\n{frame}"
    );
    Ok(())
}

// ── Notes tab · section-move flow (plan 003 · session 4) ─────────────────

/// Vault tailored for the section-move flow. Two notes with a few headings
/// and a known nested structure: `project.md` has H1 + two H2s, one of
/// which has an H3 child — the nested heading lets us exercise the
/// implicit-selection cascade.
fn notes_move_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::write(
        vault_path.join("project.md"),
        "# Project\n\n## Background\n\nIntro.\n\n### Details\n\nMore.\n\n## Tasks\n\n- Do thing\n",
    )
    .unwrap();
    std::fs::write(
        vault_path.join("archive.md"),
        "# Archive\n\n## Old\n\nStale notes.\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

/// Drive the Notes tab into the heading-multi-select step with
/// `project.md` as the source. Returns the populated App.
fn drive_to_multiselect(vault: Vault) -> Result<App> {
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('m'))?;
    for c in "project".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    Ok(app)
}

#[test]
fn notes_move_source_picker_opens_on_m() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('m'))?;
    for c in "project".chars() {
        app.dispatch(key(c))?;
    }
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_move_source_picker_80x24", frame);
    Ok(())
}

#[test]
fn notes_move_source_picker_esc_returns_to_idle() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('m'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("1/4 source"),
        "source picker should be closed:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_multiselect_renders_after_source_pick() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_multiselect(vault)?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("2/4 select"),
        "should land on multi-select step:\n{frame}"
    );
    assert!(
        frame.contains("Background"),
        "headings should be listed:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_multiselect_implicit_descendants_dim() -> Result<()> {
    // Select the H2 "Background" — its H3 child "Details" should show
    // as implicitly included, with the dimmed marker glyph.
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_multiselect(vault)?;
    // Focus is on heading 0 (H1 "Project"). Move to "Background" (idx 1).
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_move_multiselect_80x24", frame);
    Ok(())
}

#[test]
fn notes_move_multiselect_descendant_toggle_blocked_by_parent() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_multiselect(vault)?;
    // Select Background (idx 1) — Details (idx 2) becomes implicit.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    // Move to Details and try to toggle — should be a no-op.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    // Now deselect Background — Details should return to unselected (no
    // implicit, no explicit).
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    // After deselecting Background, no row should carry an explicit or
    // implicit marker — only the empty box.
    assert!(
        !frame.contains('■') && !frame.contains('▣'),
        "all selection markers should be cleared:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_multiselect_enter_advances_to_target_picker() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_multiselect(vault)?;
    // Pick H1 Project (focus starts here).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    for c in "archive".chars() {
        app.dispatch(key(c))?;
    }
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_move_target_picker_80x24", frame);
    Ok(())
}

#[test]
fn notes_move_multiselect_enter_with_no_selection_stays() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_multiselect(vault)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("2/4 select"),
        "should remain on multi-select with no picks:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_multiselect_esc_returns_to_source_picker() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_multiselect(vault)?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("1/4 source"),
        "should return to source picker:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_target_same_file_rejected_inline() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_multiselect(vault)?;
    // Pick H1 and advance to target picker.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Type a query that matches the source file.
    for c in "project".chars() {
        app.dispatch(key(c))?;
    }
    // Press Enter on the source file (same path) — should be rejected.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("same-file move"),
        "footer should explain the rejection:\n{frame}"
    );
    assert!(
        frame.contains("3/4 target"),
        "should still be on target step:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_target_enter_advances_to_compose() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_multiselect(vault)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    for c in "archive".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("4/4 compose"),
        "should advance to compose:\n{frame}"
    );
    assert!(
        frame.contains("archive.md"),
        "target file should appear in title:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_target_esc_returns_to_multiselect_preserving_picks() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_multiselect(vault)?;
    // Pick H1 Project (focus starts here) then advance.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Back out without picking a target.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("2/4 select"),
        "should be back on multi-select:\n{frame}"
    );
    // The explicit-pick marker should still be visible for `Project`.
    assert!(
        frame.contains('■'),
        "selection should be preserved:\n{frame}"
    );
    Ok(())
}

// ── Notes tab · section-move flow (plan 003 · session 5) ─────────────────

/// Drive the Notes tab all the way to the compose step, with the H1
/// "Project" picked as the only section. Source is `project.md`, target
/// is `archive.md`.
fn drive_to_compose(vault: Vault) -> Result<App> {
    let mut app = drive_to_multiselect(vault)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    for c in "archive".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    Ok(app)
}

#[test]
fn notes_move_compose_renders_interleaved_layout() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_move_compose_80x24", frame);
    Ok(())
}

#[test]
fn notes_move_compose_esc_returns_to_target_picker() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("3/4 target"),
        "Esc should return to target picker:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_compose_level_shift_clamps_at_one() -> Result<()> {
    // Pending starts at H1; Left should be ignored (already min).
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    let before = render(&mut app, 80, 24);
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)))?;
    let after = render(&mut app, 80, 24);
    assert_eq!(before, after, "Left at H1 should be a no-op");
    Ok(())
}

#[test]
fn notes_move_compose_level_shift_right_increments() -> Result<()> {
    // Move Pending from H1 to H2. Note: source content has H1 (Project)
    // with H2/H3 descendants, so shifting from 1→2 would cascade an H3
    // to H4, which is fine (no overflow). Shifting to H2 should succeed.
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Right,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    // After shift the focused Pending row should be H2.
    assert!(
        frame.contains("H2  Project"),
        "Pending row should now be H2:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_compose_enter_commits_and_writes_files() -> Result<()> {
    let (dir, vault) = notes_move_vault();
    let vault_path = vault.path.clone();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let toast = app
        .current_toast()
        .expect("commit should surface a success toast");
    assert!(
        toast.text.starts_with("Moved 1 section(s):"),
        "success toast text: {}",
        toast.text
    );
    let new_source = std::fs::read_to_string(vault_path.join("project.md"))?;
    let new_target = std::fs::read_to_string(vault_path.join("archive.md"))?;
    assert!(
        !new_source.contains("# Project"),
        "H1 should be moved out of source:\n{new_source}"
    );
    assert!(
        new_target.contains("# Project"),
        "H1 should appear in target:\n{new_target}"
    );
    // Returned to idle.
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("4/4 compose"),
        "should leave compose after commit:\n{frame}"
    );
    drop(dir);
    Ok(())
}

#[test]
fn notes_move_compose_reorder_shift_down_swaps_with_anchor() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    // Focus starts on the first Pending row, which sits after the target's
    // anchors. Shift+Up swaps the Pending up past one Anchor.
    let before = render(&mut app, 80, 24);
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)))?;
    let after = render(&mut app, 80, 24);
    assert_ne!(
        before, after,
        "Shift+Up on the first Pending should reorder it past an Anchor"
    );
    Ok(())
}

// ── Notes tab · section-move flow (plan 007 · rename) ────────────────────

/// Drive into compose with two H2 picks (Background + Tasks). Source
/// `project.md` keeps H1 Project; target is `archive.md`. Two Pending
/// rows in the compose layout.
fn drive_to_compose_two_h2_picks(vault: Vault) -> Result<App> {
    let mut app = drive_to_multiselect(vault)?;
    // Focus starts on heading 0 (H1 Project). Move down to H2 Background.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    // Down past H3 Details (implicit) to H2 Tasks.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    for c in "archive".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    Ok(app)
}

#[test]
fn notes_move_compose_r_opens_rename_buffer_prefilled() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(key('r'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("rename → Project"),
        "buffer should be pre-filled with source text:\n{frame}"
    );
    assert!(
        frame.contains("commit rename"),
        "footer should switch to the rename-buffer keymap:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_compose_rename_enter_commits_override() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(key('r'))?;
    // Clear pre-filled text and type a new title.
    for _ in 0..32 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "Sprint 1".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("→ Sprint 1"),
        "Pending row should show the rename override:\n{frame}"
    );
    assert!(
        !frame.contains("rename → "),
        "edit field should be gone after commit:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_compose_rename_empty_keeps_buffer_open_with_toast() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(key('r'))?;
    // Empty out the pre-filled text.
    for _ in 0..32 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let toast = app
        .current_toast()
        .expect("empty rename should surface a toast");
    assert_eq!(toast.text, "rename cannot be empty");
    assert_eq!(toast.style, crate::tui::tab::ToastStyle::Error);
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("rename → "),
        "buffer should stay open after invalid Enter:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_compose_rename_whitespace_only_keeps_buffer_open_with_toast() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(key('r'))?;
    for _ in 0..32 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    app.dispatch(key(' '))?;
    app.dispatch(key(' '))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let toast = app
        .current_toast()
        .expect("whitespace-only rename should surface a toast");
    assert_eq!(toast.text, "rename cannot be empty");
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("rename → "),
        "buffer should stay open after whitespace-only Enter:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_compose_rename_esc_discards_buffer() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(key('r'))?;
    for c in "garbage".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("rename → "),
        "buffer should be closed after Esc:\n{frame}"
    );
    assert!(
        !frame.contains("→ garbage"),
        "row should not carry the discarded override:\n{frame}"
    );
    assert!(
        frame.contains("4/4 compose"),
        "should still be on compose step:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_compose_rename_buffer_swallows_shift_up() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(key('r'))?;
    let before = render(&mut app, 80, 24);
    // Shift+Up would reorder a Pending row in normal compose; the
    // buffer must swallow it so the layout stays put.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)))?;
    let after = render(&mut app, 80, 24);
    assert_eq!(
        before, after,
        "Shift+Up should be a no-op while the rename buffer is open"
    );
    Ok(())
}

#[test]
fn notes_move_compose_rename_preserved_after_shift_up() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(key('r'))?;
    for _ in 0..32 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "Renamed".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Now reorder the renamed Pending row up past an Anchor.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("→ Renamed"),
        "rename override should survive a reorder:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_compose_rename_preserved_after_level_shift() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(key('r'))?;
    for _ in 0..32 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "Renamed".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Bump level from H1 to H2; cascade is safe (H3 → H4 still in range).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Right,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("H2  Project"),
        "level shift should apply:\n{frame}"
    );
    assert!(
        frame.contains("→ Renamed"),
        "rename override should survive a level shift:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_compose_rename_writes_renamed_heading_to_disk() -> Result<()> {
    let (dir, vault) = notes_move_vault();
    let vault_path = vault.path.clone();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(key('r'))?;
    for _ in 0..32 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "Renamed Project".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Commit the move.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let toast = app
        .current_toast()
        .expect("commit should surface a success toast");
    assert!(
        toast.text.starts_with("Moved 1 section(s):"),
        "success toast: {}",
        toast.text
    );
    let new_target = std::fs::read_to_string(vault_path.join("archive.md"))?;
    assert!(
        new_target.contains("# Renamed Project"),
        "target should contain the renamed H1:\n{new_target}"
    );
    assert!(
        !new_target.contains("# Project\n"),
        "target should NOT contain the original H1 line:\n{new_target}"
    );
    drop(dir);
    Ok(())
}

#[test]
fn notes_move_compose_renamed_snapshot() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(key('r'))?;
    for _ in 0..32 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "Renamed Project".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_move_compose_renamed_80x24", frame);
    Ok(())
}

#[test]
fn notes_move_compose_renaming_snapshot() -> Result<()> {
    let (_dir, vault) = notes_move_vault();
    let mut app = drive_to_compose(vault)?;
    app.dispatch(key('r'))?;
    for _ in 0..32 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "Sprint".chars() {
        app.dispatch(key(c))?;
    }
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_move_compose_renaming_80x24", frame);
    Ok(())
}

#[test]
fn notes_move_compose_rename_e2e_two_h2_picks() -> Result<()> {
    let (dir, vault) = notes_move_vault();
    let vault_path = vault.path.clone();
    let mut app = drive_to_compose_two_h2_picks(vault)?;
    // Layout: anchors [Archive, Old] then pending [Background, Tasks].
    // Focus lands on the first Pending (Background). Move focus down to
    // the second Pending (Tasks) and rename only that one.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    app.dispatch(key('r'))?;
    for _ in 0..32 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "Sprint 1".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Commit.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let toast = app
        .current_toast()
        .expect("commit should surface a success toast");
    assert!(
        toast.text.starts_with("Moved 2 section(s):"),
        "success toast: {}",
        toast.text
    );
    let new_source = std::fs::read_to_string(vault_path.join("project.md"))?;
    let new_target = std::fs::read_to_string(vault_path.join("archive.md"))?;
    // Source loses both moved sections.
    assert!(
        !new_source.contains("## Background"),
        "Background should be removed from source:\n{new_source}"
    );
    assert!(
        !new_source.contains("## Tasks"),
        "Tasks should be removed from source:\n{new_source}"
    );
    // Target keeps prior content and gains both sections; Tasks is renamed.
    assert!(
        new_target.contains("# Archive"),
        "Archive H1 preserved in target:\n{new_target}"
    );
    assert!(
        new_target.contains("## Background"),
        "un-renamed pending should land verbatim:\n{new_target}"
    );
    assert!(
        new_target.contains("### Details"),
        "nested H3 should cascade with its parent:\n{new_target}"
    );
    assert!(
        new_target.contains("## Sprint 1"),
        "renamed pending should land with the new title:\n{new_target}"
    );
    assert!(
        !new_target.contains("## Tasks"),
        "original 'Tasks' title should NOT appear:\n{new_target}"
    );
    drop(dir);
    Ok(())
}

// ── plan 008: empty-input picker shows recents ───────────────────────────────

/// Build a notes vault with explicit deterministic mtimes so recents tests
/// can assert ordering by recency rather than relying on file-system
/// resolution. `files` is `(rel_path, body, mtime_offset_seconds_from_base)`
/// — bigger offset = newer file.
fn recents_vault(files: &[(&str, &str, u64)]) -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    let base = std::time::SystemTime::now();
    for (rel, body, offset) in files {
        let abs = vault_path.join(rel);
        if let Some(p) = abs.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(&abs, body).unwrap();
        let mt = base + std::time::Duration::from_secs(*offset);
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&abs) {
            let _ = f.set_times(std::fs::FileTimes::new().set_modified(mt));
        }
    }
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

fn make_test_recents(vault: &Vault, tmp: &TempDir) -> Arc<RecentsLog> {
    let log_path = tmp.path().join("recents.jsonl");
    Arc::new(RecentsLog::with_log_path(vault.path.clone(), log_path))
}

#[test]
fn notes_open_picker_shows_logged_open_first() -> Result<()> {
    let (dir, vault) = recents_vault(&[
        ("alpha.md", "# Alpha\n", 100),
        ("beta.md", "# Beta\n", 200),
        ("gamma.md", "# Gamma\n", 300),
    ]);
    let recents = make_test_recents(&vault, &dir);
    // Log an open on alpha — even though gamma has the newest mtime,
    // alpha should lead the recents list because opens beat mtime.
    recents.record_open(std::path::Path::new("alpha.md"));

    let mut app = App::for_test_with_recents(vault, recents);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('o'))?;

    let frame = render(&mut app, 80, 24);
    // The "recent" title flips on for empty input + populated items.
    assert!(
        frame.contains("recent"),
        "expected `recent` in title for empty-input picker:\n{frame}"
    );
    // All three files appear; alpha is on the first row (after the
    // input-box rows).
    assert!(frame.contains("alpha.md"));
    assert!(frame.contains("beta.md"));
    assert!(frame.contains("gamma.md"));
    let alpha_pos = frame.find("alpha.md").unwrap();
    let beta_pos = frame.find("beta.md").unwrap();
    let gamma_pos = frame.find("gamma.md").unwrap();
    assert!(
        alpha_pos < beta_pos && alpha_pos < gamma_pos,
        "alpha (opened) must appear above beta and gamma (mtime only)"
    );
    Ok(())
}

#[test]
fn notes_open_picker_empty_log_falls_back_to_mtime() -> Result<()> {
    let (dir, vault) = recents_vault(&[
        ("oldest.md", "# O\n", 10),
        ("middle.md", "# M\n", 100),
        ("newest.md", "# N\n", 1000),
    ]);
    let recents = make_test_recents(&vault, &dir);
    let mut app = App::for_test_with_recents(vault, recents);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('o'))?;

    let frame = render(&mut app, 80, 24);
    let newest_pos = frame.find("newest.md").unwrap();
    let middle_pos = frame.find("middle.md").unwrap();
    let oldest_pos = frame.find("oldest.md").unwrap();
    assert!(
        newest_pos < middle_pos && middle_pos < oldest_pos,
        "expected mtime order newest→middle→oldest; got positions {newest_pos}, {middle_pos}, {oldest_pos}\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_open_picker_cold_start_shows_type_to_search_hint() -> Result<()> {
    // Vault has zero `.md` files — recents list is empty, picker should
    // fall back to the legacy "type to search…" hint.
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    let recents = make_test_recents(&vault, &dir);
    let mut app = App::for_test_with_recents(vault, recents);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('o'))?;

    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("type to search"),
        "cold-start picker should show legacy hint:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_open_picker_typing_transitions_from_recents_to_results() -> Result<()> {
    let (dir, vault) = recents_vault(&[("alpha.md", "# A\n", 100), ("beta.md", "# B\n", 200)]);
    let recents = make_test_recents(&vault, &dir);
    let mut app = App::for_test_with_recents(vault, recents);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('o'))?;

    // Empty input → "recent · type to search" title.
    let frame = render(&mut app, 80, 24);
    assert!(frame.contains("recent"));

    // Typing one char flips to fuzzy mode → title is " results ".
    app.dispatch(key('a'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("results"),
        "typing should switch to results mode:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_open_picker_backspace_returns_to_recents() -> Result<()> {
    let (dir, vault) = recents_vault(&[("alpha.md", "# A\n", 100), ("beta.md", "# B\n", 200)]);
    let recents = make_test_recents(&vault, &dir);
    let mut app = App::for_test_with_recents(vault, recents);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('o'))?;
    // Type then immediately erase — should land back in recents mode.
    app.dispatch(key('a'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Backspace,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("recent"),
        "backspace to empty input should restore recents mode:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_open_picker_enter_on_recent_records_and_reopens_at_top() -> Result<()> {
    // End-to-end: open picker, select gamma (mtime-newest), the open is
    // recorded, then re-open picker and assert gamma still leads — but
    // now because it was *opened* (its log entry beats any mtime tail).
    let (dir, vault) = recents_vault(&[
        ("alpha.md", "# A\n", 100),
        ("beta.md", "# B\n", 200),
        ("gamma.md", "# G\n", 300),
    ]);
    let recents = make_test_recents(&vault, &dir);
    // Pre-seed with alpha so it's "second" in the merged list — gamma's
    // mtime puts it first. After the user opens gamma, we should still
    // see gamma at top (now via the opens slice).
    recents.record_open(std::path::Path::new("alpha.md"));
    let recents_clone = Arc::clone(&recents);

    let mut app = App::for_test_with_recents(vault, recents_clone);
    app.switch_to(NOTES_TAB_INDEX)?;

    // First open: pick gamma (rendered at top thanks to mtime).
    app.dispatch(key('o'))?;
    // Navigate: with opens-first, alpha is row 0, then mtime-ordered
    // gamma (row 1) → beta (row 2). Press Down to land on gamma.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let req = app
        .take_pending_request()
        .expect("Enter should queue OpenInEditor");
    match req {
        AppRequest::OpenInEditor { path, .. } => {
            assert!(
                path.to_string_lossy().ends_with("gamma.md"),
                "expected gamma.md got {path:?}"
            );
        }
        other => panic!("expected OpenInEditor, got {other:?}"),
    }

    // After the open, both alpha and gamma should be in the recents
    // log, with gamma most recent.
    let logged = recents.load_recent(10);
    assert_eq!(
        logged,
        vec![
            std::path::PathBuf::from("gamma.md"),
            std::path::PathBuf::from("alpha.md")
        ],
        "recents log should reflect both opens with gamma newest"
    );

    // Re-open picker. gamma is now top of the opens slice.
    app.dispatch(key('o'))?;
    let frame = render(&mut app, 80, 24);
    let gamma_pos = frame.find("gamma.md").unwrap();
    let alpha_pos = frame.find("alpha.md").unwrap();
    let beta_pos = frame.find("beta.md").unwrap();
    assert!(
        gamma_pos < alpha_pos && alpha_pos < beta_pos,
        "after open, expected gamma → alpha → beta order; got positions {gamma_pos}, {alpha_pos}, {beta_pos}\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_open_picker_recents_snapshot_80x24() -> Result<()> {
    let (dir, vault) = recents_vault(&[
        ("project.md", "# Project\n", 100),
        ("inbox.md", "# Inbox\n", 200),
        ("notes/daily.md", "# Daily\n", 300),
    ]);
    let recents = make_test_recents(&vault, &dir);
    // Mixed signals: project is opened (top); inbox + daily fill via
    // mtime tail (daily newer than inbox).
    recents.record_open(std::path::Path::new("project.md"));

    let mut app = App::for_test_with_clock_and_recents(vault, fixed_clock, recents);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('o'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_open_picker_recents_80x24", frame);
    Ok(())
}

#[test]
fn cli_record_open_through_recents_log() -> Result<()> {
    // Verify the CLI path: `RecentsLog::for_vault(&vault).record_open(...)`
    // writes to the per-vault log. Uses an isolated XDG_STATE_HOME so we
    // don't touch the user's real state dir.
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::write(vault_path.join("note.md"), "# N\n").unwrap();
    let vault = Vault::discover(Some(vault_path.clone())).unwrap();

    let state_root = dir.path().join("state");
    let prev = std::env::var_os("XDG_STATE_HOME");
    std::env::set_var("XDG_STATE_HOME", &state_root);
    let log = RecentsLog::for_vault(&vault);
    log.record_open(std::path::Path::new("note.md"));
    // Read it back via the same construction to confirm round-trip.
    let log2 = RecentsLog::for_vault(&vault);
    let entries = log2.load_recent(10);
    match prev {
        Some(v) => std::env::set_var("XDG_STATE_HOME", v),
        None => std::env::remove_var("XDG_STATE_HOME"),
    }
    assert_eq!(entries, vec![std::path::PathBuf::from("note.md")]);
    Ok(())
}

// ── Notes tab · create flow (plan 009 · session 4) ───────────────────────

#[test]
fn notes_tab_c_opens_folder_picker() -> Result<()> {
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('c'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("2/2 folder · blank") || frame.contains("folder · blank"),
        "c should open the folder picker on the blank path:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_tab_capital_c_opens_template_picker() -> Result<()> {
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("1/4 template"),
        "Shift+C should open template picker:\n{frame}"
    );
    assert!(
        frame.contains("new.md") && frame.contains("quick-add.md"),
        "template picker should list templates:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_create_template_picker_snapshot() -> Result<()> {
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_create_template_picker_80x24", frame);
    Ok(())
}

#[test]
fn notes_create_folder_picker_snapshot() -> Result<()> {
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('c'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_create_folder_picker_80x24", frame);
    Ok(())
}

#[test]
fn notes_create_filename_prompt_snapshot() -> Result<()> {
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('c'))?;
    // Pick the first folder (vault root, ".").
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Type a partial filename.
    for c in "scrat".chars() {
        app.dispatch(key(c))?;
    }
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_create_filename_prompt_80x24", frame);
    Ok(())
}

#[test]
fn notes_create_var_prompt_snapshot() -> Result<()> {
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    // Filter to quick-add (the only template with a custom var).
    for c in "quick".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Pick first folder (root).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Type a filename.
    for c in "lunch".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Now in var prompt for `name`. Type a partial value.
    for c in "Sandw".chars() {
        app.dispatch(key(c))?;
    }
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_create_var_prompt_80x24", frame);
    Ok(())
}

#[test]
fn notes_create_collision_prompt_snapshot() -> Result<()> {
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('c'))?;
    // Filter folders to "inbox".
    for c in "inbox".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Filename that collides with inbox/existing.md.
    for c in "existing".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_create_collision_prompt_80x24", frame);
    Ok(())
}

#[test]
fn notes_create_blank_end_to_end_writes_file_and_queues_editor() -> Result<()> {
    let (dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('c'))?;
    // Vault root folder.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    for c in "scratch".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let req = app
        .take_pending_request()
        .expect("commit should queue OpenInEditor");
    let abs = dir.path().join("test-vault/scratch.md");
    match req {
        AppRequest::OpenInEditor { path, line } => {
            assert_eq!(path.canonicalize().unwrap(), abs.canonicalize().unwrap());
            assert_eq!(line, 1);
        }
        other => panic!("expected OpenInEditor, got {other:?}"),
    }
    let content = std::fs::read_to_string(&abs)?;
    assert_eq!(content, "# scratch\n");
    Ok(())
}

#[test]
fn notes_create_template_with_var_writes_rendered_content() -> Result<()> {
    let (dir, vault) = notes_create_vault();
    // FT_TODAY = 2026-05-10 via fixed_clock; the template renders today.
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    for c in "quick".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Vault root folder.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    for c in "lunch".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Var prompt for `name`.
    for c in "Lunch sandwich".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // File should now exist with rendered content.
    let content = std::fs::read_to_string(dir.path().join("test-vault/lunch.md"))?;
    assert!(content.contains("# Lunch sandwich"), "{content}");
    assert!(content.contains("Created-"), "{content}");
    Ok(())
}

#[test]
fn notes_create_collision_overwrite_writes_new_content() -> Result<()> {
    let (dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('c'))?;
    for c in "inbox".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    for c in "existing".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Now in CollisionPrompt — press `o` to overwrite.
    app.dispatch(key('o'))?;
    let _ = app.take_pending_request(); // OpenInEditor on the new file
    let content = std::fs::read_to_string(dir.path().join("test-vault/inbox/existing.md"))?;
    assert_eq!(content, "# existing\n");
    Ok(())
}

#[test]
fn notes_create_collision_use_existing_does_not_overwrite() -> Result<()> {
    let (dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('c'))?;
    for c in "inbox".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    for c in "existing".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // `u` → use existing → OpenInEditor on the unchanged file.
    app.dispatch(key('u'))?;
    let req = app
        .take_pending_request()
        .expect("u should queue OpenInEditor");
    let abs = dir.path().join("test-vault/inbox/existing.md");
    match req {
        AppRequest::OpenInEditor { path, line: _ } => {
            assert_eq!(path.canonicalize().unwrap(), abs.canonicalize().unwrap());
        }
        other => panic!("expected OpenInEditor, got {other:?}"),
    }
    // Content unchanged.
    let content = std::fs::read_to_string(&abs)?;
    assert_eq!(content, "# Existing\n");
    Ok(())
}

#[test]
fn notes_create_collision_cancel_returns_to_filename() -> Result<()> {
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('c'))?;
    for c in "inbox".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    for c in "existing".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // `c` (collision-prompt cancel) → back to filename prompt.
    app.dispatch(key('c'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("filename:") && frame.contains("existing"),
        "expected filename prompt with `existing` preserved:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_create_filename_empty_errors_in_place() -> Result<()> {
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('c'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Hit Enter with empty buffer.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("filename is required"),
        "expected error in footer:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_create_filename_with_slash_errors() -> Result<()> {
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('c'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    for c in "a/b".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("can't contain path separators"),
        "expected separator-rejection error:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_create_esc_from_template_picker_returns_to_idle() -> Result<()> {
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("template") || frame.contains("notes"),
        "should be back at notes-idle:\n{frame}"
    );
    // We're back at idle if the help-line list shows up at the top.
    assert!(
        frame.contains("create note (blank)"),
        "expected idle keymap panel:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_create_esc_from_folder_picker_blank_returns_to_idle() -> Result<()> {
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('c'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("create note (blank)"),
        "expected idle keymap panel:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_create_esc_from_folder_picker_template_path_returns_to_template_picker() -> Result<()> {
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    for c in "new".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Esc out of folder picker.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("1/4 template"),
        "expected template picker again:\n{frame}"
    );
    Ok(())
}

// ── Notes tab · section-move new-target sub-flow (plan 009 · session 5) ───

/// Vault tailored for the new-target sub-flow: a source note with a
/// movable H2 section, a templates dir with a couple of templates,
/// plus a pre-existing file that we can collide against.
fn notes_new_target_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::create_dir_all(vault_path.join("proj")).unwrap();
    std::fs::write(
        vault_path.join("daily.md"),
        "# 2026-05-13\n\n## Meeting Notes\n\nDiscussed roadmap.\n\n## Personal\n\nMisc.\n",
    )
    .unwrap();
    std::fs::write(vault_path.join("proj/existing.md"), "# Existing\nbody\n").unwrap();

    let templates_dir = vault_path.join("templates-ft");
    std::fs::create_dir_all(&templates_dir).unwrap();
    std::fs::write(
        templates_dir.join("proj.md"),
        "---\ntags: [Created-{{ today | date(format=\"%Y-%m-%d\") }}, Project]\n---\n# {{ title }}\n## Status\nProposed\n",
    )
    .unwrap();
    std::fs::write(
        templates_dir.join("new.md"),
        "---\ntags: [Created-{{ today | date(format=\"%Y-%m-%d\") }}]\n---\n# {{ title }}\n",
    )
    .unwrap();

    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

fn enter_target_picker(app: &mut crate::tui::App) -> Result<()> {
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('m'))?;
    // Pick `daily.md` via prefix filter.
    for c in "daily".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Multi-select first heading (line 1, # 2026-05-13).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Now in TargetPicking.
    Ok(())
}

#[test]
fn notes_move_target_ctrl_n_enters_new_target_flow() -> Result<()> {
    let (_dir, vault) = notes_new_target_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    enter_target_picker(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("new target · template"),
        "Ctrl+N should open new-target template picker:\n{frame}"
    );
    assert!(
        frame.contains("(no template / blank)"),
        "synthetic blank row should appear:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_move_new_target_template_picker_snapshot() -> Result<()> {
    let (_dir, vault) = notes_new_target_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    enter_target_picker(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_move_new_target_template_picker_80x24", frame);
    Ok(())
}

#[test]
fn notes_move_new_target_filename_prompt_snapshot() -> Result<()> {
    let (_dir, vault) = notes_new_target_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    enter_target_picker(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )))?;
    // Filter to `proj` template + select.
    for c in "proj".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Select the `proj/` folder.
    for c in "proj".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Now in filename prompt — type a partial name.
    for c in "Q3-".chars() {
        app.dispatch(key(c))?;
    }
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_move_new_target_filename_prompt_80x24", frame);
    Ok(())
}

#[test]
fn notes_move_new_target_collision_prompt_snapshot() -> Result<()> {
    let (_dir, vault) = notes_new_target_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    enter_target_picker(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )))?;
    // Pick blank.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Pick proj/ folder.
    for c in "proj".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Filename that collides with proj/existing.md.
    for c in "existing".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_move_new_target_collision_prompt_80x24", frame);
    Ok(())
}

#[test]
fn notes_move_new_target_compose_snapshot() -> Result<()> {
    let (_dir, vault) = notes_new_target_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    enter_target_picker(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )))?;
    // proj template.
    for c in "proj".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // proj/ folder.
    for c in "proj".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Filename.
    for c in "Foo".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Now in Composing with target_is_new = true. Render.
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_move_new_target_compose_80x24", frame);
    Ok(())
}

#[test]
fn notes_move_new_target_end_to_end_writes_both_files() -> Result<()> {
    let (dir, vault) = notes_new_target_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    enter_target_picker(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )))?;
    // Blank template (skip render).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // proj/ folder.
    for c in "proj".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Filename.
    for c in "Brand New".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Commit compose.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // Source file should have the H1 stripped; the new target file
    // should exist with the H1 prepended to its stub.
    let source = std::fs::read_to_string(dir.path().join("test-vault/daily.md"))?;
    let target = std::fs::read_to_string(dir.path().join("test-vault/proj/Brand New.md"))?;
    assert!(
        !source.contains("# 2026-05-13"),
        "H1 should have moved out of source: {source}"
    );
    assert!(
        target.contains("# 2026-05-13"),
        "H1 should appear in new target: {target}"
    );
    // The blank stub starts with `# Brand New` for the new note's title.
    assert!(
        target.contains("# Brand New") || target.starts_with("# 2026-05-13"),
        "target should include either stub heading or moved H1: {target}"
    );
    Ok(())
}

#[test]
fn notes_move_new_target_cancel_leaves_filesystem_untouched() -> Result<()> {
    let (dir, vault) = notes_new_target_vault();
    let vault_path = dir.path().join("test-vault");
    let listing_before: Vec<_> = std::fs::read_dir(vault_path.join("proj"))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();

    let mut app = App::for_test_with_clock(vault, fixed_clock);
    enter_target_picker(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )))?;
    // Pick template.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Pick folder.
    for c in "proj".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Type filename and commit to enter compose.
    for c in "WillCancel".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Esc out of compose → TargetPicking → Esc → multiselect → Esc → source
    // picker. No write should ever happen.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;

    let listing_after: Vec<_> = std::fs::read_dir(vault_path.join("proj"))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();
    assert_eq!(
        listing_before, listing_after,
        "no file should have been written before commit"
    );
    Ok(())
}

#[test]
fn notes_move_new_target_collision_overwrite_writes_new_content() -> Result<()> {
    let (dir, vault) = notes_new_target_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    enter_target_picker(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )))?;
    // Blank template.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // proj/ folder.
    for c in "proj".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Filename that collides with existing.md.
    for c in "existing".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // CollisionPrompt visible. Press `o` → Overwrite → compose.
    app.dispatch(key('o'))?;
    // Now in Composing. Commit.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let content = std::fs::read_to_string(dir.path().join("test-vault/proj/existing.md"))?;
    // Old "# Existing" should be gone (overwritten by the stub + moved
    // section).
    assert!(
        !content.contains("# Existing"),
        "old content should have been overwritten: {content}"
    );
    assert!(
        content.contains("# 2026-05-13"),
        "moved heading should be in the new target: {content}"
    );
    Ok(())
}

#[test]
fn notes_move_new_target_collision_use_existing_preserves_file() -> Result<()> {
    let (dir, vault) = notes_new_target_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    enter_target_picker(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )))?;
    // Blank.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // proj/.
    for c in "proj".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Filename that collides.
    for c in "existing".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // `u` → use existing (no template render; existing content kept).
    app.dispatch(key('u'))?;
    // Commit.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let content = std::fs::read_to_string(dir.path().join("test-vault/proj/existing.md"))?;
    // Pre-existing `# Existing` should still be there; moved heading
    // should be added.
    assert!(content.contains("# Existing"), "{content}");
    assert!(content.contains("# 2026-05-13"), "{content}");
    Ok(())
}

#[test]
fn notes_move_new_target_collision_cancel_returns_to_filename() -> Result<()> {
    let (_dir, vault) = notes_new_target_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    enter_target_picker(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    for c in "proj".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    for c in "existing".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // `c` cancels → back to filename prompt with buffer preserved.
    app.dispatch(key('c'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("filename:") && frame.contains("existing"),
        "expected filename prompt with buffer pre-filled:\n{frame}"
    );
    Ok(())
}

// ── Notes tab · periodic notes flow (plan 010 · session 3) ────────────────

/// Vault with no `[periodic_notes.*]` configured — used to test the
/// "<period> not configured" error toast path.
fn periodic_vault_no_config() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn notes_tab_t_opens_today_when_daily_configured() -> Result<()> {
    let (dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    // `t` is the one-shot daily synonym for `p` then `d`.
    app.dispatch(key('t'))?;

    let req = app
        .take_pending_request()
        .expect("`t` should queue OpenInEditor for today's daily");
    match req {
        AppRequest::OpenInEditor { path, line } => {
            assert_eq!(line, 1, "should jump to line 1 of the daily note");
            let expected = dir
                .path()
                .join("test-vault/journal/2026/2026-05-10.md")
                .canonicalize()
                .unwrap();
            assert_eq!(path.canonicalize().unwrap(), expected);
            // File body must be the blank stub the periodic helper writes.
            let body = std::fs::read_to_string(&path).unwrap();
            assert_eq!(body, "# 2026-05-10\n\n");
        }
        other => panic!("expected OpenInEditor, got {other:?}"),
    }
    Ok(())
}

#[test]
fn notes_tab_t_emits_error_toast_when_daily_unconfigured() -> Result<()> {
    let (_dir, vault) = periodic_vault_no_config();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('t'))?;

    let toast = app
        .current_toast()
        .expect("`t` should surface a Toast when daily isn't configured");
    assert!(
        toast.text.contains("daily not configured"),
        "toast should call out the missing period: {:?}",
        toast.text
    );
    assert_eq!(toast.style, crate::tui::tab::ToastStyle::Error);
    Ok(())
}

#[test]
fn notes_tab_p_enters_leader_modal() -> Result<()> {
    let (_dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('p'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("periodic note · pick a period"),
        "expected leader modal title:\n{frame}"
    );
    assert!(
        frame.contains("d") && frame.contains("daily"),
        "expected daily row in leader modal:\n{frame}"
    );
    // No editor open should have been queued — leader is just a state change.
    assert!(
        app.take_pending_request().is_none(),
        "`p` alone must not queue any request"
    );
    Ok(())
}

#[test]
fn notes_tab_p_esc_returns_to_idle_silently() -> Result<()> {
    let (_dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('p'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("periodic note · pick a period"),
        "Esc should dismiss the leader modal:\n{frame}"
    );
    assert!(
        frame.contains("Notes — Obsidian-flavoured editing"),
        "should be back at idle:\n{frame}"
    );
    assert!(
        app.take_pending_request().is_none(),
        "Esc from leader must not queue anything"
    );
    Ok(())
}

#[test]
fn notes_tab_p_then_d_opens_daily() -> Result<()> {
    let (dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('p'))?;
    app.dispatch(key('d'))?;

    let req = app
        .take_pending_request()
        .expect("p,d should queue OpenInEditor");
    match req {
        AppRequest::OpenInEditor { path, .. } => {
            let expected = dir
                .path()
                .join("test-vault/journal/2026/2026-05-10.md")
                .canonicalize()
                .unwrap();
            assert_eq!(path.canonicalize().unwrap(), expected);
        }
        other => panic!("expected OpenInEditor, got {other:?}"),
    }
    Ok(())
}

#[test]
fn notes_tab_p_then_w_opens_weekly() -> Result<()> {
    let (dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('p'))?;
    app.dispatch(key('w'))?;

    let req = app
        .take_pending_request()
        .expect("p,w should queue OpenInEditor");
    match req {
        AppRequest::OpenInEditor { path, .. } => {
            // 2026-05-10 (fixed_clock) is Sunday of ISO week 19, 2026.
            let expected = dir
                .path()
                .join("test-vault/journal/2026/2026-W19.md")
                .canonicalize()
                .unwrap();
            assert_eq!(path.canonicalize().unwrap(), expected);
        }
        other => panic!("expected OpenInEditor, got {other:?}"),
    }
    Ok(())
}

#[test]
fn notes_tab_p_then_m_opens_monthly() -> Result<()> {
    let (dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('p'))?;
    app.dispatch(key('m'))?;
    let req = app
        .take_pending_request()
        .expect("p,m should queue OpenInEditor");
    match req {
        AppRequest::OpenInEditor { path, .. } => {
            let expected = dir
                .path()
                .join("test-vault/journal/2026/2026-05.md")
                .canonicalize()
                .unwrap();
            assert_eq!(path.canonicalize().unwrap(), expected);
        }
        other => panic!("expected OpenInEditor, got {other:?}"),
    }
    Ok(())
}

#[test]
fn notes_tab_p_then_q_opens_quarterly() -> Result<()> {
    let (dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('p'))?;
    app.dispatch(key('q'))?;
    let req = app
        .take_pending_request()
        .expect("p,q should queue OpenInEditor");
    match req {
        AppRequest::OpenInEditor { path, .. } => {
            let expected = dir
                .path()
                .join("test-vault/journal/2026/2026-Q2.md")
                .canonicalize()
                .unwrap();
            assert_eq!(path.canonicalize().unwrap(), expected);
        }
        other => panic!("expected OpenInEditor, got {other:?}"),
    }
    Ok(())
}

#[test]
fn notes_tab_p_then_y_opens_yearly() -> Result<()> {
    let (dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('p'))?;
    app.dispatch(key('y'))?;
    let req = app
        .take_pending_request()
        .expect("p,y should queue OpenInEditor");
    match req {
        AppRequest::OpenInEditor { path, .. } => {
            let expected = dir
                .path()
                .join("test-vault/journal/2026.md")
                .canonicalize()
                .unwrap();
            assert_eq!(path.canonicalize().unwrap(), expected);
        }
        other => panic!("expected OpenInEditor, got {other:?}"),
    }
    Ok(())
}

#[test]
fn notes_tab_p_then_unknown_letter_cancels_silently() -> Result<()> {
    let (_dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('p'))?;
    // `x` isn't a valid period letter — should drop us back to idle
    // without queuing anything (neither editor nor toast).
    app.dispatch(key('x'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("periodic note · pick a period"),
        "leader modal should be dismissed after unknown letter:\n{frame}"
    );
    assert!(
        frame.contains("Notes — Obsidian-flavoured editing"),
        "should be back at idle:\n{frame}"
    );
    assert!(
        app.take_pending_request().is_none(),
        "unknown-letter cancel must not queue anything"
    );
    Ok(())
}

#[test]
fn notes_tab_periodic_second_call_does_not_overwrite() -> Result<()> {
    let (dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;

    // First `t`: file gets created with the blank stub.
    app.dispatch(key('t'))?;
    let _ = app.take_pending_request();
    let path = dir.path().join("test-vault/journal/2026/2026-05-10.md");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "# 2026-05-10\n\n");

    // Hand-edit the file to simulate the user typing into the daily.
    std::fs::write(&path, "# manually edited\n").unwrap();

    // Second `t`: the helper sees the file exists and must NOT rewrite it;
    // it only queues OpenInEditor for the existing file.
    app.dispatch(key('t'))?;
    let req = app.take_pending_request().expect("second `t` should queue");
    match req {
        AppRequest::OpenInEditor { path: opened, .. } => {
            assert_eq!(opened.canonicalize().unwrap(), path.canonicalize().unwrap());
        }
        other => panic!("expected OpenInEditor, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# manually edited\n",
        "second invocation must not rewrite an existing file"
    );
    Ok(())
}

#[test]
fn notes_tab_p_then_w_emits_toast_when_weekly_unconfigured() -> Result<()> {
    // Vault has daily configured but not weekly — `p,w` should hit the
    // missing-config error path.
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::create_dir_all(vault_path.join(".ft")).unwrap();
    std::fs::write(
        vault_path.join(".ft/config.toml"),
        "[periodic_notes.daily]\npath = \"journal\"\nformat = \"%Y-%m-%d\"\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();

    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('p'))?;
    app.dispatch(key('w'))?;

    let toast = app
        .current_toast()
        .expect("p,w with no weekly config should surface an error toast");
    assert!(
        toast.text.contains("weekly not configured"),
        "toast should name the unconfigured period: {:?}",
        toast.text
    );
    assert_eq!(toast.style, crate::tui::tab::ToastStyle::Error);
    Ok(())
}

#[test]
fn notes_periodic_leader_modal_snapshot() -> Result<()> {
    let (_dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('p'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("notes_periodic_leader_80x24", frame);
    Ok(())
}

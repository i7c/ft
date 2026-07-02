//! Timeblocks tab: rendering, add/edit/delete chords, templates.

use super::*;

// ── Timeblocks tab (plan 015 session 4) ─────────────────────────────────────

/// Vault wired up so `Vault::resolve_target(date, None)` finds a daily
/// note at `journal/YYYY-MM-DD.md`. Anchored to the fixed clock
/// (Sun 2026-05-10), so "today" = 2026-05-10 and "tomorrow" = 2026-05-11.
fn timeblocks_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::create_dir_all(vault_path.join(".ft")).unwrap();
    std::fs::write(
        vault_path.join(".ft/config.toml"),
        "[periodic_notes.daily]\npath = \"journal\"\nformat = \"%Y-%m-%d\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(vault_path.join("journal")).unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

fn seed_day(vault: &Vault, date: &str, body: &str) {
    let p = vault.path.join(format!("journal/{date}.md"));
    std::fs::write(p, body).unwrap();
}

#[test]
fn timeblocks_tab_empty_renders_placeholders() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    let frame = render(&mut app, 100, 24);
    assert_tui_snapshot!("timeblocks_tab_empty_100x24", frame);
    Ok(())
}

#[test]
fn timeblocks_tab_populated_today_renders_blocks_and_totals() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 standup @work\n- 10:00 - 10:30 review @work/code\n- 11:00 - 11:15 coffee @break\n",
    );
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    let frame = render(&mut app, 100, 24);
    assert_tui_snapshot!("timeblocks_tab_populated_today_100x24", frame);
    Ok(())
}

#[test]
fn timeblocks_tab_populated_today_missing_tomorrow_shows_placeholder() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 standup @work\n",
    );
    // Don't seed 2026-05-11 — tomorrow's pane should show the placeholder.
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    // Default is now Single-day; toggle to Split to see both panes.
    app.dispatch(key('f'))?;
    let frame = render(&mut app, 100, 24);
    assert!(
        frame.contains("no daily note yet"),
        "tomorrow placeholder should appear: {frame}"
    );
    Ok(())
}

#[test]
fn timeblocks_tab_both_days_populated_renders_two_lists() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 today-a @work\n",
    );
    seed_day(
        &vault,
        "2026-05-11",
        "## Time Blocks\n- 09:00 - 10:00 tomorrow-a @personal\n",
    );
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    // Default is now Single-day; toggle to Split to see both panes.
    app.dispatch(key('f'))?;
    let frame = render(&mut app, 100, 24);
    assert!(frame.contains("today-a"));
    assert!(frame.contains("tomorrow-a"));
    Ok(())
}

#[test]
fn timeblocks_tab_l_shifts_focus_to_tomorrow() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 today-a @work\n",
    );
    seed_day(
        &vault,
        "2026-05-11",
        "## Time Blocks\n- 09:00 - 10:00 tomorrow-a @personal\n",
    );
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    // Initial frame: focus indicator points at today's date.
    let initial = render(&mut app, 100, 24);
    assert!(initial.contains("▶ 2026-05-10"), "got: {initial}");
    // After `l`: focus should jump to the next pane (tomorrow's date).
    app.dispatch(key('l'))?;
    let after = render(&mut app, 100, 24);
    assert!(after.contains("▶ 2026-05-11"), "got: {after}");
    Ok(())
}

#[test]
fn timeblocks_tab_j_k_move_selection_in_focused_pane() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 first\n- 10:00 - 11:00 second\n- 11:00 - 12:00 third\n",
    );
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    let after_two_j = render(&mut app, 100, 24);
    // The selection symbol "▶" appears next to the focused row; with two
    // `j`s on a focused list of length 3, "third" should be selected.
    // We assert the third row has the highlight prefix in the rendered
    // frame by looking for the row text immediately after a "▶" symbol.
    assert!(
        after_two_j
            .lines()
            .any(|l| l.contains("▶") && l.contains("third")),
        "expected third row highlighted: {after_two_j}"
    );
    Ok(())
}

#[test]
fn timeblocks_tab_g_and_capital_g_jump_to_ends() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 first\n- 10:00 - 11:00 second\n- 11:00 - 12:00 third\n",
    );
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('G'))?;
    let last = render(&mut app, 100, 24);
    assert!(last.lines().any(|l| l.contains("▶") && l.contains("third")));
    app.dispatch(key('g'))?;
    let first = render(&mut app, 100, 24);
    assert!(first
        .lines()
        .any(|l| l.contains("▶") && l.contains("first")));
    Ok(())
}

#[test]
fn timeblocks_tab_r_refreshes_from_disk() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    // Start empty.
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    let initial = render(&mut app, 100, 24);
    assert!(initial.contains("no daily note yet"));
    // Drop a file on disk and press `r`.
    std::fs::write(
        vault_path.join("journal/2026-05-10.md"),
        "## Time Blocks\n- 09:00 - 10:00 fresh @work\n",
    )
    .unwrap();
    app.dispatch(key('r'))?;
    let after = render(&mut app, 100, 24);
    assert!(after.contains("fresh"));
    Ok(())
}

// ── session 5: mutation chords ──────────────────────────────────────────────

/// Vault wired up with a `[periodic_notes.daily]` block AND a template,
/// so the `c` chord and `ft timeblocks add` create the daily note from
/// the same template `ft notes today` uses.
fn timeblocks_vault_with_template() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::create_dir_all(vault_path.join(".ft")).unwrap();
    std::fs::create_dir_all(vault_path.join("templates-ft")).unwrap();
    std::fs::write(
        vault_path.join("templates-ft/daily.md"),
        "# {{ title }}\n\n## Notes\n\n## Time Blocks\n",
    )
    .unwrap();
    std::fs::write(
        vault_path.join(".ft/config.toml"),
        "[periodic_notes.daily]\npath = \"journal\"\nformat = \"%Y-%m-%d\"\ntemplate = \"daily\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(vault_path.join("journal")).unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn timeblocks_tab_close_bracket_extends_end_by_5m() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(&vault, "2026-05-10", "## Time Blocks\n- 09:00 - 10:00 a\n");
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key(']'))?;
    app.service_pending_requests()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("- 09:00 - 10:05 a"), "got: {body}");
    Ok(())
}

#[test]
fn timeblocks_tab_open_bracket_shrinks_end_by_5m() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(&vault, "2026-05-10", "## Time Blocks\n- 09:00 - 10:00 a\n");
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('['))?;
    app.service_pending_requests()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("- 09:00 - 09:55 a"), "got: {body}");
    Ok(())
}

#[test]
fn timeblocks_tab_close_brace_shifts_start_by_5m() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(&vault, "2026-05-10", "## Time Blocks\n- 09:00 - 10:00 a\n");
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('}'))?;
    app.service_pending_requests()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("- 09:05 - 10:00 a"), "got: {body}");
    Ok(())
}

#[test]
fn timeblocks_tab_gt_shifts_block_5m_later_preserving_duration() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(&vault, "2026-05-10", "## Time Blocks\n- 09:00 - 10:00 a\n");
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('>'))?;
    app.service_pending_requests()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("- 09:05 - 10:05 a"), "got: {body}");
    Ok(())
}

#[test]
fn timeblocks_tab_lt_shifts_block_5m_earlier_preserving_duration() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(&vault, "2026-05-10", "## Time Blocks\n- 09:00 - 10:00 a\n");
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('<'))?;
    app.service_pending_requests()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("- 08:55 - 09:55 a"), "got: {body}");
    Ok(())
}

#[test]
fn timeblocks_tab_gt_keeps_cursor_on_shifted_block_after_resort() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 09:30 first\n- 09:10 - 09:40 second\n",
    );
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    // Select "first" (idx 0) and push it +15m → 09:15 - 09:45. Now
    // second (09:10) comes first; cursor should follow "first" to idx 1.
    for _ in 0..3 {
        app.dispatch(key('>'))?;
        app.service_pending_requests()?;
    }
    let frame = render(&mut app, 100, 24);
    assert!(
        frame
            .lines()
            .any(|l| l.contains("▶") && l.contains("first")),
        "cursor should follow `first` after re-sort: {frame}"
    );
    Ok(())
}

#[test]
fn timeblocks_tab_open_brace_pulls_start_5m_earlier() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(&vault, "2026-05-10", "## Time Blocks\n- 09:00 - 10:00 a\n");
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('{'))?;
    app.service_pending_requests()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("- 08:55 - 10:00 a"), "got: {body}");
    Ok(())
}

#[test]
fn timeblocks_tab_dd_deletes_focused_block() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 a\n- 10:00 - 11:00 b\n",
    );
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    // First `d` arms the chord; second `d` commits.
    app.dispatch(key('d'))?;
    app.service_pending_requests()?;
    let after_first = render(&mut app, 100, 24);
    assert!(
        after_first.contains("d again = delete"),
        "armed-state indicator missing"
    );
    app.dispatch(key('d'))?;
    app.service_pending_requests()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(!body.contains("- 09:00 - 10:00 a"), "deleted: {body}");
    assert!(body.contains("- 10:00 - 11:00 b"));
    Ok(())
}

#[test]
fn timeblocks_tab_dd_esc_cancels() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(&vault, "2026-05-10", "## Time Blocks\n- 09:00 - 10:00 a\n");
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('d'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("- 09:00 - 10:00 a"));
    Ok(())
}

#[test]
fn timeblocks_tab_quickline_a_adds_block() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(&vault, "2026-05-10", "## Time Blocks\n");
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('a'))?;
    for c in "09:00 - 10:00 standup".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.service_pending_requests()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("- 09:00 - 10:00 standup"), "got: {body}");
    Ok(())
}

#[test]
fn timeblocks_tab_quickline_parse_error_keeps_buffer() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(&vault, "2026-05-10", "## Time Blocks\n");
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('a'))?;
    for c in "garbage".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Frame should still show the quickline strip with the bad text.
    let frame = render(&mut app, 100, 24);
    assert!(frame.contains("garbage"), "buffer preserved: {frame}");
    Ok(())
}

#[test]
fn timeblocks_tab_edit_desc_e_chord() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 old\n",
    );
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('e'))?;
    // EditBuffer prefilled with "old"; clear and type "new".
    for _ in 0..3 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "new".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("- 09:00 - 10:00 new"), "got: {body}");
    assert!(!body.contains("old"));
    Ok(())
}

#[test]
fn timeblocks_tab_form_capital_a_commits_on_desc_enter() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(&vault, "2026-05-10", "## Time Blocks\n");
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('A'),
        KeyModifiers::SHIFT,
    )))?;
    // Start field is prefilled with the snapped clock; cycle to Desc.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)))?;
    for c in "from form".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.service_pending_requests()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("from form"), "got: {body}");
    Ok(())
}

#[test]
fn timeblocks_tab_time_chord_keeps_selection_on_non_first_block() -> Result<()> {
    // Regression: with two blocks, selecting the second and then hitting
    // any time-adjust chord must NOT jump the cursor back to the first
    // block. (Pre-fix, `reload` reset selection to 0 on every refresh.)
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 first\n- 10:00 - 11:00 second\n",
    );
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    // Move down to select the second block, then extend its end by 5m
    // three times — each `]` should leave the cursor on "second".
    app.dispatch(key('j'))?;
    for _ in 0..3 {
        app.dispatch(key(']'))?;
        app.service_pending_requests()?;
    }
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    // First block unchanged, second block's end shifted by 3*5 = 15m.
    assert!(body.contains("- 09:00 - 10:00 first"), "got: {body}");
    assert!(body.contains("- 10:00 - 11:15 second"), "got: {body}");
    // And the highlight (▶) should be next to "second", not "first".
    let frame = render(&mut app, 100, 24);
    assert!(
        frame
            .lines()
            .any(|l| l.contains("▶") && l.contains("second")),
        "selection should still be on second: {frame}"
    );
    Ok(())
}

#[test]
fn timeblocks_tab_equal_start_blocks_are_editable() -> Result<()> {
    // Regression: when two blocks share a start time, every TUI
    // mutation chord must still work — Selector::Time would match
    // both and fail with "ambiguous selector". Selector::Line on the
    // display-order line disambiguates.
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 first\n- 09:00 - 10:30 second\n",
    );
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;

    // Time-shift on the first block (line 1, 09:00 - 10:00).
    app.dispatch(key(']'))?;
    app.service_pending_requests()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("- 09:00 - 10:05 first"), "got: {body}");
    assert!(
        body.contains("- 09:00 - 10:30 second"),
        "second untouched: {body}"
    );

    // Time-shift on the second block (line 2).
    app.dispatch(key('j'))?;
    app.dispatch(key(']'))?;
    app.service_pending_requests()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(
        body.contains("- 09:00 - 10:05 first"),
        "first untouched: {body}"
    );
    assert!(body.contains("- 09:00 - 10:35 second"), "got: {body}");

    // Edit-desc on the second block (still focused).
    app.dispatch(key('e'))?;
    for _ in 0..6 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "renamed".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("- 09:00 - 10:35 renamed"), "got: {body}");
    assert!(
        body.contains("- 09:00 - 10:05 first"),
        "first untouched: {body}"
    );

    // Delete the first block (move cursor up and run `d d`).
    app.dispatch(key('k'))?;
    app.dispatch(key('d'))?;
    app.service_pending_requests()?;
    app.dispatch(key('d'))?;
    app.service_pending_requests()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(!body.contains("first"), "first deleted: {body}");
    assert!(body.contains("- 09:00 - 10:35 renamed"));
    Ok(())
}

#[test]
fn timeblocks_tab_capital_l_slides_anchor_to_next_day() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 today-block\n",
    );
    seed_day(
        &vault,
        "2026-05-12",
        "## Time Blocks\n- 14:00 - 15:00 day-plus-2\n",
    );
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    // Slide forward twice — left pane should now be 2026-05-12.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('L'),
        KeyModifiers::SHIFT,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('L'),
        KeyModifiers::SHIFT,
    )))?;
    let frame = render(&mut app, 100, 24);
    assert!(frame.contains("2026-05-12"), "got: {frame}");
    assert!(frame.contains("day-plus-2"), "got: {frame}");
    assert!(!frame.contains("today-block"), "got: {frame}");
    Ok(())
}

#[test]
fn timeblocks_tab_capital_h_slides_anchor_to_previous_day() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-09",
        "## Time Blocks\n- 09:00 - 10:00 yblk\n",
    );
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('H'),
        KeyModifiers::SHIFT,
    )))?;
    let frame = render(&mut app, 100, 24);
    assert!(frame.contains("2026-05-09"), "got: {frame}");
    assert!(frame.contains("yblk"), "got: {frame}");
    // Left pane's title should carry the (yesterday) badge since
    // 2026-05-09 is one day before the fixed clock's 2026-05-10.
    assert!(frame.contains("(yesterday)"), "got: {frame}");
    Ok(())
}

#[test]
fn timeblocks_tab_capital_t_jumps_anchor_back_to_today() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 home-block\n",
    );
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    // Wander 5 days into the future.
    for _ in 0..5 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char('L'),
            KeyModifiers::SHIFT,
        )))?;
    }
    let wandered = render(&mut app, 100, 24);
    assert!(!wandered.contains("home-block"));
    assert!(!wandered.contains("(today)"));
    // T resets the anchor to ctx.today (2026-05-10).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('T'),
        KeyModifiers::SHIFT,
    )))?;
    let frame = render(&mut app, 100, 24);
    assert!(frame.contains("home-block"), "got: {frame}");
    assert!(frame.contains("(today)"), "got: {frame}");
    Ok(())
}

#[test]
fn timeblocks_tab_f_toggles_to_single_day_view() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 today-a\n",
    );
    seed_day(
        &vault,
        "2026-05-11",
        "## Time Blocks\n- 09:00 - 10:00 tomorrow-a\n",
    );
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    // Default is now Single-day: only today visible.
    let single = render(&mut app, 100, 24);
    assert!(single.contains("today-a"));
    assert!(!single.contains("tomorrow-a"), "got: {single}");
    assert!(single.contains("view: single (f)"));
    // Toggle to Split: both visible.
    app.dispatch(key('f'))?;
    let split = render(&mut app, 100, 24);
    assert!(split.contains("today-a"));
    assert!(split.contains("tomorrow-a"));
    assert!(split.contains("view: split"));
    // Toggle back to Single.
    app.dispatch(key('f'))?;
    let single2 = render(&mut app, 100, 24);
    assert!(single2.contains("today-a"));
    assert!(!single2.contains("tomorrow-a"), "got: {single2}");
    assert!(single2.contains("view: single (f)"));
    // `l` flips focus → in single mode, that flips which day is shown.
    app.dispatch(key('l'))?;
    let after_l = render(&mut app, 100, 24);
    assert!(after_l.contains("tomorrow-a"));
    assert!(!after_l.contains("today-a"), "got: {after_l}");
    // `f` again returns to split.
    app.dispatch(key('f'))?;
    let split_again = render(&mut app, 100, 24);
    assert!(split_again.contains("today-a"));
    assert!(split_again.contains("tomorrow-a"));
    Ok(())
}

#[test]
fn timeblocks_tab_form_cursor_lands_after_visible_prefix() -> Result<()> {
    // Regression: the `A` modal placed the cursor two cells past the
    // actual end of the prefix because `▸` was 2 cells wide in some
    // fonts and the hardcoded offset was off by one. The prefix is now
    // ASCII (`> start `) and the offset is derived from `chars().count()`.
    let (_dir, vault) = timeblocks_vault();
    seed_day(&vault, "2026-05-10", "## Time Blocks\n");
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('A'),
        KeyModifiers::SHIFT,
    )))?;
    let frame = render(&mut app, 100, 24);
    assert!(frame.contains("> start"), "ASCII prefix expected: {frame}");
    assert!(!frame.contains("▸ start"));
    // Typing extends the buffer, character lands at the cursor position.
    app.dispatch(key('X'))?;
    let frame = render(&mut app, 100, 24);
    // Default Start ("14:30" snapped from fixed clock) gains a trailing
    // "X". Confirms the cursor sat at end-of-text, not mid-prefix.
    assert!(frame.contains("14:30X"), "got: {frame}");
    Ok(())
}

#[test]
fn timeblocks_tab_block_height_scales_with_duration() -> Result<()> {
    // 30-min block → 1 line. 90-min block → 2 lines. 150-min block → 3.
    // A `│` continuation marker should appear on extra lines so the
    // total row count for three blocks is 1 + 2 + 3 = 6 rendered rows.
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n\
         - 09:00 - 09:30 thirty\n\
         - 10:00 - 11:30 ninety\n\
         - 12:00 - 14:30 onefifty\n",
    );
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('f'))?; // fullscreen so the column is wide enough
    let frame = render(&mut app, 100, 40);
    // Each header still shows once.
    assert_eq!(frame.matches("thirty").count(), 1);
    assert_eq!(frame.matches("ninety").count(), 1);
    assert_eq!(frame.matches("onefifty").count(), 1);
    // 90-min block contributes 1 continuation row, 150-min contributes 2
    // — so there are at least 3 `│` markers under headers.
    let bar_rows = frame
        .lines()
        .filter(|l| {
            l.contains('│')
                && !l.contains("thirty")
                && !l.contains("ninety")
                && !l.contains("onefifty")
        })
        .count();
    assert!(
        bar_rows >= 3,
        "expected 3+ continuation bars, got {bar_rows}: {frame}"
    );
    Ok(())
}

#[test]
fn timeblocks_tab_t_modal_adds_and_removes_tags() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 standup @work\n",
    );
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('t'))?;
    for c in "+@meeting -@work".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("@meeting"), "added tag missing: {body}");
    assert!(!body.contains("@work"), "removed tag still present: {body}");
    Ok(())
}

#[test]
fn timeblocks_tab_t_modal_rejects_invalid_token() -> Result<()> {
    let (_dir, vault) = timeblocks_vault();
    seed_day(
        &vault,
        "2026-05-10",
        "## Time Blocks\n- 09:00 - 10:00 standup\n",
    );
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.dispatch(key('t'))?;
    // Tokens without `+` / `-` prefix should be rejected; file untouched.
    for c in "work".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(!body.contains("@work"), "should not have applied: {body}");
    Ok(())
}

#[test]
fn timeblocks_tab_c_creates_daily_via_template() -> Result<()> {
    let (_dir, vault) = timeblocks_vault_with_template();
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    // Initial frame: today's file doesn't exist yet.
    let initial = render(&mut app, 100, 24);
    assert!(initial.contains("no daily note yet"));
    app.dispatch(key('c'))?;
    app.service_pending_requests()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    // Template's title + Notes section must be present.
    assert!(body.contains("# 2026-05-10"), "title missing: {body}");
    assert!(body.contains("## Notes"));
    assert!(body.contains("## Time Blocks"));
    Ok(())
}

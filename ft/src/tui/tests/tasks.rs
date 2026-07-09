//! App-level navigation (tab switching, quit) and the Tasks tab:
//! search view, query bar, quick-key mutations, quickline, edit popup.

use super::*;

#[test]
fn initial_tab_is_graph() {
    // WelcomeTab was removed (plan 019 ext); GraphTab is now first.
    let (_dir, vault) = test_vault();
    let app = App::for_test(vault);
    assert_eq!(app.active_index(), 0);
    assert_eq!(app.active_title(), "Graph");
}

#[test]
fn digit_jumps_directly_to_target_tab() -> Result<()> {
    // `3` should land on Pulse in one keypress, even from Graph.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    assert_eq!(app.active_index(), 0);
    app.dispatch(key('3'))?;
    assert_eq!(app.active_index(), 2);
    assert_eq!(app.active_title(), "Pulse");
    Ok(())
}

#[test]
fn q_quits_from_initial_tab() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    assert_eq!(app.active_index(), 0);
    app.dispatch(key('q'))?;
    assert!(app.is_quit());
    Ok(())
}

#[test]
fn q_quits_from_tasks_tab() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.switch_to(5)?;
    app.dispatch(key('q'))?;
    assert!(app.is_quit());
    Ok(())
}

#[test]
fn ctrl_c_quits() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.switch_to(5)?;
    let ev = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    app.dispatch(ev)?;
    assert!(app.is_quit());
    Ok(())
}

#[test]
fn tab_key_cycles_tabs() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    // Start on Tasks so Tab isn't intercepted by Graph's input mode.
    app.switch_to(5)?;
    let tab_ev = Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    app.dispatch(tab_ev.clone())?;
    assert_eq!(app.active_title(), "Timeblocks");
    app.dispatch(tab_ev.clone())?;
    assert_eq!(app.active_title(), "Graph");
    app.dispatch(tab_ev.clone())?;
    assert_eq!(app.active_title(), "Notes");
    app.dispatch(tab_ev.clone())?;
    assert_eq!(app.active_title(), "Pulse");
    app.dispatch(tab_ev.clone())?;
    assert_eq!(app.active_title(), "Recent");
    app.dispatch(tab_ev.clone())?;
    assert_eq!(app.active_title(), "Gather");
    app.dispatch(tab_ev)?;
    assert_eq!(app.active_title(), "Tasks");
    Ok(())
}

#[test]
fn search_arrow_navigation_wraps() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    let down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let up = Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    // 5 matches in default window — going up from selection 0 should wrap.
    let initial = render(&mut app, 80, 24);
    assert!(initial.contains("▶"), "selected indicator missing");
    for _ in 0..6 {
        app.dispatch(down.clone())?;
    }
    for _ in 0..7 {
        app.dispatch(up.clone())?;
    }
    let frame = render(&mut app, 80, 24);
    assert!(frame.contains("▶"));
    Ok(())
}

#[test]
fn search_query_edit_apply_updates_list() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;

    // Replace query with one that only matches a single task.
    app.dispatch(key('/'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..200 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "priority = High".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    let frame = render(&mut app, 80, 24);
    // "Pay rent" and "Submit Q2 report" are the two High-priority tasks.
    assert!(frame.contains("Pay rent"), "expected high-pri task in list");
    assert!(
        !frame.contains("Reply to Sara"),
        "non-matching task should be filtered out: \n{frame}"
    );
    Ok(())
}

#[test]
fn search_esc_cancels_edit_without_changing_query() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    let before = render(&mut app, 80, 24);

    app.dispatch(key('/'))?;
    for c in "garbage".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let after = render(&mut app, 80, 24);
    assert_eq!(
        before.lines().nth(1).unwrap(),
        after.lines().nth(1).unwrap(),
        "query bar should revert on Esc"
    );
    Ok(())
}

#[test]
fn search_capital_r_reloads_and_picks_up_disk_changes() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    let before = render(&mut app, 80, 24);
    assert!(before.contains("Pay rent"));

    // Mutate disk: append a new overdue task to tasks.md.
    let path = dir.path().join("test-vault").join("tasks.md");
    let mut existing = std::fs::read_to_string(&path).unwrap();
    existing.push_str("- [ ] Brand new urgent task 🔺 📅 2026-05-07\n");
    std::fs::write(&path, existing).unwrap();

    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('R'),
        KeyModifiers::SHIFT,
    )))?;
    app.pump_graph_rebuild_for_test();
    let after = render(&mut app, 80, 24);
    assert!(
        after.contains("Brand new urgent"),
        "R should pick up disk changes:\n{after}"
    );
    Ok(())
}

#[test]
fn quick_key_bracket_close_nudges_due_forward() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    // Selection starts on "Pay rent" (overdue 2026-05-08).
    app.dispatch(key(']'))?;
    let body = std::fs::read_to_string(populated_tasks_path(&dir)).unwrap();
    assert!(
        body.contains("Pay rent ⏫ 📅 2026-05-09"),
        "due should bump to 2026-05-09: \n{body}"
    );
    Ok(())
}

#[test]
fn quick_key_bracket_open_nudges_due_back() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('['))?;
    let body = std::fs::read_to_string(populated_tasks_path(&dir)).unwrap();
    assert!(
        body.contains("Pay rent ⏫ 📅 2026-05-07"),
        "due should bump back to 2026-05-07: \n{body}"
    );
    Ok(())
}

#[test]
fn quick_key_brace_close_nudges_scheduled_forward() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    // Move down to "Submit Q2 report" which has a scheduled date.
    for _ in 0..3 {
        app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    }
    app.dispatch(key('}'))?;
    let body = std::fs::read_to_string(populated_tasks_path(&dir)).unwrap();
    assert!(
        body.contains("⏳ 2026-05-12"),
        "scheduled should bump from 2026-05-11 to 2026-05-12: \n{body}"
    );
    Ok(())
}

#[test]
fn quick_key_p_cycles_priority_forward() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    // Selection: "Pay rent" already has priority High (⏫). Cycle: high → none.
    app.dispatch(key('p'))?;
    let body = std::fs::read_to_string(populated_tasks_path(&dir)).unwrap();
    assert!(
        !body.contains("Pay rent ⏫"),
        "p should clear priority on a high-pri task: \n{body}"
    );
    assert!(
        body.contains("Pay rent 📅"),
        "Pay rent line should still exist sans priority: \n{body}"
    );
    Ok(())
}

#[test]
fn quick_key_capital_p_cycles_priority_backward() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    // "Reply to Sara" has no priority — selection 2 (overdue 2 + first upcoming).
    for _ in 0..2 {
        app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('P'),
        KeyModifiers::SHIFT,
    )))?;
    let body = std::fs::read_to_string(populated_tasks_path(&dir)).unwrap();
    assert!(
        body.contains("Reply to Sara ⏫"),
        "P (reverse) on no-pri task should land on High: \n{body}"
    );
    Ok(())
}

#[test]
fn quick_key_x_completes_selected_task() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('x'))?;
    let body = std::fs::read_to_string(populated_tasks_path(&dir)).unwrap();
    assert!(
        body.contains("- [x] Pay rent"),
        "x should mark Pay rent done: \n{body}"
    );
    assert!(
        body.contains("✅ 2026-05-10"),
        "completion date should be today: \n{body}"
    );
    app.pump_graph_rebuild_for_test();
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("Pay rent"),
        "completed task should disappear from default `not done` query: \n{frame}"
    );
    Ok(())
}

#[test]
fn quick_key_capital_x_cancels_selected_task() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('X'),
        KeyModifiers::SHIFT,
    )))?;
    let body = std::fs::read_to_string(populated_tasks_path(&dir)).unwrap();
    assert!(
        body.contains("- [-] Pay rent"),
        "X should mark Pay rent cancelled: \n{body}"
    );
    assert!(
        body.contains("❌ 2026-05-10"),
        "cancellation date should be today: \n{body}"
    );
    Ok(())
}

#[test]
fn quick_key_t_sets_due_to_today() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    // Selection starts on "Pay rent" (📅 2026-05-08, overdue).
    app.dispatch(key('t'))?;
    let body = std::fs::read_to_string(populated_tasks_path(&dir)).unwrap();
    assert!(
        body.contains("Pay rent ⏫ 📅 2026-05-10"),
        "t should set due to today (2026-05-10): \n{body}"
    );
    Ok(())
}

#[test]
fn edit_popup_opens_on_e_with_current_values() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('e'))?;
    let frame = render(&mut app, 80, 24);
    assert!(frame.contains("edit task"), "popup title missing:\n{frame}");
    assert!(
        frame.contains("Pay rent"),
        "description prefilled:\n{frame}"
    );
    assert!(frame.contains("2026-05-08"), "due prefilled:\n{frame}");
    assert!(frame.contains("high"), "priority prefilled:\n{frame}");
    Ok(())
}

#[test]
fn edit_popup_renders_at_80x24_snapshot() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('e'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("edit_popup_80x24", frame);
    Ok(())
}

#[test]
fn edit_popup_ctrl_s_saves_changes() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('e'))?;
    // Tab to the due field, clear it, type "+3d" (CLI relative-date).
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..20 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "+3d".chars() {
        app.dispatch(key(c))?;
    }
    // Ctrl+S submit.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    )))?;
    let body = std::fs::read_to_string(populated_tasks_path(&dir)).unwrap();
    // 2026-05-10 + 3 days = 2026-05-13.
    assert!(
        body.contains("Pay rent ⏫ 📅 2026-05-13"),
        "+3d should resolve to 2026-05-13: \n{body}"
    );
    Ok(())
}

#[test]
fn edit_popup_esc_cancels_without_writing() -> Result<()> {
    let (dir, vault) = populated_vault();
    let before = std::fs::read_to_string(populated_tasks_path(&dir)).unwrap();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('e'))?;
    for c in "garbage".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let after = std::fs::read_to_string(populated_tasks_path(&dir)).unwrap();
    assert_eq!(before, after, "Esc must not touch disk");
    Ok(())
}

#[test]
fn edit_popup_invalid_date_keeps_popup_open_with_error() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('e'))?;
    // Tab to due, clear, type garbage.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..20 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "not-a-date-at-all".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("⚠"),
        "error indicator should appear:\n{frame}"
    );
    assert!(
        frame.contains("due:"),
        "error should call out the offending field:\n{frame}"
    );
    let body = std::fs::read_to_string(populated_tasks_path(&dir)).unwrap();
    assert!(
        body.contains("📅 2026-05-08"),
        "disk unchanged on parse error"
    );
    Ok(())
}

#[test]
fn enter_on_search_view_queues_editor_open_request() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let req = app
        .take_pending_request()
        .expect("Enter should queue an editor-open request");
    match req {
        AppRequest::OpenInEditor { path, line } => {
            // Both paths get canonicalized to compare reliably across
            // macOS' /var → /private/var symlink.
            let expected = dir
                .path()
                .join("test-vault/tasks.md")
                .canonicalize()
                .unwrap();
            let actual = path.canonicalize().unwrap();
            assert_eq!(actual, expected);
            assert_eq!(line, 1, "first selection should be at line 1");
        }
        other => panic!("expected OpenInEditor, got {other:?}"),
    }
    Ok(())
}

#[test]
fn quick_keys_recurring_complete_inserts_next_instance() -> Result<()> {
    // Spin up a fresh vault with a recurring task so we can exercise the
    // ft-core recurrence path without polluting populated_vault.
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    let body = "- [ ] Water plants 🔁 every week 📅 2026-05-09\n";
    std::fs::write(vault_path.join("tasks.md"), body).unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();

    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('x'))?;
    let body = std::fs::read_to_string(dir.path().join("test-vault/tasks.md")).unwrap();
    assert!(
        body.contains("- [ ] Water plants 🔁 every week 📅 2026-05-16"),
        "next instance should be inserted with due = 2026-05-09 + 7d: \n{body}"
    );
    assert!(
        body.contains("- [x] Water plants"),
        "completed instance should remain: \n{body}"
    );
    Ok(())
}

#[test]
fn question_mark_toggles_help_overlay() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.switch_to(5)?;
    app.dispatch(key('?'))?;
    let frame_with_help = render(&mut app, 80, 24);
    assert!(frame_with_help.contains("Keybindings"));
    app.dispatch(key('?'))?;
    let frame_after = render(&mut app, 80, 24);
    assert!(!frame_after.contains("Keybindings"));
    Ok(())
}

// --- session 6: snapshots ----------------------------------------------------

#[test]
fn help_overlay_over_tasks_tab_renders() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.enter_help();
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("help_overlay_over_tasks_80x24", frame);
    Ok(())
}

/// The Tasks tab's `help_sections()` advertises the Ctrl+P preset row.
/// The rendered 80×24 overlay only shows the Global section (the
/// Tasks-specific sections are below the fold), so assert on the
/// section data directly — the coverage the `?` overlay can't give at
/// default geometry.
#[test]
fn tasks_help_sections_advertise_ctrl_p() {
    use crate::tui::help::HelpSection;
    use crate::tui::tab::Tab;
    use crate::tui::tabs::tasks::TasksTab;
    let tab = TasksTab::new();
    let nav: HelpSection = tab
        .help_sections()
        .into_iter()
        .find(|s| s.title == "Navigation")
        .expect("Tasks tab must have a Navigation help section");
    assert!(
        nav.entries.iter().any(|e| e.keys == "Ctrl+P"),
        "Navigation help section must list Ctrl+P: {:?}",
        nav.entries
    );
    // And the description matches the command's intent.
    let row = nav
        .entries
        .iter()
        .find(|e| e.keys == "Ctrl+P")
        .expect("Ctrl+P row present (checked above)");
    assert_eq!(row.desc, "load preset into query");
}

#[test]
fn edit_popup_error_state_renders() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('e'))?;
    // Tab to due, clear, type garbage, submit — popup stays open with ⚠.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..20 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "not-a-date".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("edit_popup_error_80x24", frame);
    Ok(())
}

#[test]
fn tasks_tab_wide_terminal_snapshot() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    let frame = render(&mut app, 120, 30);
    assert_tui_snapshot!("tasks_tab_populated_120x30", frame);
    Ok(())
}

// --- session 6: help-overlay audit ------------------------------------------

/// The active tab name appears in the overlay header, so a glance at the
/// popup tells you which keymap you're looking at. Asserted against each
/// of the four tabs so the per-tab keymap wiring stays honest. (Helps
/// catch a regression where a tab's `Tab::keymap()` returned the empty
/// default — the header still renders but no rows appear.)
#[test]
fn help_overlay_header_names_active_tab() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    for (idx, title) in ["Graph", "Notes", "Pulse", "Recent"].iter().enumerate() {
        app.switch_to(idx)?;
        app.enter_help();
        let frame = render(&mut app, 80, 40);
        let needle = format!("Keybindings — {title}");
        assert!(
            frame.contains(&needle),
            "help overlay on tab {idx} missing header `{needle}`:\n{frame}"
        );
        // Leave help mode before switching to the next tab.
        app.dispatch(key('?'))?;
    }
    Ok(())
}

// --- session 6: real-vault smoke check ---------------------------------------

/// Gated smoke test: render the Tasks tab against the user's real vault.
/// Activates only with `FT_REAL_VAULT_TESTS=1` so CI never depends on a local
/// path. Mirrors the gating already used by `tests/real_vault_cli.rs`.
#[test]
fn real_vault_tasks_tab_renders_without_panic() -> Result<()> {
    if std::env::var("FT_REAL_VAULT_TESTS").as_deref() != Ok("1") {
        return Ok(());
    }
    let path = std::path::PathBuf::from("/Users/cmw/git/fortytwo");
    if !path.exists() {
        return Ok(()); // gracefully skip if the real vault isn't on this host
    }
    let vault = Vault::discover(Some(path))?;
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    // First render runs vault.scan() + filter + sort under the hood.
    let frame = render(&mut app, 120, 40);
    assert!(
        frame.contains("Tasks") || frame.contains("tasks"),
        "real-vault first render should still render the Tasks chrome:\n{frame}"
    );
    Ok(())
}

// --- session 6: performance budgets on a 5k-note vault -----------------------

/// Build a synthetic 5k-note vault for the perf budget tests. Each note has
/// one task with a varying due date and priority so the default query is
/// non-trivial. ~5000 files written to a tempdir — setup is slow but only
/// runs when `FT_PERF_TESTS=1` is set.
fn synthetic_5k_vault(today: chrono::NaiveDate) -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();

    for i in 0..5000u32 {
        // Spread dates across a 60-day window, half before today and half after.
        let offset = (i as i64 % 60) - 30;
        let due = today + chrono::Duration::days(offset);
        let priority = match i % 4 {
            0 => "⏫ ",
            1 => "🔼 ",
            2 => "🔽 ",
            _ => "",
        };
        let body = format!(
            "# Note {i}\n\n- [ ] Synthetic task {i} {priority}📅 {}\n",
            due.format("%Y-%m-%d")
        );
        std::fs::write(vault_path.join(format!("note_{i:05}.md")), body).unwrap();
    }
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

fn perf_tests_enabled() -> bool {
    std::env::var("FT_PERF_TESTS").as_deref() == Ok("1")
}

#[test]
fn perf_first_render_5k_vault_under_budget() -> Result<()> {
    if !perf_tests_enabled() {
        return Ok(());
    }
    let today = chrono::NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
    let (_dir, vault) = synthetic_5k_vault(today);

    let start = std::time::Instant::now();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    let _ = render(&mut app, 80, 24);
    let elapsed = start.elapsed();

    // Plan budget is 500ms; allow 4x for debug builds & noisy CI. Run with
    // --release for tight timing: `cargo test --release ... perf_first_render`.
    let budget_ms: u128 = 2000;
    assert!(
        elapsed.as_millis() < budget_ms,
        "first render took {:?}; budget {budget_ms}ms (4x of 500ms target). \
         Run --release for tight timing.",
        elapsed
    );
    Ok(())
}

// --- session 6 follow-ups: status indicator in rows ------------------------

/// Vault with one task per status, used to exercise the status-glyph column
/// when the query is broad enough to include done / cancelled / in-progress.
fn mixed_status_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    let body = "\
- [ ] Open task 📅 2026-05-15
- [x] Done task 📅 2026-05-15 ✅ 2026-05-09
- [-] Cancelled task 📅 2026-05-15 ❌ 2026-05-09
- [/] In-progress task 📅 2026-05-15
";
    std::fs::write(vault_path.join("tasks.md"), body).unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn search_view_renders_status_glyphs_when_query_includes_all_statuses() -> Result<()> {
    let (_dir, vault) = mixed_status_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    // Default query is `not done` — replace with a no-op filter so every
    // task shows up regardless of status.
    app.dispatch(key('/'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..200 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    // Empty query matches everything; the parser treats this as no filter.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    let frame = render(&mut app, 100, 24);
    assert!(frame.contains("Open task"), "open row missing:\n{frame}");
    assert!(frame.contains("Done task"), "done row missing:\n{frame}");
    assert!(
        frame.contains("Cancelled task"),
        "cancelled row missing:\n{frame}"
    );
    assert!(
        frame.contains("In-progress task"),
        "in-progress row missing:\n{frame}"
    );

    // Each non-open status renders a unique glyph in the new column.
    let done_line = frame
        .lines()
        .find(|l| l.contains("Done task"))
        .expect("done line missing");
    assert!(
        done_line.contains('✓'),
        "done row should display ✓:\n{done_line}"
    );
    let cancelled_line = frame
        .lines()
        .find(|l| l.contains("Cancelled task"))
        .expect("cancelled line missing");
    assert!(
        cancelled_line.contains('✗'),
        "cancelled row should display ✗:\n{cancelled_line}"
    );
    let inprogress_line = frame
        .lines()
        .find(|l| l.contains("In-progress task"))
        .expect("in-progress line missing");
    assert!(
        inprogress_line.contains('▷'),
        "in-progress row should display ▷:\n{inprogress_line}"
    );
    Ok(())
}

// --- session 6 follow-ups: ctrl+backspace word-delete in edit fields --------

#[test]
fn ctrl_backspace_deletes_word_in_query_bar() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;

    // Open the query bar and replace its contents with a known string.
    app.dispatch(key('/'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..200 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "alpha beta gamma".chars() {
        app.dispatch(key(c))?;
    }

    // Ctrl+Backspace should remove "gamma" (the trailing word).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Backspace,
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("alpha beta "),
        "after Ctrl+Backspace the trailing word should be gone:\n{frame}"
    );
    assert!(
        !frame.contains("gamma"),
        "gamma should be deleted:\n{frame}"
    );
    Ok(())
}

#[test]
fn ctrl_w_deletes_word_in_query_bar() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('/'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..200 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "foo bar".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 80, 24);
    // The query bar is the line immediately after the top tab bar.
    let query_line = frame
        .lines()
        .find(|l| l.contains("foo"))
        .expect("query bar should still contain `foo`");
    assert!(
        !query_line.contains("bar"),
        "bar should be deleted from query bar:\n{query_line}"
    );
    Ok(())
}

/// §7 per-mount-site coverage: readline bindings reach the tasks tab
/// `/` search picker via `EditBuffer::handle_event`. Pre-§7 the
/// picker hand-rolled key dispatch and dropped Ctrl+A/E/Alt+B.
#[test]
fn ctrl_a_jumps_to_start_in_tasks_search_picker() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('/'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..200 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "xyz".chars() {
        app.dispatch(key(c))?;
    }
    // Ctrl+A jumps cursor to start; insert 'Z' there.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )))?;
    app.dispatch(key('Z'))?;
    let frame = render(&mut app, 80, 24);
    // After Ctrl+A then Z, the buffer holds "Zxyz" with the cursor
    // between Z and x; the renderer paints the cursor as a `│` glyph
    // between them. Strip all `│` (box-drawing + cursor) from the
    // line so we can match the contiguous buffer text.
    let query_line = frame
        .lines()
        .find(|l| l.contains('Z') && l.contains("xyz"))
        .unwrap_or_else(|| panic!("expected query line with Z + xyz:\n{frame}"));
    let stripped: String = query_line.chars().filter(|&c| c != '│').collect();
    assert!(
        stripped.contains("Zxyz"),
        "stripped query line should contain `Zxyz`:\n{stripped}\n(raw: {query_line})"
    );
    Ok(())
}

/// §7: readline bindings reach the tasks tab edit popup (`e`) via
/// `EditBuffer::handle_event`. The popup wraps multiple fields and
/// previously hand-rolled key dispatch.
#[test]
fn ctrl_a_jumps_to_start_in_tasks_edit_popup() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('e'))?;
    // Focus starts on description, which holds "Pay rent" in the fixture.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    // Ctrl+A jumps to start of description; insert "X".
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )))?;
    app.dispatch(key('X'))?;
    let frame = render(&mut app, 80, 24);
    let line = frame
        .lines()
        .find(|l| l.contains('X') && l.contains("Pay rent"))
        .unwrap_or_else(|| panic!("expected line with X + Pay rent:\n{frame}"));
    let stripped: String = line.chars().filter(|&c| c != '│').collect();
    assert!(
        stripped.contains("XPay rent"),
        "stripped description line should contain `XPay rent`:\n{stripped}\n(raw: {line})"
    );
    Ok(())
}

/// §7: readline bindings reach the tasks tab quickline (`c`).
#[test]
fn alt_b_word_jump_in_tasks_quickline() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "buy milk".chars() {
        app.dispatch(key(c))?;
    }
    // Cursor at end (8). Alt+B → cursor lands at 4 (start of "milk").
    // Insert 'X' there.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('b'),
        KeyModifiers::ALT,
    )))?;
    app.dispatch(key('X'))?;
    let frame = render(&mut app, 80, 24);
    let line = frame
        .lines()
        .find(|l| l.contains("buy") && l.contains('X') && l.contains("milk"))
        .unwrap_or_else(|| panic!("expected quickline with buy + X + milk:\n{frame}"));
    let stripped: String = line.chars().filter(|&c| c != '│').collect();
    assert!(
        stripped.contains("buy Xmilk"),
        "stripped quickline should contain `buy Xmilk`:\n{stripped}\n(raw: {line})"
    );
    Ok(())
}

#[test]
fn ctrl_backspace_deletes_word_in_edit_popup_field() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('e'))?;
    // Focus starts on description, which holds "Pay rent". Ctrl+Backspace
    // should erase "rent" but leave "Pay ".
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Backspace,
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Pay "),
        "Pay should remain in the description field:\n{frame}"
    );
    // The word "rent" only appears in the description column of the
    // background task list (which is still visible to the left/right of
    // the popup). Make sure it's no longer inside the popup's
    // description value cell — check that the line right of "description :"
    // doesn't contain "rent".
    let popup_line = frame
        .lines()
        .find(|l| l.contains("description :"))
        .expect("popup description row missing");
    assert!(
        !popup_line.contains("rent"),
        "rent should be deleted from the description field:\n{popup_line}"
    );
    Ok(())
}

// --- session 6 follow-ups: tag round-trip + editor-exit drain ---------------

#[test]
fn edit_popup_saves_tag_changes_back_to_description() -> Result<()> {
    // Tags are derived from the description on parse, so the popup has to
    // rewrite the description to persist tag edits. Regression for: "tag
    // changes don't get saved" after using `e`.
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::write(
        vault_path.join("tasks.md"),
        "- [ ] Pay rent #old 📅 2026-05-08\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();

    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('e'))?;
    // Jump straight to the tags field (description, due, scheduled, priority, tags).
    for _ in 0..4 {
        app.dispatch(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..20 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "work urgent".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    )))?;

    let body = std::fs::read_to_string(dir.path().join("test-vault/tasks.md")).unwrap();
    assert!(
        body.contains("#work"),
        "new tag should be embedded in description: \n{body}"
    );
    assert!(
        body.contains("#urgent"),
        "new tag should be embedded in description: \n{body}"
    );
    assert!(
        !body.contains("#old"),
        "tag removed from popup tags field should be stripped: \n{body}"
    );
    Ok(())
}

#[test]
fn edit_popup_emptying_tags_field_removes_all_tags() -> Result<()> {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::write(
        vault_path.join("tasks.md"),
        "- [ ] Pay rent #work #urgent 📅 2026-05-08\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();

    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('e'))?;
    for _ in 0..4 {
        app.dispatch(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..40 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    )))?;

    let body = std::fs::read_to_string(dir.path().join("test-vault/tasks.md")).unwrap();
    assert!(
        !body.contains('#'),
        "clearing the tags field should strip inline tags: \n{body}"
    );
    assert!(
        body.contains("Pay rent"),
        "description text must survive:\n{body}"
    );
    Ok(())
}

#[test]
fn event_stream_drain_consumes_pending_events() {
    use crate::tui::event::EventStream;
    use std::time::Duration;

    // Standing up a real EventStream relies on a TTY for the crossterm
    // poll loop. In a non-tty test environment poll fails fast and the
    // background thread exits — drain still has to behave (no events to
    // consume, returns within the window without spinning).
    let stream = EventStream::new(Duration::from_secs(60)); // long tick so no ticks queue
    let start = std::time::Instant::now();
    stream.drain(Duration::from_millis(80));
    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_millis(60),
        "drain should consume the full window: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_millis(400),
        "drain should not block past the window: {elapsed:?}"
    );
}

// --- session 6: perf budgets (re-anchor so file order is stable) ------------

#[test]
fn perf_keystrokes_5k_vault_under_budget() -> Result<()> {
    if !perf_tests_enabled() {
        return Ok(());
    }
    let today = chrono::NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
    let (_dir, vault) = synthetic_5k_vault(today);
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    let _ = render(&mut app, 80, 24);

    // Dispatch 100 down-arrows + redraw each time. In-memory navigation
    // should never re-scan; the cost is purely filter+layout.
    let down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let iterations = 100u32;
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        app.dispatch(down.clone())?;
        let _ = render(&mut app, 80, 24);
    }
    let elapsed = start.elapsed();
    let per_key_ms = elapsed.as_millis() / u128::from(iterations);

    // Plan budget is 50ms per keystroke; allow 2x for debug. Release builds
    // typically come in well under 10ms.
    let budget_ms: u128 = 100;
    assert!(
        per_key_ms < budget_ms,
        "per-keystroke {per_key_ms}ms exceeded budget {budget_ms}ms \
         (target 50ms). Total: {:?} across {iterations} iters.",
        elapsed
    );
    Ok(())
}

// --- plan 004 session 2: quickline (new task) ------------------------------

/// Vault preconfigured to drop a daily note at `<root>/Daily/2026-05-10.md`
/// via `[periodic_notes.daily]`, so quickline writes without `in:` land
/// somewhere predictable for assertions.
fn quickline_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::create_dir_all(vault_path.join(".ft")).unwrap();
    std::fs::write(
        vault_path.join(".ft/config.toml"),
        "[periodic_notes.daily]\npath = \"Daily\"\nformat = \"%Y-%m-%d\"\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn quickline_opens_with_c_and_closes_on_esc() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    let frame = render(&mut app, 100, 24);
    assert!(
        frame.contains("new task"),
        "panel title missing after `c`:\n{frame}"
    );
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let after = render(&mut app, 100, 24);
    assert!(
        !after.contains("new task"),
        "panel should close on Esc:\n{after}"
    );
    Ok(())
}

#[test]
fn quickline_enter_writes_to_daily_note() -> Result<()> {
    let (dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "buy milk due:tomorrow pri:high #grocery".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let daily = dir.path().join("test-vault/Daily/2026-05-10.md");
    let body = std::fs::read_to_string(&daily)
        .unwrap_or_else(|e| panic!("daily note missing: {}: {e}", daily.display()));
    assert!(body.contains("buy milk"), "description missing:\n{body}");
    assert!(body.contains("⏫"), "high priority emoji missing:\n{body}");
    assert!(body.contains("📅 2026-05-11"), "due date missing:\n{body}");
    assert!(body.contains("#grocery"), "tag missing:\n{body}");
    // Panel should close on success.
    let frame = render(&mut app, 100, 24);
    assert!(
        !frame.contains("new task"),
        "panel should close after a successful write:\n{frame}"
    );
    Ok(())
}

#[test]
fn quickline_honors_default_section_and_frontmatter() -> Result<()> {
    // Vault with a configured `[tasks] default_section`. A quickline write
    // into the daily note lands under that heading; a write into a note
    // whose frontmatter pins `ft-tasks-section` uses that instead.
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::create_dir_all(vault_path.join(".ft")).unwrap();
    std::fs::write(
        vault_path.join(".ft/config.toml"),
        "[periodic_notes.daily]\npath = \"Daily\"\nformat = \"%Y-%m-%d\"\n\n[tasks]\ndefault_section = \"Tasks\"\n",
    )
    .unwrap();
    std::fs::write(
        vault_path.join("Inbox.md"),
        "---\nft-tasks-section: Captured\n---\n# Inbox\n\n## Captured\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;

    // Daily note → config default section.
    app.dispatch(key('c'))?;
    for c in "buy milk".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let daily = dir.path().join("test-vault/Daily/2026-05-10.md");
    let body = std::fs::read_to_string(&daily).unwrap();
    assert!(
        body.contains("## Tasks\n- [ ] buy milk"),
        "task should land under the configured default section:\n{body}"
    );

    // in:Inbox.md → frontmatter section wins over config default.
    app.dispatch(key('c'))?;
    for c in "call dentist in:Inbox.md".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let inbox = dir.path().join("test-vault/Inbox.md");
    let body = std::fs::read_to_string(&inbox).unwrap();
    assert!(
        body.contains("## Captured\n- [ ] call dentist"),
        "task should land under the frontmatter section:\n{body}"
    );
    assert!(
        !body.contains("## Tasks"),
        "config default shouldn't apply:\n{body}"
    );
    Ok(())
}

#[test]
fn quickline_in_path_overrides_target() -> Result<()> {
    let (dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "remember to call dentist in:Inbox.md".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let inbox = dir.path().join("test-vault/Inbox.md");
    let body = std::fs::read_to_string(&inbox).unwrap();
    assert!(body.contains("call dentist"));
    // Daily note shouldn't have been touched.
    let daily = dir.path().join("test-vault/Daily/2026-05-10.md");
    assert!(
        !daily.exists()
            || !std::fs::read_to_string(&daily)
                .unwrap()
                .contains("call dentist"),
        "daily note shouldn't have the in:-overridden task"
    );
    Ok(())
}

#[test]
fn quickline_parse_error_blocks_write() -> Result<()> {
    let (dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "draft due:not-a-date".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 100, 24);
    assert!(
        frame.contains("new task"),
        "panel should stay open on parse error:\n{frame}"
    );
    assert!(frame.contains("⚠"), "error indicator missing:\n{frame}");
    // Nothing landed on disk.
    let daily = dir.path().join("test-vault/Daily/2026-05-10.md");
    assert!(
        !daily.exists() || std::fs::read_to_string(&daily).unwrap().trim().is_empty(),
        "daily note should be empty when parse fails"
    );
    Ok(())
}

#[test]
fn quickline_duplicate_detection_surfaces_inline() -> Result<()> {
    let (dir, vault) = quickline_vault();
    // Pre-seed an identical task so the second create hits the duplicate
    // detector inside ops::create_task.
    let inbox = dir.path().join("test-vault/Inbox.md");
    std::fs::write(&inbox, "- [ ] follow up with team 📅 2026-05-11\n").unwrap();

    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "follow up with team due:tomorrow in:Inbox.md".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    let frame = render(&mut app, 100, 24);
    assert!(
        frame.contains("duplicate"),
        "duplicate error should surface inline:\n{frame}"
    );
    assert!(
        frame.contains("new task"),
        "panel should stay open on duplicate:\n{frame}"
    );
    // Inbox unchanged (still only the pre-seeded line).
    let body = std::fs::read_to_string(&inbox).unwrap();
    assert_eq!(body.lines().filter(|l| l.contains("follow up")).count(), 1);
    Ok(())
}

#[test]
fn quickline_empty_description_blocks_write() -> Result<()> {
    let (dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    // Only a tag — no description text.
    for c in "due:tomorrow".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 100, 24);
    assert!(frame.contains("new task"), "panel stays open: \n{frame}");
    assert!(
        frame.contains("description is empty"),
        "error missing: \n{frame}"
    );
    let daily = dir.path().join("test-vault/Daily/2026-05-10.md");
    assert!(!daily.exists() || std::fs::read_to_string(daily).unwrap().trim().is_empty());
    Ok(())
}

#[test]
fn quickline_success_raises_green_toast_request() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "buy milk due:tomorrow".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Service the queued AppRequest::Toast so the App's toast slot
    // becomes populated (the run-loop does this between iterations).
    app.service_pending_requests()?;
    let toast = app
        .current_toast()
        .expect("a toast should be active after a successful create");
    assert!(
        toast.text.starts_with("created "),
        "toast text: {}",
        toast.text
    );
    assert_eq!(toast.style, crate::tui::tab::ToastStyle::Success);
    Ok(())
}

#[test]
fn quickline_success_renders_toast_in_status_bar_center_cell() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "draft report due:tomorrow".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.service_pending_requests()?;
    let frame = render(&mut app, 120, 24);
    let status = frame.lines().last().expect("status bar row");
    assert!(
        status.contains("created"),
        "status bar should show the toast: {status}"
    );
    Ok(())
}

#[test]
fn quickline_success_anchors_cursor_to_new_task_when_it_matches_filter() -> Result<()> {
    // New task is due tomorrow → passes the default `not done and due
    // before today+8d` filter, so it should appear in the list AND the
    // cursor should land on its row.
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "anchor target due:tomorrow".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.pump_graph_rebuild_for_test();
    let frame = render(&mut app, 120, 24);
    // Find the row with the new task and assert it carries the `▶` cursor.
    let row = frame
        .lines()
        .find(|l| l.contains("anchor target"))
        .expect("new task missing from list");
    assert!(
        row.contains('▶'),
        "cursor should anchor to the new task row: {row}"
    );
    Ok(())
}

#[test]
fn quickline_duplicate_does_not_raise_toast() -> Result<()> {
    // Duplicate detection stays inline (the user can edit and retry),
    // so it must NOT also fire a toast — that'd be redundant noise.
    let (dir, vault) = quickline_vault();
    let inbox = dir.path().join("test-vault/Inbox.md");
    std::fs::write(&inbox, "- [ ] dup task 📅 2026-05-11\n").unwrap();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "dup task due:tomorrow in:Inbox.md".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.service_pending_requests()?;
    assert!(
        app.current_toast().is_none(),
        "duplicate should stay inline, not fire a toast"
    );
    Ok(())
}

// --- session 4: expanded popup (Shift+C / Ctrl+E) -------------------------

#[test]
fn shift_c_opens_blank_new_task_popup() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    let frame = render(&mut app, 100, 24);
    assert!(
        frame.contains("new task"),
        "popup title should be `new task`:\n{frame}"
    );
    // Target field is part of the New-mode form.
    assert!(
        frame.contains("target"),
        "target field should be in the New popup:\n{frame}"
    );
    Ok(())
}

#[test]
fn ctrl_e_in_quickline_opens_popup_with_pre_populated_fields() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "review report due:tomorrow pri:high #work in:Inbox.md".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 100, 24);
    assert!(frame.contains("new task"), "popup not open:\n{frame}");
    assert!(
        frame.contains("review report"),
        "description missing:\n{frame}"
    );
    assert!(frame.contains("2026-05-11"), "due missing:\n{frame}");
    assert!(frame.contains("high"), "priority missing:\n{frame}");
    assert!(frame.contains("Inbox.md"), "target missing:\n{frame}");
    assert!(frame.contains("work"), "tags missing:\n{frame}");
    Ok(())
}

#[test]
fn new_popup_ctrl_s_writes_to_in_target() -> Result<()> {
    let (dir, vault) = quickline_vault();
    // Inbox.md needs to exist so the picker can find it on the first
    // keystroke — the target field is now pick-driven, not type-literal.
    let inbox = dir.path().join("test-vault/Inbox.md");
    std::fs::write(&inbox, "# Inbox\n").unwrap();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    for c in "kickoff sync".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)))?;
    // Type into target — first char opens the picker, subsequent chars
    // feed it. Enter selects the highlighted hit and fills the field as
    // `Inbox.md`.
    for c in "Inbox".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)))?;
    for c in "tomorrow".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    )))?;

    let body = std::fs::read_to_string(&inbox)
        .unwrap_or_else(|e| panic!("inbox missing: {}: {e}", inbox.display()));
    assert!(
        body.contains("kickoff sync"),
        "description missing:\n{body}"
    );
    assert!(body.contains("📅 2026-05-11"), "due missing:\n{body}");
    Ok(())
}

#[test]
fn new_popup_target_with_heading_uses_under_heading_position() -> Result<()> {
    let (dir, vault) = quickline_vault();
    let inbox = dir.path().join("test-vault/Inbox.md");
    std::fs::write(
        &inbox,
        "# Inbox\n\n## Triage\n- [ ] existing 📅 2026-05-12\n\n## Done\n",
    )
    .unwrap();

    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    for c in "new triage item".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)))?;
    // Typing the file+heading query opens the picker on the first char
    // and feeds the rest. Picker matches the `## Triage` heading inside
    // Inbox.md. Enter selects it; the target field gets filled with
    // `Inbox.md#Triage`.
    for c in "Inbox.md#Triage".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    )))?;

    let body = std::fs::read_to_string(&inbox).unwrap();
    let triage_line = body
        .lines()
        .position(|l| l.contains("## Triage"))
        .expect("Triage section missing");
    let done_line = body
        .lines()
        .position(|l| l.contains("## Done"))
        .expect("Done section missing");
    let new_line = body
        .lines()
        .position(|l| l.contains("new triage item"))
        .expect("new task missing");
    assert!(
        triage_line < new_line && new_line < done_line,
        "new task should be under Triage, before Done:\n{body}"
    );
    Ok(())
}

#[test]
fn new_popup_empty_description_blocks_write() -> Result<()> {
    let (dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 100, 24);
    assert!(frame.contains("new task"), "popup stays open:\n{frame}");
    assert!(
        frame.contains("description is empty"),
        "error missing:\n{frame}"
    );
    let daily = dir.path().join("test-vault/Daily/2026-05-10.md");
    assert!(!daily.exists() || std::fs::read_to_string(&daily).unwrap().trim().is_empty());
    Ok(())
}

#[test]
fn edit_popup_still_works_after_refactor() -> Result<()> {
    // Regression check: refactoring EditPopup to support both modes
    // mustn't break the existing `e`-on-selected-task edit flow.
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('e'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for c in " (updated)".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    )))?;
    let body = std::fs::read_to_string(dir.path().join("test-vault/tasks.md")).unwrap();
    assert!(
        body.contains("Pay rent (updated)"),
        "edit should still write:\n{body}"
    );
    Ok(())
}

#[test]
fn new_popup_snapshot_80x24() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("new_popup_blank_80x24", frame);
    Ok(())
}

#[test]
fn quickline_empty_snapshot_80x24() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("quickline_empty_80x24", frame);
    Ok(())
}

#[test]
fn quickline_valid_preview_snapshot_80x24() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "buy milk due:tomorrow pri:high #grocery".chars() {
        app.dispatch(key(c))?;
    }
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("quickline_valid_preview_80x24", frame);
    Ok(())
}

#[test]
fn quickline_parse_error_snapshot_80x24() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "draft due:not-a-date".chars() {
        app.dispatch(key(c))?;
    }
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("quickline_parse_error_80x24", frame);
    Ok(())
}

#[test]
fn new_popup_prefilled_snapshot_80x24() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "review report due:tomorrow pri:high #work".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("new_popup_prefilled_80x24", frame);
    Ok(())
}

#[test]
fn quickline_toast_success_snapshot_80x24() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "ship feature due:tomorrow".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.service_pending_requests()?;
    app.pump_graph_rebuild_for_test();
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("quickline_toast_success_80x24", frame);
    Ok(())
}

#[test]
fn quickline_ctrl_w_works_in_input() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('c'))?;
    for c in "foo bar".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 100, 24);
    // Pick the input row from the new-task panel specifically; the rest
    // of the frame contains "sidebar"/"Inbox.md" etc. that would yield
    // false positives for the "bar" substring check.
    let input_row = frame
        .lines()
        .find(|l| l.contains("foo"))
        .expect("input row with `foo` missing");
    assert!(
        !input_row.contains("bar"),
        "bar deleted from input row: {input_row}"
    );
    Ok(())
}

// --- target-field fuzzy picker (plan 006) -------------------------------

/// Test fixture: a vault with a couple of pickable files so the target
/// picker has something to match. Mirrors `quickline_vault` but adds two
/// known notes — `Areas/General Considerations.md` (with a `## Triage`
/// heading) and `Inbox.md` — so we don't have to repeat the boilerplate
/// in every picker test.
fn target_picker_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::create_dir_all(vault_path.join(".ft")).unwrap();
    std::fs::create_dir_all(vault_path.join("Areas")).unwrap();
    std::fs::write(
        vault_path.join(".ft/config.toml"),
        "[periodic_notes.daily]\npath = \"Daily\"\nformat = \"%Y-%m-%d\"\n",
    )
    .unwrap();
    std::fs::write(vault_path.join("Inbox.md"), "# Inbox\n").unwrap();
    std::fs::write(
        vault_path.join("Areas/General Considerations.md"),
        "# Intro\n\n## Triage\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

/// Open the new-task popup with target focused and the picker ready
/// to be triggered. Shared setup for the picker tests below.
fn open_new_popup_on_target(app: &mut App) -> Result<()> {
    app.switch_to(5)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    // Description → Target (single Tab).
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)))?;
    Ok(())
}

#[test]
fn target_picker_opens_on_enter_with_field_text_as_seed() -> Result<()> {
    let (_dir, vault) = target_picker_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    open_new_popup_on_target(&mut app)?;
    // Press Enter on the empty target field — picker opens with empty
    // input, so the header is visible but no rows.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 100, 30);
    assert!(
        frame.contains("pick target"),
        "picker title missing:\n{frame}"
    );
    Ok(())
}

#[test]
fn target_picker_opens_on_first_keystroke_and_seeds_input() -> Result<()> {
    let (_dir, vault) = target_picker_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    open_new_popup_on_target(&mut app)?;
    // `g` opens the picker with `g` already in the input, narrowing
    // the result list to `General Considerations.md`.
    app.dispatch(key('g'))?;
    // Wider terminal so the path doesn't get truncated inside the
    // popup-in-popup column.
    let frame = render(&mut app, 140, 30);
    assert!(frame.contains("pick target"), "picker not open:\n{frame}");
    assert!(
        frame.contains("General Considerations"),
        "expected file match after seeding `g`:\n{frame}"
    );
    Ok(())
}

#[test]
fn target_picker_enter_fills_field_with_path_only() -> Result<()> {
    let (_dir, vault) = target_picker_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    open_new_popup_on_target(&mut app)?;
    for c in "Inbox".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 100, 30);
    assert!(
        !frame.contains("pick target"),
        "picker should close after select:\n{frame}"
    );
    assert!(
        frame.contains("Inbox.md"),
        "target field should hold `Inbox.md`:\n{frame}"
    );
    Ok(())
}

#[test]
fn target_picker_enter_fills_field_with_path_and_heading() -> Result<()> {
    let (_dir, vault) = target_picker_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    open_new_popup_on_target(&mut app)?;
    // Heading-query syntax mirrors the literal text the field
    // would have accepted before plan 006 — the round-trip stays
    // symmetric.
    for c in "gen consid#Tri".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 120, 30);
    assert!(
        !frame.contains("pick target"),
        "picker should close after select:\n{frame}"
    );
    assert!(
        frame.contains("General Considerations.md#Triage"),
        "target field should hold path#heading:\n{frame}"
    );
    Ok(())
}

#[test]
fn target_picker_navigation_changes_selection() -> Result<()> {
    let (_dir, vault) = target_picker_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    open_new_popup_on_target(&mut app)?;
    // Query that hits multiple files so navigation has an effect:
    // `.md` matches every markdown file in the vault.
    for c in ".md".chars() {
        app.dispatch(key(c))?;
    }
    let initial = render(&mut app, 100, 30);
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    let after_down = render(&mut app, 100, 30);
    assert!(
        initial != after_down,
        "Down arrow should change the highlighted row:\nbefore:\n{initial}\nafter:\n{after_down}"
    );
    Ok(())
}

#[test]
fn target_picker_esc_cancels_and_leaves_field_unchanged() -> Result<()> {
    let (_dir, vault) = target_picker_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    open_new_popup_on_target(&mut app)?;
    for c in "Inbox".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 100, 30);
    assert!(
        !frame.contains("pick target"),
        "picker should be closed:\n{frame}"
    );
    assert!(
        frame.contains("new task"),
        "popup should still be open:\n{frame}"
    );
    Ok(())
}

#[test]
fn target_picker_does_not_open_from_description_field() -> Result<()> {
    let (_dir, vault) = target_picker_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    // Description is the default focus — typing here must not open
    // the picker, it should insert into the description buffer.
    for c in "Inbox".chars() {
        app.dispatch(key(c))?;
    }
    let frame = render(&mut app, 100, 30);
    assert!(
        !frame.contains("pick target"),
        "picker must not open from description focus:\n{frame}"
    );
    assert!(
        frame.contains("Inbox"),
        "description should show typed text:\n{frame}"
    );
    Ok(())
}

// ── Ctrl+P task preset picker ───────────────────────────────────────

/// Helper: build an app on the Tasks tab over a populated vault with the
/// fixed test clock, with the default query active.
fn tasks_app_populated() -> App {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5).expect("switch to Tasks");
    app
}

/// Ctrl+P opens the task-preset-picker modal (built-in task presets are
/// always available). Mirrors the graph tab's
/// `graph_tab_preset_picker_opens_on_ctrl_n`.
#[test]
fn ctrl_p_opens_task_preset_picker() -> Result<()> {
    let mut app = tasks_app_populated();
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    )))?;
    assert_eq!(
        app.active_modal_name(),
        Some("task-preset-picker"),
        "Ctrl+P must open the task-preset-picker modal"
    );
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("tasks_tab_preset_picker_open_80x24", frame);
    Ok(())
}

/// Selecting a preset (Enter) replaces the active query with the preset's
/// DSL, recomputes matches, and returns to normal mode (modal closed).
/// The built-in `overdue` preset is `(status in {Open, InProgress}) and
/// due < today`.
#[test]
fn ctrl_p_selecting_preset_replaces_query_and_stays_in_normal_mode() -> Result<()> {
    let mut app = tasks_app_populated();
    // Ctrl+P opens the picker; dispatch services the OpenModal request.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    )))?;
    assert_eq!(app.active_modal_name(), Some("task-preset-picker"));

    // Move the cursor to land on the `overdue` built-in (presets are
    // listed user-first then built-ins alphabetically: done-today,
    // not-done, overdue, ...). Two Down presses from the top reach it.
    for _ in 0..2 {
        app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    }
    // Enter commits: the modal posts AppRequest::Tasks(ApplyPreset),
    // which service_pending_requests routes to TasksTab::handle_tasks_request.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // Modal is closed — we're back in normal mode.
    assert_eq!(
        app.active_modal_name(),
        None,
        "Enter must close the picker and return to normal mode"
    );

    // The active view's query bar now shows the overdue preset DSL.
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("(status in {Open, InProgress}) and due < today"),
        "query bar should show the overdue preset DSL:\n{frame}"
    );
    // The default query (due < <date>) is no longer present.
    assert!(
        !frame.contains("due < 2026-05-18"),
        "default query should be replaced by the preset:\n{frame}"
    );
    Ok(())
}

/// Esc dismisses the picker without modifying the active query.
#[test]
fn ctrl_p_cancel_leaves_query_unchanged() -> Result<()> {
    let mut app = tasks_app_populated();
    // Capture the default query before opening the picker.
    let before = render(&mut app, 80, 24);
    let default_query = before
        .lines()
        .find(|l| l.contains("due < 2026"))
        .expect("default query should be visible before Ctrl+P");

    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    )))?;
    assert_eq!(app.active_modal_name(), Some("task-preset-picker"));
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    assert_eq!(app.active_modal_name(), None, "Esc must close the picker");

    let after = render(&mut app, 80, 24);
    let after_query = after
        .lines()
        .find(|l| l.contains("due < 2026"))
        .expect("default query should still be visible after cancel");
    assert_eq!(
        default_query, after_query,
        "Esc must leave the active query unchanged"
    );
    Ok(())
}

/// Ctrl+P is a no-op while the query bar is in edit mode (the edit_state
/// sub-mode swallows keys before the idle keymap is consulted). Opening
/// the picker from edit mode must not happen.
#[test]
fn ctrl_p_does_not_open_picker_while_editing_query() -> Result<()> {
    let mut app = tasks_app_populated();
    // `/` enters query edit mode.
    app.dispatch(key('/'))?;
    assert!(
        app.active_modal_name().is_none(),
        "query edit is an inline sub-mode, not a modal"
    );
    // Ctrl+P is consumed by the edit buffer, not the idle keymap.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    )))?;
    assert_eq!(
        app.active_modal_name(),
        None,
        "Ctrl+P must not open the picker while the query bar is being edited"
    );
    Ok(())
}

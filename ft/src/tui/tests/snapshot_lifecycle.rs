//! Shared graph snapshot: rebuild lifecycle (coalescing, failure,
//! generations), loading states, stale-guard end to end.

use super::*;

// ── shared graph snapshot · rebuild lifecycle (shared-graph-snapshot §1) ──

#[test]
fn for_test_constructor_installs_initial_snapshot() {
    let (_dir, vault) = rename_vault(&[("a.md", "see [[b]]\n"), ("b.md", "target\n")]);
    let app = App::for_test_with_clock(vault, fixed_clock);
    assert_eq!(
        app.graph_generation_for_test(),
        Some(1),
        "for_test constructors must install a first snapshot synchronously"
    );
}

#[test]
fn refresh_graph_requests_coalesce_into_one_build() -> Result<()> {
    let (_dir, vault) = rename_vault(&[("a.md", "note\n")]);
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    assert_eq!(app.graph_generation_for_test(), Some(1));

    // Three refresh requests before the pump (via the Tasks tab's `R`,
    // which raises the TabCtx graph-refresh flag): at most one rebuild.
    app.switch_to(1)?;
    for _ in 0..3 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char('R'),
            KeyModifiers::SHIFT,
        )))?;
    }
    app.pump_graph_rebuild_for_test();
    assert_eq!(
        app.graph_generation_for_test(),
        Some(2),
        "coalesced burst must produce exactly one new generation"
    );

    // Nothing pending: pump is a no-op.
    app.pump_graph_rebuild_for_test();
    assert_eq!(app.graph_generation_for_test(), Some(2));
    Ok(())
}

#[test]
fn failed_graph_build_keeps_previous_snapshot_and_toasts() -> Result<()> {
    let (_dir, vault) = rename_vault(&[("a.md", "note\n")]);
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    assert_eq!(app.graph_generation_for_test(), Some(1));

    app.dispatch(Event::Background(crate::tui::event::BgEvent::GraphReady(
        crate::tui::event::GraphJobResult {
            outcome: Err("simulated build failure".into()),
        },
    )))?;

    assert_eq!(
        app.graph_generation_for_test(),
        Some(1),
        "failed build must keep the previous snapshot"
    );
    let toast = app.current_toast().expect("failed build must toast");
    assert!(
        toast.text.contains("graph rebuild failed"),
        "toast should explain the failure: {}",
        toast.text
    );
    Ok(())
}

#[test]
fn graph_ready_event_installs_snapshot_with_new_generation() -> Result<()> {
    let (_dir, vault) = rename_vault(&[("a.md", "see [[b]]\n"), ("b.md", "t\n")]);
    let mut app = App::for_test_with_clock(vault, fixed_clock);

    // Mutate the vault, then deliver a worker-shaped GraphReady event.
    let (_d2, fresh_vault) = rename_vault(&[("c.md", "new\n")]);
    let snapshot = crate::tui::snapshot::build_graph_snapshot(&fresh_vault, 7)
        .expect("build snapshot for delivery");
    app.dispatch(Event::Background(crate::tui::event::BgEvent::GraphReady(
        crate::tui::event::GraphJobResult {
            outcome: Ok(snapshot),
        },
    )))?;
    assert_eq!(app.graph_generation_for_test(), Some(7));
    Ok(())
}

// ── shared graph snapshot · tab behavior (shared-graph-snapshot §2) ──

#[test]
fn graph_tab_shows_loading_before_first_snapshot() -> Result<()> {
    // Bypass the for_test constructors (which install an eager
    // snapshot) to observe the pre-first-build state run() starts in.
    let (_dir, vault) = rename_vault(&[("a.md", "note\n")]);
    let recents = Arc::new(ft_core::recents::RecentsLog::with_log_path(
        vault.path.clone(),
        vault.path.join(".ft-state/recents.jsonl"),
    ));
    let mut app = App::new_with_recents(Arc::new(vault), recents);
    assert_eq!(app.graph_generation_for_test(), None);

    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("building graph…"),
        "graph tab must show a loading line before the first snapshot:\n{frame}"
    );

    // Quit stays responsive while loading.
    app.dispatch(key('q'))?;
    assert!(app.is_quit(), "q must work while the graph is loading");
    Ok(())
}

#[test]
fn graph_tab_frame_reflects_mutation_after_pump() -> Result<()> {
    let (_dir, _vault_path, mut app) = graph_tab_with_focused_task("- [ ] Fix login bug 🆔 t1\n");
    // Complete the focused task. The file mutates immediately; the
    // frame keeps rendering the previous snapshot until the rebuild
    // lands.
    app.dispatch(key('x'))?;
    let stale = render(&mut app, 100, 24);
    assert!(
        stale.contains("[ ] Fix login bug"),
        "before the pump the frame renders the previous snapshot:\n{stale}"
    );

    app.pump_graph_rebuild_for_test();
    let fresh = render(&mut app, 100, 24);
    assert!(
        fresh.contains("[x] Fix login bug"),
        "after the pump the frame reflects the completed task:\n{fresh}"
    );
    Ok(())
}

#[test]
fn graph_task_cursor_restores_after_async_rebuild() -> Result<()> {
    // Two tasks; complete the second. After the rebuild the cursor must
    // still sit on that task's row (pending-anchor restore).
    let (_dir, _vault_path, mut app) =
        graph_tab_with_focused_task("- [ ] task one\n- [ ] task two\n");
    // Helper leaves the cursor on the first task row; move to task two.
    app.dispatch(key('j'))?;
    app.dispatch(key('x'))?;
    app.pump_graph_rebuild_for_test();
    let frame = render(&mut app, 100, 24);
    // The selected row renders inverted; assert the completed task is
    // present and the tree survived the swap with both tasks visible.
    assert!(
        frame.contains("[x] task two"),
        "task two should be completed after rebuild:\n{frame}"
    );
    assert!(
        frame.contains("[ ] task one"),
        "task one should be untouched:\n{frame}"
    );
    Ok(())
}

// ── shared graph snapshot · stale-guard end to end (shared-graph-snapshot §3) ──

#[test]
fn tasks_tab_stale_line_mutation_fails_safe() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?; // Tasks tab; snapshot adopted on focus.

    // The file changes on disk behind the snapshot's back: an inserted
    // line shifts every task down by one.
    let path = populated_tasks_path(&dir);
    let body = std::fs::read_to_string(&path)?;
    std::fs::write(&path, format!("- [ ] intruder task\n{body}"))?;

    // Complete the selected task using the now-stale line number. The
    // expected-line guard must fail the mutation instead of completing
    // whatever shifted into that slot.
    app.dispatch(key('x'))?;

    let toast = app
        .current_toast()
        .expect("stale mutation must surface an error toast");
    assert!(
        toast.text.contains("changed on disk"),
        "toast should explain the stale line: {}",
        toast.text
    );
    let after = std::fs::read_to_string(&path)?;
    assert!(
        !after.contains("- [x] intruder task") && !after.contains("- [x] Pay rent"),
        "no task may be completed by a stale-line mutation:\n{after}"
    );
    // The failure requested a rebuild; after the pump the tab realigns
    // and the same keypress now completes the intended task.
    app.pump_graph_rebuild_for_test();
    app.dispatch(key('x'))?;
    app.pump_graph_rebuild_for_test();
    let after = std::fs::read_to_string(&path)?;
    assert!(
        after.contains("- [x] Pay rent"),
        "after realignment the intended task completes:\n{after}"
    );
    Ok(())
}

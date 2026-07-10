//! Graph tab: tree navigation, multi-select, rename/move/delete
//! flows, task interaction, related modal, frame snapshots.

use super::*;
use crate::tui::tab::ToastStyle;

// ── Graph tab (plan 019) ─────────────────────────────────────────────

fn dirs_vault_for_graph() -> (TempDir, Vault) {
    // Copy the workspace dirs fixture into a TempDir so the Vault
    // points at a writable path with the same shape (and so test
    // ordering doesn't matter).
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests/fixtures/dirs");
    let dir = TempDir::new().unwrap();
    let dst = dir.path().join("vault");
    copy_dir_recursive(&src, &dst).unwrap();
    let vault = Vault::discover(Some(dst)).unwrap();
    (dir, vault)
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_child = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_child)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), &dst_child)?;
        }
    }
    Ok(())
}

#[test]
fn graph_tab_empty_default_query_renders() -> Result<()> {
    // Vault with no notes — directory-contains expansion finds nothing
    // below the root, so the tree is just the root.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    // GraphTab is the active tab from construction; bounce through tab 1
    // and back so on_focus fires (switch_tab is a no-op when idx == active).
    app.switch_to(1)?;
    app.switch_to(0)?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("graph_tab_empty_default_query_80x24", frame);
    Ok(())
}

#[test]
fn graph_tab_populated_default_query_renders() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("graph_tab_populated_default_query_80x24", frame);
    Ok(())
}

/// §0 baseline regression: after migrating the query bar onto
/// `EditBuffer`, every pre-migration edit key (Char insert, Backspace,
/// Delete, Left, Right, Home, End) still produces the same visible
/// query text the old hand-rolled handler did. New readline bindings
/// land in §2 — this test only guards the migration.
#[test]
fn graph_tab_query_bar_basic_editing_preserved_after_migration() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;

    // Open the query bar and clear any default-seeded query.
    app.dispatch(key('/'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..300 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    // Type a known string: "abcdef".
    for c in "abcdef".chars() {
        app.dispatch(key(c))?;
    }
    // Home then two Rights → cursor between 'b' and 'c'. Insert 'X'.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Right,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Right,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('X'))?;
    // End, then Backspace → drop trailing 'f'.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Backspace,
        KeyModifiers::NONE,
    )))?;
    // Home then Delete → drop leading 'a'.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Delete,
        KeyModifiers::NONE,
    )))?;

    let frame = render(&mut app, 80, 24);
    let query_line = frame
        .lines()
        .find(|l| l.contains("bXcde"))
        .unwrap_or_else(|| panic!("expected query bar to contain 'bXcde':\n{frame}"));
    assert!(
        query_line.contains("> bXcde"),
        "query line should be '> bXcde':\n{query_line}"
    );
    assert!(
        !query_line.contains('a') || query_line.matches('a').count() == 0,
        "leading 'a' should have been Delete-d:\n{query_line}"
    );
    assert!(
        !query_line.contains('f'),
        "trailing 'f' should have been Backspace-d:\n{query_line}"
    );

    // Esc closes the modal without applying (cancel path).
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    Ok(())
}

/// §2 wired EDIT_KEYMAP into the buffer. Readline chords that were
/// previously dropped by the modal forwarder now reach the buffer and
/// rewrite the query text.
#[test]
fn graph_tab_query_bar_ctrl_a_e_k_work_after_keymap_wired() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;

    app.dispatch(key('/'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..400 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "alpha beta gamma".chars() {
        app.dispatch(key(c))?;
    }

    // Ctrl+A → cursor to start; type 'Z' → 'Z' lands at position 0.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )))?;
    app.dispatch(key('Z'))?;

    // Ctrl+E → cursor to end; Ctrl+K with nothing to kill → no-op.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('k'),
        KeyModifiers::CONTROL,
    )))?;

    let frame = render(&mut app, 80, 24);
    let line = frame
        .lines()
        .find(|l| l.contains("Zalpha"))
        .unwrap_or_else(|| panic!("expected query bar to contain 'Zalpha':\n{frame}"));
    assert!(
        line.contains("> Zalpha beta gamma"),
        "Ctrl+A then 'Z' should have inserted 'Z' at the start:\n{line}"
    );

    Ok(())
}

/// §2 alt-keys: word-jump and word-kill bindings work in the graph
/// query bar.
#[test]
fn graph_tab_query_bar_alt_bindings_work() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;

    app.dispatch(key('/'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..400 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "foo bar baz".chars() {
        app.dispatch(key(c))?;
    }

    // Alt+B → cursor jumps from end (11) back to start of `baz` (8).
    // Then Alt+D kills the word forward ("baz"), leaving "foo bar ".
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('b'),
        KeyModifiers::ALT,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('d'),
        KeyModifiers::ALT,
    )))?;

    let frame = render(&mut app, 80, 24);
    let line = frame
        .lines()
        .find(|l| l.contains("foo bar"))
        .unwrap_or_else(|| panic!("expected query bar to contain 'foo bar':\n{frame}"));
    assert!(
        !line.contains("baz"),
        "Alt+B then Alt+D should have killed `baz`:\n{line}"
    );

    // Ctrl+Y should yank `baz` back into the buffer.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("foo bar baz"),
        "Ctrl+Y should yank `baz` back from the kill ring:\n{frame}"
    );

    Ok(())
}

/// §6 modal-driver precedence: with a completion popup open on the
/// graph query bar, `Esc` dismisses the popup (modal stays open);
/// pressing `Esc` again closes the modal. Wires the host_popup_open
/// signal end-to-end: GraphTab → TabCtx → QueryBar::handle_event.
#[test]
fn graph_tab_query_bar_esc_dismisses_popup_before_modal() -> Result<()> {
    use crate::tui::widgets::{
        CompletionContext, CompletionItem, CompletionKind, CompletionProvider, TriggerSet,
    };

    /// Local provider for this test (the StubProvider in
    /// `completion::tests` is `pub(crate)` to the widgets module, so
    /// we re-declare a minimal one here).
    #[derive(Debug)]
    struct LocalStub;
    impl CompletionProvider for LocalStub {
        fn complete(&mut self, _ctx: &CompletionContext) -> Vec<CompletionItem> {
            vec![CompletionItem {
                label: "node".into(),
                insert_text: "node".into(),
                replace_span: None,
                kind: CompletionKind::Keyword,
                description: None,
            }]
        }
        fn trigger_on(&self) -> TriggerSet {
            TriggerSet::printable()
        }
    }

    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    // Attach the provider before opening the modal — the provider
    // sits on the view's buffer and gets queried on each char insert.
    app.set_focused_buffer_completion_for_test(Box::new(LocalStub));

    app.dispatch(key('/'))?;
    // Clear the default-seeded query, then type a char so the
    // provider returns its item and the popup opens.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..400 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    app.dispatch(key('n'))?;

    // First Esc: popup is open, so QueryBar forwards Esc through the
    // buffer, the popup dismisses, the modal stays open.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    assert_eq!(
        app.active_modal_name(),
        Some("query-bar"),
        "first Esc with popup open should dismiss the popup, not the modal"
    );

    // Second Esc: popup is closed (host_popup_open = false), so
    // QueryBar handles Esc itself and closes.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    assert_eq!(
        app.active_modal_name(),
        None,
        "second Esc with no popup should close the modal"
    );

    Ok(())
}

#[test]
fn graph_tab_o_opens_selected_note_in_editor() -> Result<()> {
    // dirs fixture has Areas/finance.md, Projects/alpha.md, root.md, and
    // Areas/operations/shifts.md. Drive: open default → expand root →
    // walk to a note → press 'o'.
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    // Expand the root directory so the children show up.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Move selection past the root onto its first child. Children are
    // listed in graph order; the dirs fixture inserts Projects/, Areas/,
    // root.md (3 children). Walk down until we land on a note (.md).
    for _ in 0..5 {
        app.dispatch(key('j'))?;
        if let Some(AppRequest::OpenInEditor { .. }) = app.take_pending_request() {
            // Already opened — shouldn't happen, but guard.
            panic!("open fired before pressing o");
        }
        // Try pressing 'o' — silently no-ops on a Directory row.
        app.dispatch(key('o'))?;
        if let Some(req) = app.take_pending_request() {
            match req {
                AppRequest::OpenInEditor { path, line } => {
                    let s = path.to_string_lossy();
                    assert!(s.ends_with(".md"), "expected an .md note path, got {s}");
                    assert_eq!(line, 1);
                    return Ok(());
                }
                other => panic!("unexpected pending request: {other:?}"),
            }
        }
    }
    panic!("did not reach a note row after 5 j-presses");
}

/// graph-task-interaction §7: `x` on a Task row completes it on disk and
/// refreshes the graph; `o` on a Task row opens its source note at the line.
#[test]
fn graph_tab_x_completes_focused_task() -> Result<()> {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    // A query that surfaces the note's tasks: root note + has-task expansion.
    std::fs::write(
        vault_path.join("root.md"),
        "- [ ] Fix login bug 📅 2026-05-09 🆔 t1\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(vault_path.clone())).unwrap();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    // Bounce through another tab so switch_to(0) actually fires on_focus
    // (switch_to to the already-active tab is a no-op).
    app.switch_to(1)?;
    app.switch_to(0)?; // Graph tab

    // Sanity: the default query renders the root directory.
    let baseline = render(&mut app, 80, 24);
    assert!(
        baseline.contains("D /"),
        "default query should show the root directory:\n{baseline}"
    );

    // Set the query to show tasks: root note expanded via has-task+subtask.
    // Clear the default query first.
    app.dispatch(key('/'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..300 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "node where kind = Note and path = \"root.md\"; expand where edge.kind in {has-task, subtask} and to.kind in {Task};".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // Expand the root note so its task child appears.
    app.dispatch(key('j'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Cursor should now be on (or able to reach) the task row. Walk down
    // until we see the task row focused. The tree shows the task as a
    // child of the note; press j to descend onto it.
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Fix login bug"),
        "task row should be visible after expanding the note:\n{frame}"
    );
    app.dispatch(key('j'))?;

    // Press `x` to complete the focused task.
    app.dispatch(key('x'))?;
    // Drain any pending request (the completion path may post none).
    let _ = app.take_pending_request();

    let content = std::fs::read_to_string(vault_path.join("root.md"))?;
    assert!(
        content.contains("[x]"),
        "expected the task to be marked done on disk, got: {content}"
    );
    assert!(
        content.contains("✅ 2026-05-10"),
        "expected today's done-date (fixed_clock), got: {content}"
    );
    Ok(())
}

#[test]
fn graph_tab_e_edits_focused_task_due_date() -> Result<()> {
    let (_dir, vault_path, mut app) = graph_tab_with_focused_task("- [ ] Fix login bug 🆔 t1\n");
    // `e` opens the edit popup on the focused task.
    app.dispatch(key('e'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("edit task"),
        "e should open the edit-task popup:\n{frame}"
    );
    // Tab to the due field (description → due in edit-mode field order),
    // type a date, and Ctrl+S to save.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)))?; // → due
    for c in "2026-07-01".chars() {
        app.dispatch(key(c))?;
    }
    let pre_save = render(&mut app, 80, 24);
    assert!(
        pre_save.contains("2026-07-01"),
        "due field should show the typed date before save:\n{pre_save}"
    );
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    )))?; // Ctrl+S save
          // Disk-mutating requests go through service_request, not drain_simple.
    app.service_pending_requests()?;
    let _ = app.take_pending_request();

    let content = std::fs::read_to_string(vault_path.join("root.md"))?;
    assert!(
        content.contains("📅 2026-07-01"),
        "expected the due date to be saved, got: {content}"
    );
    Ok(())
}

#[test]
fn graph_tab_v_rewrites_view_to_note_tasks() -> Result<()> {
    let (_dir, _vault_path, mut app) = graph_tab_with_focused_task("- [ ] Fix login bug 🆔 t1\n");
    // Move back up to the note row, then press `v`.
    app.dispatch(key('k'))?; // note row
    app.dispatch(key('v'))?;
    // The query rewrites to the note-scoped task view; expand the note so
    // its task appears.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Fix login bug"),
        "v should show the note's task subtree:\n{frame}"
    );
    // The query bar should now reflect the note-scoped query.
    assert!(
        frame.contains("kind = Note and path"),
        "query bar should show the note-scoped query:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_tab_a_then_esc_cancels_leader() -> Result<()> {
    let (_dir, _vault_path, mut app) = graph_tab_with_focused_task("- [ ] Fix login bug 🆔 t1\n");
    // `a` opens the task-create leader modal.
    app.dispatch(key('a'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("c=create") && frame.contains("s=subtask"),
        "a should open the task leader:\n{frame}"
    );
    // Esc cancels.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame2 = render(&mut app, 80, 24);
    assert!(
        !frame2.contains("c=create"),
        "Esc should close the leader:
{frame2}"
    );
    Ok(())
}

#[test]
fn graph_tab_a_c_creates_top_level_task() -> Result<()> {
    let (_dir, vault_path, mut app) = graph_tab_with_focused_task("- [ ] Fix login bug 🆔 t1\n");
    // `a` → `c` opens the create popup, seeded with the focused task's note.
    app.dispatch(key('a'))?;
    app.dispatch(key('c'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("new task"),
        "a→c should open the create popup:\n{frame}"
    );
    // The target field is pre-seeded from the focused note.
    assert!(
        frame.contains("root.md"),
        "create popup should seed the target with the focused note:\n{frame}"
    );
    // Description is focused by default; type one and Ctrl+S to create.
    for c in "Write the tests".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    )))?;
    app.service_pending_requests()?;
    let _ = app.take_pending_request();

    let content = std::fs::read_to_string(vault_path.join("root.md"))?;
    assert!(
        content.contains("- [ ] Write the tests"),
        "expected the new top-level task on disk, got: {content}"
    );
    Ok(())
}

#[test]
fn graph_tab_a_s_creates_subtask() -> Result<()> {
    let (_dir, vault_path, mut app) = graph_tab_with_focused_task("- [ ] Fix login bug 🆔 t1\n");
    // `a` → `s` creates a subtask under the focused task.
    app.dispatch(key('a'))?;
    app.dispatch(key('s'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("new task"),
        "a→s should open the create popup:\n{frame}"
    );
    for c in "Reproduce the bug".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    )))?;
    app.service_pending_requests()?;
    let _ = app.take_pending_request();

    let content = std::fs::read_to_string(vault_path.join("root.md"))?;
    // The subtask lands indented directly beneath its parent (line 1).
    assert!(
        content.contains("  - [ ] Reproduce the bug"),
        "expected the new subtask indented under its parent, got: {content}"
    );
    Ok(())
}

#[test]
fn graph_tab_a_s_on_non_task_toasts() -> Result<()> {
    let (_dir, _vault_path, mut app) = graph_tab_with_focused_task("- [ ] Fix login bug 🆔 t1\n");
    // Move up to the note row (not a task), then `a` → `s` should refuse.
    app.dispatch(key('k'))?;
    app.dispatch(key('a'))?;
    app.dispatch(key('s'))?;
    app.service_pending_requests()?; // flush the toast request
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("new task"),
        "a→s on a non-task row must not open the create popup:\n{frame}"
    );
    assert!(
        frame.contains("select a task first"),
        "a→s on a non-task row should toast:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_tab_ctrl_o_opens_selected_note_in_obsidian() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let ctrl_o = Event::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    for _ in 0..5 {
        app.dispatch(key('j'))?;
        app.dispatch(ctrl_o.clone())?;
        if let Some(req) = app.take_pending_request() {
            match req {
                AppRequest::OpenInObsidian { url } => {
                    assert!(
                        url.starts_with("obsidian://"),
                        "expected obsidian URL, got {url}"
                    );
                    return Ok(());
                }
                other => panic!("unexpected pending request: {other:?}"),
            }
        }
    }
    panic!("did not reach a note row after 5 j-presses");
}

#[test]
fn graph_tab_input_mode_consumes_digits_instead_of_switching_tabs() -> Result<()> {
    // Bug repro for ext: pressing a digit while typing a query used to
    // switch tabs because the digit-passthrough check ran before the
    // input-mode check.
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('/'))?; // Enter input mode.
                             // Wipe pre-seeded default; type a query containing digits.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..200 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "node where indegree = 0;".chars() {
        app.dispatch(key(c))?;
    }
    // Should still be on the Graph tab.
    assert_eq!(app.active_title(), "Graph");
    Ok(())
}

#[test]
fn graph_tab_input_mode_shows_cursor() -> Result<()> {
    // Cursor position lives on the backend, not in the rendered cells —
    // assert against `get_cursor_position` directly. Also snapshot the
    // rendered frame to lock in the yellow prompt + hidden-hint state
    // for input mode.
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('/'))?;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render_to(f)).unwrap();
    let cursor = terminal
        .backend_mut()
        .get_cursor_position()
        .expect("input mode should position the cursor");
    // The default query is pre-seeded; cursor sits at end of text +
    // 2 (the "> " prompt offset). The test just asserts the cursor is
    // on the input bar (top row of body) and past the prompt. Exact column
    // depends on default-query length which is intentional UI state.
    // The 80×24 frame's tab bar is row 0; the graph tab's input bar now
    // sits on row 1 (top of body area).
    assert_eq!(
        cursor.y, 1,
        "cursor must be on the input bar row (now at top)"
    );
    assert!(
        cursor.x >= 2,
        "cursor must be past the `> ` prompt (got x={})",
        cursor.x
    );
    let buf = terminal.backend().buffer().clone();
    let frame = buffer_to_string(&buf);
    assert_tui_snapshot!("graph_tab_input_mode_80x24", frame);
    Ok(())
}

#[test]
fn graph_tab_strip_renders_two_views_active_highlighted() -> Result<()> {
    // Default vault → focus the Graph tab so view 0 picks up the
    // builtin default query, then Ctrl+N to spawn a second empty
    // view and Esc to leave its input mode. The tab strip should
    // show both views with view 2 (the new one) highlighted active.
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("graph_tab_strip_two_views_80x24", frame);
    Ok(())
}

#[test]
fn graph_tab_alt_digit_switches_active_view() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    // Spawn three extra views (Ctrl+N drops into input mode each time;
    // Esc returns to normal). After this the tab strip has 4 views and
    // active = 3.
    for _ in 0..3 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char('n'),
            KeyModifiers::CONTROL,
        )))?;
        app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    }
    // Alt+1 jumps back to view 0.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('1'),
        KeyModifiers::ALT,
    )))?;
    // App must still be on Graph (plain digit would've switched outer
    // tab — confirm Alt-digit was consumed locally).
    assert_eq!(app.active_title(), "Graph");
    Ok(())
}

#[test]
fn graph_tab_expansion_survives_refresh() -> Result<()> {
    // Open the dirs fixture, expand the root, refresh — root should
    // still be expanded afterwards. Direct regression for the
    // editor-return tree-collapse bug.
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    // Expand the root.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let before = render(&mut app, 80, 24);
    assert!(
        before.contains("▼   D /"),
        "root should be expanded before refresh"
    );
    // Refresh rebuilds the graph and re-derives the tree from spec.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
    )))?;
    let after = render(&mut app, 80, 24);
    assert!(
        after.contains("▼   D /"),
        "root should still be expanded after refresh — got:\n{after}"
    );
    Ok(())
}

#[test]
fn graph_tab_preset_picker_opens_on_ctrl_n() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    // Ctrl+N opens the preset picker (built-in presets are available).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("graph_tab_preset_picker_open", frame);
    Ok(())
}

// 7.7 — end-to-end: open Graph, press `f`, type a leaf name from a
// deep directory, press Enter, verify the cursor lands on that node
// and ancestors are expanded.
#[test]
fn graph_f_opens_search_picker_and_jumps_to_typed_target() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;

    // Default tree has only the root row; pressing `f` opens the
    // search picker over the dirs subgraph (root, Areas/, Projects/,
    // Areas/operations/, the notes, …).
    app.dispatch(key('f'))?;
    let opened = render(&mut app, 80, 24);
    assert!(
        opened.contains("find"),
        "search picker should be open and show its 'find' chrome — got:\n{opened}"
    );

    // Type "shifts" → matches the deep note Areas/operations/shifts.md.
    for c in "shifts".chars() {
        app.dispatch(key(c))?;
    }
    let typed = render(&mut app, 80, 24);
    assert!(
        typed.contains("shifts"),
        "picker rows should include the typed target — got:\n{typed}"
    );

    // Enter → jump.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let after = render(&mut app, 80, 24);

    // Picker is closed (no more "find" border title), root is expanded
    // (▼ on the `/` row), Areas/ is expanded (▼ on the `Areas/` row),
    // and the highlighted (selected) row in the tree carries "shifts".
    assert!(
        !after.contains(" find "),
        "search picker should be closed after Enter — got:\n{after}"
    );
    assert!(
        after.contains("▼   D /"),
        "root directory should be expanded — got:\n{after}"
    );
    assert!(
        after.contains("▼") && after.contains("Areas/"),
        "Areas/ should be expanded — got:\n{after}"
    );
    assert!(
        after.contains("shifts"),
        "the target row should be visible — got:\n{after}"
    );
    Ok(())
}

// 7.8 — f then Esc leaves the view's spec unchanged.
#[test]
fn graph_f_then_esc_leaves_view_unchanged() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;

    let before = render(&mut app, 80, 24);
    app.dispatch(key('f'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let after = render(&mut app, 80, 24);

    assert_eq!(
        before, after,
        "f then Esc should round-trip the rendered frame exactly"
    );
    Ok(())
}

// 7.9 — f on an empty tree is a no-op.
#[test]
fn graph_f_on_empty_tree_is_noop() -> Result<()> {
    // test_vault has no notes → BUILTIN_DEFAULT_QUERY's expand finds
    // nothing below the root. The tree is still "non-empty" (the root
    // dir row exists), so to truly test the empty-tree guard we clear
    // the query via Ctrl+W (close view replaces with empty), then
    // press `f`.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    // Ctrl+W closes the current view; since it's the only view, it
    // gets replaced with a fresh empty view (no parsed query).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    )))?;
    let before = render(&mut app, 80, 24);
    app.dispatch(key('f'))?;
    let after = render(&mut app, 80, 24);
    assert_eq!(
        before, after,
        "f with no parsed query / empty tree must be a no-op"
    );
    Ok(())
}

// 7.10 — snapshot of the search picker overlay.
#[test]
fn graph_f_search_picker_snapshot() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('f'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("graph_tab_search_picker_open", frame);
    Ok(())
}

#[test]
fn graph_c_opens_filename_prompt_seeded_from_directory_selection() -> Result<()> {
    // Default tree starts with the vault root directory selected.
    // Pressing `c` should open the create overlay's FilenamePrompt with
    // folder `.` (root). The popup title carries the folder label so we
    // can assert against the rendered frame.
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('c'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("filename"),
        "create overlay (filename step) should be visible — got:\n{frame}"
    );
    assert!(
        frame.contains(". ") || frame.contains("/ . ") || frame.contains("·  . "),
        "filename prompt title should advertise folder `.` — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_c_opens_filename_prompt_seeded_from_note_selection() -> Result<()> {
    // Expand the root, navigate to a note row, then press `c`. The
    // filename prompt's title bar must include the note's containing
    // directory (Areas or Projects depending on fixture order).
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Walk down until selection lands on a note row (N kind char).
    // Up to 8 steps is plenty for the dirs fixture's depth.
    let mut frame = render(&mut app, 80, 24);
    for _ in 0..8 {
        // Press `c`; if the resulting popup contains a parent-folder
        // marker for a note (e.g. " Areas " or " Projects "), succeed.
        app.dispatch(key('c'))?;
        let popup = render(&mut app, 80, 24);
        if popup.contains("filename") && (popup.contains(" Areas ") || popup.contains(" Projects "))
        {
            return Ok(());
        }
        // Otherwise close (Esc twice — once to folder picker, once to
        // close the flow) and advance.
        app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
        app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
        app.dispatch(key('j'))?;
        frame = render(&mut app, 80, 24);
    }
    panic!("never landed on a note row with a non-empty parent folder; last frame:\n{frame}");
}

#[test]
fn graph_capital_c_opens_template_picker() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains(" create · 1/4 template "),
        "template picker title should be visible — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_capital_c_skips_folder_picker_after_template() -> Result<()> {
    // Use the notes-create fixture so we have real templates. The graph
    // tab pre-seeds the folder from selection (vault root `.` here), so
    // after picking a template the flow must skip `FolderPicking` and
    // land directly in the filename prompt.
    let (_dir, vault) = notes_create_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    // Graph tab is index 0.
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('C'),
        KeyModifiers::SHIFT,
    )))?;
    let picker_frame = render(&mut app, 80, 24);
    assert!(
        picker_frame.contains("1/4 template"),
        "template picker should be open:\n{picker_frame}"
    );
    // Select the first template (Enter). For the graph-tab path this
    // must skip the folder picker and land in the filename prompt.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let next_frame = render(&mut app, 80, 24);
    assert!(
        next_frame.contains("filename"),
        "after template pick on graph tab, expected filename prompt — got:\n{next_frame}"
    );
    assert!(
        !next_frame.contains("folder · "),
        "graph tab must NOT show the folder picker after template — got:\n{next_frame}"
    );
    Ok(())
}

#[test]
fn graph_create_overlay_captures_keys_before_tree_bindings() -> Result<()> {
    // While the create overlay is up, the tree's `j`/`k` should be
    // inert — keypresses go to the picker / edit buffer. Smoke-check by
    // pressing `c`, then pressing `j` a few times, then snapshotting:
    // we should still see the create overlay (no expansion behaviour
    // leaked through to the tree underneath).
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('c'))?;
    for _ in 0..5 {
        app.dispatch(key('j'))?;
    }
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("filename"),
        "create overlay must still own the keyboard — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_create_filename_prompt_snapshot() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('c'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("graph_create_filename_prompt_80x24", frame);
    Ok(())
}

// ── Graph tab · periodic notes (021 · session 3) ──────────────────────

#[test]
fn graph_t_queues_toast_when_daily_unreachable() -> Result<()> {
    // With the default graph query (directory-contains only), the
    // daily note file won't be in the graph results — `t` queues
    // an informational toast instead of opening an editor.
    let (_dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('t'))?;
    let toast = app
        .current_toast()
        .expect("`t` should surface a Toast when daily is unreachable");
    assert!(
        toast.text.contains("daily"),
        "toast should mention daily: {:?}",
        toast.text
    );
    assert!(
        toast.text.contains("not in the current graph results"),
        "toast should explain the note isn't reachable: {:?}",
        toast.text
    );
    assert_eq!(toast.style, crate::tui::tab::ToastStyle::Info);
    Ok(())
}

#[test]
fn graph_p_enters_periodic_leader() -> Result<()> {
    let (_dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('p'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("periodic note · pick a period"),
        "expected leader modal:\n{frame}"
    );
    assert!(
        app.take_pending_request().is_none(),
        "`p` alone must not queue any request"
    );
    Ok(())
}

#[test]
fn graph_p_then_d_navigates_or_toasts() -> Result<()> {
    // With the default graph query, the daily note isn't reachable →
    // a `GraphNavigatePeriodic` request is posted, which the App
    // services as a toast (note not in current graph results).
    let (_dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('p'))?;
    app.dispatch(key('d'))?;
    // The modal posts GraphNavigatePeriodic; service it to trigger the tab's handler.
    app.service_pending_requests()?;
    // The handler queues a Toast in pending_request; service that too.
    app.service_pending_requests()?;
    // Result: toast indicating the daily note is not in graph results.
    let req = app.take_pending_request();
    assert!(
        req.is_none(),
        "toast should have been serviced, got {req:?}"
    );
    // Leader must clear after firing.
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("periodic note · pick a period"),
        "leader modal should be gone after firing:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_p_then_unknown_key_cancels() -> Result<()> {
    let (_dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('p'))?;
    // `x` is not a period — should silently cancel.
    app.dispatch(key('x'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("periodic note · pick a period"),
        "leader modal should be dismissed by an unknown key:\n{frame}"
    );
    assert!(
        app.take_pending_request().is_none(),
        "unknown key must not fire the open flow"
    );
    Ok(())
}

#[test]
fn graph_p_then_esc_cancels() -> Result<()> {
    let (_dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('p'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("periodic note · pick a period"),
        "Esc should dismiss the leader modal:\n{frame}"
    );
    assert!(
        app.take_pending_request().is_none(),
        "Esc from leader must not queue anything"
    );
    Ok(())
}

#[test]
fn graph_periodic_leader_status_snapshot() -> Result<()> {
    let (_dir, vault) = periodic_vault_all_periods();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('p'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("graph_periodic_leader_80x24", frame);
    Ok(())
}

// ── Graph tab · periodic navigation ──────────────────────────────────

/// Vault with daily note actually on disk, so the graph tracks it.
fn periodic_vault_with_daily_file() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::create_dir_all(vault_path.join(".ft")).unwrap();
    std::fs::create_dir_all(vault_path.join("journal/2026")).unwrap();
    std::fs::write(
        vault_path.join(".ft/config.toml"),
        "[periodic_notes.daily]\npath = \"journal/%Y\"\nformat = \"%Y-%m-%d\"\n",
    )
    .unwrap();
    std::fs::write(
        vault_path.join("journal/2026/2026-05-10.md"),
        "# 2026-05-10\n\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn graph_t_navigates_to_daily_when_reachable() -> Result<()> {
    let (_dir, vault) = periodic_vault_with_daily_file();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    // Switch to graph.
    app.switch_to(1)?;
    app.switch_to(0)?;

    // Replace the default query with one that selects all notes
    // (the daily note is a Note, so it will be in the tree).
    app.dispatch(key('/'))?;
    // Clear the pre-seeded query and type a new one.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..200 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "node where kind = Note;".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // Now press `t` — navigate to the daily note within the current tree.
    app.dispatch(key('t'))?;
    let frame = render(&mut app, 80, 24);
    // The daily note should now be visible and selected (≈ highlighted).
    assert!(
        frame.contains("2026-05-10"),
        "daily note should be visible in the tree after navigation:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_p_then_d_navigates_to_daily_when_reachable() -> Result<()> {
    let (_dir, vault) = periodic_vault_with_daily_file();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;

    // Set a query that includes the daily note.
    app.dispatch(key('/'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..200 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "node where kind = Note;".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // p → opens leader, d → navigate to daily note.
    app.dispatch(key('p'))?;
    app.dispatch(key('d'))?;
    // Service the GraphNavigatePeriodic request.
    app.service_pending_requests()?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("2026-05-10"),
        "daily note should be visible after p+d navigation:\n{frame}"
    );
    Ok(())
}

// ── Graph tab · move section (021 · session 5) ────────────────────────

#[test]
fn graph_m_enters_source_from_tree_phase() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('m'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("MOVE source"),
        "source banner should appear after `m`:\n{frame}"
    );
    assert!(
        app.take_pending_request().is_none(),
        "`m` alone must not queue any request"
    );
    Ok(())
}

#[test]
fn graph_m_again_on_directory_emits_toast() -> Result<()> {
    // Default tree starts with the vault root *directory* selected.
    // Pressing `m` then `m` should not advance — the source row needs
    // to be a Note. An error toast surfaces in the App's toast slot
    // (via `OpenModalWithToast` from the host hook) and the source
    // banner stays.
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('m'))?;
    app.dispatch(key('m'))?;
    let toast = app
        .current_toast()
        .expect("`m` on directory should queue an error toast");
    assert_eq!(toast.style, ToastStyle::Error, "toast style: {toast:?}");
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("MOVE source"),
        "should still be in source phase:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_m_again_on_note_opens_heading_multiselect() -> Result<()> {
    // Expand the root and walk to a note row, then press m, m. The
    // shared multiselect popup should appear (its title includes
    // "step 2/4" via the keymap line).
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    // Expand root.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Walk down to find a Note row. Hit a max of 8 rows.
    // First, enter source-from-tree phase.
    app.dispatch(key('m'))?;
    for _ in 0..8 {
        // Try confirming the currently-selected row as source.
        app.dispatch(key('m'))?;
        let req = app.take_pending_request();
        let frame = render(&mut app, 80, 24);
        if frame.contains("move · 2/4 select") {
            return Ok(());
        }
        // Not a Note row — toast was queued and we stayed in source
        // phase. Move down and try again.
        let _ = req;
        app.dispatch(key('j'))?;
    }
    let frame = render(&mut app, 80, 24);
    panic!("never reached heading multi-select after walking 8 rows; last frame:\n{frame}");
}

#[test]
fn graph_move_t_opens_fuzzy_source_picker() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('m'))?;
    app.dispatch(key('t'))?;
    let frame = render(&mut app, 80, 24);
    // The shared move overlay renders SourcePicking with a "pick file"
    // popup title — exact wording from MOVE_STEP_1_KEYS / picker title.
    assert!(
        frame.contains("source") || frame.contains("Select source"),
        "fuzzy source picker should be visible after `m`+`t`:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_move_esc_cancels_source_phase() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    app.dispatch(key('m'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("MOVE source"),
        "Esc should cancel the move flow:\n{frame}"
    );
    Ok(())
}

#[test]
fn no_more_syncing_mode_exists() {
    // Compile-time guard: `Mode::Syncing` was removed in plan 014.
    // If a future change reintroduces a `Syncing` variant, this test
    // will fail to compile (the match would become non-exhaustive),
    // forcing the author to either revive the modal path or update
    // this guard intentionally.
    use crate::tui::ui::Mode;
    fn _enumerate(m: Mode) -> u8 {
        match m {
            Mode::Normal => 0,
            Mode::Help => 1,
            Mode::GitLeader => 2,
            Mode::SyncConflict => 3,
        }
    }
}

#[test]
fn graph_tab_z_roots_on_selected_note() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    // Expand the root directory so children show up.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Navigate past root to first child, press z. Children are Areas/,
    // Projects/, root.md. Pressing z on any of them rewrites the query.
    app.dispatch(key('j'))?;
    app.dispatch(key('z'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("graph_tab_z_rooted_on_note", frame);
    Ok(())
}

// ── Graph tab · rename/move (plan graph-tui-note-rename) ─────────────

// ── Multi-select tests ───────────────────────────────────────────────

#[test]
fn graph_space_toggles_selection_on_note() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    // Expand root.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Navigate to a Note row (3rd child is root.md, a Note).
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    // Verify we're on a Note row by pressing Space and checking for ●.
    // Press Space to toggle.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains('●'),
        "● selection marker expected — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_space_toggles_selection_on_directory() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // First child is a directory (Areas). Space should add marker.
    app.dispatch(key('j'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains('●'),
        "● expected on directory row — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_space_is_noop_on_root() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    // Root is the first row. Space should not add marker.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains('●'),
        "no ● expected on root — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_space_toggles_off() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Navigate to Note row (3 j presses).
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    // Space on, then Space off.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains('●'),
        "marker should be gone after toggle-off — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_esc_clears_all_selections() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Navigate to Note row.
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    // Esc clears.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains('●'),
        "selection marker should be gone after Esc — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_esc_empty_selection_passes_through() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    // With nothing selected, Esc should not change state.
    let before = render(&mut app, 80, 24);
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let after = render(&mut app, 80, 24);
    assert_eq!(before, after, "Esc with empty selection should be a no-op");
    Ok(())
}

#[test]
fn graph_selections_survive_ctrl_r_refresh() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    let before = render(&mut app, 80, 24);
    assert!(
        before.contains('●'),
        "Space should drop a selection marker — got:\n{before}"
    );
    // Ctrl+r refresh. The marked node still exists on disk, so the
    // marker is rehydrated against the rebuilt graph and stays.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains('●'),
        "selection marker should survive Ctrl+R refresh — got:\n{frame}"
    );
    Ok(())
}

// ── Flow B · rename note tests ───────────────────────────────────────

#[test]
fn graph_r_on_note_opens_rename_modal() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Navigate to Note row.
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('r'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Rename note"),
        "rename modal title expected — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_r_on_note_renames_file_on_disk() -> Result<()> {
    let (dir, vault) = rename_vault(&[("foo.md", "# Foo\n"), ("a.md", "see [[foo]] now\n")]);
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Navigate to the a.md row.
    app.dispatch(key('j'))?;
    app.dispatch(key('r'))?;
    // Clear pre-filled text and type new name.
    for _ in 0..10 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "bar".chars() {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // After commit, a.md → bar.md. Original content references foo.
    let vault_path = dir.path().join("vault");
    assert!(!vault_path.join("a.md").exists(), "a.md should be gone");
    assert!(vault_path.join("bar.md").exists(), "bar.md should exist");
    let bar_content = std::fs::read_to_string(vault_path.join("bar.md")).unwrap();
    assert!(
        bar_content.contains("see [[foo]]"),
        "bar.md should contain the original link"
    );
    Ok(())
}

#[test]
fn graph_r_empty_name_shows_error() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('r'))?;
    // Clear pre-filled text.
    for _ in 0..20 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Rename note"),
        "modal should stay open on empty name — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_r_slash_in_name_shows_error() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('r'))?;
    // Clear and type "a/b".
    for _ in 0..20 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "a/b".chars() {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Rename note"),
        "modal should stay open on slash in name — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_r_on_ghost_toasts() -> Result<()> {
    let (dir, vault) = rename_vault(&[("a.md", "see [[Phantom]]\n")]);
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    // Expand root to see ghost children.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Navigate to a ghost row (G kind char).
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('r'))?;
    let frame = render(&mut app, 80, 24);
    // Should not show rename modal.
    assert!(
        !frame.contains("Rename note"),
        "rename modal should not open for ghost — got:\n{frame}"
    );
    let _ = dir;
    Ok(())
}

#[test]
fn graph_esc_in_rename_modal_closes() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('r'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("Rename note"),
        "rename modal should be closed after Esc — got:\n{frame}"
    );
    Ok(())
}

// ── Flow B · rename directory tests ──────────────────────────────────

#[test]
fn graph_r_on_directory_opens_rename_modal() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    // Expand root to see subdirectories.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // First child is a directory (Areas).
    app.dispatch(key('j'))?;
    app.dispatch(key('r'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Rename directory"),
        "rename directory modal title expected — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_r_on_root_toasts() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    // Root is the first row. Press r.
    app.dispatch(key('r'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("Rename directory"),
        "rename modal should NOT open for root — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_rename_directory_moves_files_and_updates_refs() -> Result<()> {
    let (dir, vault) = rename_vault(&[
        ("docs/guide.md", "# Guide\n"),
        ("docs/ref.md", "# Reference\n"),
        ("external.md", "see [[docs/guide]] for help\n"),
    ]);
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Navigate to docs/ directory (first child after root).
    app.dispatch(key('j'))?;
    app.dispatch(key('r'))?;
    // Clear and type new name.
    for _ in 0..10 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "manual".chars() {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let vault_path = dir.path().join("vault");
    assert!(!vault_path.join("docs").exists());
    assert!(vault_path.join("manual/guide.md").exists());
    assert!(vault_path.join("manual/ref.md").exists());
    let ext = std::fs::read_to_string(vault_path.join("external.md")).unwrap();
    assert!(ext.contains("[[manual/guide]]"));
    Ok(())
}

#[test]
fn graph_rename_directory_target_exists_errors() -> Result<()> {
    let (_dir, vault) = rename_vault(&[
        ("docs/guide.md", "# Guide\n"),
        ("manual/other.md", "# Other\n"),
    ]);
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('r'))?;
    for _ in 0..10 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "manual".chars() {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    // Modal should stay open on error.
    assert!(
        frame.contains("Rename directory"),
        "rename modal should stay open on target-exists error — got:\n{frame}"
    );
    Ok(())
}

// ── Flow A · move tests ──────────────────────────────────────────────

#[test]
fn graph_r_with_selections_enters_move_phase() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    // Space-select a note.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    // r with selection → move phase.
    app.dispatch(key('r'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Move 1 selection(s)"),
        "move banner expected — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_move_enter_on_directory_executes_move() -> Result<()> {
    // Move a note to an existing subdirectory.
    let (dir, vault) = rename_vault(&[
        ("foo.md", "# Foo\n"),
        ("sub/placeholder.md", "# placeholder\n"),
    ]);
    let vault_path = dir.path().join("vault");
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    // Expand root.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Navigate to foo.md (2nd child after root D — appears after sub/).
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    // Space-select.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    // r enters move phase.
    app.dispatch(key('r'))?;
    // Navigate to sub/ directory row and confirm with Enter.
    // Children: root(D,0), sub(D,1), foo(N,2).
    app.dispatch(key('k'))?; // up to sub (D, index 1)
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Verify file moved.
    assert!(!vault_path.join("foo.md").exists());
    assert!(vault_path.join("sub/foo.md").exists());
    Ok(())
}

#[test]
fn graph_move_two_notes_preserves_relative_md_link() -> Result<()> {
    // x.md links to y.md via [other note](y.md). Move both to sub/.
    // Since they stay in the same directory, the relative link is unchanged.
    let (dir, vault) = rename_vault(&[
        ("x.md", "# X\nsee [other note](y.md)\n"),
        ("y.md", "# Y\n"),
        ("sub/placeholder.md", "# placeholder\n"),
    ]);
    let vault_path = dir.path().join("vault");
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    // Expand root.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Children: root(D,0), sub(D,1), x(N,2), y(N,3) — alphabetical.
    // Navigate to x(N) and Space-select.
    app.dispatch(key('j'))?; // sub(D,1)
    app.dispatch(key('j'))?; // x(N,2)
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?; // y(N,3)
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    // r enters move phase.
    app.dispatch(key('r'))?;
    // Navigate to sub(D) and confirm.
    app.dispatch(key('k'))?; // up to x(N,2)
    app.dispatch(key('k'))?; // up to sub(D,1)
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Verify files moved.
    assert!(!vault_path.join("x.md").exists());
    assert!(!vault_path.join("y.md").exists());
    assert!(vault_path.join("sub/x.md").exists());
    assert!(vault_path.join("sub/y.md").exists());
    // Relative markdown link should be unchanged (both in same dir).
    let x_content = std::fs::read_to_string(vault_path.join("sub/x.md")).unwrap();
    assert!(
        x_content.contains("[other note](y.md)"),
        "relative md link should stay [other note](y.md), got:\n{x_content}"
    );
    Ok(())
}

#[test]
fn graph_move_enter_on_note_toasts_and_stays() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('r'))?;
    // Try to confirm on a Note row (not a directory).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    // Banner should still be visible (phase stays active).
    assert!(
        frame.contains("Move 1 selection(s)"),
        "move banner should still be visible after bad target — got:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_move_esc_cancels_flow() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('r'))?;
    // Esc cancels.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("Move 1 selection(s)"),
        "move banner should be gone after Esc — got:\n{frame}"
    );
    assert!(
        !frame.contains('●'),
        "selection marker should be cleared — got:\n{frame}"
    );
    Ok(())
}

// ── Snapshot tests ───────────────────────────────────────────────────

#[test]
fn graph_rename_note_modal_snapshot() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('r'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("graph_rename_note_modal_80x24", frame);
    Ok(())
}

#[test]
fn graph_move_target_banner_snapshot() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('r'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("graph_move_target_banner_80x24", frame);
    Ok(())
}

#[test]
fn graph_multi_select_markers_snapshot() -> Result<()> {
    let (_dir, vault) = dirs_vault_for_graph();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("graph_multi_select_markers_80x24", frame);
    Ok(())
}

// ── Related-section modal ────────────────────────────────────────────

#[test]
fn graph_related_modal_opens_on_initial_action() -> Result<()> {
    let (_dir, vault) = related_modal_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.set_initial_action(Some(crate::tui::InitialAction::OpenRelatedModal {
        note_path: std::path::PathBuf::from("N.md"),
    }));
    app.apply_initial_action_for_test()?;
    let frame = render(&mut app, 80, 24);
    // Modal renders concept titles (Alias as already-in-related, C and D as candidates).
    assert!(frame.contains("[[Alias]]"), "modal shows alias:\n{frame}");
    assert!(frame.contains("[[C]]"), "modal shows candidate C:\n{frame}");
    assert!(frame.contains("[[D]]"), "modal shows candidate D:\n{frame}");
    // Unified read/write panel framing — the title dropped "Update".
    assert!(
        frame.contains("Related:"),
        "modal title reads 'Related:' (not 'Update Related:'):\n{frame}"
    );
    assert!(
        !frame.contains("Update Related"),
        "old 'Update Related' wording is gone:\n{frame}"
    );
    Ok(())
}

#[test]
fn graph_related_modal_is_read_safe_without_commit() -> Result<()> {
    // Opening the panel and closing with Esc must not write anything,
    // even after toggling a candidate — the modal is a read surface
    // that *can* commit, not a write-only flow.
    let (dir, vault) = related_modal_vault();
    let note_path = vault.path.join("N.md");
    let before = std::fs::read_to_string(&note_path)?;
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.set_initial_action(Some(crate::tui::InitialAction::OpenRelatedModal {
        note_path: std::path::PathBuf::from("N.md"),
    }));
    app.apply_initial_action_for_test()?;
    // Toggle the first candidate (mutates in-memory checked state),
    // then cancel without committing.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let after = std::fs::read_to_string(&note_path)?;
    assert_eq!(before, after, "Esc without Enter writes nothing");
    drop(dir);
    Ok(())
}

#[test]
fn graph_related_modal_help_sections_include_shift_r() -> Result<()> {
    let (_dir, vault) = related_modal_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    let sections = app.active_tab_help_sections();
    let merged: String = sections
        .iter()
        .flat_map(|s| s.entries.iter().map(|e| format!("{}={}\n", e.keys, e.desc)))
        .collect();
    assert!(
        merged.contains("Shift+r"),
        "help must surface Shift+r for Related modal:\n{merged}"
    );
    Ok(())
}

#[test]
fn graph_related_modal_confirm_writes_to_note() -> Result<()> {
    let (dir, vault) = related_modal_vault();
    let note_path = vault.path.join("N.md");
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.set_initial_action(Some(crate::tui::InitialAction::OpenRelatedModal {
        note_path: std::path::PathBuf::from("N.md"),
    }));
    app.apply_initial_action_for_test()?;
    // Toggle the first candidate and confirm.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let after = std::fs::read_to_string(&note_path)?;
    // Existing Alias preserved; at least one new concept appended.
    assert!(after.contains("[[Alias]]"), "alias preserved:\n{after}");
    let new_links = after
        .lines()
        .filter(|l| l.starts_with("- [[") && !l.contains("[[Alias]]"))
        .count();
    assert!(new_links >= 1, "expected new related entry:\n{after}");
    drop(dir);
    Ok(())
}

// ── Ghost ranking + promotion (ghost-promotion change) ───────────────

/// Type a query into the graph query bar (clearing the seeded one) and
/// apply it with Enter.
fn apply_graph_query(app: &mut App, query: &str) -> Result<()> {
    app.dispatch(key('/'))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..300 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in query.chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    Ok(())
}

/// Vault with two ghosts: `busy` mentioned in three distinct
/// paragraphs, `quiet` in one. No git — ranking is pure graph.
fn ghost_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vp = dir.path().join("vault");
    std::fs::create_dir_all(vp.join(".obsidian")).unwrap();
    std::fs::write(
        vp.join("a.md"),
        "[[busy]] one.\n\n[[busy]] two.\n\n[[busy]] three.\n\n[[quiet]] once.\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(vp)).unwrap();
    (dir, vault)
}

#[test]
fn ghost_rows_show_ranked_mention_counts() -> Result<()> {
    let (_dir, vault) = ghost_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    apply_graph_query(&mut app, "node where kind in {Ghost};")?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("busy (3)"),
        "ghost row missing count:\n{frame}"
    );
    assert!(
        frame.contains("quiet (1)"),
        "ghost row missing count:\n{frame}"
    );
    let busy = frame.find("busy (3)").unwrap();
    let quiet = frame.find("quiet (1)").unwrap();
    assert!(
        busy < quiet,
        "expected ranked order busy before quiet:\n{frame}"
    );
    assert_tui_snapshot!("graph_tab_ghosts_ranked_counts_80x24", frame);
    Ok(())
}

/// Git-backed ghost vault for the promote flow (blame dates pinned).
fn ghost_git_vault() -> (TempDir, Vault) {
    use std::process::Command as StdCommand;
    let (dir, vault) = ghost_vault();
    let vp = vault.path.clone();
    let run_git = |args: &[&str]| {
        let out = StdCommand::new("git")
            .current_dir(&vp)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_DATE", "2025-03-01T00:00:00")
            .env("GIT_COMMITTER_DATE", "2025-03-01T00:00:00")
            .args(args)
            .output()
            .expect("git binary on PATH");
        assert!(out.status.success(), "git {args:?}");
    };
    run_git(&["init", "-b", "main"]);
    run_git(&["config", "user.name", "T"]);
    run_git(&["config", "user.email", "t@e.com"]);
    run_git(&["config", "commit.gpgsign", "false"]);
    run_git(&["add", "."]);
    run_git(&["commit", "-m", "init"]);
    (dir, vault)
}

#[test]
fn promote_ghost_creates_seeded_synth_note() -> Result<()> {
    let (_dir, vault) = ghost_git_vault();
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    apply_graph_query(&mut app, "node where kind in {Ghost};")?;
    // Cursor sits on the first (top-ranked) row: busy.
    app.dispatch(key('P'))?;

    let promoted = vault_path.join("busy.md");
    assert!(promoted.exists(), "promote should create busy.md");
    let content = std::fs::read_to_string(&promoted).unwrap();
    assert!(
        content.contains("ft:\n  synth:\n    enabled: true"),
        "missing synth marker:\n{content}"
    );
    assert!(
        content.contains("targets:") && content.contains("[[busy]]"),
        "missing ft.synth.targets:\n{content}"
    );
    assert_eq!(
        content.matches("[!ft-source]").count(),
        3,
        "expected one section per mentioning paragraph:\n{content}"
    );
    // The success toast is superseded in the status line by the
    // graph-refresh notification the promote itself triggers, so the
    // on-disk note is the assertion surface here; the non-ghost test
    // below covers the toast path.
    Ok(())
}

#[test]
fn promote_on_non_ghost_row_is_a_noop_with_toast() -> Result<()> {
    let (_dir, vault) = ghost_git_vault();
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    app.switch_to(0)?;
    // Default dirs query: cursor on the root directory row.
    app.dispatch(key('P'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("promote applies to ghost"),
        "missing explanatory toast:\n{frame}"
    );
    assert!(
        !vault_path.join("busy.md").exists(),
        "nothing should be created"
    );
    Ok(())
}

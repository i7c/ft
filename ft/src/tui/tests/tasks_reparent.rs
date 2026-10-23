//! Tasks tab restructuring: cut set (`d`), paste as child / sibling
//! (`p` / `P`), clearing (`Esc`), and indent / outdent (`>` / `<`).

use super::*;

/// Rows under the default query (sorted by due): 0 `call vendor` (with an
/// undated, collapsed subtask), 1 `keep`, 2 `Vendor onboarding`, 3 `Other`.
fn reparent_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("test-vault");
    std::fs::create_dir_all(root.join(".obsidian")).unwrap();
    std::fs::write(
        root.join("inbox.md"),
        "- [ ] call vendor 📅 2026-05-11\n  - [ ] find number\n- [ ] keep 📅 2026-05-12\n",
    )
    .unwrap();
    std::fs::write(
        root.join("proj.md"),
        "- [ ] Vendor onboarding 📅 2026-05-13\n- [ ] Other 📅 2026-05-14\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(root)).unwrap();
    (dir, vault)
}

fn read(dir: &TempDir, rel: &str) -> String {
    std::fs::read_to_string(dir.path().join("test-vault").join(rel)).unwrap()
}

fn loaded_app(vault: Vault) -> Result<App> {
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(3)?;
    app.pump_graph_rebuild_for_test();
    Ok(app)
}

fn down(app: &mut App, n: usize) -> Result<()> {
    for _ in 0..n {
        app.dispatch(key('j'))?;
    }
    Ok(())
}

fn toast_text(app: &mut App) -> Result<String> {
    app.service_pending_requests()?;
    Ok(app.current_toast().map(|t| t.text).unwrap_or_default())
}

#[test]
fn cut_marks_rows_and_shows_hint() -> Result<()> {
    let (_dir, vault) = reparent_vault();
    let mut app = loaded_app(vault)?;
    app.dispatch(key('d'))?;
    down(&mut app, 1)?;
    app.dispatch(key('d'))?;
    let frame = render(&mut app, 100, 14);
    assert!(
        frame.contains("2 cut — p child · P sibling · Esc clear"),
        "{frame}"
    );
    assert_tui_snapshot!("tasks_tab_cut_two_rows_100x14", frame);

    // Toggle one off, then clear the rest.
    app.dispatch(key('d'))?;
    assert!(render(&mut app, 100, 14).contains("1 cut"));
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    assert!(!render(&mut app, 100, 14).contains("cut — p child"));
    Ok(())
}

#[test]
fn paste_child_moves_across_files_and_selects_moved_task() -> Result<()> {
    let (dir, vault) = reparent_vault();
    let mut app = loaded_app(vault)?;
    app.dispatch(key('d'))?; // call vendor
    down(&mut app, 2)?; // Vendor onboarding
    app.dispatch(key('p'))?;
    assert_eq!(
        toast_text(&mut app)?,
        "moved 1 task(s) under Vendor onboarding"
    );
    app.pump_graph_rebuild_for_test();

    assert_eq!(read(&dir, "inbox.md"), "- [ ] keep 📅 2026-05-12\n");
    assert_eq!(
        read(&dir, "proj.md"),
        "- [ ] Vendor onboarding 📅 2026-05-13\n  - [ ] call vendor 📅 2026-05-11\n    - [ ] find number\n- [ ] Other 📅 2026-05-14\n"
    );
    let frame = render(&mut app, 100, 14);
    assert!(
        !frame.contains("cut — p child"),
        "cut set clears after paste:\n{frame}"
    );
    let selected = frame
        .lines()
        .find(|l| l.contains('▶'))
        .unwrap_or_default()
        .to_string();
    assert!(
        selected.contains("call vendor"),
        "cursor follows the moved task, revealed under its parent:\n{frame}"
    );
    Ok(())
}

#[test]
fn paste_sibling_lands_after_target_subtree() -> Result<()> {
    let (dir, vault) = reparent_vault();
    let mut app = loaded_app(vault)?;
    down(&mut app, 3)?; // Other
    app.dispatch(key('d'))?;
    app.dispatch(key('k'))?;
    app.dispatch(key('k'))?;
    app.dispatch(key('k'))?; // call vendor
    app.dispatch(key('P'))?;
    app.pump_graph_rebuild_for_test();
    assert_eq!(
        read(&dir, "inbox.md"),
        "- [ ] call vendor 📅 2026-05-11\n  - [ ] find number\n- [ ] Other 📅 2026-05-14\n- [ ] keep 📅 2026-05-12\n"
    );
    assert_eq!(
        read(&dir, "proj.md"),
        "- [ ] Vendor onboarding 📅 2026-05-13\n"
    );
    Ok(())
}

#[test]
fn paste_into_own_subtree_is_refused_and_keeps_cut_set() -> Result<()> {
    let (dir, vault) = reparent_vault();
    let before = read(&dir, "inbox.md");
    let mut app = loaded_app(vault)?;
    app.dispatch(key('d'))?; // call vendor
    app.dispatch(key('l'))?; // expand
    down(&mut app, 1)?; // find number
    app.dispatch(key('p'))?;
    assert_eq!(toast_text(&mut app)?, "can't move a task under itself");
    assert_eq!(read(&dir, "inbox.md"), before);
    assert!(render(&mut app, 100, 14).contains("1 cut"));
    Ok(())
}

#[test]
fn paste_with_empty_cut_set_toasts_and_leaves_priority() -> Result<()> {
    let (dir, vault) = reparent_vault();
    let before = read(&dir, "inbox.md");
    let mut app = loaded_app(vault)?;
    app.dispatch(key('p'))?;
    assert_eq!(
        toast_text(&mut app)?,
        "nothing cut — press d on a task first"
    );
    assert_eq!(read(&dir, "inbox.md"), before);
    Ok(())
}

#[test]
fn cut_survives_unrelated_line_shift() -> Result<()> {
    let (dir, vault) = reparent_vault();
    let mut app = loaded_app(vault)?;
    down(&mut app, 1)?; // keep (inbox.md:3)
    app.dispatch(key('d'))?;
    // An external edit inserts a line above the cut task.
    let inbox = dir.path().join("test-vault/inbox.md");
    let body = std::fs::read_to_string(&inbox).unwrap();
    std::fs::write(&inbox, format!("# Inbox\n{body}")).unwrap();
    app.dispatch(key('R'))?;
    app.pump_graph_rebuild_for_test();
    assert!(render(&mut app, 100, 14).contains("1 cut"));

    // Paste under `Other` (row 3) still moves the right line.
    down(&mut app, 3)?;
    app.dispatch(key('p'))?;
    app.pump_graph_rebuild_for_test();
    assert_eq!(
        read(&dir, "inbox.md"),
        "# Inbox\n- [ ] call vendor 📅 2026-05-11\n  - [ ] find number\n"
    );
    assert_eq!(
        read(&dir, "proj.md"),
        "- [ ] Vendor onboarding 📅 2026-05-13\n- [ ] Other 📅 2026-05-14\n  - [ ] keep 📅 2026-05-12\n"
    );
    Ok(())
}

#[test]
fn cut_entry_dropped_when_task_changes_on_disk() -> Result<()> {
    let (dir, vault) = reparent_vault();
    let mut app = loaded_app(vault)?;
    down(&mut app, 1)?; // keep
    app.dispatch(key('d'))?;
    let inbox = dir.path().join("test-vault/inbox.md");
    let body = std::fs::read_to_string(&inbox).unwrap();
    std::fs::write(&inbox, body.replace("keep", "keep edited")).unwrap();
    app.dispatch(key('R'))?;
    app.pump_graph_rebuild_for_test();
    assert_eq!(
        toast_text(&mut app)?,
        "1 cut task(s) changed on disk — dropped"
    );
    assert!(!render(&mut app, 100, 14).contains("cut — p child"));
    Ok(())
}

#[test]
fn indent_and_outdent_keys() -> Result<()> {
    let (dir, vault) = reparent_vault();
    let mut app = loaded_app(vault)?;
    down(&mut app, 3)?; // Other
    app.dispatch(key('>'))?;
    app.pump_graph_rebuild_for_test();
    assert_eq!(
        read(&dir, "proj.md"),
        "- [ ] Vendor onboarding 📅 2026-05-13\n  - [ ] Other 📅 2026-05-14\n"
    );
    // Cursor stays on `Other` (revealed under its parent); outdent it back.
    app.dispatch(key('<'))?;
    app.pump_graph_rebuild_for_test();
    assert_eq!(
        read(&dir, "proj.md"),
        "- [ ] Vendor onboarding 📅 2026-05-13\n- [ ] Other 📅 2026-05-14\n"
    );
    app.dispatch(key('<'))?;
    assert_eq!(toast_text(&mut app)?, "already top-level");
    Ok(())
}

#[test]
fn indent_without_previous_sibling_toasts() -> Result<()> {
    let (dir, vault) = reparent_vault();
    let before = read(&dir, "proj.md");
    let mut app = loaded_app(vault)?;
    down(&mut app, 2)?; // Vendor onboarding: first line of proj.md
    app.dispatch(key('>'))?;
    assert_eq!(toast_text(&mut app)?, "no previous sibling to indent under");
    assert_eq!(read(&dir, "proj.md"), before);
    Ok(())
}

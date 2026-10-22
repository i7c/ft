//! Tasks tab logical parents: `L` link the cut set, `U` unlink (by row
//! context or picker), `S` create a linked subtask, `↳` / `⤴N` rendering,
//! and block-reference retargeting on paste.

use super::*;

/// Rows under the default query (sorted by due): 0 `call vendor` (with an
/// undated, collapsed subtask), 1 `keep`, 2 `Vendor onboarding`, 3 `Other`.
fn link_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("test-vault");
    std::fs::create_dir_all(root.join(".obsidian")).unwrap();
    std::fs::create_dir_all(root.join(".ft")).unwrap();
    std::fs::write(
        root.join(".ft/config.toml"),
        "[periodic_notes.daily]\npath = \"Daily\"\nformat = \"%Y-%m-%d\"\n",
    )
    .unwrap();
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
    app.switch_to(5)?;
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

fn id_of(line: &str) -> String {
    line.split("🆔 ")
        .nth(1)
        .and_then(|r| r.split_whitespace().next())
        .unwrap()
        .to_string()
}

fn enter(app: &mut App) -> Result<()> {
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.service_pending_requests()
}

#[test]
fn link_cut_task_under_cursor_task_without_moving_it() -> Result<()> {
    let (dir, vault) = link_vault();
    let mut app = loaded_app(vault)?;
    down(&mut app, 1)?; // keep
    app.dispatch(key('d'))?;
    down(&mut app, 1)?; // Vendor onboarding
    app.dispatch(key('L'))?;
    assert_eq!(
        toast_text(&mut app)?,
        "linked 1 task(s) under Vendor onboarding"
    );
    app.pump_graph_rebuild_for_test();

    let proj = read(&dir, "proj.md");
    let id = id_of(proj.lines().next().unwrap());
    assert_eq!(
        read(&dir, "inbox.md"),
        format!(
            "- [ ] call vendor 📅 2026-05-11\n  - [ ] find number\n- [ ] keep ⤴ {id} 📅 2026-05-12\n"
        )
    );
    let frame = render(&mut app, 100, 14);
    assert!(frame.contains("↳ keep"), "{frame}");
    let frame = frame.replace(&id, "<id>");
    assert_tui_snapshot!("tasks_tab_linked_child_100x14", frame);
    Ok(())
}

#[test]
fn unlink_selected_task() -> Result<()> {
    let (dir, vault) = link_vault();
    let inbox = dir.path().join("test-vault/inbox.md");
    std::fs::write(
        &inbox,
        "- [ ] call vendor 📅 2026-05-11\n  - [ ] find number\n- [ ] keep ⤴ vo 📅 2026-05-12\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("test-vault/proj.md"),
        "- [ ] Vendor onboarding 📅 2026-05-13 🆔 vo\n- [ ] Other 📅 2026-05-14\n",
    )
    .unwrap();
    let mut app = loaded_app(vault)?;
    // Rows: call vendor, Vendor onboarding, (collapsed) … — keep is nested
    // under Vendor onboarding, so expand it and step onto keep.
    down(&mut app, 1)?;
    app.dispatch(key('l'))?;
    down(&mut app, 1)?;
    app.dispatch(key('U'))?;
    app.pump_graph_rebuild_for_test();
    assert_eq!(
        read(&dir, "inbox.md"),
        "- [ ] call vendor 📅 2026-05-11\n  - [ ] find number\n- [ ] keep 📅 2026-05-12\n"
    );
    // Unlinking a task without a link toasts and writes nothing.
    app.dispatch(key('k'))?;
    app.dispatch(key('k'))?;
    app.dispatch(key('U'))?;
    assert_eq!(toast_text(&mut app)?, "task has no parent link");
    Ok(())
}

/// `call vendor` hangs under both `Vendor onboarding` and `Other`.
fn two_parent_vault() -> (TempDir, Vault) {
    let (dir, vault) = link_vault();
    std::fs::write(
        dir.path().join("test-vault/inbox.md"),
        "- [ ] call vendor ⤴ vo,ot 📅 2026-05-11\n- [ ] keep 📅 2026-05-12\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("test-vault/proj.md"),
        "- [ ] Vendor onboarding 📅 2026-05-13 🆔 vo\n- [ ] Other 📅 2026-05-14 🆔 ot\n",
    )
    .unwrap();
    (dir, vault)
}

#[test]
fn task_shows_under_every_parent_with_badge() -> Result<()> {
    let (_dir, vault) = two_parent_vault();
    let mut app = loaded_app(vault)?;
    // Rows: keep, Vendor onboarding, Other — expand both parents.
    down(&mut app, 1)?;
    app.dispatch(key('l'))?;
    down(&mut app, 2)?;
    app.dispatch(key('l'))?;
    let frame = render(&mut app, 100, 14);
    assert_eq!(frame.matches("↳ call vendor ⤴2").count(), 2, "{frame}");
    assert_tui_snapshot!("tasks_tab_two_parents_100x14", frame);
    Ok(())
}

#[test]
fn unlink_uses_row_context_then_picker() -> Result<()> {
    let (dir, vault) = two_parent_vault();
    let mut app = loaded_app(vault)?;
    // Under `Other` (rows: keep, Vendor onboarding, Other, ↳ call vendor).
    down(&mut app, 2)?;
    app.dispatch(key('l'))?;
    down(&mut app, 1)?;
    app.dispatch(key('U'))?;
    app.pump_graph_rebuild_for_test();
    assert!(read(&dir, "inbox.md").starts_with("- [ ] call vendor ⤴ vo 📅"));

    // A root row with several references opens the picker.
    let (dir, vault) = two_parent_vault();
    std::fs::write(
        dir.path().join("test-vault/proj.md"),
        "- [x] Vendor onboarding 🆔 vo\n- [x] Other 🆔 ot\n",
    )
    .unwrap();
    let mut app = loaded_app(vault)?;
    // Parents are done (not in the default query), so call vendor is a root.
    app.dispatch(key('U'))?;
    app.service_pending_requests()?;
    let frame = render(&mut app, 100, 40);
    assert!(frame.contains("all parents"), "{frame}");
    for c in "Other".chars() {
        app.dispatch(key(c))?;
    }
    enter(&mut app)?;
    app.pump_graph_rebuild_for_test();
    assert!(read(&dir, "inbox.md").starts_with("- [ ] call vendor ⤴ vo 📅"));
    Ok(())
}

#[test]
fn new_linked_subtask_lands_in_daily_note() -> Result<()> {
    let (dir, vault) = link_vault();
    let mut app = loaded_app(vault)?;
    down(&mut app, 2)?; // Vendor onboarding
    app.dispatch(key('S'))?;
    for c in "call legal due:tomorrow".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.pump_graph_rebuild_for_test();
    let id = id_of(read(&dir, "proj.md").lines().next().unwrap());
    let daily = read(&dir, "Daily/2026-05-10.md");
    assert!(
        daily.contains(&format!("- [ ] call legal ⤴ {id}")),
        "{daily}"
    );
    let frame = render(&mut app, 100, 14);
    assert!(frame.contains("↳ call legal"), "{frame}");
    Ok(())
}

#[test]
fn link_refusals_toast_and_keep_cut_set() -> Result<()> {
    let (dir, vault) = link_vault();
    let before = (read(&dir, "inbox.md"), read(&dir, "proj.md"));
    let mut app = loaded_app(vault)?;
    // Physically nested child: cut `find number`, link under its own
    // parent `call vendor`.
    app.dispatch(key('l'))?;
    down(&mut app, 1)?;
    app.dispatch(key('d'))?;
    app.dispatch(key('k'))?; // call vendor
    app.dispatch(key('L'))?;
    assert_eq!(
        toast_text(&mut app)?,
        "find number is already nested under it"
    );
    assert!(render(&mut app, 100, 14).contains("1 cut"));
    assert_eq!((read(&dir, "inbox.md"), read(&dir, "proj.md")), before);
    Ok(())
}

#[test]
fn link_under_own_subtask_is_refused() -> Result<()> {
    let (_dir, vault) = link_vault();
    let mut app = loaded_app(vault)?;
    app.dispatch(key('d'))?; // call vendor
    app.dispatch(key('l'))?;
    down(&mut app, 1)?; // find number (its child)
    app.dispatch(key('L'))?;
    assert_eq!(
        toast_text(&mut app)?,
        "can't link a task under its own subtask"
    );
    Ok(())
}

#[test]
fn paste_retargets_incoming_links() -> Result<()> {
    let (dir, vault) = link_vault();
    std::fs::write(
        dir.path().join("test-vault/inbox.md"),
        "- [ ] call vendor 📅 2026-05-11\n  - [ ] find number\n- [ ] keep 📅 2026-05-12\n- [ ] ref [[proj#^vo]]\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("test-vault/proj.md"),
        "- [ ] Vendor onboarding 📅 2026-05-13 ^vo\n- [ ] Other 📅 2026-05-14\n",
    )
    .unwrap();
    let mut app = loaded_app(vault)?;
    // Cut Vendor onboarding, paste it as a sibling after `call vendor`.
    down(&mut app, 2)?;
    app.dispatch(key('d'))?;
    app.dispatch(key('k'))?;
    app.dispatch(key('k'))?;
    app.dispatch(key('P'))?;
    assert_eq!(
        toast_text(&mut app)?,
        "moved 1 task(s) after call vendor · retargeted 1 link(s)"
    );
    assert_eq!(
        read(&dir, "inbox.md"),
        "- [ ] call vendor 📅 2026-05-11\n  - [ ] find number\n- [ ] Vendor onboarding 📅 2026-05-13 ^vo\n- [ ] keep 📅 2026-05-12\n- [ ] ref [[#^vo]]\n"
    );
    Ok(())
}

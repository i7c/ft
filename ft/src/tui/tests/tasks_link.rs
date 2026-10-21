//! Tasks tab linked parents: `L` link the cut set, `U` unlink, `S` create
//! a linked subtask, `↳` rendering, and link retargeting on paste.

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

fn block_id(line: &str) -> String {
    ft_core::scan::trailing_block_id(line).unwrap().to_string()
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
    let id = block_id(proj.lines().next().unwrap());
    assert_eq!(
        read(&dir, "inbox.md"),
        format!(
            "- [ ] call vendor 📅 2026-05-11\n  - [ ] find number\n- [ ] keep [[proj#^{id}]] 📅 2026-05-12\n"
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
        "- [ ] call vendor 📅 2026-05-11\n  - [ ] find number\n- [ ] keep [[proj#^vo]] 📅 2026-05-12\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("test-vault/proj.md"),
        "- [ ] Vendor onboarding 📅 2026-05-13 ^vo\n- [ ] Other 📅 2026-05-14\n",
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
    let id = block_id(read(&dir, "proj.md").lines().next().unwrap());
    let daily = read(&dir, "Daily/2026-05-10.md");
    assert!(
        daily.contains(&format!("- [ ] call legal [[proj#^{id}]]")),
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
    // Physically nested child: cut `find number`, link under `Other`.
    app.dispatch(key('l'))?;
    down(&mut app, 1)?;
    app.dispatch(key('d'))?;
    down(&mut app, 3)?; // Other
    app.dispatch(key('L'))?;
    assert_eq!(
        toast_text(&mut app)?,
        "find number is nested under another task — outdent it first"
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

//! Tasks tab colors: `#color/<name>` tags color a row and its subtree
//! (inherited along the displayed path); `m` sets / clears them.

use ratatui::style::Color;

use super::*;

fn color_vault(files: &[(&str, &str)]) -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("test-vault");
    std::fs::create_dir_all(root.join(".obsidian")).unwrap();
    for (rel, body) in files {
        std::fs::write(root.join(rel), body).unwrap();
    }
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

fn buffer(app: &mut App, w: u16, h: u16) -> ratatui::buffer::Buffer {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render_to(f)).unwrap();
    terminal.backend().buffer().clone()
}

/// Foreground of the first cell of each occurrence of `needle` in the
/// task list, top to bottom.
fn fgs(buf: &ratatui::buffer::Buffer, needle: &str) -> Vec<Color> {
    let area = buf.area();
    let mut out = Vec::new();
    for y in 0..area.height {
        let line: String = (0..area.width).map(|x| buf[(x, y)].symbol()).collect();
        if let Some(byte) = line.find(needle) {
            let col = line[..byte].chars().count() as u16;
            out.push(buf[(col, y)].fg);
        }
    }
    out
}

fn expand_all(app: &mut App, rows: usize) -> Result<()> {
    for _ in 0..rows {
        app.dispatch(key('l'))?;
        app.dispatch(key('j'))?;
    }
    Ok(())
}

#[test]
fn own_and_inherited_colors() -> Result<()> {
    let (_dir, vault) = color_vault(&[
        (
            "goals.md",
            "- [ ] Ship Q4 #color/blue 🆔 q4 📅 2026-05-11\n  - [ ] call vendor\n  - [ ] own red #color/red\n    - [ ] red kid\n",
        ),
        ("j.md", "- [ ] linked kid ⤴ q4\n"),
    ]);
    let mut app = loaded_app(vault)?;
    app.dispatch(key('l'))?; // expand Ship Q4
    app.dispatch(key('j'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('l'))?; // expand own red
    let buf = buffer(&mut app, 100, 20);
    let frame = buffer_to_string(&buf);
    assert_eq!(fgs(&buf, "Ship Q4"), [Color::Blue], "{frame}");
    assert!(
        !frame.contains("#color/"),
        "color tags are hidden:\n{frame}"
    );
    assert_eq!(fgs(&buf, "call vendor"), [Color::Blue]);
    assert_eq!(fgs(&buf, "linked kid"), [Color::Blue]);
    assert_eq!(fgs(&buf, "own red"), [Color::Red]);
    assert_eq!(fgs(&buf, "red kid"), [Color::Red]);
    Ok(())
}

#[test]
fn two_parents_two_colors_and_filtered_parent() -> Result<()> {
    let (_dir, vault) = color_vault(&[
        (
            "goals.md",
            "- [ ] Blue goal #color/blue 🆔 a1 📅 2026-05-11\n- [ ] Green goal #color/green 🆔 b1 📅 2026-05-12\n- [x] Done goal #color/magenta 🆔 d1\n",
        ),
        (
            "j.md",
            "- [ ] shared ⤴ a1,b1\n- [ ] orphan of done ⤴ d1 📅 2026-05-13\n",
        ),
    ]);
    let mut app = loaded_app(vault)?;
    expand_all(&mut app, 3)?;
    let buf = buffer(&mut app, 100, 20);
    let frame = buffer_to_string(&buf);
    assert_eq!(fgs(&buf, "shared"), [Color::Blue, Color::Green], "{frame}");
    // `Done goal` isn't in the default query; its child is a root that
    // still inherits.
    assert_eq!(fgs(&buf, "orphan of done"), [Color::Magenta], "{frame}");
    Ok(())
}

fn enter(app: &mut App) -> Result<()> {
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.service_pending_requests()
}

fn pick(app: &mut App, name: &str) -> Result<()> {
    app.dispatch(key('m'))?;
    app.service_pending_requests()?;
    for c in name.chars() {
        app.dispatch(key(c))?;
    }
    enter(app)?;
    app.pump_graph_rebuild_for_test();
    Ok(())
}

#[test]
fn m_sets_replaces_and_clears_colors() -> Result<()> {
    let (dir, vault) = color_vault(&[(
        "t.md",
        "- [ ] alpha #work 📅 2026-05-11\n- [ ] beta 📅 2026-05-12\n- [ ] gamma 📅 2026-05-13\n",
    )]);
    let mut app = loaded_app(vault)?;
    app.dispatch(key('m'))?;
    app.service_pending_requests()?;
    let frame = render(&mut app, 100, 30);
    assert!(
        frame.contains("magenta") && frame.contains("none"),
        "{frame}"
    );
    assert_tui_snapshot!("tasks_color_picker_100x30", frame);
    for c in "green".chars() {
        app.dispatch(key(c))?;
    }
    enter(&mut app)?;
    app.pump_graph_rebuild_for_test();
    assert!(read(&dir, "t.md").starts_with("- [ ] alpha #work #color/green 📅"));
    pick(&mut app, "red")?;
    assert!(read(&dir, "t.md").starts_with("- [ ] alpha #work #color/red 📅"));
    pick(&mut app, "none")?;
    assert!(read(&dir, "t.md").starts_with("- [ ] alpha #work 📅"));

    // Cut set: beta + gamma.
    app.dispatch(key('j'))?;
    app.dispatch(key('d'))?;
    app.dispatch(key('j'))?;
    app.dispatch(key('d'))?;
    pick(&mut app, "cyan")?;
    let t = read(&dir, "t.md");
    assert!(
        t.contains("- [ ] beta #color/cyan 📅") && t.contains("- [ ] gamma #color/cyan 📅"),
        "{t}"
    );
    assert!(
        !render(&mut app, 100, 14).contains("cut"),
        "cut set cleared"
    );
    Ok(())
}

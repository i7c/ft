use std::sync::Arc;

use anyhow::Result;
use assert_fs::TempDir;
use chrono::{DateTime, Local, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ft_core::recents::RecentsLog;
use ft_core::vault::Vault;
use ratatui::{
    backend::{Backend, TestBackend},
    Terminal,
};

use crate::tui::{event::Event, tab::AppRequest, App};

fn fixed_clock() -> DateTime<Local> {
    // Sun 10 May 2026, 14:32:05 — matches the FT_TODAY used elsewhere.
    Local
        .with_ymd_and_hms(2026, 5, 10, 14, 32, 5)
        .single()
        .expect("fixed test clock must be unambiguous")
}

fn test_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

/// Vault with a known set of tasks: two overdue (priority high/medium), three
/// upcoming (within 7 days), one outside the default-query window. Dates are
/// anchored to `fixed_clock` so the snapshot is stable.
fn populated_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    let body = "\
- [ ] Pay rent ⏫ 📅 2026-05-08
- [ ] Renew passport 🔼 📅 2026-05-09
- [ ] Reply to Sara 📅 2026-05-10
- [ ] Submit Q2 report ⏫ 📅 2026-05-12 ⏳ 2026-05-11
- [ ] Buy birthday gift 🔽 📅 2026-05-15
- [ ] Plan vacation 📅 2026-08-01
- [x] Old task 📅 2026-05-01 ✅ 2026-05-02
";
    std::fs::write(vault_path.join("tasks.md"), body).unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

/// Snapshot helper that redacts the wall-clock part of the status bar's
/// `refreshed HH:MM:SS` cell, so snapshots don't depend on real time.
macro_rules! assert_tui_snapshot {
    ($name:literal, $value:expr) => {{
        insta::with_settings!({
            filters => vec![(r"refreshed \d\d:\d\d:\d\d", "refreshed [HH:MM:SS]")],
            // Submodule paths stay out of snapshot filenames so tests can
            // move between the files below without re-blessing frames.
            prepend_module_to_snapshot => false,
        }, {
            insta::assert_snapshot!($name, $value);
        });
    }};
}

fn render(app: &mut App, w: u16, h: u16) -> String {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render_to(f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    buffer_to_string(&buf)
}

fn buffer_to_string(buf: &ratatui::buffer::Buffer) -> String {
    let area = buf.area();
    let mut out = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buf[(x, y)];
            out.push_str(cell.symbol());
        }
        out.push('\n');
    }
    out
}

fn key(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

#[test]
fn help_overlay_renders_over_initial_tab() {
    // GraphTab is the first tab now; help should overlay it cleanly.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.enter_help();
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("help_overlay_80x24", frame);
}

#[test]
fn tasks_tab_empty_vault_renders_no_matches() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("tasks_tab_empty_80x24", frame);
    Ok(())
}

#[test]
fn tasks_tab_populated_renders_overdue_and_upcoming() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("tasks_tab_populated_80x24", frame);
    Ok(())
}

/// Vault with tasks at each age band (anchored to `fixed_clock` =
/// 2026-05-10), all due today so they pass the default query. Covers
/// Fresh / Aging / Stale / Rotten / Unknown in one frame.
fn aged_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    // created dates step through each band; the last row has no ➕.
    let body = "\
- [ ] Fresh today ➕ 2026-05-10 📅 2026-05-10
- [ ] Fresh 2d ➕ 2026-05-08 📅 2026-05-10
- [ ] Aging 6d ➕ 2026-05-04 📅 2026-05-10
- [ ] Stale 20d ➕ 2026-04-20 📅 2026-05-10
- [ ] Rotten 56d ➕ 2026-03-15 📅 2026-05-10
- [ ] Unknown age 📅 2026-05-10
";
    std::fs::write(vault_path.join("tasks.md"), body).unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn tasks_tab_renders_age_badge_bands() -> Result<()> {
    let (_dir, vault) = aged_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    let frame = render(&mut app, 100, 24);
    assert_tui_snapshot!("tasks_tab_age_bands_100x24", frame);
    Ok(())
}

/// Vault with a nested task tree whose top-level parents pass the default
/// query (Open + due soon) while the indented subtasks have no due date, so
/// they only surface when the user expands a parent.
fn nested_tasks_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    let body = "\
- [ ] Build a house 📅 2026-05-12
  - [ ] Build the walls
    - [ ] Lay bricks
  - [ ] Pipes and plumbing
- [ ] Buy groceries 📅 2026-05-13
";
    std::fs::write(vault_path.join("tasks.md"), body).unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn tasks_tab_subtasks_collapse_and_expand() -> Result<()> {
    let (_dir, vault) = nested_tasks_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;

    // Collapsed: the parent shows a ▸ affordance and its subtasks are hidden.
    let collapsed = render(&mut app, 80, 24);
    assert!(collapsed.contains('▸'), "parent shows a collapsed marker");
    assert!(
        !collapsed.contains("Build the walls"),
        "subtasks stay hidden while collapsed"
    );
    assert_tui_snapshot!("tasks_tab_subtasks_collapsed_80x24", collapsed);

    // Expand the selected parent (cursor starts on the first match).
    app.dispatch(key('l'))?;
    let expanded = render(&mut app, 80, 24);
    assert!(
        expanded.contains("Build the walls") && expanded.contains("Pipes and plumbing"),
        "direct subtasks appear on expand"
    );
    assert!(
        !expanded.contains("Lay bricks"),
        "the grandchild stays hidden until its parent expands"
    );
    assert_tui_snapshot!("tasks_tab_subtasks_expanded_80x24", expanded);

    // Step into the first subtask and expand it → the grandchild appears.
    app.dispatch(key('j'))?;
    app.dispatch(key('l'))?;
    let deep = render(&mut app, 80, 24);
    assert!(
        deep.contains("Lay bricks"),
        "grandchild revealed after expanding the middle task"
    );

    // Move back up to the root and collapse it: the whole subtree disappears.
    app.dispatch(key('k'))?;
    app.dispatch(key('h'))?;
    let recollapsed = render(&mut app, 80, 24);
    assert!(
        !recollapsed.contains("Build the walls"),
        "collapsing the root hides its entire subtree"
    );
    Ok(())
}

/// A matched subtask whose parent is also matched appears exactly once,
/// nested — not also as a depth-0 root. (graph-task-interaction §D7.)
#[test]
fn tasks_tab_dedups_matched_parent_and_subtask() -> Result<()> {
    let (_dir, vault) = nested_tasks_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?; // Tasks tab

    // A query matching both the top-level task and its subtask (via the
    // shared source file). Both live in tasks.md.
    app.dispatch(key('/'))?; // enter query bar
                             // Clear the default query before typing.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..200 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "path includes \"tasks\"".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // Expand the first match so any nested children would show.
    app.dispatch(key('l'))?;
    let frame = render(&mut app, 80, 24);

    // The parent "Build a house" and subtask "Build the walls" both match;
    // the subtask must appear exactly once (nested), not twice.
    let parent_count = frame.matches("Build a house").count();
    let walls_count = frame.matches("Build the walls").count();
    assert_eq!(parent_count, 1, "parent appears once: {frame}");
    assert_eq!(
        walls_count, 1,
        "subtask appears once (nested, not duplicated): {frame}"
    );
    Ok(())
}

#[test]
fn create_subtask_nests_under_selected_and_auto_expands() -> Result<()> {
    let (_dir, vault) = nested_tasks_vault();
    let task_file = vault.path.join("tasks.md");
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;

    // Cursor starts on "Build a house". `s` opens the subtask quickline.
    app.dispatch(key('s'))?;
    for c in "Wiring".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // On disk: indented one level, appended after the parent's existing
    // subtasks (which sit at two spaces).
    let content = std::fs::read_to_string(&task_file)?;
    assert!(
        content.contains("  - [ ] Pipes and plumbing\n  - [ ] Wiring ➕ 2026-05-10\n"),
        "subtask should be written indented at the end of the parent block:\n{content}"
    );

    // In the UI the parent auto-expands so the new child is visible
    // once the rebuilt snapshot lands.
    app.pump_graph_rebuild_for_test();
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Wiring"),
        "new subtask should be visible after create:\n{frame}"
    );
    Ok(())
}

#[test]
fn create_subtask_survives_expand_to_form() -> Result<()> {
    // The subtask target must carry through the quickline → full-form
    // (Ctrl+E) transition, so a form submit still nests under the parent.
    let (_dir, vault) = nested_tasks_vault();
    let task_file = vault.path.join("tasks.md");
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;

    app.dispatch(key('s'))?;
    for c in "Plumbing".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    )))?;

    let content = std::fs::read_to_string(&task_file)?;
    assert!(
        content.contains("  - [ ] Plumbing"),
        "form submit in subtask mode must still nest under the parent:\n{content}"
    );
    Ok(())
}

/// Vault with a long task description, used to verify the description column
/// expands when the terminal is wider than the 80x24 minimum.
fn long_description_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    let body = "\
- [ ] This is a fairly long task description that would not fit at 80 cols 📅 2026-05-12
";
    std::fs::write(vault_path.join("tasks.md"), body).unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn tasks_tab_wide_terminal_uses_available_width() -> Result<()> {
    let (_dir, vault) = long_description_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    let narrow = render(&mut app, 80, 24);
    assert!(
        narrow.contains("This is a fairl") && narrow.contains('…'),
        "narrow terminal should truncate long description: {narrow}"
    );

    let wide = render(&mut app, 160, 24);
    assert!(
        wide.contains("This is a fairly long task description that would not fit at 80 cols"),
        "wide terminal should show full description without truncation: {wide}"
    );
    Ok(())
}

#[test]
fn tasks_tab_query_edit_mode_renders() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    app.dispatch(key('/'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("tasks_tab_editing_80x24", frame);
    Ok(())
}

#[test]
fn tasks_tab_query_parse_error_renders() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    // Open the editor, clear it, type garbage, apply.
    app.dispatch(key('/'))?;
    // Select all + delete: simulate by pressing End then Backspace many times.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))?;
    for _ in 0..200 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))?;
    }
    for c in "totally bogus".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("tasks_tab_parse_error_80x24", frame);
    Ok(())
}

#[test]
fn arrow_keys_navigate_view_dropdown_without_panic() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    let down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let up = Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    // Single-view list — these wrap to themselves but must not panic or
    // change the active tab.
    app.dispatch(down.clone())?;
    app.dispatch(up.clone())?;
    assert_eq!(app.active_title(), "Tasks");
    Ok(())
}

#[test]
fn enter_on_dropdown_is_consumed_by_tasks_tab() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(5)?;
    let enter = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.dispatch(enter)?;
    // Tasks tab consumed Enter — global keymap (which has no Enter binding)
    // should not have run, and the app must still be alive.
    assert!(!app.is_quit());
    assert_eq!(app.active_title(), "Tasks");
    Ok(())
}

// ── Shared fixtures used across the test files below ────────────────

/// Drive the Graph tab to a vault with one note + one task, apply the
/// note-scoped task query, expand the note, and land the cursor on the
/// task row. Returns the vault path so the caller can assert on disk.
fn graph_tab_with_focused_task(body: &str) -> (TempDir, std::path::PathBuf, App) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::write(vault_path.join("root.md"), body).unwrap();
    let vault = Vault::discover(Some(vault_path.clone())).unwrap();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1).unwrap();
    app.switch_to(0).unwrap(); // Graph tab
                               // Apply the note-scoped task query.
    app.dispatch(key('/')).unwrap();
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))
        .unwrap();
    for _ in 0..300 {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )))
        .unwrap();
    }
    for c in "node where kind = Note and path = \"root.md\"; expand where edge.kind in {has-task, subtask} and to.kind in {Task};".chars() {
        app.dispatch(key(c)).unwrap();
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))
    .unwrap();
    // Walk onto the note and expand it so the task row appears, then descend.
    app.dispatch(key('j')).unwrap();
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))
    .unwrap();
    app.dispatch(key('j')).unwrap(); // onto the task
    (dir, vault_path, app)
}

/// Build a vault with two paragraphs that co-occur with N: one
/// same-paragraph hit (+3 for C) and one same-file cross-paragraph
/// hit (+1 for D). Plus one alias in N's existing Related section.
fn related_modal_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();

    std::fs::write(vault_path.join("N.md"), "# N\n\n## Related\n- [[Alias]]\n").unwrap();
    std::fs::write(vault_path.join("Alias.md"), "# Alias\n").unwrap();
    std::fs::write(vault_path.join("C.md"), "# C\n").unwrap();
    std::fs::write(vault_path.join("D.md"), "# D\n").unwrap();
    std::fs::write(
        vault_path.join("Notes.md"),
        "Mentions [[N]] and [[C]] in the same paragraph.\n\nLater, [[D]] gets mentioned alone.\n",
    )
    .unwrap();

    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

/// Build a vault in a TempDir with the given files (creates .obsidian).
fn rename_vault(files: &[(&str, &str)]) -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    for (rel, content) in files {
        let file_path = vault_path.join(rel);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file_path, content).unwrap();
    }
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

/// Switch to the Graph tab (index 0). Bounces through a different tab
/// so on_focus fires.
fn switch_to_graph(app: &mut App) -> Result<()> {
    app.switch_to(1)?;
    app.switch_to(0)?;
    Ok(())
}

/// Build a vault with two folders + two templates so the create flow has
/// realistic state to render.
fn notes_create_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::create_dir_all(vault_path.join("inbox")).unwrap();
    std::fs::create_dir_all(vault_path.join("proj")).unwrap();
    std::fs::write(vault_path.join("inbox/existing.md"), "# Existing\n").unwrap();

    // Drop two ft-flavoured templates into the default templates dir.
    let templates_dir = vault_path.join("templates-ft");
    std::fs::create_dir_all(&templates_dir).unwrap();
    std::fs::write(
        templates_dir.join("new.md"),
        "---\ntags: [Created-{{ today | date(format=\"%Y-%m-%d\") }}]\n---\n# {{ title }}\n\n",
    )
    .unwrap();
    std::fs::write(
        templates_dir.join("quick-add.md"),
        "---\ntags: [Created-{{ today | date(format=\"%Y-%m-%d\") }}]\n---\n# {{ vars.name }}\n",
    )
    .unwrap();

    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

const NOTES_TAB_INDEX: usize = 1;

/// Vault wired with every periodic-note config block (daily, weekly,
/// monthly, quarterly, yearly). No template references — the periodic
/// helper writes a blank `# <title>\n\n` stub when `template` is unset,
/// which is enough for the leader-chord behavior tests.
fn periodic_vault_all_periods() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::create_dir_all(vault_path.join(".ft")).unwrap();
    std::fs::write(
        vault_path.join(".ft/config.toml"),
        "[periodic_notes.daily]\npath = \"journal/%Y\"\nformat = \"%Y-%m-%d\"\n\
         [periodic_notes.weekly]\npath = \"journal/%Y\"\nformat = \"%G-W%V\"\n\
         [periodic_notes.monthly]\npath = \"journal/%Y\"\nformat = \"%Y-%m\"\n\
         [periodic_notes.quarterly]\npath = \"journal/%Y\"\nformat = \"%Y-Q%q\"\n\
         [periodic_notes.yearly]\npath = \"journal\"\nformat = \"%Y\"\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

/// Path to the markdown file inside `populated_vault`. Tests use this to
/// inspect on-disk state after a quick-key mutation.
fn populated_tasks_path(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join("test-vault").join("tasks.md")
}

mod git;
mod graph;
mod history;
mod notes;
mod snapshot_lifecycle;
mod synthesis;
mod tasks;
mod timeblocks;

// ── [tui] tab toggles (note-flow-renames) ────────────────────────────

/// Default config: the adjacent tabs are off; the note-flow five are
/// the whole layout, in workflow order.
#[test]
fn default_layout_hides_adjacent_tabs() {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_default_tabs(vault);
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("1 Graph") && frame.contains("2 Notes") && frame.contains("3 Pulse"),
        "workflow tabs missing:\n{frame}"
    );
    assert!(
        frame.contains("4 Recent") && frame.contains("5 Gather"),
        "workflow tabs missing:\n{frame}"
    );
    assert!(
        !frame.contains("Tasks") && !frame.contains("Timeblocks"),
        "adjacent tabs must be hidden by default:\n{frame}"
    );
    assert_tui_snapshot!("default_layout_five_tabs_80x24", frame);
}

/// `[tui] tasks_tab/timeblocks_tab = true` appends the adjacent tabs
/// after Gather.
#[test]
fn tui_config_enables_adjacent_tabs() {
    let (_dir, vault) = test_vault();
    std::fs::create_dir_all(vault.path.join(".ft")).unwrap();
    std::fs::write(
        vault.path.join(".ft/config.toml"),
        "[tui]\ntasks_tab = true\ntimeblocks_tab = true\n",
    )
    .unwrap();
    let vault = Vault::discover(Some(vault.path.clone())).unwrap();
    let mut app = App::for_test_default_tabs(vault);
    let frame = render(&mut app, 90, 24);
    assert!(
        frame.contains("6 Tasks") && frame.contains("7 Ti"),
        "enabled adjacent tabs must appear after Gather:\n{frame}"
    );
}

/// Unknown keys in `[tui]` are rejected like every other config table.
#[test]
fn tui_config_unknown_key_rejected() {
    let (_dir, vault) = test_vault();
    std::fs::create_dir_all(vault.path.join(".ft")).unwrap();
    std::fs::write(
        vault.path.join(".ft/config.toml"),
        "[tui]\ntask_tab = true\n",
    )
    .unwrap();
    let err = Vault::discover(Some(vault.path.clone()));
    assert!(err.is_err(), "typo'd [tui] key must be rejected");
}

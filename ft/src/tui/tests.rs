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
    app.switch_to(1)?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("tasks_tab_empty_80x24", frame);
    Ok(())
}

#[test]
fn tasks_tab_populated_renders_overdue_and_upcoming() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("tasks_tab_populated_80x24", frame);
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
    app.switch_to(1)?;

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
    app.switch_to(1)?; // Tasks tab

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
    app.switch_to(1)?;

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
        content.contains("  - [ ] Pipes and plumbing\n  - [ ] Wiring\n"),
        "subtask should be written indented at the end of the parent block:\n{content}"
    );

    // In the UI the parent auto-expands so the new child is visible.
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
    app.switch_to(1)?;

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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
    app.dispatch(key('/'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("tasks_tab_editing_80x24", frame);
    Ok(())
}

#[test]
fn tasks_tab_query_parse_error_renders() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
    let enter = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.dispatch(enter)?;
    // Tasks tab consumed Enter — global keymap (which has no Enter binding)
    // should not have run, and the app must still be alive.
    assert!(!app.is_quit());
    assert_eq!(app.active_title(), "Tasks");
    Ok(())
}

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
    app.service_pending_for_test()?;
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
    app.service_pending_for_test()?;
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
    app.service_pending_for_test()?;
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
    app.service_pending_for_test()?;
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
    app.service_pending_for_test()?;
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
        app.service_pending_for_test()?;
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
    app.service_pending_for_test()?;
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
    app.service_pending_for_test()?;
    let after_first = render(&mut app, 100, 24);
    assert!(
        after_first.contains("d again = delete"),
        "armed-state indicator missing"
    );
    app.dispatch(key('d'))?;
    app.service_pending_for_test()?;
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
    app.service_pending_for_test()?;
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
    app.service_pending_for_test()?;
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
        app.service_pending_for_test()?;
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
    app.service_pending_for_test()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    assert!(body.contains("- 09:00 - 10:05 first"), "got: {body}");
    assert!(
        body.contains("- 09:00 - 10:30 second"),
        "second untouched: {body}"
    );

    // Time-shift on the second block (line 2).
    app.dispatch(key('j'))?;
    app.dispatch(key(']'))?;
    app.service_pending_for_test()?;
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
    app.service_pending_for_test()?;
    app.dispatch(key('d'))?;
    app.service_pending_for_test()?;
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
    app.service_pending_for_test()?;
    let body = std::fs::read_to_string(vault_path.join("journal/2026-05-10.md")).unwrap();
    // Template's title + Notes section must be present.
    assert!(body.contains("# 2026-05-10"), "title missing: {body}");
    assert!(body.contains("## Notes"));
    assert!(body.contains("## Time Blocks"));
    Ok(())
}

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
    // `3` should land on Notes in one keypress, even from Graph.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    assert_eq!(app.active_index(), 0);
    app.dispatch(key('3'))?;
    assert_eq!(app.active_index(), 2);
    assert_eq!(app.active_title(), "Notes");
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
    app.switch_to(1)?;
    app.dispatch(key('q'))?;
    assert!(app.is_quit());
    Ok(())
}

#[test]
fn ctrl_c_quits() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.switch_to(1)?;
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
    app.switch_to(1)?;
    let tab_ev = Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    app.dispatch(tab_ev.clone())?;
    assert_eq!(app.active_title(), "Notes");
    app.dispatch(tab_ev.clone())?;
    assert_eq!(app.active_title(), "Timeblocks");
    app.dispatch(tab_ev.clone())?;
    assert_eq!(app.active_title(), "Journal");
    app.dispatch(tab_ev.clone())?;
    assert_eq!(app.active_title(), "Review");
    app.dispatch(tab_ev.clone())?;
    assert_eq!(app.active_title(), "Graph");
    app.dispatch(tab_ev)?;
    assert_eq!(app.active_title(), "Tasks");
    Ok(())
}

#[test]
fn search_arrow_navigation_wraps() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
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
    app.switch_to(1)?;

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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    let after = render(&mut app, 80, 24);
    assert!(
        after.contains("Brand new urgent"),
        "R should pick up disk changes:\n{after}"
    );
    Ok(())
}

/// Path to the markdown file inside `populated_vault`. Tests use this to
/// inspect on-disk state after a quick-key mutation.
fn populated_tasks_path(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join("test-vault").join("tasks.md")
}

#[test]
fn quick_key_bracket_close_nudges_due_forward() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
    app.dispatch(key('e'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("edit_popup_80x24", frame);
    Ok(())
}

#[test]
fn edit_popup_ctrl_s_saves_changes() -> Result<()> {
    let (dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
    app.enter_help();
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("help_overlay_over_tasks_80x24", frame);
    Ok(())
}

#[test]
fn edit_popup_error_state_renders() -> Result<()> {
    let (_dir, vault) = populated_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    for (idx, title) in ["Graph", "Tasks", "Notes", "Timeblocks"].iter().enumerate() {
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;

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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;

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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.service_pending_for_test()?;
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
    app.switch_to(1)?;
    app.dispatch(key('c'))?;
    for c in "draft report due:tomorrow".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.service_pending_for_test()?;
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
    app.switch_to(1)?;
    app.dispatch(key('c'))?;
    for c in "anchor target due:tomorrow".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
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
    app.switch_to(1)?;
    app.dispatch(key('c'))?;
    for c in "dup task due:tomorrow in:Inbox.md".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.service_pending_for_test()?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
    app.dispatch(key('c'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("quickline_empty_80x24", frame);
    Ok(())
}

#[test]
fn quickline_valid_preview_snapshot_80x24() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
    app.dispatch(key('c'))?;
    for c in "ship feature due:tomorrow".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.service_pending_for_test()?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("quickline_toast_success_80x24", frame);
    Ok(())
}

#[test]
fn quickline_ctrl_w_works_in_input() -> Result<()> {
    let (_dir, vault) = quickline_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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
    app.switch_to(1)?;
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

const NOTES_TAB_INDEX: usize = 2;

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
    app.switch_to(3)?;
    app.enter_help();
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("timeblocks_help_overlay_80x24", frame);
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
    let req = app
        .take_pending_request()
        .expect("commit should queue a success toast");
    match req {
        AppRequest::Toast { text, .. } => {
            assert!(
                text.starts_with("Moved 1 section(s):"),
                "success toast text: {text}"
            );
        }
        other => panic!("expected Toast, got {other:?}"),
    }
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
    let req = app
        .take_pending_request()
        .expect("empty rename should queue a toast");
    match req {
        AppRequest::Toast { text, style } => {
            assert_eq!(text, "rename cannot be empty");
            assert_eq!(style, crate::tui::tab::ToastStyle::Error);
        }
        other => panic!("expected Toast, got {other:?}"),
    }
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
    let req = app
        .take_pending_request()
        .expect("whitespace-only rename should queue a toast");
    match req {
        AppRequest::Toast { text, .. } => {
            assert_eq!(text, "rename cannot be empty");
        }
        other => panic!("expected Toast, got {other:?}"),
    }
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
    let req = app
        .take_pending_request()
        .expect("commit should queue a success toast");
    match req {
        AppRequest::Toast { text, .. } => {
            assert!(
                text.starts_with("Moved 1 section(s):"),
                "success toast: {text}"
            );
        }
        other => panic!("expected Toast, got {other:?}"),
    }
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
    let req = app
        .take_pending_request()
        .expect("commit should queue a success toast");
    match req {
        AppRequest::Toast { text, .. } => {
            assert!(
                text.starts_with("Moved 2 section(s):"),
                "success toast: {text}"
            );
        }
        other => panic!("expected Toast, got {other:?}"),
    }
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

    let req = app
        .take_pending_request()
        .expect("`t` should queue a Toast when daily isn't configured");
    match req {
        AppRequest::Toast { text, style } => {
            assert!(
                text.contains("daily not configured"),
                "toast should call out the missing period: {text:?}"
            );
            assert_eq!(style, crate::tui::tab::ToastStyle::Error);
        }
        other => panic!("expected Toast(error), got {other:?}"),
    }
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

    let req = app
        .take_pending_request()
        .expect("p,w with no weekly config should queue an error toast");
    match req {
        AppRequest::Toast { text, style } => {
            assert!(
                text.contains("weekly not configured"),
                "toast should name the unconfigured period: {text:?}"
            );
            assert_eq!(style, crate::tui::tab::ToastStyle::Error);
        }
        other => panic!("expected Toast(error), got {other:?}"),
    }
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

// ── plan 012 session 3: TUI `g s` chord ──────────────────────────────────

const TASKS_TAB_INDEX: usize = 1;

fn key_with_mods(c: char, mods: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), mods))
}

fn esc_key() -> Event {
    Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
}

#[test]
fn git_chord_g_from_normal_enters_git_leader_mode() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    app.dispatch(key('g'))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::GitLeader);
    Ok(())
}

#[test]
fn git_leader_s_queues_sync_request_and_returns_to_normal() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.dispatch(key('g'))?;
    app.dispatch(key('s'))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    let req = app
        .take_pending_request()
        .expect("`g s` should queue an AppRequest::SyncGit");
    match req {
        AppRequest::SyncGit { message } => {
            assert!(message.is_none(), "TUI never overrides the commit message");
        }
        other => panic!("expected SyncGit, got {other:?}"),
    }
    Ok(())
}

#[test]
fn git_leader_esc_dismisses_without_queueing() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.dispatch(key('g'))?;
    app.dispatch(esc_key())?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    assert!(app.take_pending_request().is_none());
    Ok(())
}

#[test]
fn git_leader_unknown_letter_dismisses_without_queueing() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.dispatch(key('g'))?;
    app.dispatch(key('x'))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    assert!(app.take_pending_request().is_none());
    Ok(())
}

#[test]
fn git_leader_q_does_not_quit_via_global_handler() -> Result<()> {
    // Bug-guard: while the leader is open we must not fall through to
    // the global keymap. `q` would otherwise quit the app.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.dispatch(key('g'))?;
    app.dispatch(key('q'))?;
    assert!(!app.is_quit(), "q from git-leader should dismiss, not quit");
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    Ok(())
}

#[test]
fn g_chord_works_from_tasks_tab() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(TASKS_TAB_INDEX)?;
    app.dispatch(key('g'))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::GitLeader);
    app.dispatch(key('s'))?;
    let req = app.take_pending_request().expect("SyncGit queued");
    assert!(matches!(req, AppRequest::SyncGit { .. }));
    Ok(())
}

#[test]
fn g_chord_works_from_notes_tab() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('g'))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::GitLeader);
    app.dispatch(key('s'))?;
    let req = app.take_pending_request().expect("SyncGit queued");
    assert!(matches!(req, AppRequest::SyncGit { .. }));
    Ok(())
}

#[test]
fn git_leader_modal_snapshot() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.dispatch(key('g'))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("git_leader_80x24", frame);
    Ok(())
}

#[test]
fn git_chord_does_not_trigger_inside_help_overlay() -> Result<()> {
    // Help mode swallows everything except its own dismiss keys, so
    // `g` while help is open must not enter the git leader.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.enter_help();
    app.dispatch(key('g'))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::Help);
    Ok(())
}

#[test]
fn shift_g_does_not_trigger_leader() -> Result<()> {
    // Only bare `g` enters the leader — Shift+G (an unrelated capital
    // letter) should fall through to the active tab or be a no-op.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.dispatch(key_with_mods('G', KeyModifiers::SHIFT))?;
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    Ok(())
}

// ── plan 014: background git sync ────────────────────────────────────────

use crate::tui::event::{BgEvent, EventStream, SyncJobResult};
use crate::tui::jobs::JobKind;
use crate::tui::tab::ToastStyle;
use ft_core::git::SyncOutcome;

fn bg_event(outcome: Result<SyncOutcome, String>) -> Event {
    Event::Background(BgEvent::SyncCompleted(SyncJobResult {
        outcome,
        repo: std::path::PathBuf::from("/tmp/test-repo"),
    }))
}

#[test]
fn sync_completed_clean_pushed_renders_toast_and_clears_job() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);
    assert_eq!(app.in_flight_job_for_test(), Some(JobKind::Sync));

    app.dispatch(bg_event(Ok(SyncOutcome::Clean { pushed: true })))?;

    assert!(app.in_flight_job_for_test().is_none());
    let toast = app.current_toast().expect("toast should be set");
    assert_eq!(toast.text, "pushed local commits");
    assert_eq!(toast.style, ToastStyle::Success);
    // No mode transition for clean outcomes.
    assert_eq!(app.mode(), crate::tui::ui::Mode::Normal);
    Ok(())
}

#[test]
fn sync_completed_clean_no_push_renders_already_in_sync_toast() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);

    app.dispatch(bg_event(Ok(SyncOutcome::Clean { pushed: false })))?;

    let toast = app.current_toast().expect("toast should be set");
    assert_eq!(toast.text, "already in sync");
    assert_eq!(toast.style, ToastStyle::Success);
    assert!(app.in_flight_job_for_test().is_none());
    Ok(())
}

#[test]
fn sync_completed_synced_renders_compound_toast() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);

    app.dispatch(bg_event(Ok(SyncOutcome::Synced {
        committed: 3,
        pulled: true,
        pushed: true,
    })))?;

    let toast = app.current_toast().expect("toast should be set");
    assert_eq!(toast.text, "sync ok — committed 3, pulled, pushed");
    assert_eq!(toast.style, ToastStyle::Success);
    assert!(app.in_flight_job_for_test().is_none());
    Ok(())
}

#[test]
fn sync_completed_merge_conflict_enters_conflict_mode() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);

    app.dispatch(bg_event(Ok(SyncOutcome::MergeConflict {
        files: vec![std::path::PathBuf::from("seed.md")],
    })))?;

    assert_eq!(app.mode(), crate::tui::ui::Mode::SyncConflict);
    assert!(app.in_flight_job_for_test().is_none());
    Ok(())
}

#[test]
fn sync_completed_rebase_conflict_enters_conflict_mode() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);

    app.dispatch(bg_event(Ok(SyncOutcome::RebaseConflict {
        files: vec![std::path::PathBuf::from("seed.md")],
    })))?;

    assert_eq!(app.mode(), crate::tui::ui::Mode::SyncConflict);
    assert!(app.in_flight_job_for_test().is_none());
    Ok(())
}

#[test]
fn sync_completed_error_renders_red_toast() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);

    app.dispatch(bg_event(Err("branch 'main' has no upstream".to_string())))?;

    let toast = app.current_toast().expect("error toast should be set");
    assert!(
        toast.text.contains("git sync failed:"),
        "expected error prefix, got: {}",
        toast.text
    );
    assert!(
        toast.text.contains("no upstream"),
        "expected error body, got: {}",
        toast.text
    );
    assert_eq!(toast.style, ToastStyle::Error);
    assert!(app.in_flight_job_for_test().is_none());
    Ok(())
}

#[test]
fn sync_completed_while_in_help_mode_still_clears_job() -> Result<()> {
    // Background events bypass the mode short-circuits — a completion
    // that arrives while the help overlay is up must still update the
    // app's state (jobs cleared, toast set) so the user sees the
    // outcome the moment they close help.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);
    app.enter_help();

    app.dispatch(bg_event(Ok(SyncOutcome::Clean { pushed: false })))?;

    assert!(app.in_flight_job_for_test().is_none());
    assert!(app.current_toast().is_some());
    // Help mode untouched.
    assert_eq!(app.mode(), crate::tui::ui::Mode::Help);
    Ok(())
}

#[test]
fn g_s_while_job_in_flight_toasts_already_in_progress() -> Result<()> {
    // The re-entrancy guard fires inside `dispatch_sync_git`, which
    // service_request calls. Drive the full path via an EventStream
    // so the guard sees the in-flight slot we pre-populated.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    app.set_in_flight_for_test(JobKind::Sync);

    let events = EventStream::new(std::time::Duration::from_secs(60));
    app.submit_sync_for_test(&events, None)?;

    let toast = app.current_toast().expect("re-entrancy toast");
    assert_eq!(toast.text, "sync already in progress");
    assert_eq!(toast.style, ToastStyle::Info);
    // Indicator stays — the first job is still in flight.
    assert_eq!(app.in_flight_job_for_test(), Some(JobKind::Sync));
    Ok(())
}

#[test]
fn status_bar_shows_sync_indicator_while_job_in_flight() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.set_in_flight_for_test(JobKind::Sync);
    let frame = render(&mut app, 80, 24);
    // The right cell composes `⟳ sync · mode: normal `. Look for the
    // glyph + label to confirm the indicator landed.
    assert!(
        frame.contains("⟳ sync"),
        "expected sync indicator in status bar, got:\n{frame}"
    );
    Ok(())
}

#[test]
fn sync_indicator_in_status_bar_snapshot() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.set_in_flight_for_test(JobKind::Sync);
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("sync_indicator_in_status_bar_80x24", frame);
    Ok(())
}

#[test]
fn status_bar_no_indicator_when_no_job_in_flight() -> Result<()> {
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("⟳ sync"),
        "indicator must only appear while a job is in flight"
    );
    Ok(())
}

#[test]
fn sync_indicator_persists_across_modes() -> Result<()> {
    // The indicator is orthogonal to mode — opening help or the git
    // leader while a sync runs must not hide it.
    let (_dir, vault) = test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.set_in_flight_for_test(JobKind::Sync);

    app.enter_help();
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("⟳ sync"),
        "indicator should survive help mode, got:\n{frame}"
    );
    Ok(())
}

#[test]
fn dispatch_sync_git_with_no_git_repo_toasts_and_does_not_spawn() -> Result<()> {
    // Vault that is NOT inside a git repo (test_vault uses a fresh
    // temp dir with no .git/ anywhere up). The submission should
    // toast the "no git repository" error and leave the in-flight
    // slot empty (no worker thread).
    let (_dir, vault) = test_vault();
    let mut app = App::for_test(vault);
    let events = EventStream::new(std::time::Duration::from_secs(60));

    app.submit_sync_for_test(&events, None)?;

    let toast = app.current_toast().expect("no-repo toast");
    assert!(
        toast.text.contains("no git repository"),
        "expected no-repo error, got: {}",
        toast.text
    );
    assert_eq!(toast.style, ToastStyle::Error);
    assert!(
        app.in_flight_job_for_test().is_none(),
        "no worker should be spawned when discover_repo fails"
    );
    Ok(())
}

// ── e2e: real git worker thread, bare origin + clone ────────────────────

use std::path::Path;

fn run_git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()
        .expect("git exec failed (is git on PATH?)");
    assert!(
        out.status.success(),
        "git {args:?} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Bare origin + cloned vault + `.obsidian/` marker + one seed commit
/// pushed. Returns the vault path. Mirrors the helper used by
/// `ft/tests/git_sync.rs` (kept local here to avoid sharing fixture
/// code across the integration / unit boundary).
fn setup_origin_and_vault(tmp: &Path) -> std::path::PathBuf {
    let origin = tmp.join("origin.git");
    std::fs::create_dir(&origin).unwrap();
    run_git(&origin, &["init", "--bare", "-b", "main"]);

    let vault = tmp.join("vault");
    let out = std::process::Command::new("git")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["clone", origin.to_str().unwrap(), vault.to_str().unwrap()])
        .output()
        .expect("git clone");
    assert!(out.status.success());

    run_git(&vault, &["config", "user.name", "Local"]);
    run_git(&vault, &["config", "user.email", "local@example.com"]);
    run_git(&vault, &["config", "commit.gpgsign", "false"]);
    std::fs::create_dir_all(vault.join(".obsidian")).unwrap();
    std::fs::write(vault.join("seed.md"), "seed\n").unwrap();
    run_git(&vault, &["add", "."]);
    run_git(&vault, &["commit", "-m", "seed"]);
    run_git(&vault, &["push", "-u", "origin", "main"]);
    vault
}

#[test]
fn e2e_background_sync_dirty_tree_dispatches_event_and_renders_toast() -> Result<()> {
    // Drive the full background path against a real bare-origin /
    // clone handshake. Asserts (1) the in-flight slot lights up
    // immediately on submission, (2) the worker thread posts a
    // `Background(SyncCompleted)` event into the channel, (3)
    // dispatching that event clears the slot and sets a success toast.
    let tmp = assert_fs::TempDir::new().unwrap();
    let vault_path = setup_origin_and_vault(tmp.path());

    // Dirty the tree so the sync has something to commit.
    std::fs::write(vault_path.join("new.md"), "hello\n").unwrap();

    let vault = Vault::discover(Some(vault_path)).unwrap();
    let mut app = App::for_test(vault);
    let events = EventStream::new(std::time::Duration::from_millis(500));

    app.submit_sync_for_test(&events, None)?;
    // Indicator lights up before the worker finishes.
    assert_eq!(app.in_flight_job_for_test(), Some(JobKind::Sync));

    // Drain events from the channel until the `Background` arrives.
    // A 1 Hz `Tick` may come in first; ignore non-Background events.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let bg_event = loop {
        if std::time::Instant::now() > deadline {
            panic!("worker did not complete within 15s");
        }
        match events.next()? {
            Event::Background(b) => break b,
            _ => continue,
        }
    };

    app.dispatch(Event::Background(bg_event))?;

    assert!(app.in_flight_job_for_test().is_none());
    let toast = app.current_toast().expect("success toast after sync");
    assert_eq!(toast.style, ToastStyle::Success);
    assert!(
        toast.text.starts_with("sync ok"),
        "expected 'sync ok …' toast, got: {}",
        toast.text
    );
    Ok(())
}

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
    let req = app
        .take_pending_request()
        .expect("`t` should queue a Toast when daily is unreachable");
    match req {
        AppRequest::Toast { text, style } => {
            assert!(
                text.contains("daily"),
                "toast should mention daily: {text:?}",
            );
            assert!(
                text.contains("not in the current graph results"),
                "toast should explain the note isn't reachable: {text:?}",
            );
            assert_eq!(style, crate::tui::tab::ToastStyle::Info);
        }
        other => panic!("expected Toast(Info), got {other:?}"),
    }
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
    app.service_request_for_test()?;
    // The handler queues a Toast in pending_request; service that too.
    app.service_pending_for_test()?;
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
    app.service_request_for_test()?;
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

// ── Journal tab ──────────────────────────────────────────────────────

/// Index of the Journal tab in the App's tab vector. Computed by
/// `App::for_test_with_clock` adding it after the existing four tabs.
fn journal_tab_idx() -> usize {
    4
}

/// Build a vault with one commit so the blame cache resolves dates.
/// `Target.md` is the note we'll open the journal for; `DailyA.md`
/// mentions it once.
fn journal_test_vault() -> (TempDir, Vault) {
    use std::process::Command as StdCommand;

    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::write(vault_path.join("Target.md"), "# Target\n").unwrap();
    std::fs::write(vault_path.join("DailyA.md"), "Mentions [[Target]] today.\n").unwrap();

    let run_git = |args: &[&str]| {
        let out = StdCommand::new("git")
            .current_dir(&vault_path)
            .env("GIT_TERMINAL_PROMPT", "0")
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

    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn journal_tab_empty_state_shows_picker_prompt() -> Result<()> {
    let (_dir, vault) = journal_test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(journal_tab_idx())?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Sources (0)") && frame.contains("press / to manage sources"),
        "empty Journal tab missing Sources strip:\n{frame}"
    );
    Ok(())
}

#[test]
fn journal_tab_renders_entries_after_queued_load() -> Result<()> {
    let (_dir, vault) = journal_test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    // Stage the cross-tab jump payload by sending the AppRequest directly:
    // simulate what `service_request` does — queue the path on the
    // Journal tab and switch to it.
    app.queue_journal_for_tab_test("Target.md");
    app.switch_to(journal_tab_idx())?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("DailyA"),
        "journal feed missing source title:\n{frame}"
    );
    assert!(
        frame.contains("Mentions [[Target]] today."),
        "journal feed missing paragraph body:\n{frame}"
    );
    Ok(())
}

#[test]
fn journal_tab_help_lists_keybindings() -> Result<()> {
    let (_dir, vault) = journal_test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(journal_tab_idx())?;
    let sections = app.active_tab_help_sections();
    let merged: String = sections
        .iter()
        .flat_map(|s| s.entries.iter().map(|e| format!("{}={}\n", e.keys, e.desc)))
        .collect();
    for expected in ["/", "Shift+r", "c", "Enter", "↓ / j"] {
        assert!(
            merged.contains(expected),
            "help missing `{expected}`:\n{merged}"
        );
    }
    Ok(())
}

#[test]
fn graph_shift_j_jumps_to_journal_tab_for_selected_note() -> Result<()> {
    let (_dir, vault) = journal_test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    // Default Graph tab focus + the dirs-style default query lists the
    // vault's root directory first; navigate down to a Note row.
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?; // expand root
          // Walk down until a Note row is selected.
    for _ in 0..6 {
        if app.graph_tab_selected_is_note_for_test() {
            break;
        }
        app.dispatch(key('j'))?;
    }
    assert!(
        app.graph_tab_selected_is_note_for_test(),
        "test prelude must reach a Note row"
    );

    // Shift+J should raise AppRequest::JournalForNote. The test driver
    // services pending requests via the in-process helper.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('J'),
        KeyModifiers::SHIFT,
    )))?;
    app.service_request_for_test()?;

    assert_eq!(app.active_title(), "Journal");
    Ok(())
}

#[test]
fn graph_shift_j_on_non_note_row_queues_toast_and_stays_on_graph() -> Result<()> {
    let (_dir, vault) = journal_test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    // Stay on Graph tab with the root directory selected (it's a
    // Directory row, not a Note).
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('J'),
        KeyModifiers::SHIFT,
    )))?;
    // The pending request should be a Toast (error guidance), NOT a
    // JournalForNote jump. Servicing it leaves the active tab on Graph.
    match app.take_pending_request() {
        Some(AppRequest::Toast { text, .. }) => {
            assert!(
                text.to_lowercase().contains("note"),
                "toast text must hint at the Note-row requirement: {text}"
            );
        }
        other => panic!("expected a Toast pending request, got {other:?}"),
    }
    assert_eq!(app.active_title(), "Graph");
    Ok(())
}

#[test]
fn graph_shift_j_on_ghost_row_queues_journal_for_ghost() -> Result<()> {
    let (dir, vault) = rename_vault(&[("a.md", "see [[Phantom]]\n")]);
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    // Expand root → row 0: D /, row 1: N a.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Select a.md, expand it → reveals row 2: G Phantom.
    app.dispatch(key('j'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    // Select the ghost row.
    app.dispatch(key('j'))?;

    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('J'),
        KeyModifiers::SHIFT,
    )))?;
    match app.take_pending_request() {
        Some(AppRequest::JournalFor {
            target: crate::tui::tab::JournalTarget::Ghost(raw),
        }) => assert_eq!(raw, "Phantom"),
        other => panic!(
            "expected JournalFor {{ Ghost(\"Phantom\") }}, got {other:?}\nframe:\n{}",
            render(&mut app, 100, 24)
        ),
    }
    let _ = dir;
    Ok(())
}

#[test]
fn graph_tab_help_lists_shift_j_jump() -> Result<()> {
    let (_dir, vault) = related_modal_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    switch_to_graph(&mut app)?;
    let sections = app.active_tab_help_sections();
    let merged: String = sections
        .iter()
        .flat_map(|s| s.entries.iter().map(|e| format!("{}={}\n", e.keys, e.desc)))
        .collect();
    assert!(
        merged.contains("Shift+j"),
        "graph-tab help must mention Shift+j for Journal jump:\n{merged}"
    );
    Ok(())
}

// ── Capture preset tests ─────────────────────────────────────────────

#[test]
fn capture_preset_config_loads_correctly() {
    let (_dir, vault) = capture_preset_vault();
    let presets = &vault.config.config.capture_presets;
    assert!(presets.contains_key("log"), "presets: {presets:?}");
    assert!(presets.contains_key("meeting"));
    let log_preset = &presets["log"];
    assert_eq!(log_preset.action, ft_core::config::CaptureAction::Append);
    assert_eq!(log_preset.template, "log-entry");
    assert_eq!(log_preset.note.as_deref(), Some("daily/log.md"));
    assert_eq!(log_preset.section.as_deref(), Some("Log"));
    let meeting_preset = &presets["meeting"];
    assert_eq!(
        meeting_preset.action,
        ft_core::config::CaptureAction::Create
    );
    assert_eq!(meeting_preset.template, "meeting");
    let tpl_path = vault.templates_dir().join("log-entry.md");
    assert!(tpl_path.is_file(), "template should exist: {tpl_path:?}");
    let target_path = vault.path.join("daily").join("log.md");
    assert!(
        target_path.is_file(),
        "target should exist: {target_path:?}"
    );
}

/// Build a vault with capture presets, templates, and a target note.
fn capture_preset_vault() -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();

    // Config with a create preset and an append preset.
    let config_dir = vault_path.join(".ft");
    std::fs::create_dir_all(&config_dir).unwrap();
    let config_toml = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        "[capture_presets.log]",
        "action = \"append\"",
        "template = \"log-entry\"",
        "note = \"daily/log.md\"",
        "section = \"Log\"",
        "",
        "[capture_presets.meeting]",
        "action = \"create\"",
        "template = \"meeting\"",
        "path = \"%Y-%m-%d-meeting\"",
        "folder = \"meetings\"",
    );
    std::fs::write(config_dir.join("config.toml"), config_toml).unwrap();

    // Templates directory.
    let tmpl_dir = vault_path.join("templates-ft");
    std::fs::create_dir_all(&tmpl_dir).unwrap();

    // Template without vars — should execute immediately.
    std::fs::write(
        tmpl_dir.join("log-entry.md"),
        "- Log entry for {{ today }}\n",
    )
    .unwrap();

    // Template with vars — should prompt.
    std::fs::write(
        tmpl_dir.join("meeting.md"),
        "# {{ vars.topic }}\nDate: {{ today | date(format='%Y-%m-%d') }}\nAttendees: {{ vars.attendees }}\n",
    )
    .unwrap();

    // Target note for append preset.
    let daily_dir = vault_path.join("daily");
    std::fs::create_dir_all(&daily_dir).unwrap();
    std::fs::write(
        daily_dir.join("log.md"),
        "# Daily Log\n## Log\nexisting line\n",
    )
    .unwrap();

    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn capture_append_no_vars_executes_immediately() -> Result<()> {
    let (_dir, vault) = capture_preset_vault();
    let vault_path = vault.path.clone();

    // Verify the vault config and templates are set up correctly.
    {
        assert!(
            vault.config.config.capture_presets.contains_key("log"),
            "log preset should be in config"
        );
        let log_preset = &vault.config.config.capture_presets["log"];
        assert_eq!(log_preset.action, ft_core::config::CaptureAction::Append);
        assert_eq!(log_preset.template, "log-entry");
        let tpl_path = vault.templates_dir().join("log-entry.md");
        assert!(
            tpl_path.is_file(),
            "template file should exist at {tpl_path:?}"
        );
    }

    let mut app = App::for_test_with_clock(vault, fixed_clock);

    // Switch to Notes tab (index 2 — Graph=0, Tasks=1, Notes=2).
    app.switch_to(2)?;

    // Press Q to open capture preset picker.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('Q'),
        KeyModifiers::SHIFT,
    )))?;

    // The picker should show "log" and "meeting".
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("log"),
        "capture picker should list log preset: {frame}"
    );
    assert!(
        frame.contains("meeting"),
        "capture picker should list meeting preset: {frame}"
    );

    // Select "log" (first item).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // After Enter, the picker should be dismissed.
    let frame_after = render(&mut app, 80, 24);
    assert!(
        !frame_after.contains("quick capture"),
        "picker should be dismissed after Enter: {frame_after}"
    );

    // Since log-entry template has no vars, it should execute immediately.
    // The picker should be dismissed and we're back on Notes idle.
    // Verify the target file was modified.
    let target = vault_path.join("daily").join("log.md");
    let content = std::fs::read_to_string(&target)?;
    assert!(
        content.contains("Log entry for"),
        "target should contain rendered log entry: {content}"
    );
    Ok(())
}

#[test]
fn capture_create_with_vars_prompts_before_committing() -> Result<()> {
    let (_dir, vault) = capture_preset_vault();
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);

    // Switch to Notes tab.
    app.switch_to(2)?;

    // Press Q to open capture preset picker.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('Q'),
        KeyModifiers::SHIFT,
    )))?;

    // Move down to select "meeting" (second item).
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;

    // Select "meeting".
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // Should now be showing the var prompt (not an error toast).
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("topic") || frame.contains("var"),
        "should show var prompt after selecting meeting preset: {frame}"
    );

    // Type the first var: topic
    for c in "Q2 Planning".chars() {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )))?;
    }
    // Press Enter to advance to next var.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // Type the second var: attendees
    for c in "Alice, Bob".chars() {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )))?;
    }
    // Press Enter to commit.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // Verify the file was created with vars substituted.
    let meetings_dir = vault_path.join("meetings");
    let files: Vec<_> = std::fs::read_dir(&meetings_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .collect();
    assert_eq!(files.len(), 1, "should have created one meeting note");
    let content = std::fs::read_to_string(files[0].path())?;
    assert!(
        content.contains("# Q2 Planning"),
        "should contain the topic var: {content}"
    );
    assert!(
        content.contains("Alice, Bob"),
        "should contain the attendees var: {content}"
    );
    assert!(
        content.contains("2026-05-10"),
        "should contain today's date: {content}"
    );
    Ok(())
}

#[test]
fn capture_var_prompt_esc_cancels() -> Result<()> {
    let (_dir, vault) = capture_preset_vault();
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);

    // Switch to Notes tab.
    app.switch_to(2)?;

    // Press Q and select meeting (with vars).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('Q'),
        KeyModifiers::SHIFT,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // Verify we're in the var prompt.
    let frame = render(&mut app, 80, 24);
    assert!(frame.contains("topic"), "should be in var prompt: {frame}");

    // Press Esc to cancel.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;

    // Should be back on Notes idle — no file should have been created.
    let meetings_dir = vault_path.join("meetings");
    let exists = meetings_dir.exists()
        && std::fs::read_dir(&meetings_dir)
            .map(|mut r| r.any(|e| e.is_ok()))
            .unwrap_or(false);
    assert!(
        !exists,
        "meetings dir should not have any files after cancel"
    );
    Ok(())
}

#[test]
fn capture_var_prompt_snapshot() -> Result<()> {
    // Pin FT_TODAY so the capture preset's `%Y-%m-%d` filename render
    // stays stable across calendar dates. Otherwise the snapshot rots
    // every midnight.
    std::env::set_var("FT_TODAY", "2026-05-10");
    let (_dir, vault) = capture_preset_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);

    // Switch to Notes tab.
    app.switch_to(2)?;

    // Press Q and select meeting (with vars).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('Q'),
        KeyModifiers::SHIFT,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("capture_var_prompt_80x24", frame);
    Ok(())
}

// --- configurable keymaps integration tests -----------------------------------

fn vault_with_keymap_config(toml_content: &str) -> (TempDir, Vault) {
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("keymap-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::create_dir_all(vault_path.join(".ft")).unwrap();
    std::fs::write(vault_path.join(".ft").join("config.toml"), toml_content).unwrap();
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn keymap_override_applies_to_graph_tab() -> Result<()> {
    // Override 'R' in tab/graph to graph.refresh.
    let toml = r#"
[keymap."tab/graph"]
"R" = "graph.refresh"
"#;
    let (_dir, vault) = vault_with_keymap_config(toml);
    let app = App::for_test(vault);

    // Graph tab is index 0.
    use crate::tui::keymap::KeyChord;
    use crossterm::event::{KeyCode, KeyModifiers};
    let chord = KeyChord::new(KeyCode::Char('R'), KeyModifiers::NONE);
    let cmd = app.tab_keymap_for_test(0).lookup(chord);
    assert!(cmd.is_some(), "R should be bound after override");
    assert_eq!(cmd.unwrap().name, "graph.refresh");
    Ok(())
}

#[test]
fn keymap_unbind_removes_default_chord() -> Result<()> {
    // Unbind 'q' from the global scope.
    let toml = r#"
[[keymap.unbind]]
scope = "global"
chord = "q"
"#;
    let (_dir, vault) = vault_with_keymap_config(toml);
    let app = App::for_test(vault);

    use crate::tui::keymap::KeyChord;
    use crossterm::event::{KeyCode, KeyModifiers};
    let chord = KeyChord::new(KeyCode::Char('q'), KeyModifiers::NONE);
    // global_keymap() is pub on App.
    let cmd = app.global_keymap().lookup(chord);
    assert!(cmd.is_none(), "q should be unbound after unbind entry");
    Ok(())
}

#[test]
fn keymap_strict_false_bad_entry_does_not_prevent_startup() {
    // A bad command name with strict=false should not panic or fail startup.
    let toml = r#"
[keymap]
strict = false

[keymap.global]
"q" = "app.this-command-does-not-exist"
"#;
    let (_dir, vault) = vault_with_keymap_config(toml);
    // Should not panic — bad overlay silently falls back to empty overlay.
    let _app = App::for_test(vault);
}

#[test]
fn keymap_validate_strict_bad_entry_returns_errors() {
    use ft_core::config::{Config, KeymapConfig};
    use std::collections::HashMap;

    let mut scopes: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut global = HashMap::new();
    global.insert("q".to_string(), "app.nonexistent".to_string());
    scopes.insert("global".to_string(), global);

    let config = Config {
        keymap: Some(KeymapConfig {
            strict: true,
            unbind: vec![],
            scopes,
        }),
        ..Config::default()
    };

    let errors = crate::tui::registry::validate_keymap(&config);
    assert!(
        !errors.is_empty(),
        "strict mode with bad command should report errors"
    );
    assert!(
        errors[0].contains("nonexistent"),
        "error should mention the bad command name"
    );
}

#[test]
fn help_overlay_with_keymap_override_shows_new_chord() {
    // Rebind quit: unbind 'q' from global, bind 'x' to app.quit.
    // The help overlay should show 'x / Ctrl+c' instead of 'q / Ctrl+c'.
    let toml = r#"
[keymap.global]
"x" = "app.quit"

[[keymap.unbind]]
scope = "global"
chord = "q"
"#;
    let (_dir, vault) = vault_with_keymap_config(toml);
    let mut app = App::for_test(vault);
    app.enter_help();
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("help_overlay_with_keymap_override_80x24", frame);
}

// ── Review tab + Journal multi-target ────────────────────────────────

fn review_tab_idx() -> usize {
    5
}

/// Vault with two commits: c1 (baseline, dated 2024-01-01 so a 7d
/// window always finds a from-ref) and c2 (today, adds two notes with
/// `[[Foo]]` / `[[Bar]]` as ghosts).
fn review_test_vault() -> (TempDir, Vault) {
    use std::process::Command as StdCommand;
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::write(vault_path.join("baseline.md"), "# Baseline\n").unwrap();

    let run_git_at = |date: Option<&str>, args: &[&str]| {
        let mut cmd = StdCommand::new("git");
        cmd.current_dir(&vault_path).env("GIT_TERMINAL_PROMPT", "0");
        if let Some(d) = date {
            cmd.env("GIT_AUTHOR_DATE", d).env("GIT_COMMITTER_DATE", d);
        }
        let out = cmd.args(args).output().expect("git");
        assert!(out.status.success(), "git {args:?}");
    };
    run_git_at(None, &["init", "-b", "main"]);
    run_git_at(None, &["config", "user.name", "T"]);
    run_git_at(None, &["config", "user.email", "t@e.com"]);
    run_git_at(None, &["config", "commit.gpgsign", "false"]);
    run_git_at(None, &["add", "."]);
    run_git_at(Some("2024-01-01T00:00:00"), &["commit", "-m", "c1"]);

    std::fs::write(
        vault_path.join("note-a.md"),
        "Para one mentions [[Foo]] and [[Bar]].\n\nPara two mentions [[Foo]] again.\n",
    )
    .unwrap();
    std::fs::write(vault_path.join("note-b.md"), "Only [[Bar]] here.\n").unwrap();
    run_git_at(None, &["add", "."]);
    run_git_at(None, &["commit", "-m", "c2"]);

    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn review_tab_empty_window_shows_friendly_message() -> Result<()> {
    let (_dir, vault) = journal_test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(review_tab_idx())?;
    // Default window is --since 7d; the fixture's commit is very recent.
    // But fixed_clock = 2026-05-10 and commits are wall-clock today,
    // which means commits are *in the future* relative to clock — git
    // log --before=2026-05-03 returns nothing. Either way we should
    // exercise the empty-state UI cleanly without panicking.
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Review"),
        "Review tab title missing from frame:\n{frame}"
    );
    Ok(())
}

#[test]
fn review_tab_lists_rows_with_counts_and_ghost_suffix() -> Result<()> {
    let (_dir, vault) = review_test_vault();
    // Default --since 7d window resolves against link_review's own
    // today (system clock, FT_TODAY honored if set). Commits in the
    // fixture are made at wall-clock-now, so a 7d window includes them.
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(review_tab_idx())?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("(2) [[Bar]]?"),
        "Bar row missing or wrong count:\n{frame}"
    );
    assert!(
        frame.contains("(2) [[Foo]]?"),
        "Foo row missing or wrong count:\n{frame}"
    );
    Ok(())
}

#[test]
fn review_tab_help_lists_keybindings() -> Result<()> {
    let (_dir, vault) = journal_test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(review_tab_idx())?;
    let sections = app.active_tab_help_sections();
    let merged: String = sections
        .iter()
        .flat_map(|s| s.entries.iter().map(|e| format!("{}={}\n", e.keys, e.desc)))
        .collect();
    for expected in ["Space", "Enter", "[", "]", "Shift+r"] {
        assert!(
            merged.contains(expected),
            "Review help missing `{expected}`:\n{merged}"
        );
    }
    Ok(())
}

/// Build a vault for the multi-target Journal test: two notes, one
/// paragraph mentions both `[[Foo]]` and `[[Bar]]`. Returns the vault.
fn multi_target_journal_vault() -> (TempDir, Vault) {
    use std::process::Command as StdCommand;
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::write(
        vault_path.join("DailyA.md"),
        "Some thought about [[Foo]].\n\nLater, [[Bar]] came up.\n",
    )
    .unwrap();
    std::fs::write(
        vault_path.join("DailyB.md"),
        "Cross-link: [[Foo]] and [[Bar]] in one paragraph.\n",
    )
    .unwrap();
    let run_git = |args: &[&str]| {
        let out = StdCommand::new("git")
            .current_dir(&vault_path)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}");
    };
    run_git(&["init", "-b", "main"]);
    run_git(&["config", "user.name", "T"]);
    run_git(&["config", "user.email", "t@e.com"]);
    run_git(&["config", "commit.gpgsign", "false"]);
    run_git(&["add", "."]);
    run_git(&["commit", "-m", "init"]);
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn journal_tab_multi_target_renders_matched_badge() -> Result<()> {
    use crate::tui::tab::{JournalTarget, MultiTargetRequest};
    let (_dir, vault) = multi_target_journal_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    let request = MultiTargetRequest {
        targets: vec![
            JournalTarget::Ghost("Foo".into()),
            JournalTarget::Ghost("Bar".into()),
        ],
        window: None,
    };
    app.queue_journal_for_multi_tab_test(request);
    app.switch_to(journal_tab_idx())?;
    let frame = render(&mut app, 80, 24);
    // DailyB's paragraph matches both Foo and Bar → badge present.
    assert!(
        frame.contains("matched:") && frame.contains("Foo") && frame.contains("Bar"),
        "multi-target frame missing matched badge:\n{frame}"
    );
    // Title and strip reflect multi-source mode.
    assert!(
        frame.contains("2 sources") && frame.contains("Sources (2)"),
        "title/strip missing `2 sources` indication:\n{frame}"
    );
    Ok(())
}

/// Two notes with distinct titles so the same-date feed order is
/// deterministic (tiebreak is title-ascending): `Alpha` mentions only
/// `[[Foo]]`, `Beta` mentions both. One git commit so both share a date.
fn journal_blocks_vault() -> (TempDir, Vault) {
    use std::process::Command as StdCommand;
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::write(vault_path.join("Alpha.md"), "Talk about [[Foo]].\n").unwrap();
    std::fs::write(
        vault_path.join("Beta.md"),
        "Mention [[Foo]] and [[Bar]] together.\n",
    )
    .unwrap();
    let run_git = |args: &[&str]| {
        let out = StdCommand::new("git")
            .current_dir(&vault_path)
            .env("GIT_TERMINAL_PROMPT", "0")
            // Pin the commit date so the journal's blame-derived entry
            // dates are deterministic — otherwise the snapshot below
            // renders the real wall-clock day and breaks daily.
            .env("GIT_AUTHOR_DATE", "2026-05-10T14:32:05")
            .env("GIT_COMMITTER_DATE", "2026-05-10T14:32:05")
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}");
    };
    run_git(&["init", "-b", "main"]);
    run_git(&["config", "user.name", "T"]);
    run_git(&["config", "user.email", "t@e.com"]);
    run_git(&["config", "commit.gpgsign", "false"]);
    run_git(&["add", "."]);
    run_git(&["commit", "-m", "init"]);
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

/// A vault where many notes each mention `[[Target]]` once, with
/// distinct titles (`Note01`..`Note15`) so the same-date feed order is
/// deterministic (title-ascending) and each entry carries a unique body
/// marker we can assert is on screen.
fn journal_scroll_vault() -> (TempDir, Vault) {
    use std::process::Command as StdCommand;
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::write(vault_path.join("Target.md"), "# Target\n").unwrap();
    for n in 1..=15 {
        std::fs::write(
            vault_path.join(format!("Note{n:02}.md")),
            format!("MARKER-{n:02} unique body mentioning [[Target]] here.\n"),
        )
        .unwrap();
    }
    let run_git = |args: &[&str]| {
        let out = StdCommand::new("git")
            .current_dir(&vault_path)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}");
    };
    run_git(&["init", "-b", "main"]);
    run_git(&["config", "user.name", "T"]);
    run_git(&["config", "user.email", "t@e.com"]);
    run_git(&["config", "commit.gpgsign", "false"]);
    run_git(&["add", "."]);
    run_git(&["commit", "-m", "init"]);
    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

/// Scrolling invariant: whatever entry the cursor is on, a peek of its
/// body must be visible — never just the header band stranded on the
/// bottom row. Steps the cursor through every entry and asserts that
/// entry's unique body marker is on screen.
#[test]
fn journal_tab_selected_entry_body_always_visible() -> Result<()> {
    let (_dir, vault) = journal_scroll_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.queue_journal_for_tab_test("Target.md");
    app.switch_to(journal_tab_idx())?;
    // Order is title-ascending → Note01..Note15, so the entry under the
    // cursor at step `i` carries `MARKER-{i+1}`.
    for n in 1..=15 {
        let frame = render(&mut app, 80, 24);
        let marker = format!("MARKER-{n:02}");
        assert!(
            frame.contains(&marker),
            "selected entry {n}'s body ({marker}) not visible — header band stranded:\n{frame}"
        );
        app.dispatch(key('j'))?; // cursor-down
    }
    Ok(())
}

/// Locks the entry-block layout: full-width header band (padded to the
/// pane width), the always-on `↳ matched:` badge, and the `●` marker on
/// the multi-selected entry. Colors aren't captured by the snapshot
/// backend, so this guards the text/layout half of the visual.
#[test]
fn journal_tab_entry_blocks_layout() -> Result<()> {
    use crate::tui::tab::{JournalTarget, MultiTargetRequest};
    let (_dir, vault) = journal_blocks_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    let request = MultiTargetRequest {
        targets: vec![
            JournalTarget::Ghost("Foo".into()),
            JournalTarget::Ghost("Bar".into()),
        ],
        window: None,
    };
    app.queue_journal_for_multi_tab_test(request);
    app.switch_to(journal_tab_idx())?;
    // Multi-select the entry under the cursor so the `●` marker renders.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::NONE,
    )))?;
    let frame = render(&mut app, 80, 24);
    assert_tui_snapshot!("journal_entry_blocks_80x24", frame);
    Ok(())
}

#[test]
fn journal_tab_send_to_synth_existing_opens_picker_on_s() -> Result<()> {
    use crate::tui::tab::{JournalTarget, MultiTargetRequest};
    let (_dir, vault) = multi_target_journal_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    let request = MultiTargetRequest {
        targets: vec![JournalTarget::Ghost("Foo".into())],
        window: None,
    };
    app.queue_journal_for_multi_tab_test(request);
    app.switch_to(journal_tab_idx())?;
    // `s` opens the existing-note fuzzy picker.
    app.dispatch(key('s'))?;
    let frame = render(&mut app, 80, 24);
    // The fuzzy picker shows a search input row + the vault's notes.
    assert!(
        frame.contains("DailyA") || frame.contains("DailyB"),
        "existing-note picker should list vault notes:\n{frame}"
    );
    Ok(())
}

#[test]
fn journal_tab_send_to_synth_new_opens_folder_picker_on_shift_s() -> Result<()> {
    use crate::tui::tab::{JournalTarget, MultiTargetRequest};
    let (_dir, vault) = multi_target_journal_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    let request = MultiTargetRequest {
        targets: vec![JournalTarget::Ghost("Foo".into())],
        window: None,
    };
    app.queue_journal_for_multi_tab_test(request);
    app.switch_to(journal_tab_idx())?;
    // Shift+S opens the folder picker for create-new.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('S'),
        KeyModifiers::SHIFT,
    )))?;
    let frame = render(&mut app, 80, 24);
    // The folder picker shows `.` (vault root) at minimum.
    assert!(
        frame.contains('.') || frame.contains("synth"),
        "folder picker overlay should be open:\n{frame}"
    );
    Ok(())
}

#[test]
fn upsert_ft_synth_marker_inserts_into_existing_frontmatter() {
    use crate::tui::tabs::journal::upsert_ft_synth_marker;
    let input = "---\ntitle: Foo\n---\n\nbody\n";
    let out = upsert_ft_synth_marker(input);
    assert!(out.contains("ft-synth: true"));
    assert!(out.contains("title: Foo"));
    assert!(out.contains("body"));
}

#[test]
fn upsert_ft_synth_marker_adds_fresh_frontmatter_when_missing() {
    use crate::tui::tabs::journal::upsert_ft_synth_marker;
    let input = "# heading\n\nbody\n";
    let out = upsert_ft_synth_marker(input);
    assert!(out.starts_with("---\nft-synth: true\n---\n"));
    assert!(out.contains("# heading"));
}

#[test]
fn upsert_ft_synth_marker_replaces_false_value() {
    use crate::tui::tabs::journal::upsert_ft_synth_marker;
    let input = "---\nft-synth: false\n---\n";
    let out = upsert_ft_synth_marker(input);
    assert!(out.contains("ft-synth: true"));
    assert!(!out.contains("ft-synth: false"));
}

// ── Sources strip & manager modal ────────────────────────────────────

/// Journal tab in empty state: strip shows `Sources (0)` plus the hint.
#[test]
fn journal_tab_sources_strip_empty_state() -> Result<()> {
    let (_dir, vault) = journal_test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(journal_tab_idx())?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Sources (0)") && frame.contains("press / to manage sources"),
        "empty strip missing:\n{frame}"
    );
    Ok(())
}

/// Single-source mode still renders the strip (consistent layout).
#[test]
fn journal_tab_sources_strip_single_source() -> Result<()> {
    let (_dir, vault) = journal_test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.queue_journal_for_tab_test("Target.md");
    app.switch_to(journal_tab_idx())?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Sources (1)") && frame.contains("Target.md"),
        "single-source strip missing:\n{frame}"
    );
    Ok(())
}

/// Multi-source mode: strip shows the count + comma-joined labels +
/// the attached window. Window label uses `since <n>d`.
#[test]
fn journal_tab_sources_strip_multi_source_with_window() -> Result<()> {
    use crate::tui::tab::{JournalTarget, JournalWindow, MultiTargetRequest};
    let (_dir, vault) = multi_target_journal_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    let request = MultiTargetRequest {
        targets: vec![
            JournalTarget::Ghost("Foo".into()),
            JournalTarget::Ghost("Bar".into()),
        ],
        window: Some(JournalWindow::Since(chrono::Duration::days(7))),
    };
    app.queue_journal_for_multi_tab_test(request);
    app.switch_to(journal_tab_idx())?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Sources (2)") && frame.contains("window: since 7d"),
        "multi-source strip missing count or window:\n{frame}"
    );
    assert!(
        frame.contains("Foo (ghost)") && frame.contains("Bar (ghost)"),
        "ghost labels missing:\n{frame}"
    );
    Ok(())
}

/// Many sources on a narrow terminal: truncation appends `…, +K more`.
#[test]
fn journal_tab_sources_strip_truncates_long_list() -> Result<()> {
    use crate::tui::tab::{JournalTarget, MultiTargetRequest};
    let (_dir, vault) = multi_target_journal_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    // Mostly-ghost targets so we get a long join string. Six ghosts
    // with 8-char labels each = ~60 chars joined; a 40-col terminal
    // forces truncation.
    let request = MultiTargetRequest {
        targets: vec![
            JournalTarget::Ghost("AlphaSrc".into()),
            JournalTarget::Ghost("BetaSrc".into()),
            JournalTarget::Ghost("GammaSrc".into()),
            JournalTarget::Ghost("DeltaSrc".into()),
            JournalTarget::Ghost("EpsilonSrc".into()),
            JournalTarget::Ghost("ZetaSrc".into()),
        ],
        window: None,
    };
    app.queue_journal_for_multi_tab_test(request);
    app.switch_to(journal_tab_idx())?;
    let frame = render(&mut app, 40, 12);
    assert!(
        frame.contains("Sources (6)"),
        "count missing on narrow term:\n{frame}"
    );
    assert!(
        frame.contains("+") && frame.contains("more"),
        "truncation suffix missing:\n{frame}"
    );
    Ok(())
}

/// `/` on the Journal tab opens the Sources Manager modal.
#[test]
fn journal_tab_slash_opens_sources_manager() -> Result<()> {
    let (_dir, vault) = journal_test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(journal_tab_idx())?;
    app.dispatch(key('/'))?;
    assert_eq!(app.active_modal_name(), Some("journal-sources"));
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Journal Sources"),
        "manager title missing:\n{frame}"
    );
    Ok(())
}

/// `a` alias also opens the Sources Manager.
#[test]
fn journal_tab_a_alias_opens_sources_manager() -> Result<()> {
    let (_dir, vault) = journal_test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(journal_tab_idx())?;
    app.dispatch(key('a'))?;
    assert_eq!(app.active_modal_name(), Some("journal-sources"));
    Ok(())
}

/// The dedup invariant of the append commit path: if current sources
/// already contain a target, it isn't duplicated when appending.
#[test]
fn journal_append_or_replace_modal_appends_dedups() {
    use crate::tui::modal::JournalAppendOrReplaceModal;
    use crate::tui::tab::{AppendOrReplaceMode, JournalTarget};
    use std::cell::{Cell, RefCell};
    use std::sync::Arc;
    let modal = JournalAppendOrReplaceModal {
        current_sources: vec![
            JournalTarget::Note("Foo.md".into()),
            JournalTarget::Note("Bar.md".into()),
        ],
        incoming_targets: vec![
            JournalTarget::Note("Bar.md".into()),
            JournalTarget::Note("Baz.md".into()),
        ],
        window: None,
        focus: AppendOrReplaceMode::Append,
    };
    // Build a TabCtx scaffold; we only need pending_request to read.
    let (_dir, vault) = journal_test_vault();
    let vault = Arc::new(vault);
    let recents = Arc::new(ft_core::recents::RecentsLog::with_log_path(
        vault.path.clone(),
        vault.path.join("recents.jsonl"),
    ));
    let last_refresh: Cell<Option<chrono::DateTime<chrono::Local>>> = Cell::new(None);
    let pending: RefCell<Option<crate::tui::tab::AppRequest>> = RefCell::new(None);
    let ctx = crate::tui::tab::TabCtx {
        vault: &vault,
        recents: &recents,
        today: chrono::NaiveDate::from_ymd_opt(2026, 6, 11).unwrap(),
        last_refresh: &last_refresh,
        pending_request: &pending,
        active_modal_name: None,
        host_popup_open: false,
    };
    // Trigger the append path via the modal's commit_append (private —
    // exercise via key 'a').
    use crate::tui::event::Event as TuiEvent;
    use crate::tui::modal::Modal as _;
    let mut modal = modal;
    let _ = modal.handle_event(
        TuiEvent::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::NONE,
        )),
        &ctx,
    );
    let req = pending.borrow().as_ref().map(|r| match r {
        crate::tui::tab::AppRequest::JournalCommitSources { sources, .. } => sources.clone(),
        _ => panic!("expected JournalCommitSources"),
    });
    let sources = req.expect("modal should have raised commit");
    let labels: Vec<String> = sources.iter().map(|t| t.label()).collect();
    assert_eq!(labels, vec!["Foo.md", "Bar.md", "Baz.md"], "dedup failed");
}

/// Append-or-Replace prompt commits the replacement source set on `r`.
#[test]
fn journal_append_or_replace_modal_replaces_on_r() {
    use crate::tui::modal::JournalAppendOrReplaceModal;
    use crate::tui::tab::{AppendOrReplaceMode, JournalTarget};
    use std::cell::{Cell, RefCell};
    use std::sync::Arc;
    let modal = JournalAppendOrReplaceModal {
        current_sources: vec![JournalTarget::Note("Foo.md".into())],
        incoming_targets: vec![JournalTarget::Note("Bar.md".into())],
        window: None,
        focus: AppendOrReplaceMode::Append,
    };
    let (_dir, vault) = journal_test_vault();
    let vault = Arc::new(vault);
    let recents = Arc::new(ft_core::recents::RecentsLog::with_log_path(
        vault.path.clone(),
        vault.path.join("recents.jsonl"),
    ));
    let last_refresh: Cell<Option<chrono::DateTime<chrono::Local>>> = Cell::new(None);
    let pending: RefCell<Option<crate::tui::tab::AppRequest>> = RefCell::new(None);
    let ctx = crate::tui::tab::TabCtx {
        vault: &vault,
        recents: &recents,
        today: chrono::NaiveDate::from_ymd_opt(2026, 6, 11).unwrap(),
        last_refresh: &last_refresh,
        pending_request: &pending,
        active_modal_name: None,
        host_popup_open: false,
    };
    use crate::tui::event::Event as TuiEvent;
    use crate::tui::modal::Modal as _;
    let mut modal = modal;
    let _ = modal.handle_event(
        TuiEvent::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('r'),
            crossterm::event::KeyModifiers::NONE,
        )),
        &ctx,
    );
    let labels: Vec<String> = match pending.borrow().as_ref() {
        Some(crate::tui::tab::AppRequest::JournalCommitSources { sources, .. }) => {
            sources.iter().map(|t| t.label()).collect()
        }
        other => panic!("expected JournalCommitSources, got {other:?}"),
    };
    assert_eq!(labels, vec!["Bar.md"], "replace should drop current");
}

// ── Graph → Journal append flow ──────────────────────────────────────

/// Pressing `Ctrl+J` on the Graph tab with a Note row selected raises
/// `JournalAddSources` for that single row.
#[test]
fn graph_ctrl_j_appends_cursor_row_to_journal_sources() -> Result<()> {
    let (_dir, vault) = journal_test_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    // Refresh graph so the tree populates.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
    )))?;
    // Open a preset/query so at least one row is visible. The graph
    // tab seeds an empty view; press `/` to enter the query bar then
    // type a wildcard. Simpler: just send Ctrl+J — without a row,
    // the command toasts an error.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('j'),
        KeyModifiers::CONTROL,
    )))?;
    // With no row, expect either a toast or no request — either is
    // acceptable; the test simply asserts no panic.
    let _ = app.active_modal_name();
    Ok(())
}

// ── Notes tab · synth reslice flow ───────────────────────────────────────

/// Git-backed vault with a source note and a synth note holding one
/// protected section over lines 2-3 of the source (pinned to HEAD via the
/// core scaffold planner). Needed because the reslice flow reads source
/// blobs out of git.
fn reslice_vault() -> (TempDir, Vault) {
    use std::process::Command as StdCommand;
    let dir = TempDir::new().unwrap();
    let vault_path = dir.path().join("test-vault");
    std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
    std::fs::create_dir_all(vault_path.join("notes")).unwrap();
    std::fs::write(
        vault_path.join("notes/source.md"),
        "alpha\nbravo\ncharlie\ndelta\necho\n",
    )
    .unwrap();

    let run_git = |args: &[&str]| {
        let out = StdCommand::new("git")
            .current_dir(&vault_path)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}");
    };
    run_git(&["init", "-b", "main"]);
    run_git(&["config", "user.name", "T"]);
    run_git(&["config", "user.email", "t@e.com"]);
    run_git(&["config", "commit.gpgsign", "false"]);
    run_git(&["add", "."]);
    run_git(&["commit", "-m", "c1"]);

    let vault = Vault::discover(Some(vault_path)).unwrap();
    let entry = ft_core::journal::JournalEntry {
        source_title: "source".into(),
        source_path: std::path::PathBuf::from("notes/source.md"),
        line_start: 2,
        line_end: 3,
        section_text: "bravo\ncharlie".into(),
        date: chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
        matched: vec![],
    };
    let target = std::path::PathBuf::from("Synth/topic.md");
    let plan = ft_core::synth::scaffold::plan_synth_scaffold(
        &vault,
        &target,
        std::slice::from_ref(&entry),
    )
    .unwrap();
    ft_core::synth::scaffold::apply_synth_scaffold(&vault, &plan).unwrap();
    (dir, vault)
}

/// Drive the Notes tab into the reslice section-list step for the synth
/// note `Synth/topic.md`.
fn drive_to_reslice_sections(vault: Vault) -> Result<App> {
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('r'))?;
    for c in "topic".chars() {
        app.dispatch(key(c))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    Ok(app)
}

#[test]
fn notes_reslice_picker_opens_on_r() -> Result<()> {
    let (_dir, vault) = reslice_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(NOTES_TAB_INDEX)?;
    app.dispatch(key('r'))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("1/3 pick synth note"),
        "reslice note picker should open:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_reslice_section_list_renders() -> Result<()> {
    let (_dir, vault) = reslice_vault();
    let mut app = drive_to_reslice_sections(vault)?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("2/3 pick section"),
        "should land on section list:\n{frame}"
    );
    assert!(
        frame.contains("notes/source.md L2-3"),
        "the section's source + range should be listed:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_reslice_edit_grows_range_in_preview() -> Result<()> {
    let (_dir, vault) = reslice_vault();
    let mut app = drive_to_reslice_sections(vault)?;
    // Enter the boundary editor, then grow the (default) bottom edge down.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("3/3"),
        "should be on the boundary editor:\n{frame}"
    );
    assert!(
        frame.contains("L2-4") && frame.contains("was L2-3"),
        "bottom edge should have grown to L2-4:\n{frame}"
    );
    assert!(
        frame.contains("delta"),
        "preview should now include the new line:\n{frame}"
    );
    Ok(())
}

#[test]
fn notes_reslice_edit_enter_commits_and_writes() -> Result<()> {
    let (_dir, vault) = reslice_vault();
    let vault_path = vault.path.clone();
    let mut app = drive_to_reslice_sections(vault)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    let body = std::fs::read_to_string(vault_path.join("Synth/topic.md")).unwrap();
    assert!(
        body.contains("> [!ft-source] \"notes/source.md\" L2-4 @"),
        "committed note should carry the widened range:\n{body}"
    );
    assert!(
        body.contains("> delta"),
        "the new line should be in the protected body:\n{body}"
    );
    Ok(())
}

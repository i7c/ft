//! Synthesis surfaces: capture presets, Pulse tab, graph → Search
//! handoffs, send-to-synth (via the Search tab), synth reslice flow.

use super::*;

// ── Graph → Search handoffs ─────────────────────────────────────────

/// Build a vault with one git commit so blame/pulse dates resolve.
/// `Target.md` plus one note mentioning it — enough for the review-tab
/// fixtures that need a git-backed vault without history semantics.
fn git_backed_vault() -> (TempDir, Vault) {
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
    run_git(&["config", "maintenance.auto", "false"]);
    run_git(&["add", "."]);
    run_git(&["commit", "-m", "init"]);

    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

/// `J` on a Note row opens the Search tab prefilled with the note as a
/// `[[…]]` clause (replacing the removed journal tab jump).
#[test]
fn graph_j_jumps_to_search_for_selected_note() -> Result<()> {
    // The note mentions itself so the prefilled `[[Target]]` query has a
    // real result to render.
    let (_dir, vault) = rename_vault(&[("Target.md", "# Target\n\nMentions [[Target]] today.\n")]);
    let mut app = App::for_test(vault);
    // Default Graph tab focus + the dirs-style default query lists the
    // vault's root directory first; navigate down to a Note row.
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?; // expand root
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

    // Shift+J raises SearchWithQuery; the App services it in-process.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('J'),
        KeyModifiers::SHIFT,
    )))?;
    app.service_pending_requests()?;

    assert_eq!(app.active_title(), "Search");
    let frame = render(&mut app, 90, 24);
    assert!(
        frame.contains("[[Target]]") && frame.contains("Mentions [[Target]] today."),
        "Search tab must show the prefilled clause and the note's mentioning paragraph:\n{frame}"
    );
    Ok(())
}

/// `J` on a non-Note row produces an error toast and stays on Graph.
#[test]
fn graph_j_on_non_note_row_queues_toast_and_stays_on_graph() -> Result<()> {
    let (_dir, vault) = rename_vault(&[("Target.md", "# Target\n")]);
    let mut app = App::for_test(vault);
    // Stay on Graph tab with the root directory selected (it's a
    // Directory row, not a Note).
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('J'),
        KeyModifiers::SHIFT,
    )))?;
    let toast = app.current_toast().expect("expected an error toast");
    assert!(
        toast.text.to_lowercase().contains("note"),
        "toast text must hint at the Note-row requirement: {}",
        toast.text
    );
    assert_eq!(app.active_title(), "Graph");
    Ok(())
}

/// `J` on a Ghost row opens Search with the raw target as the clause.
#[test]
fn graph_j_on_ghost_row_opens_search_for_ghost() -> Result<()> {
    let (dir, vault) = rename_vault(&[("a.md", "see [[Phantom]]\n")]);
    let mut app = App::for_test(vault);
    switch_to_graph(&mut app)?;
    // Expand root → row 0: D /, row 1: N a. Expand a → G Phantom.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    app.dispatch(key('j'))?;

    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('J'),
        KeyModifiers::SHIFT,
    )))?;
    let frame = render(&mut app, 90, 24);
    assert_eq!(
        app.active_title(),
        "Search",
        "Shift+J on a ghost row must land on the Search tab\nframe:\n{frame}"
    );
    assert!(
        frame.contains("[[Phantom]]") || frame.contains("Phantom"),
        "Search tab must show the ghost clause/results:\n{frame}"
    );
    let _ = dir;
    Ok(())
}

/// `Ctrl+J` on a Note row opens Search with the row's mentions in
/// any-mode (the old multi-target OR semantics).
#[test]
fn graph_ctrl_j_opens_search_with_mentions() -> Result<()> {
    let (_dir, vault) = rename_vault(&[("Target.md", "# Target\n\nMentions [[Target]] today.\n")]);
    let mut app = App::for_test(vault);
    switch_to_graph(&mut app)?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
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

    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('j'),
        KeyModifiers::CONTROL,
    )))?;
    app.service_pending_requests()?;

    assert_eq!(app.active_title(), "Search");
    let frame = render(&mut app, 90, 24);
    assert!(
        frame.contains("[[Target]]") && frame.contains("Mentions [[Target]] today."),
        "Search must show the prefilled clause and the note's mentioning paragraph:\n{frame}"
    );
    Ok(())
}

/// The graph tab's help overlay lists the Search handoff keys.
#[test]
fn graph_tab_help_lists_search_handoff_keys() -> Result<()> {
    let (_dir, vault) = related_modal_vault();
    let mut app = App::for_test(vault);
    switch_to_graph(&mut app)?;
    let sections = app.active_tab_help_sections();
    let merged: String = sections
        .iter()
        .flat_map(|s| s.entries.iter().map(|e| format!("{}={}\n", e.keys, e.desc)))
        .collect();
    assert!(
        merged.contains("Shift+j"),
        "graph-tab help must mention Shift+j for the Search jump:\n{merged}"
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
    let config_toml = [
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
        "",
        "[capture_presets.jot]",
        "action = \"append\"",
        "template = \"quick-log\"",
        "section = \"Log\"",
        "",
        "[capture_presets.noted]",
        "action = \"append\"",
        "template = \"quick-note\"",
        "section = \"Log\"",
    ]
    .join("\n");
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

    // Template without vars, used by the `jot` preset (no `note` field)
    // to exercise the Notes-tab file-picker path.
    std::fs::write(tmpl_dir.join("quick-log.md"), "- Jot for {{ today }}\n").unwrap();

    // Template with vars, used by a no-`note` append preset to exercise
    // the picker-then-var-prompt path.
    std::fs::write(
        tmpl_dir.join("quick-note.md"),
        "- {{ vars.text }} ({{ today }})\n",
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

    // A second note the `jot` preset can append to via the file picker.
    std::fs::write(
        vault_path.join("scratch.md"),
        "# Scratch\n## Log\nold jot\n",
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

    // Switch to Notes tab (index 1 — Graph=0, Notes=1).
    app.switch_to(1)?;

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

    // Select "log" by typing to filter (order is alphabetical and
    // other presets now share the fixture).
    for c in "log".chars() {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )))?;
    }
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
    app.switch_to(1)?;

    // Press Q to open capture preset picker.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('Q'),
        KeyModifiers::SHIFT,
    )))?;

    // Select "meeting" by typing to filter (order is alphabetical and
    // other presets now share the fixture).
    for c in "meeting".chars() {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )))?;
    }

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
    app.switch_to(1)?;

    // Press Q and select meeting (with vars).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('Q'),
        KeyModifiers::SHIFT,
    )))?;
    for c in "meeting".chars() {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )))?;
    }
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
    app.switch_to(1)?;

    // Press Q and select meeting (with vars).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('Q'),
        KeyModifiers::SHIFT,
    )))?;
    for c in "meeting".chars() {
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
    assert_tui_snapshot!("capture_var_prompt_80x24", frame);
    Ok(())
}

// ── Notes-tab file-picker path (append preset without `note`) ────────

/// Press Q from the Notes tab and select the `jot` preset (no `note`
/// field). Instead of erroring, a vault file picker should open to choose
/// the target note.
#[test]
fn capture_append_no_note_opens_file_picker() -> Result<()> {
    let (_dir, vault) = capture_preset_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?; // Notes tab.

    // Q opens the preset picker.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('Q'),
        KeyModifiers::SHIFT,
    )))?;

    // `jot` sorts first alphabetically (jot, log, meeting, noted).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // A vault file picker should be open — not an error toast.
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("pick target note"),
        "should open the target-note file picker, not error: {frame}"
    );
    Ok(())
}

/// Selecting a note in the file picker appends the rendered template
/// under the preset's `section`, and the picker is dismissed.
#[test]
fn capture_append_no_note_picks_target_and_appends() -> Result<()> {
    let (_dir, vault) = capture_preset_vault();
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?; // Notes tab.

    // Q → select `jot` (first preset).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('Q'),
        KeyModifiers::SHIFT,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // File picker is open. Type to surface `scratch.md`.
    for c in "scratch".chars() {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // Picker dismissed, editor open requested.
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("pick target note"),
        "picker should be dismissed after selecting a note: {frame}"
    );
    assert!(
        app.take_pending_request().is_some(),
        "selecting a target should queue an editor-open request"
    );

    // The rendered template landed under the `## Log` section.
    let content = std::fs::read_to_string(vault_path.join("scratch.md"))?;
    assert!(
        content.contains("- Jot for"),
        "target should contain rendered jot entry: {content}"
    );
    // Section placement: the jot line should come after the `## Log`
    // heading and the existing `old jot` line, not before the heading.
    let log_idx = content.find("## Log").unwrap();
    let jot_idx = content.find("- Jot for").unwrap();
    assert!(
        jot_idx > log_idx,
        "jot entry should be under the Log section: {content}"
    );
    Ok(())
}

/// Esc in the file picker returns to idle with no append and no toast.
#[test]
fn capture_append_no_note_esc_cancels() -> Result<()> {
    let (_dir, vault) = capture_preset_vault();
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?; // Notes tab.

    // Q → select `jot` → file picker opens.
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('Q'),
        KeyModifiers::SHIFT,
    )))?;
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;
    assert!(
        render(&mut app, 80, 24).contains("pick target note"),
        "file picker should be open"
    );

    // Esc cancels.
    app.dispatch(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))?;
    let frame = render(&mut app, 80, 24);
    assert!(
        !frame.contains("pick target note"),
        "picker should be dismissed after Esc: {frame}"
    );
    assert!(
        app.take_pending_request().is_none(),
        "Esc should not queue any request"
    );

    // No append happened.
    let content = std::fs::read_to_string(vault_path.join("scratch.md"))?;
    assert!(
        !content.contains("- Jot for"),
        "no jot entry should have been written after cancel: {content}"
    );
    Ok(())
}

/// A no-`note` append preset whose template has `{{ vars.* }}` opens the
/// file picker first, then the var prompt, then commits.
#[test]
fn capture_append_no_note_with_vars_prompts_after_picker() -> Result<()> {
    let (_dir, vault) = capture_preset_vault();
    let vault_path = vault.path.clone();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(1)?; // Notes tab.

    // Q → pick `noted` (4th: jot, log, meeting, noted).
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Char('Q'),
        KeyModifiers::SHIFT,
    )))?;
    for _ in 0..3 {
        app.dispatch(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // File picker first.
    assert!(
        render(&mut app, 80, 24).contains("pick target note"),
        "file picker should open before var prompt"
    );

    // Pick scratch.md.
    for c in "scratch".chars() {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // Now the var prompt for `text`.
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("text"),
        "should show var prompt after picking target: {frame}"
    );

    // Type the var and commit.
    for c in "hello world".chars() {
        app.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )))?;
    }
    app.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))?;

    // The rendered var landed in scratch.md under Log.
    let content = std::fs::read_to_string(vault_path.join("scratch.md"))?;
    assert!(
        content.contains("- hello world ("),
        "target should contain the rendered var: {content}"
    );
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

fn pulse_tab_idx() -> usize {
    2
}

/// Vault with two commits: c1 (baseline, dated 2024-01-01 so a 7d
/// window always finds a from-ref) and c2 (today, adds two notes with
/// `[[Foo]]` / `[[Bar]]` as ghosts).
fn pulse_test_vault() -> (TempDir, Vault) {
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
    run_git_at(None, &["config", "maintenance.auto", "false"]);
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
    let (_dir, vault) = git_backed_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(pulse_tab_idx())?;
    // Default window is --since 7d; the fixture's commit is very recent.
    // But fixed_clock = 2026-05-10 and commits are wall-clock today,
    // which means commits are *in the future* relative to clock — git
    // log --before=2026-05-03 returns nothing. Either way we should
    // exercise the empty-state UI cleanly without panicking.
    let frame = render(&mut app, 80, 24);
    assert!(
        frame.contains("Pulse"),
        "Review tab title missing from frame:\n{frame}"
    );
    Ok(())
}

#[test]
fn review_tab_lists_rows_with_counts_and_ghost_suffix() -> Result<()> {
    let (_dir, vault) = pulse_test_vault();
    // Default --since 7d window resolves against pulse's own
    // today (system clock, FT_TODAY honored if set). Commits in the
    // fixture are made at wall-clock-now, so a 7d window includes them.
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(pulse_tab_idx())?;
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

/// Vault whose single recent commit mentions many distinct ghost
/// links, so the pulse yields more rows than a 24-row viewport fits.
/// Used to verify the list viewport auto-follows the cursor past the
/// fold (the pre-split render never scrolled, leaving lower rows
/// unreachable).
fn pulse_overflow_vault() -> (TempDir, Vault) {
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
    run_git_at(None, &["config", "maintenance.auto", "false"]);
    run_git_at(None, &["add", "."]);
    run_git_at(Some("2024-01-01T00:00:00"), &["commit", "-m", "c1"]);

    // 30 distinct ghost links → 30 pulse rows, more than a 24-row
    // viewport can show at once.
    let mut body = String::new();
    for i in 0..30 {
        body.push_str(&format!("Mentions [[Link{i}]] here.\n\n"));
    }
    std::fs::write(vault_path.join("many.md"), body).unwrap();
    run_git_at(None, &["add", "."]);
    run_git_at(None, &["commit", "-m", "c2"]);

    let vault = Vault::discover(Some(vault_path)).unwrap();
    (dir, vault)
}

#[test]
fn review_tab_cursor_stays_visible_past_the_fold() -> Result<()> {
    let (_dir, vault) = pulse_overflow_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(pulse_tab_idx())?;
    // Sanity: the pulse produced many rows.
    let initial = render(&mut app, 80, 24);
    assert!(initial.contains("[[Link0]]"), "Link0 missing:\n{initial}");
    // Move the cursor well past the first screen (30 rows, viewport ~20).
    let down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    for _ in 0..25 {
        app.dispatch(down.clone())?;
    }
    let frame = render(&mut app, 80, 24);
    // The cursor row (Link25) must be visible after scrolling; before
    // the scroll-follow fix it was rendered off-screen and unreachable.
    assert!(
        frame.contains("[[Link25]]"),
        "cursor row Link25 not visible after scrolling:\n{frame}"
    );
    Ok(())
}

#[test]
fn review_tab_help_lists_keybindings() -> Result<()> {
    let (_dir, vault) = git_backed_vault();
    let mut app = App::for_test_with_clock(vault, fixed_clock);
    app.switch_to(pulse_tab_idx())?;
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
// The pure `upsert_ft_synth_marker` transform has moved to
// `ft_core::synth::callout::upsert_synth_frontmatter` (which also handles
// the `ft.synth.targets` key). These three tests now exercise the core
// helper to keep coverage of the marker-only behavior the TUI relied on.

#[test]
fn upsert_ft_synth_marker_inserts_into_existing_frontmatter() {
    use ft_core::synth::callout::upsert_synth_frontmatter;
    let input = "---\ntitle: Foo\n---\n\nbody\n";
    let out = upsert_synth_frontmatter(input, None);
    assert!(out.contains("ft:\n  synth:\n    enabled: true"));
    assert!(out.contains("title: Foo"));
    assert!(out.contains("body"));
}

#[test]
fn upsert_ft_synth_marker_adds_fresh_frontmatter_when_missing() {
    use ft_core::synth::callout::upsert_synth_frontmatter;
    let input = "# heading\n\nbody\n";
    let out = upsert_synth_frontmatter(input, None);
    assert!(out.starts_with("---\nft:\n  synth:\n    enabled: true\n---\n"));
    assert!(out.contains("# heading"));
}

#[test]
fn upsert_ft_synth_marker_replaces_false_value() {
    use ft_core::synth::callout::upsert_synth_frontmatter;
    let input = "---\nft:\n  synth:\n    enabled: false\n---\n";
    let out = upsert_synth_frontmatter(input, None);
    assert!(out.contains("ft:\n  synth:\n    enabled: true"));
    assert!(!out.contains("enabled: false"));
}

// ── Sources strip & manager modal ────────────────────────────────────

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
    run_git(&["config", "maintenance.auto", "false"]);
    run_git(&["add", "."]);
    run_git(&["commit", "-m", "c1"]);

    let vault = Vault::discover(Some(vault_path)).unwrap();
    let entry = ft_core::synth::source::SynthSource {
        source_path: std::path::PathBuf::from("notes/source.md"),
        line_start: 2,
        line_end: 3,
        body: "bravo\ncharlie".into(),
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

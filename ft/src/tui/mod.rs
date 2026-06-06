mod app;
mod app_commands;
mod command;
mod editor;
mod event;
mod help;
mod jobs;
mod keymap;
mod modal;
mod modal_commands;
mod notes_actions;
mod palette;
mod tab;
mod tabs;
#[cfg(test)]
mod tests;
mod ui;
mod widgets;

use std::io::{self, Stdout};
use std::sync::Arc;

use anyhow::{Context, Result};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ft_core::vault::Vault;
use ratatui::{backend::CrosstermBackend, Terminal};

pub use app::App;

/// Re-export the command/keymap building blocks so the top-level CLI
/// (`ft commands list`, `ft completions docs`) can construct a
/// `CommandRegistry` without instantiating a full `App`.
pub mod registry {
    pub use crate::tui::command::{CommandDef, CommandRegistry, CommandScope};

    /// Build the binary's full `CommandRegistry` from static slices.
    /// Mirrors what `tui::App::with_tabs` does at runtime but doesn't
    /// need live tab instances — every command in the binary is
    /// declared as a `static [CommandDef]` so the union is purely
    /// data-driven.
    pub fn build() -> CommandRegistry {
        use crate::tui::{app_commands, modal_commands};

        let tab_slices: &[&'static [CommandDef]] = &[
            crate::tui::tabs::graph::GRAPH_COMMANDS,
            crate::tui::tabs::tasks::TASKS_COMMANDS,
            crate::tui::tabs::notes::NOTES_COMMANDS,
            crate::tui::tabs::timeblocks::TIMEBLOCKS_COMMANDS,
            crate::tui::tabs::journal::JOURNAL_COMMANDS,
        ];
        let modal_slices: &[&'static [CommandDef]] = &[
            modal_commands::CREATE_COMMANDS,
            modal_commands::APPEND_COMMANDS,
            modal_commands::SECTION_MOVE_COMMANDS,
            modal_commands::CAPTURE_VAR_COMMANDS,
            modal_commands::PERIODIC_LEADER_COMMANDS,
            modal_commands::QUERY_BAR_COMMANDS,
            modal_commands::RENAME_COMMANDS,
            modal_commands::SEARCH_COMMANDS,
            modal_commands::PRESET_PICKER_COMMANDS,
            modal_commands::CAPTURE_PICKER_COMMANDS,
            modal_commands::RELATED_COMMANDS,
            modal_commands::MOVE_OUTER_COMMANDS,
        ];

        let mut slices: Vec<&'static [CommandDef]> = tab_slices.to_vec();
        slices.extend_from_slice(modal_slices);
        slices.push(app_commands::APP_COMMANDS);
        CommandRegistry::from_slices(&slices)
    }

    /// Validate `config.keymap` against every known scope's default keymap.
    ///
    /// Returns a list of human-readable error strings. Empty = valid.
    /// Used by `ft commands check-keymap`.
    pub fn validate_keymap(config: &ft_core::config::Config) -> Vec<String> {
        use crate::tui::{
            app_commands::APP_KEYMAP,
            keymap::{parse_scope, KeymapOverlay},
            modal_commands,
            tabs::{
                graph::GRAPH_KEYMAP, journal::JOURNAL_KEYMAP, notes::NOTES_KEYMAP,
                tasks::TASKS_KEYMAP, timeblocks::TIMEBLOCKS_KEYMAP,
            },
        };

        let registry = build();
        let kc = config.keymap.as_ref();
        let raw_unbinds: Vec<(String, String)> = kc
            .map(|k| {
                k.unbind
                    .iter()
                    .map(|e| (e.scope.clone(), e.chord.clone()))
                    .collect()
            })
            .unwrap_or_default();

        let scope_bases: &[(&str, &crate::tui::keymap::KeyMap)] = &[
            ("global", &APP_KEYMAP),
            ("tab/graph", &GRAPH_KEYMAP),
            ("tab/tasks", &TASKS_KEYMAP),
            ("tab/notes", &NOTES_KEYMAP),
            ("tab/timeblocks", &TIMEBLOCKS_KEYMAP),
            ("tab/journal", &JOURNAL_KEYMAP),
            ("modal/create", &modal_commands::CREATE_KEYMAP),
            ("modal/append", &modal_commands::APPEND_KEYMAP),
            ("modal/section-move", &modal_commands::SECTION_MOVE_KEYMAP),
            ("modal/capture-var", &modal_commands::CAPTURE_VAR_KEYMAP),
            (
                "modal/periodic-leader",
                &modal_commands::PERIODIC_LEADER_KEYMAP,
            ),
            ("modal/query-bar", &modal_commands::QUERY_BAR_KEYMAP),
            ("modal/rename", &modal_commands::RENAME_KEYMAP),
            ("modal/search", &modal_commands::SEARCH_KEYMAP),
            ("modal/preset-picker", &modal_commands::PRESET_PICKER_KEYMAP),
            (
                "modal/capture-picker",
                &modal_commands::CAPTURE_PICKER_KEYMAP,
            ),
            ("modal/related", &modal_commands::RELATED_KEYMAP),
            ("modal/move", &modal_commands::MOVE_OUTER_KEYMAP),
        ];

        let mut errors: Vec<String> = Vec::new();
        let empty_scope = std::collections::HashMap::new();

        // Flag unknown scope strings in the scopes table.
        if let Some(kc) = kc {
            for scope_str in kc.scopes.keys() {
                if parse_scope(scope_str).is_none() {
                    errors.push(format!("unknown scope {scope_str:?} in [keymap] config"));
                }
            }
        }

        // Validate all known scopes.
        for (scope_str, base) in scope_bases {
            let scope_table = kc
                .and_then(|k| k.scopes.get(*scope_str))
                .unwrap_or(&empty_scope);
            if let Err(errs) =
                KeymapOverlay::from_raw(scope_table, &raw_unbinds, &registry, scope_str, base)
            {
                for e in errs {
                    errors.push(e.to_string());
                }
            }
        }

        errors
    }

    /// Return effective chord-to-command bindings per scope after applying
    /// `config.keymap` overlays. Each entry is `(scope_str, chord_str, command_name)`.
    /// Used by `ft commands list --effective`.
    pub fn effective_bindings(config: &ft_core::config::Config) -> Vec<(String, String, String)> {
        use crate::tui::{
            app_commands::APP_KEYMAP,
            keymap::{chord_to_str, KeymapOverlay},
            modal_commands,
            tabs::{
                graph::GRAPH_KEYMAP, journal::JOURNAL_KEYMAP, notes::NOTES_KEYMAP,
                tasks::TASKS_KEYMAP, timeblocks::TIMEBLOCKS_KEYMAP,
            },
        };

        let registry = build();
        let kc = config.keymap.as_ref();
        let raw_unbinds: Vec<(String, String)> = kc
            .map(|k| {
                k.unbind
                    .iter()
                    .map(|e| (e.scope.clone(), e.chord.clone()))
                    .collect()
            })
            .unwrap_or_default();

        let scope_bases: &[(&str, &crate::tui::keymap::KeyMap)] = &[
            ("global", &APP_KEYMAP),
            ("tab/graph", &GRAPH_KEYMAP),
            ("tab/tasks", &TASKS_KEYMAP),
            ("tab/notes", &NOTES_KEYMAP),
            ("tab/timeblocks", &TIMEBLOCKS_KEYMAP),
            ("tab/journal", &JOURNAL_KEYMAP),
            ("modal/create", &modal_commands::CREATE_KEYMAP),
            ("modal/append", &modal_commands::APPEND_KEYMAP),
            ("modal/section-move", &modal_commands::SECTION_MOVE_KEYMAP),
            ("modal/capture-var", &modal_commands::CAPTURE_VAR_KEYMAP),
            (
                "modal/periodic-leader",
                &modal_commands::PERIODIC_LEADER_KEYMAP,
            ),
            ("modal/query-bar", &modal_commands::QUERY_BAR_KEYMAP),
            ("modal/rename", &modal_commands::RENAME_KEYMAP),
            ("modal/search", &modal_commands::SEARCH_KEYMAP),
            ("modal/preset-picker", &modal_commands::PRESET_PICKER_KEYMAP),
            (
                "modal/capture-picker",
                &modal_commands::CAPTURE_PICKER_KEYMAP,
            ),
            ("modal/related", &modal_commands::RELATED_KEYMAP),
            ("modal/move", &modal_commands::MOVE_OUTER_KEYMAP),
        ];

        let mut out: Vec<(String, String, String)> = Vec::new();
        let empty_scope = std::collections::HashMap::new();

        for (scope_str, base) in scope_bases {
            let scope_table = kc
                .and_then(|k| k.scopes.get(*scope_str))
                .unwrap_or(&empty_scope);
            let effective = match KeymapOverlay::from_raw(
                scope_table,
                &raw_unbinds,
                &registry,
                scope_str,
                base,
            ) {
                Ok(ov) => base.with_overlay(&ov),
                Err(_) => (*base).clone(),
            };
            for (chord, cmd) in effective.iter() {
                out.push((
                    scope_str.to_string(),
                    chord_to_str(chord),
                    cmd.name.to_string(),
                ));
            }
        }

        out
    }
}

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// A startup action initiated from outside the TUI loop (e.g. by a
/// CLI subcommand that launches the TUI in a specific state).
#[derive(Debug, Clone)]
pub enum InitialAction {
    /// Switch to the graph tab on startup and open the Related
    /// updater modal for the note at the given vault-relative path.
    OpenRelatedModal { note_path: std::path::PathBuf },
}

/// Entry point for `ft tui`. Sets up the terminal, runs the event loop, and
/// always restores the terminal on exit (success or panic).
pub fn run(vault: Vault) -> Result<()> {
    run_with_action(vault, None)
}

/// Same as [`run`] but accepts a startup action that the app applies
/// once the initial graph build is complete.
pub fn run_with_action(vault: Vault, initial: Option<InitialAction>) -> Result<()> {
    let mut terminal = setup_terminal().context("failed to enter TUI mode")?;
    let mut app = App::new(Arc::new(vault));
    app.set_initial_action(initial);
    let result = app.run(&mut terminal);
    restore_terminal(&mut terminal).context("failed to restore terminal")?;
    result
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).map_err(Into::into)
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

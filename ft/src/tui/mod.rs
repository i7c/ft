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

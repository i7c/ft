mod app;
mod app_commands;
mod command;
mod editor;
mod event;
mod help;
mod jobs;
mod keymap;
mod modal;
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

use std::cell::{Cell, RefCell};
use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDate};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ft_core::recents::RecentsLog;
use ft_core::vault::Vault;
use ratatui::Frame;

#[cfg(test)]
use crate::tui::tabs::tasks::ClockFn;
use ft_core::config::EditorStrategy;

use crate::tui::{
    editor::{build_invocation, build_wait_for_invocation, unique_signal_name, EditorInvocation},
    event::{BgEvent, Event, EventStream, SyncJobResult},
    help::global_section,
    jobs::{JobHandle, JobKind},
    tab::{AppRequest, EventOutcome, Tab, TabCtx, ToastStyle},
    tabs::{graph::GraphTab, notes::NotesTab, tasks::TasksTab, timeblocks::TimeblocksTab},
    ui::{self, Mode, SyncConflictInfo, SyncConflictKind},
    Tui,
};
use ft_core::git::{discover_repo, sync, SyncOptions, SyncOutcome};

/// A transient status-bar message. The center cell of the status bar
/// shows the toast text in place of `refreshed HH:MM:SS` until the
/// deadline elapses; the 1-second tick already drives the redraw loop,
/// so expiry happens naturally without a separate timer.
#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub style: ToastStyle,
    pub deadline: std::time::Instant,
}

/// How long a toast stays on screen unless overwritten by a later one.
/// Picked to be long enough to read a short message but short enough not
/// to mask a subsequent action.
const TOAST_DURATION: Duration = Duration::from_secs(3);

pub struct App {
    /// Shared so widgets that outlive a single event-loop borrow (e.g. the
    /// fuzzy picker held inside a popup) can clone a handle to the vault
    /// without colliding with App's own borrow of `tabs`.
    vault: Arc<Vault>,
    /// Per-vault "recently opened" log (plan 008). Shared into every
    /// `TabCtx` so picker sites surface recents and open chokepoints
    /// record them.
    recents: Arc<RecentsLog>,
    today: NaiveDate,
    tabs: Vec<Box<dyn Tab>>,
    active: usize,
    mode: Mode,
    last_refresh: Cell<Option<DateTime<Local>>>,
    pending_request: RefCell<Option<AppRequest>>,
    /// Active toast, if any. `RefCell` because `Toast` is `!Copy`.
    toast: RefCell<Option<Toast>>,
    /// Set when sync surfaces a conflict; rendered by
    /// `render_sync_conflict` while `mode == Mode::SyncConflict`.
    sync_conflict: RefCell<Option<SyncConflictInfo>>,
    /// Single-slot tracker for background work spawned off the main
    /// event loop (plan 014). When `Some`, the right cell of the
    /// status bar renders an in-flight indicator and re-entrant
    /// submissions are rejected with a toast.
    jobs: RefCell<Option<JobHandle>>,
    should_quit: bool,
}

impl App {
    pub fn new(vault: Arc<Vault>) -> Self {
        let recents = Arc::new(RecentsLog::for_vault(&vault));
        Self::new_with_recents(vault, recents)
    }

    /// Construct with an explicit recents log. Production goes through
    /// [`Self::new`]; tests use this entry point to point the log at a
    /// `TempDir`-rooted path so they don't write to the user's real
    /// state directory.
    pub fn new_with_recents(vault: Arc<Vault>, recents: Arc<RecentsLog>) -> Self {
        let today = resolve_today();
        let tabs: Vec<Box<dyn Tab>> = vec![
            Box::new(GraphTab::new()),
            Box::new(TasksTab::new()),
            Box::new(NotesTab::new()),
            Box::new(TimeblocksTab::new()),
        ];
        Self::with_tabs(vault, recents, today, tabs)
    }

    fn with_tabs(
        vault: Arc<Vault>,
        recents: Arc<RecentsLog>,
        today: NaiveDate,
        tabs: Vec<Box<dyn Tab>>,
    ) -> Self {
        Self {
            vault,
            recents,
            today,
            tabs,
            active: 0,
            mode: Mode::Normal,
            last_refresh: Cell::new(None),
            pending_request: RefCell::new(None),
            toast: RefCell::new(None),
            sync_conflict: RefCell::new(None),
            jobs: RefCell::new(None),
            should_quit: false,
        }
    }

    pub fn run(&mut self, terminal: &mut Tui) -> Result<()> {
        let events = EventStream::new(Duration::from_secs(1));

        // Initial focus event so the first tab can lazily load if needed.
        {
            let mut ctx = TabCtx {
                vault: &self.vault,
                recents: &self.recents,
                today: self.today,
                last_refresh: &self.last_refresh,
                pending_request: &self.pending_request,
            };
            self.tabs[self.active].on_focus(&mut ctx)?;
        }

        loop {
            terminal.draw(|f| self.draw(f))?;
            let ev = events.next()?;
            self.handle_event(ev)?;
            if self.should_quit {
                return Ok(());
            }
            // Service any side-effect requests the view raised. Done outside
            // `handle_event` so the App owns the Terminal during suspend.
            if let Some(req) = self.pending_request.take() {
                self.service_request(terminal, &events, req)?;
            }
        }
    }

    /// Snapshot of the in-flight job kind for the status bar renderer.
    /// Returned `Copy` so the borrow on `self.jobs` is short-lived and
    /// doesn't outlive the draw call.
    fn in_flight_job(&self) -> Option<JobKind> {
        self.jobs.borrow().as_ref().map(|h| h.kind)
    }

    fn draw(&mut self, frame: &mut Frame) {
        let [tab_bar, body, status_bar] = ui::split_screen(frame.area());
        let titles: Vec<&str> = self.tabs.iter().map(|t| t.title()).collect();
        ui::render_tab_bar(frame, tab_bar, &titles, self.active);

        // Render the status bar after the body so the body can update
        // `last_refresh` (via the Cell) before we read it back.
        let vault_name = self
            .vault
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.vault.path.display().to_string());

        let ctx = TabCtx {
            vault: &self.vault,
            recents: &self.recents,
            today: self.today,
            last_refresh: &self.last_refresh,
            pending_request: &self.pending_request,
        };
        ui::render_body(frame, body, self.tabs[self.active].as_mut(), &ctx);

        // Expire stale toasts before drawing so the cell falls back to
        // the refresh time on the very tick the deadline passes.
        let toast_now = std::time::Instant::now();
        let active_toast = {
            let mut slot = self.toast.borrow_mut();
            if let Some(t) = slot.as_ref() {
                if t.deadline <= toast_now {
                    *slot = None;
                }
            }
            slot.clone()
        };
        ui::render_status_bar(
            frame,
            status_bar,
            ui::StatusBarState {
                vault_name: &vault_name,
                tab_title: self.tabs[self.active].title(),
                last_refresh: self.last_refresh.get(),
                toast: active_toast.as_ref(),
                mode: self.mode,
                in_flight: self.in_flight_job(),
            },
        );

        match self.mode {
            Mode::Help => {
                let global = global_section();
                let sections = self.tabs[self.active].help_sections();
                ui::render_help_overlay(
                    frame,
                    frame.area(),
                    self.tabs[self.active].title(),
                    &global,
                    &sections,
                );
            }
            Mode::GitLeader => ui::render_git_leader(frame, frame.area()),
            Mode::SyncConflict => {
                if let Some(info) = self.sync_conflict.borrow().as_ref() {
                    ui::render_sync_conflict(frame, frame.area(), info);
                }
            }
            Mode::Normal => {}
        }
    }

    fn handle_event(&mut self, ev: Event) -> Result<()> {
        // Background completion messages from worker threads. Handled
        // before any mode short-circuit so a sync that finishes while
        // the help overlay is up still toasts/transitions correctly.
        if let Event::Background(bg) = ev {
            self.handle_background(bg)?;
            return Ok(());
        }

        // Help overlay swallows everything except its own dismiss keys.
        if self.mode == Mode::Help {
            if let Event::Key(k) = ev {
                if matches!(
                    k.code,
                    KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
                ) {
                    self.mode = Mode::Normal;
                }
            }
            return Ok(());
        }

        // Git-leader: `s` fires sync, `Esc` (and any other key) dismisses.
        // We never fall through to the tab/global handler so a stray
        // global key (q) doesn't quit while the leader is open.
        if self.mode == Mode::GitLeader {
            if let Event::Key(k) = ev {
                self.mode = Mode::Normal;
                if matches!(
                    (k.code, k.modifiers),
                    (KeyCode::Char('s'), KeyModifiers::NONE)
                ) {
                    *self.pending_request.borrow_mut() =
                        Some(AppRequest::SyncGit { message: None });
                }
            }
            return Ok(());
        }

        // Conflict modal stays up until Esc or `q` dismisses it.
        if self.mode == Mode::SyncConflict {
            if let Event::Key(k) = ev {
                if matches!(k.code, KeyCode::Esc | KeyCode::Char('q')) {
                    self.mode = Mode::Normal;
                    *self.sync_conflict.borrow_mut() = None;
                }
            }
            return Ok(());
        }

        // Route to the active tab first.
        let outcome = {
            let mut ctx = TabCtx {
                vault: &self.vault,
                recents: &self.recents,
                today: self.today,
                last_refresh: &self.last_refresh,
                pending_request: &self.pending_request,
            };
            self.tabs[self.active].handle_event(ev.clone(), &mut ctx)?
        };
        match outcome {
            EventOutcome::Consumed => return Ok(()),
            EventOutcome::Quit => {
                self.should_quit = true;
                return Ok(());
            }
            EventOutcome::SwitchTab(idx) => {
                self.switch_tab(idx)?;
                return Ok(());
            }
            EventOutcome::NotHandled => {}
        }

        // Tab didn't handle it — fall back to global keybindings.
        if let Event::Key(k) = ev {
            self.handle_global_key(k)?;
        }
        Ok(())
    }

    fn handle_global_key(&mut self, k: KeyEvent) -> Result<()> {
        match (k.code, k.modifiers) {
            (KeyCode::Char('q'), KeyModifiers::NONE) => {
                self.should_quit = true;
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            (KeyCode::Char('?'), _) => {
                self.mode = Mode::Help;
            }
            (KeyCode::Char('g'), KeyModifiers::NONE) => {
                self.mode = Mode::GitLeader;
            }
            (KeyCode::Tab, _) => {
                let next = (self.active + 1) % self.tabs.len();
                self.switch_tab(next)?;
            }
            (KeyCode::BackTab, _) => {
                let prev = (self.active + self.tabs.len() - 1) % self.tabs.len();
                self.switch_tab(prev)?;
            }
            (KeyCode::Char(c), _) if c.is_ascii_digit() => {
                let idx = (c as u8 - b'1') as usize;
                if idx < self.tabs.len() {
                    self.switch_tab(idx)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn switch_tab(&mut self, idx: usize) -> Result<()> {
        if idx == self.active || idx >= self.tabs.len() {
            return Ok(());
        }
        let mut ctx = TabCtx {
            vault: &self.vault,
            recents: &self.recents,
            today: self.today,
            last_refresh: &self.last_refresh,
            pending_request: &self.pending_request,
        };
        self.tabs[self.active].on_blur(&mut ctx)?;
        self.active = idx;
        self.tabs[self.active].on_focus(&mut ctx)?;
        Ok(())
    }

    fn service_request(
        &mut self,
        terminal: &mut Tui,
        events: &EventStream,
        req: AppRequest,
    ) -> Result<()> {
        match req {
            AppRequest::OpenInEditor { path, line } => {
                self.dispatch_open_in_editor(terminal, events, &path, line)?;
                Ok(())
            }
            AppRequest::OpenInObsidian { url } => {
                spawn_url_opener(&url)
                    .with_context(|| format!("could not launch URL handler for {url}"))?;
                Ok(())
            }
            AppRequest::Toast { text, style } => {
                *self.toast.borrow_mut() = Some(Toast {
                    text,
                    style,
                    deadline: std::time::Instant::now() + TOAST_DURATION,
                });
                Ok(())
            }
            AppRequest::SyncGit { message } => {
                self.dispatch_sync_git(events, message)?;
                Ok(())
            }
        }
    }

    /// Submit a git sync to a background worker thread. Returns
    /// immediately; the worker posts an [`Event::Background(BgEvent::SyncCompleted)`]
    /// back into the shared event channel when done, and
    /// [`Self::handle_background`] renders the outcome.
    ///
    /// Re-entrancy: if a sync is already in flight (`self.jobs` is
    /// `Some`), surface a toast and do nothing — a second sync would
    /// be redundant (the first will pick up everything the second
    /// would have pushed) and queueing them serves no purpose.
    ///
    /// `discover_repo` is checked at submission time so the user gets
    /// an immediate "no git repository" toast instead of a delayed
    /// completion error. The strategy is read from config the same
    /// way the synchronous v1 did.
    fn dispatch_sync_git(&mut self, events: &EventStream, message: Option<String>) -> Result<()> {
        if self.jobs.borrow().is_some() {
            self.push_toast("sync already in progress", ToastStyle::Info);
            return Ok(());
        }

        let repo = match discover_repo(&self.vault.path) {
            Some(r) => r,
            None => {
                self.push_toast(
                    "no git repository at or above vault root",
                    ToastStyle::Error,
                );
                return Ok(());
            }
        };

        let strategy = self.vault.config.config.git.pull_strategy;
        let opts = SyncOptions { strategy, message };

        // Take a sender clone for the worker and mark the slot busy
        // *before* spawning so a fast-completing job can't race us into
        // an inconsistent state (worker posts → main loop matches →
        // `jobs.take()` finds None and the indicator never lit).
        let tx = events.sender();
        *self.jobs.borrow_mut() = Some(JobHandle::new(JobKind::Sync));
        self.push_toast("syncing in background…", ToastStyle::Info);

        std::thread::spawn(move || run_sync_job(repo, opts, tx));

        Ok(())
    }

    /// Apply a background event to the App state. Currently the only
    /// variant is [`BgEvent::SyncCompleted`]; future plans add more.
    fn handle_background(&mut self, bg: BgEvent) -> Result<()> {
        match bg {
            BgEvent::SyncCompleted(result) => self.apply_sync_result(result),
        }
    }

    /// Map a finished sync's outcome onto the user-facing surface —
    /// toast for clean / synced, modal for conflict, error toast for
    /// hard failure — and refresh the *currently active* tab so the
    /// pulled-in changes (or conflict markers) are reflected. We use
    /// `self.active` at completion time, not at submission time:
    /// tab-switching during a background sync is allowed, and the
    /// most useful tab to refresh is the one the user is looking at
    /// now.
    fn apply_sync_result(&mut self, result: SyncJobResult) -> Result<()> {
        *self.jobs.borrow_mut() = None;

        {
            let mut ctx = TabCtx {
                vault: &self.vault,
                recents: &self.recents,
                today: self.today,
                last_refresh: &self.last_refresh,
                pending_request: &self.pending_request,
            };
            let _ = self.tabs[self.active].refresh(&mut ctx);
        }

        match result.outcome {
            Ok(SyncOutcome::Clean { pushed: false }) => {
                self.push_toast("already in sync", ToastStyle::Success);
            }
            Ok(SyncOutcome::Clean { pushed: true }) => {
                self.push_toast("pushed local commits", ToastStyle::Success);
            }
            Ok(SyncOutcome::Synced {
                committed,
                pulled,
                pushed,
            }) => {
                let mut parts = vec![format!("committed {committed}")];
                if pulled {
                    parts.push("pulled".to_string());
                }
                if pushed {
                    parts.push("pushed".to_string());
                }
                let text = format!("sync ok — {}", parts.join(", "));
                self.push_toast(text, ToastStyle::Success);
            }
            Ok(SyncOutcome::MergeConflict { files }) => {
                *self.sync_conflict.borrow_mut() = Some(SyncConflictInfo {
                    kind: SyncConflictKind::Merge,
                    files,
                });
                self.mode = Mode::SyncConflict;
            }
            Ok(SyncOutcome::RebaseConflict { files }) => {
                *self.sync_conflict.borrow_mut() = Some(SyncConflictInfo {
                    kind: SyncConflictKind::Rebase,
                    files,
                });
                self.mode = Mode::SyncConflict;
            }
            Err(msg) => {
                self.push_toast(format!("git sync failed: {msg}"), ToastStyle::Error);
            }
        }
        Ok(())
    }

    fn push_toast(&self, text: impl Into<String>, style: ToastStyle) {
        *self.toast.borrow_mut() = Some(Toast {
            text: text.into(),
            style,
            deadline: std::time::Instant::now() + TOAST_DURATION,
        });
    }

    /// Strategy-aware editor handoff (plan 011). Resolves the
    /// configured [`EditorStrategy`] against `$TMUX`, builds the
    /// matching invocation, and dispatches:
    ///
    /// - [`EditorStrategy::Suspend`] — suspend the alt-screen, run
    ///   the editor inline, restore on exit, drain spurious DCS/DA
    ///   replies, then refresh.
    /// - [`EditorStrategy::TmuxPopup`] — run
    ///   `tmux display-popup -E -- <editor>`; the popup blocks until
    ///   the editor exits and ft keeps drawing behind it. No suspend.
    /// - [`EditorStrategy::TmuxWindow`] / [`EditorStrategy::TmuxSplit`]
    ///   — spawn the editor in a sibling pane wrapped in `sh -c
    ///   '<editor>; tmux wait-for -S <sig>'`, then block on `tmux
    ///   wait-for <sig>` so the post-edit refresh sees on-disk
    ///   state. No suspend.
    ///
    /// When the strategy is `Tmux*` but `tmux` isn't on `PATH`, falls
    /// back to `Suspend` and surfaces an error toast so the user knows
    /// why their popup didn't appear.
    fn dispatch_open_in_editor(
        &mut self,
        terminal: &mut Tui,
        events: &EventStream,
        path: &Path,
        line: usize,
    ) -> Result<()> {
        let cfg_editor = self.vault.config.config.editor.clone();
        let strategy = cfg_editor.strategy.resolve();
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".to_string());

        let status = match strategy {
            EditorStrategy::Suspend => {
                self.run_editor_suspended(terminal, events, &editor, path, line)
            }
            EditorStrategy::TmuxPopup => {
                let inv = build_invocation(
                    strategy,
                    &editor,
                    path,
                    line,
                    &cfg_editor.popup_width,
                    &cfg_editor.popup_height,
                );
                let outcome = run_invocation(&inv);
                self.fall_back_to_suspend_on_missing_tmux(
                    terminal, events, &editor, path, line, outcome,
                )
            }
            EditorStrategy::TmuxWindow | EditorStrategy::TmuxSplit => {
                let signal = unique_signal_name();
                let inv = build_wait_for_invocation(strategy, &editor, path, line, &signal);
                let spawn_outcome = run_invocation(&inv);
                if spawn_outcome.is_ok() {
                    // Block on the wait-for handshake so the post-edit
                    // refresh runs against on-disk state. tmux signal
                    // delivery is best-effort; if `wait-for` fails
                    // (server killed, signal name collision), we just
                    // proceed — worst case the user `r`-refreshes.
                    let _ = Command::new("tmux").args(["wait-for", &signal]).status();
                }
                self.fall_back_to_suspend_on_missing_tmux(
                    terminal,
                    events,
                    &editor,
                    path,
                    line,
                    spawn_outcome,
                )
            }
        };

        // Whatever the editor did, force a refresh so the active tab
        // reflects on-disk state. Mirrors the pre-plan-011 behavior.
        {
            let mut ctx = TabCtx {
                vault: &self.vault,
                recents: &self.recents,
                today: self.today,
                last_refresh: &self.last_refresh,
                pending_request: &self.pending_request,
            };
            self.tabs[self.active].refresh(&mut ctx)?;
        }
        status?;
        Ok(())
    }

    /// Suspend the alt-screen, run the inline-editor invocation, then
    /// restore. Used directly by the `Suspend` strategy and as the
    /// fallback when a `Tmux*` strategy can't find tmux on `PATH`.
    fn run_editor_suspended(
        &mut self,
        terminal: &mut Tui,
        events: &EventStream,
        editor: &str,
        path: &Path,
        line: usize,
    ) -> io::Result<()> {
        suspend_terminal(terminal)
            .map_err(|e| io::Error::other(format!("suspend_terminal: {e}")))?;
        let inv = build_invocation(EditorStrategy::Suspend, editor, path, line, "", "");
        let status = run_invocation(&inv);
        restore_terminal(terminal)
            .map_err(|e| io::Error::other(format!("restore_terminal: {e}")))?;
        // Terminals often emit response sequences (DA1, DCS replies for
        // XTGETTCAP) when raw mode flips back on, and the user may have
        // typed during the editor session. Drain so the next
        // `events.next()` returns a genuine keypress and not a `/` from
        // a DCS reply that puts us into query-edit mode.
        events.drain(Duration::from_millis(120));
        let _ = terminal.clear();
        status
    }

    /// If `outcome` is `NotFound`, surface an error toast saying tmux
    /// is missing and re-run the editor under the suspend strategy.
    /// Other errors pass through unchanged.
    fn fall_back_to_suspend_on_missing_tmux(
        &mut self,
        terminal: &mut Tui,
        events: &EventStream,
        editor: &str,
        path: &Path,
        line: usize,
        outcome: io::Result<()>,
    ) -> io::Result<()> {
        match outcome {
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
                *self.toast.borrow_mut() = Some(Toast {
                    text: "tmux not found — opening editor inline".into(),
                    style: ToastStyle::Error,
                    deadline: std::time::Instant::now() + TOAST_DURATION,
                });
                self.run_editor_suspended(terminal, events, editor, path, line)
            }
            other => other,
        }
    }
}

/// Spawn `invocation` and wait for it to exit. Returns `Ok(())` on
/// successful exit, `Err(io::Error)` on spawn failure or non-zero exit.
/// The non-zero exit case is preserved as an `io::Error::other` so the
/// suspend-fallback logic can distinguish "tmux not found"
/// (`NotFound`) from "editor returned non-zero" (other).
fn run_invocation(inv: &EditorInvocation) -> io::Result<()> {
    let status = Command::new(&inv.program).args(&inv.args).status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "editor `{}` exited non-zero: {status}",
            inv.program
        )));
    }
    Ok(())
}

/// Resolve "today" for the current run. Honors `FT_TODAY=YYYY-MM-DD` to keep
/// the TUI deterministic in tests and reproducible with the CLI.
fn resolve_today() -> NaiveDate {
    std::env::var("FT_TODAY")
        .ok()
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .unwrap_or_else(|| Local::now().date_naive())
}

// --- background workers ------------------------------------------------------

/// Body of the `g s` worker thread (plan 014). Owns all its inputs
/// (`repo`, `opts`, `tx`) so no borrows cross the thread boundary;
/// `Send` makes that a compile-time guarantee. Runs the synchronous
/// `ft_core::git::sync` call to completion, then posts exactly one
/// [`BgEvent::SyncCompleted`] back into the main loop.
///
/// Panics are caught and converted into `Err` payloads so a bug in
/// the sync chain doesn't strand the in-flight indicator forever.
/// Send failures (channel closed because the app is tearing down)
/// are swallowed — there's nothing left to render the result to.
fn run_sync_job(repo: std::path::PathBuf, opts: SyncOptions, tx: std::sync::mpsc::Sender<Event>) {
    let outcome =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sync(&repo, &opts))) {
            Ok(Ok(o)) => Ok(o),
            Ok(Err(e)) => Err(format!("{e:#}")),
            Err(panic) => {
                let msg = panic_message(&panic);
                Err(format!("internal panic in sync worker: {msg}"))
            }
        };

    let _ = tx.send(Event::Background(BgEvent::SyncCompleted(SyncJobResult {
        outcome,
        repo,
    })));
}

/// Extract a human-readable message from `catch_unwind`'s payload.
/// Panic payloads are typed `Box<dyn Any + Send>`; the standard payload
/// from `panic!("...")` is either `&'static str` or `String`.
fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = panic.downcast_ref::<String>() {
        return s.clone();
    }
    "unknown panic payload".to_string()
}

// --- editor handoff ----------------------------------------------------------

fn suspend_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    terminal.hide_cursor()?;
    Ok(())
}

/// Fire-and-forget the OS URL handler for `url`. `open` on macOS,
/// `xdg-open` on every other unix. We don't wait for the child — Obsidian
/// raises its own window and the TUI keeps drawing.
fn spawn_url_opener(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(target_os = "macos"))]
    let program = "xdg-open";

    Command::new(program)
        .arg(url)
        .spawn()
        .with_context(|| format!("failed to launch `{program}`"))?;
    Ok(())
}

// --- test-only helpers ---------------------------------------------------

#[cfg(test)]
impl App {
    /// Construct an App without starting the event loop. Useful for
    /// snapshot tests that drive `draw` directly with a TestBackend.
    ///
    /// Accepts an owned [`Vault`] (rather than `Arc<Vault>`) so the
    /// existing test sites stay unchanged after the production refactor;
    /// the wrap-in-`Arc` happens here so production and test go through
    /// the same internal shape.
    ///
    /// Routes the recents log under `vault.path/.ft-state/` so test runs
    /// never touch the user's real `$XDG_STATE_HOME`. Since `vault.path`
    /// is itself a `TempDir` in tests, the log is cleaned up on drop.
    pub fn for_test(vault: Vault) -> Self {
        let recents = Self::test_recents_for(&vault);
        Self::new_with_recents(Arc::new(vault), recents)
    }

    /// Like [`for_test`], but injects a fixed clock and derives `today` from
    /// it so snapshots are deterministic without relying on `FT_TODAY`.
    pub fn for_test_with_clock(vault: Vault, clock: ClockFn) -> Self {
        let today = clock().date_naive();
        let tabs: Vec<Box<dyn Tab>> = vec![
            Box::new(GraphTab::new()),
            Box::new(TasksTab::with_clock(clock)),
            Box::new(NotesTab::new()),
            // TimeblocksTab shares the same ClockFn type alias as
            // TasksTab so the same fixture-clock can drive both panes.
            Box::new(TimeblocksTab::with_clock(clock)),
        ];
        let recents = Self::test_recents_for(&vault);
        Self::with_tabs(Arc::new(vault), recents, today, tabs)
    }

    /// Variant of [`for_test`] that lets the caller inspect / pre-seed the
    /// recents log via a shared `Arc`. Used by recents-aware behavior
    /// tests that need to assert on log writes.
    pub fn for_test_with_recents(vault: Vault, recents: Arc<RecentsLog>) -> Self {
        Self::new_with_recents(Arc::new(vault), recents)
    }

    /// Like [`for_test_with_clock`] but also takes an explicit recents
    /// log so the test can pre-seed open history. Used by the
    /// recents-snapshot test which needs both a fixed clock (for stable
    /// status-bar timestamps) and a pre-populated log.
    pub fn for_test_with_clock_and_recents(
        vault: Vault,
        clock: ClockFn,
        recents: Arc<RecentsLog>,
    ) -> Self {
        let today = clock().date_naive();
        let tabs: Vec<Box<dyn Tab>> = vec![
            Box::new(GraphTab::new()),
            Box::new(TasksTab::with_clock(clock)),
            Box::new(NotesTab::new()),
            // TimeblocksTab shares the same ClockFn type alias as
            // TasksTab so the same fixture-clock can drive both panes.
            Box::new(TimeblocksTab::with_clock(clock)),
        ];
        Self::with_tabs(Arc::new(vault), recents, today, tabs)
    }

    fn test_recents_for(vault: &Vault) -> Arc<RecentsLog> {
        let log_path = vault.path.join(".ft-state").join("recents.jsonl");
        Arc::new(RecentsLog::with_log_path(vault.path.clone(), log_path))
    }

    pub fn render_to(&mut self, frame: &mut Frame) {
        self.draw(frame);
    }

    pub fn enter_help(&mut self) {
        self.mode = Mode::Help;
    }

    /// Test-only inspection of the App's current mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn switch_to(&mut self, idx: usize) -> Result<()> {
        self.switch_tab(idx)
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active_title(&self) -> &str {
        self.tabs[self.active].title()
    }

    /// Help sections the active tab contributes to the `?` overlay. Used
    /// by the test that asserts every tab wires its own keybinding doc.
    pub fn active_tab_help_sections(&self) -> Vec<crate::tui::help::HelpSection> {
        self.tabs[self.active].help_sections()
    }

    pub fn dispatch(&mut self, ev: Event) -> Result<()> {
        self.handle_event(ev)
    }

    pub fn is_quit(&self) -> bool {
        self.should_quit
    }

    /// Inspect or take any pending request that the active tab/view raised.
    /// Used by tests to assert that an Enter keypress queued an editor open.
    pub fn take_pending_request(&self) -> Option<AppRequest> {
        self.pending_request.borrow_mut().take()
    }

    /// Service whatever pending `AppRequest` is queued (or do nothing if
    /// none). Mirrors what `run` does between iterations — tests use this
    /// to drive the toast / refresh side-effects without spinning up a
    /// real event loop.
    pub fn service_pending_for_test(&mut self) -> Result<()> {
        if let Some(req) = self.pending_request.borrow_mut().take() {
            match req {
                AppRequest::Toast { text, style } => {
                    *self.toast.borrow_mut() = Some(Toast {
                        text,
                        style,
                        deadline: std::time::Instant::now() + TOAST_DURATION,
                    });
                }
                // Other variants need terminal state; tests that exercise
                // them go through the real `service_request` path.
                _ => {
                    *self.pending_request.borrow_mut() = Some(req);
                }
            }
        }
        Ok(())
    }

    /// Currently-active toast, if any. Used by tests to assert the
    /// post-create UX.
    pub fn current_toast(&self) -> Option<Toast> {
        self.toast.borrow().clone()
    }

    /// In-flight job kind for the renderer, if any. Used by tests to
    /// assert the re-entrancy guard and the status-bar indicator.
    pub fn in_flight_job_for_test(&self) -> Option<JobKind> {
        self.in_flight_job()
    }

    /// Pretend a job is in flight without actually spawning a worker.
    /// Used by re-entrancy / indicator tests that don't care about the
    /// thread side of things.
    pub fn set_in_flight_for_test(&self, kind: JobKind) {
        *self.jobs.borrow_mut() = Some(JobHandle::new(kind));
    }

    /// Drive the real `dispatch_sync_git` against a test-provided event
    /// channel. Used by the one end-to-end integration test that walks
    /// a real bare-origin / clone handshake through the worker thread.
    pub fn submit_sync_for_test(
        &mut self,
        events: &EventStream,
        message: Option<String>,
    ) -> Result<()> {
        self.dispatch_sync_git(events, message)
    }
}

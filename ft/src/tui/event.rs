use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

use anyhow::Result;
use crossterm::event::{self, Event as CtEvent, KeyEvent, KeyEventKind, MouseEvent};
use ft_core::git::{CommitOutcome, SyncOutcome};

/// Events flowing through the TUI loop. `Tick` fires once per second so the
/// sidebar clock can update without forcing a full redraw on every keystroke.
/// `Mouse` and `Resize` payloads are routed but not consumed in session 1;
/// later sessions will drive layout caches off `Resize`.
///
/// [`Event::Background`] carries results posted by worker threads spawned
/// off the main loop (plan 014). Background work uses the *same* `mpsc`
/// channel as user input: the worker holds a clone of the sender vended
/// by [`EventStream::sender`] and sends a single completion message when
/// done. The main loop then matches the new variant just like a
/// keystroke — no second receiver, no `select!`, no polling.
#[derive(Debug, Clone)]
pub enum Event {
    Key(KeyEvent),
    #[allow(dead_code)] // routed but not consumed yet; reserved for future sessions
    Mouse(MouseEvent),
    #[allow(dead_code)] // routed but not consumed yet; reserved for future sessions
    Resize(u16, u16),
    Tick,
    Background(BgEvent),
}

/// Completion messages from worker threads. One variant per concurrent
/// job kind. Future plans (file watching, fuzzy-index rebuilds) add
/// variants here without touching the event-loop shape.
#[derive(Debug, Clone)]
pub enum BgEvent {
    SyncCompleted(SyncJobResult),
    CommitCompleted(CommitJobResult),
}

/// Result of a single `ft_core::git::sync` job spawned by the TUI.
///
/// `outcome` is `Ok` for any defined `SyncOutcome` (clean, synced, or
/// conflicted) and `Err` for hard failures (no upstream, push rejected,
/// network error). Errors are stringified at the worker boundary
/// (`format!("{e:#}")`) so the on-wire `Event` enum stays `Clone` and
/// the renderer needs only a flat message.
///
/// `repo` is the discovered toplevel at submission time; carried for
/// completeness even though v1's single-slot handler doesn't disambiguate.
#[derive(Debug, Clone)]
pub struct SyncJobResult {
    pub outcome: Result<SyncOutcome, String>,
    /// Carried for future plans (multi-repo, job-id disambiguation).
    /// Unread for v1's single-slot handler.
    #[allow(dead_code)]
    pub repo: PathBuf,
}

/// Result of a single `ft_core::git::commit` job spawned by the TUI.
/// Mirrors [`SyncJobResult`] for the lightweight commit-only path:
/// `outcome` is `Ok` for [`CommitOutcome::Clean`] / [`CommitOutcome::Committed`],
/// `Err` for hard failures (conflicted tree, commit failure).
#[derive(Debug, Clone)]
pub struct CommitJobResult {
    pub outcome: Result<CommitOutcome, String>,
    #[allow(dead_code)]
    pub repo: PathBuf,
}

/// Channel-backed event source: a background thread polls crossterm and sends
/// `Event::Tick` on a 1s cadence; the main loop drains via `next()`.
pub struct EventStream {
    rx: Receiver<Event>,
    tx: Sender<Event>,
}

impl EventStream {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::channel();
        let crossterm_tx = tx.clone();
        thread::spawn(move || crossterm_loop(crossterm_tx, tick_rate));
        Self { rx, tx }
    }

    /// Block until the next event arrives. Errors only on channel teardown.
    pub fn next(&self) -> Result<Event> {
        self.rx.recv().map_err(Into::into)
    }

    /// Clone of the internal sender, for worker threads that post
    /// `Event::Background(_)` results back into the main loop. Dropping
    /// the receiver (app teardown) makes subsequent `send` calls return
    /// `Err`, which the worker treats as "give up silently."
    pub fn sender(&self) -> Sender<Event> {
        self.tx.clone()
    }

    /// Discard every event received during `window`. Used after returning
    /// from `$EDITOR` so terminal-response escape sequences (e.g. DCS
    /// `XTGETTCAP` replies) and any keys typed during the editor session
    /// don't leak into the next read — without this, the `/` byte of a
    /// DCS reply puts the search view into edit mode and the rest of the
    /// reply gets typed into the query buffer.
    pub fn drain(&self, window: Duration) {
        let deadline = std::time::Instant::now() + window;
        loop {
            // Empty the channel of everything currently queued.
            while self.rx.try_recv().is_ok() {}
            if std::time::Instant::now() >= deadline {
                return;
            }
            // Yield so the background crossterm thread has a chance to
            // ingest more bytes from stdin and push them through.
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn crossterm_loop(tx: Sender<Event>, tick_rate: Duration) {
    let mut last_tick = std::time::Instant::now();
    loop {
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::ZERO);
        let has_event = event::poll(timeout).unwrap_or(false);
        if has_event {
            match event::read() {
                Ok(CtEvent::Key(k)) if k.kind == KeyEventKind::Press => {
                    if tx.send(Event::Key(k)).is_err() {
                        return;
                    }
                }
                Ok(CtEvent::Mouse(m)) => {
                    if tx.send(Event::Mouse(m)).is_err() {
                        return;
                    }
                }
                Ok(CtEvent::Resize(w, h)) => {
                    if tx.send(Event::Resize(w, h)).is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                Err(_) => return,
            }
        }
        if last_tick.elapsed() >= tick_rate {
            if tx.send(Event::Tick).is_err() {
                return;
            }
            last_tick = std::time::Instant::now();
        }
    }
}

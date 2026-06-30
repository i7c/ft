#![allow(dead_code)] // JobKind variants surfaced in §8 (ft commands list) and the status bar

//! Background job tracking for the TUI event loop (plan 014).
//!
//! Concurrency model: the App owns a single-slot
//! `RefCell<Option<JobHandle>>` recording whether any background worker
//! is in flight. The worker thread itself is fire-and-forget — we never
//! `join()` it. It owns its inputs (moved `move ||` into the closure),
//! does the work, and posts exactly one [`crate::tui::event::BgEvent`]
//! into the shared event channel before exiting.
//!
//! Re-entrant submissions (e.g. a second `g s` while a sync is running)
//! check `App.jobs.borrow().is_some()` and toast "already in progress"
//! instead of spawning a second worker.

use std::time::Instant;

/// One in-flight background job. Single-slot for v1 — we only ever run
/// one job kind at a time. If/when concurrent jobs become a thing
/// (a fetch while a sync runs), upgrade `App.jobs` to a `HashMap<JobId,
/// JobHandle>` and add a job id to `BgEvent` variants.
#[derive(Debug)]
pub struct JobHandle {
    pub kind: JobKind,
    /// Wall-clock start, used by the status-bar renderer if we ever
    /// want to surface a "(syncing for 2s)" hint after a threshold.
    /// Stored but unread for v1.
    #[allow(dead_code)]
    pub started_at: Instant,
}

impl JobHandle {
    pub fn new(kind: JobKind) -> Self {
        Self {
            kind,
            started_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Sync,
    /// `ft git commit` equivalent — stage + commit only, no pull/push.
    /// Spawned by the `g c` leader chord.
    Commit,
}

impl JobKind {
    /// Short label rendered in the status-bar in-flight indicator.
    /// Returned verbatim from the renderer's right-cell composer.
    pub fn indicator_label(self) -> &'static str {
        match self {
            JobKind::Sync => "sync",
            JobKind::Commit => "commit",
        }
    }
}

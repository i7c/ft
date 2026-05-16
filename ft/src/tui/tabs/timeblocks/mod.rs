//! Timeblocks tab — read-only today + tomorrow view (plan 015 session 4).
//!
//! Layout: sidebar (24 cols) + main split horizontally between **Today**
//! and **Tomorrow** panes (50/50). The sidebar shows a live clock, today's
//! date, and per-top-level-tag totals for today's blocks. Each pane has
//! its own selection cursor; `h`/`l` (or `←`/`→`) toggles pane focus,
//! `j`/`k` / `↓`/`↑` move within the focused pane, `g`/`G` jump
//! first/last, `r` re-reads both days. (Tab and Shift+Tab are reserved
//! for the App's global tab-cycle, so we deliberately don't shadow them
//! here — see plan 015 session 4 outcome for the rationale.)
//!
//! Mutations land in session 5 — this session is read-only, so the tab
//! never writes to disk.

use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ft_core::timeblock::{doc::Document, Timeblock};
use ratatui::{layout::Rect, Frame};

use crate::tui::{
    event::Event,
    tab::{EventOutcome, Tab, TabCtx},
};

mod view;

/// Function pointer for "what time is it now?". Production uses
/// [`Local::now`]; tests inject a fixed value for deterministic snapshots.
pub type ClockFn = fn() -> DateTime<Local>;

fn local_now() -> DateTime<Local> {
    Local::now()
}

/// Sidebar width matches the Tasks tab so the column stays aligned when
/// the user switches tabs mid-session.
pub(crate) const SIDEBAR_WIDTH: u16 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pane {
    Today,
    Tomorrow,
}

/// Per-pane state. `path` is the resolved daily-note path (None when
/// `[periodic_notes.daily]` isn't configured — both panes share the
/// same not-configured state then). `present` distinguishes "file
/// missing on disk" (renders the placeholder) from "file exists but
/// section is empty" (renders an empty list).
pub(crate) struct PaneState {
    pub date: chrono::NaiveDate,
    /// Resolved daily-note path. Read in session 5 by the `c` chord
    /// (create-tomorrow via the daily-note template).
    #[allow(dead_code)]
    pub path: Option<PathBuf>,
    pub present: bool,
    pub blocks: Vec<Timeblock>,
    pub selection: usize,
}

impl PaneState {
    fn empty(date: chrono::NaiveDate) -> Self {
        Self {
            date,
            path: None,
            present: false,
            blocks: Vec::new(),
            selection: 0,
        }
    }
}

pub struct TimeblocksTab {
    pub(crate) clock: ClockFn,
    pub(crate) today: PaneState,
    pub(crate) tomorrow: PaneState,
    pub(crate) focus: Pane,
    /// Heading the panes were last loaded under. Refresh is cheap so we
    /// could read this from `ctx.vault.config` every render, but caching
    /// it removes an allocation on the hot path.
    pub(crate) heading: String,
    /// Most recent load error (e.g. malformed file). Surfaced in the
    /// status-bar via a Toast in session 5; for now we expose it via the
    /// test API.
    #[allow(dead_code)]
    pub(crate) last_error: Option<String>,
}

impl TimeblocksTab {
    pub fn new() -> Self {
        Self::with_clock(local_now)
    }

    pub fn with_clock(clock: ClockFn) -> Self {
        let now = (clock)().date_naive();
        Self {
            clock,
            today: PaneState::empty(now),
            tomorrow: PaneState::empty(now + chrono::Duration::days(1)),
            focus: Pane::Today,
            heading: "Time Blocks".into(),
            last_error: None,
        }
    }

    /// Re-read both days from disk. Called from `on_focus` and `r`.
    fn reload(&mut self, ctx: &mut TabCtx) {
        self.heading = ctx.vault.config.config.timeblocks_heading().to_string();
        let today = ctx.today;
        let tomorrow = today + chrono::Duration::days(1);
        self.today = self.load_pane(ctx, today);
        self.tomorrow = self.load_pane(ctx, tomorrow);
        self.clamp_selection();
    }

    fn load_pane(&mut self, ctx: &TabCtx, date: chrono::NaiveDate) -> PaneState {
        let path = match ctx.vault.resolve_target(date, None) {
            Ok(p) => Some(p),
            Err(e) => {
                self.last_error = Some(format!("{e}"));
                return PaneState::empty(date);
            }
        };
        let exists = path.as_ref().map(|p| p.exists()).unwrap_or(false);
        if !exists {
            return PaneState {
                date,
                path,
                present: false,
                blocks: Vec::new(),
                selection: 0,
            };
        }
        let p = path.as_ref().unwrap();
        match Document::read(p, &self.heading) {
            Ok(doc) => PaneState {
                date,
                path: path.clone(),
                present: true,
                blocks: doc.blocks,
                selection: 0,
            },
            Err(e) => {
                self.last_error = Some(format!("{e}"));
                PaneState {
                    date,
                    path,
                    present: true,
                    blocks: Vec::new(),
                    selection: 0,
                }
            }
        }
    }

    fn clamp_selection(&mut self) {
        for pane in [&mut self.today, &mut self.tomorrow] {
            if pane.blocks.is_empty() {
                pane.selection = 0;
            } else if pane.selection >= pane.blocks.len() {
                pane.selection = pane.blocks.len() - 1;
            }
        }
    }

    fn pane_mut(&mut self, p: Pane) -> &mut PaneState {
        match p {
            Pane::Today => &mut self.today,
            Pane::Tomorrow => &mut self.tomorrow,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let pane = self.pane_mut(self.focus);
        let len = pane.blocks.len();
        if len == 0 {
            return;
        }
        let cur = pane.selection as isize;
        let new = (cur + delta).clamp(0, (len as isize) - 1);
        pane.selection = new as usize;
    }

    fn jump_selection(&mut self, to_end: bool) {
        let pane = self.pane_mut(self.focus);
        if pane.blocks.is_empty() {
            return;
        }
        pane.selection = if to_end { pane.blocks.len() - 1 } else { 0 };
    }

    fn toggle_focus(&mut self, forward: bool) {
        self.focus = match (self.focus, forward) {
            (Pane::Today, _) => Pane::Tomorrow,
            (Pane::Tomorrow, _) => Pane::Today,
        };
    }

    fn handle_key(&mut self, key: KeyEvent) -> EventOutcome {
        // Tab / Shift+Tab are deliberately NOT consumed here — they belong
        // to the App's global tab-cycle. `h`/`l` (or `←`/`→`) toggle pane
        // focus instead.
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                EventOutcome::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                EventOutcome::Consumed
            }
            KeyCode::Char('g') => {
                self.jump_selection(false);
                EventOutcome::Consumed
            }
            KeyCode::Char('G') => {
                self.jump_selection(true);
                EventOutcome::Consumed
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.toggle_focus(false);
                EventOutcome::Consumed
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.toggle_focus(true);
                EventOutcome::Consumed
            }
            KeyCode::Char('r') => {
                // Refresh happens in handle_event so the ctx is available.
                EventOutcome::NotHandled
            }
            _ => EventOutcome::NotHandled,
        }
    }
}

impl Default for TimeblocksTab {
    fn default() -> Self {
        Self::new()
    }
}

impl Tab for TimeblocksTab {
    fn title(&self) -> &str {
        "Timeblocks"
    }

    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()> {
        self.reload(ctx);
        Ok(())
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };
        // `r` needs ctx access for the reload; handle it here before
        // delegating to the keymap.
        if matches!(k.code, KeyCode::Char('r')) && k.modifiers == KeyModifiers::NONE {
            self.reload(ctx);
            return Ok(EventOutcome::Consumed);
        }
        Ok(self.handle_key(k))
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        view::render(self, frame, area, ctx);
    }

    fn refresh(&mut self, ctx: &mut TabCtx) -> Result<()> {
        self.reload(ctx);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};

    fn clock() -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 5, 16, 9, 30, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn new_with_clock_seeds_today_and_tomorrow_dates() {
        let tab = TimeblocksTab::with_clock(clock);
        assert_eq!(
            tab.today.date,
            NaiveDate::from_ymd_opt(2026, 5, 16).unwrap()
        );
        assert_eq!(
            tab.tomorrow.date,
            NaiveDate::from_ymd_opt(2026, 5, 17).unwrap()
        );
        assert_eq!(tab.focus, Pane::Today);
    }

    #[test]
    fn toggle_focus_round_trips() {
        let mut tab = TimeblocksTab::with_clock(clock);
        assert_eq!(tab.focus, Pane::Today);
        tab.toggle_focus(true);
        assert_eq!(tab.focus, Pane::Tomorrow);
        tab.toggle_focus(true);
        assert_eq!(tab.focus, Pane::Today);
    }

    #[test]
    fn move_selection_clamps_to_block_count() {
        let mut tab = TimeblocksTab::with_clock(clock);
        tab.today.blocks = vec![mk(9, 0, 10, 0, "a"), mk(10, 0, 11, 0, "b")];
        tab.move_selection(1);
        assert_eq!(tab.today.selection, 1);
        tab.move_selection(5);
        assert_eq!(tab.today.selection, 1, "should clamp at last index");
        tab.move_selection(-99);
        assert_eq!(tab.today.selection, 0, "should clamp at zero");
    }

    #[test]
    fn jump_selection_handles_empty_pane() {
        let mut tab = TimeblocksTab::with_clock(clock);
        tab.jump_selection(true);
        assert_eq!(tab.today.selection, 0);
    }

    #[test]
    fn move_selection_does_nothing_on_empty_pane() {
        let mut tab = TimeblocksTab::with_clock(clock);
        tab.move_selection(1);
        assert_eq!(tab.today.selection, 0);
    }

    fn mk(sh: u32, sm: u32, eh: u32, em: u32, desc: &str) -> Timeblock {
        use chrono::NaiveTime;
        let start = NaiveTime::from_hms_opt(sh, sm, 0).unwrap();
        let end = NaiveTime::from_hms_opt(eh, em, 0).unwrap();
        Timeblock {
            start,
            end,
            end_explicit: true,
            desc: desc.into(),
            tags: ft_core::timeblock::parse_tags(desc),
            source_line: 1,
        }
    }
}

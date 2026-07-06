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
//! Time-adjustment chords on the focused block (5-minute steps):
//! - `]` / `[` — grow / shrink end
//! - `}` / `{` — push / pull start
//! - `>` / `<` — shift the whole block later / earlier (start + end
//!   move together, duration preserved).
//!
//! `f` toggles between split (today + tomorrow) and single-day
//! full-width view. In single-day mode, `h`/`l` flip which day is
//! shown (same key that shifts focus in split mode).
//!
//! Date navigation (move the visible window away from today):
//! - `H` / `L` — slide the anchor day back / forward by one day
//! - `T` — jump back to actual today
//!
//! Block editing:
//! - `a` quickline, `A` form, `e` edit description, `d d` delete
//! - `t` tag modal (add `+@tag` / remove `-@tag`, space-separated)
//! - `c` creates a missing daily note via the configured template
//!
//! Mutations land in session 5 — this session is read-only, so the tab
//! never writes to disk.

use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Local, NaiveTime, Timelike};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ft_core::timeblock::{
    self,
    doc::Document,
    ops::{self, AddOptions, EditMutation, Selector, TimeChange},
    Tag, Timeblock,
};
use ratatui::{layout::Rect, Frame};

use std::sync::LazyLock;

use crate::tui::{
    command::{Command, CommandDef, CommandOutcome, CommandScope},
    event::Event,
    help::HelpSection,
    keymap::{KeyChord, KeyMap},
    tab::{AppRequest, EventOutcome, Tab, TabCtx, TabKind, ToastStyle},
    widgets::EditBuffer,
};

mod view;

// ── Commands ─────────────────────────────────────────────────────────

/// Every action the Timeblocks tab exposes through the command/keymap
/// layer. Idle-mode keys only — the per-mode handlers (DeleteConfirm,
/// Quickline, EditDesc, Form, Tagging) capture their own keys raw and
/// bypass the keymap in `handle_event`.
pub(crate) static TIMEBLOCKS_COMMANDS: &[CommandDef] = &[
    // Navigation
    CommandDef {
        name: "timeblocks.cursor-up",
        description: "Move the cursor up one block",
        scope: CommandScope::Tab("timeblocks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.cursor-down",
        description: "Move the cursor down one block",
        scope: CommandScope::Tab("timeblocks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.cursor-first",
        description: "Jump to the first block",
        scope: CommandScope::Tab("timeblocks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.cursor-last",
        description: "Jump to the last block",
        scope: CommandScope::Tab("timeblocks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.toggle-pane-back",
        description: "Toggle focus to the previous pane (Split) / previous day (Single)",
        scope: CommandScope::Tab("timeblocks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.toggle-pane-forward",
        description: "Toggle focus to the next pane (Split) / next day (Single)",
        scope: CommandScope::Tab("timeblocks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.toggle-view",
        description: "Toggle between Split and Single view",
        scope: CommandScope::Tab("timeblocks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.anchor-back",
        description: "Slide the anchor day back by one day",
        scope: CommandScope::Tab("timeblocks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.anchor-forward",
        description: "Slide the anchor day forward by one day",
        scope: CommandScope::Tab("timeblocks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.anchor-today",
        description: "Jump the anchor day to today",
        scope: CommandScope::Tab("timeblocks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.reload",
        description: "Reload the visible panes from disk",
        scope: CommandScope::Tab("timeblocks"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    // Edit times
    CommandDef {
        name: "timeblocks.end-later",
        description: "Shift the focused block's end +5 minutes",
        scope: CommandScope::Tab("timeblocks"),
        group: "Edit times",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.end-earlier",
        description: "Shift the focused block's end -5 minutes",
        scope: CommandScope::Tab("timeblocks"),
        group: "Edit times",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.start-later",
        description: "Shift the focused block's start +5 minutes",
        scope: CommandScope::Tab("timeblocks"),
        group: "Edit times",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.start-earlier",
        description: "Shift the focused block's start -5 minutes",
        scope: CommandScope::Tab("timeblocks"),
        group: "Edit times",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.block-later",
        description: "Shift the focused block ±5 minutes (duration preserved)",
        scope: CommandScope::Tab("timeblocks"),
        group: "Edit times",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.block-earlier",
        description: "Shift the focused block -5 minutes (duration preserved)",
        scope: CommandScope::Tab("timeblocks"),
        group: "Edit times",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    // Create / edit / delete
    CommandDef {
        name: "timeblocks.create-daily",
        description: "Create the missing daily note",
        scope: CommandScope::Tab("timeblocks"),
        group: "Create / edit / delete",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.add-quickline",
        description: "Add a new block via quickline entry",
        scope: CommandScope::Tab("timeblocks"),
        group: "Create / edit / delete",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.add-form",
        description: "Add a new block via the multi-field form",
        scope: CommandScope::Tab("timeblocks"),
        group: "Create / edit / delete",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.edit-desc",
        description: "Edit the focused block's description inline",
        scope: CommandScope::Tab("timeblocks"),
        group: "Create / edit / delete",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.start-tagging",
        description: "Add or remove @tags on the focused block",
        scope: CommandScope::Tab("timeblocks"),
        group: "Create / edit / delete",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "timeblocks.delete-start",
        description: "Arm the two-stroke `d d` delete chord",
        scope: CommandScope::Tab("timeblocks"),
        group: "Create / edit / delete",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
];

/// Default keymap for the Timeblocks tab's Idle mode. Per-mode
/// handlers (DeleteConfirm, Quickline, EditDesc, Form, Tagging) are
/// reached via the bypass at the top of `handle_event` and do NOT
/// resolve through this map.
pub(crate) static TIMEBLOCKS_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        // Navigation — vim aliases
        .bind("Up", "timeblocks.cursor-up")
        .bind("k", "timeblocks.cursor-up")
        .bind("Down", "timeblocks.cursor-down")
        .bind("j", "timeblocks.cursor-down")
        .bind("g", "timeblocks.cursor-first")
        .bind("G", "timeblocks.cursor-last")
        .bind("Left", "timeblocks.toggle-pane-back")
        .bind("h", "timeblocks.toggle-pane-back")
        .bind("Right", "timeblocks.toggle-pane-forward")
        .bind("l", "timeblocks.toggle-pane-forward")
        .bind("f", "timeblocks.toggle-view")
        .bind("H", "timeblocks.anchor-back")
        .bind("L", "timeblocks.anchor-forward")
        .bind("T", "timeblocks.anchor-today")
        .bind("r", "timeblocks.reload")
        // Edit times — special-char bindings (normalization strips SHIFT
        // for non-alpha chars so `]` arrives the same way regardless of
        // whether the terminal sent it with or without SHIFT).
        .bind("]", "timeblocks.end-later")
        .bind("[", "timeblocks.end-earlier")
        .bind("}", "timeblocks.start-later")
        .bind("{", "timeblocks.start-earlier")
        .bind(">", "timeblocks.block-later")
        .bind("<", "timeblocks.block-earlier")
        // Create / edit / delete
        .bind("c", "timeblocks.create-daily")
        .bind("a", "timeblocks.add-quickline")
        .bind("A", "timeblocks.add-form")
        .bind("e", "timeblocks.edit-desc")
        .bind("t", "timeblocks.start-tagging")
        .bind("d", "timeblocks.delete-start")
});

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

/// Editing mode the tab is currently in. `Idle` is the default; the
/// other variants own the buffers / focus targets the corresponding
/// keymaps need. `DeleteConfirm` is a two-stroke chord: first `d`
/// transitions Idle → DeleteConfirm, second `d` commits and returns to
/// Idle.
#[allow(clippy::large_enum_variant)] // single-slot tab-level state; size doesn't matter
pub(crate) enum Mode {
    Idle,
    /// First `d` of the `d d` delete chord. Holds the pane + selected
    /// block index captured at chord start so the commit isn't shifted
    /// by an intervening selection move.
    DeleteConfirm {
        pane: Pane,
        block_idx: usize,
    },
    /// `a` quickline open. Buffer captures a blockstring to parse.
    Quickline(EditBuffer),
    /// `e` inline description edit. The pane + block index identify which
    /// block is being edited; the buffer holds the new desc.
    EditDesc {
        pane: Pane,
        block_idx: usize,
        buf: EditBuffer,
    },
    /// `A` modal form for entering a block via three rows.
    Form(FormState),
    /// `t` tag-management modal. Shows the focused block's current tags
    /// and a quickline accepting `+@tag` / `-@tag` tokens.
    Tagging {
        pane: Pane,
        block_idx: usize,
        buf: EditBuffer,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormField {
    Start,
    End,
    Desc,
}

pub(crate) struct FormState {
    pub start: EditBuffer,
    pub end: EditBuffer,
    pub desc: EditBuffer,
    pub focus: FormField,
}

/// How the main viewport is laid out.
///
/// `Split` (default) shows today and tomorrow side-by-side. `Single`
/// gives the focused pane the full width — handy for working through a
/// busy day. `f` toggles between the two; in `Single` mode, `h`/`l`
/// swap which day is shown (in `Split` mode they shift focus between
/// the two panes that are already on screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewMode {
    Split,
    Single,
}

pub struct TimeblocksTab {
    pub(crate) clock: ClockFn,
    pub(crate) today: PaneState,
    pub(crate) tomorrow: PaneState,
    pub(crate) focus: Pane,
    pub(crate) mode: Mode,
    pub(crate) view: ViewMode,
    /// The date shown in the LEFT pane (the RIGHT pane is `anchor + 1`).
    /// `None` until the first `reload` lifts `ctx.today` into it — that
    /// way the tab's notion of "today" stays aligned with the App's
    /// (`ctx.today` honors `FT_TODAY`, the tab's own clock doesn't).
    /// `H`/`L` shift it by ±1 day; `T` resets it to `ctx.today`.
    pub(crate) anchor: Option<chrono::NaiveDate>,
    /// Heading the panes were last loaded under. Refresh is cheap so we
    /// could read this from `ctx.vault.config` every render, but caching
    /// it removes an allocation on the hot path.
    pub(crate) heading: String,
    /// Most recent load error (e.g. malformed file). Surfaced in the
    /// status-bar via a Toast in session 5; for now we expose it via the
    /// test API.
    #[allow(dead_code)]
    pub(crate) last_error: Option<String>,
    keymap: crate::tui::keymap::KeyMap,
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
            mode: Mode::Idle,
            view: ViewMode::Single,
            anchor: None,
            heading: "Time Blocks".into(),
            last_error: None,
            keymap: TIMEBLOCKS_KEYMAP.clone(),
        }
    }

    pub fn with_keymap_overlay(mut self, overlay: &crate::tui::keymap::KeymapOverlay) -> Self {
        self.keymap = TIMEBLOCKS_KEYMAP.with_overlay(overlay);
        self
    }

    /// Re-read both days from disk. Called from `on_focus` and `r`.
    ///
    /// Preserves each pane's selection *index*. That's the right policy
    /// for the read-only refresh path (`r` on a file that didn't change
    /// keeps the cursor where it was, on a file that did change the
    /// clamp falls back to the last block). Mutation chords that move
    /// a block's start time (which can re-sort the list) re-anchor by
    /// start time via [`Self::select_by_start`] after this call.
    fn reload(&mut self, ctx: &mut TabCtx) {
        self.heading = ctx.vault.config.config.timeblocks_heading().to_string();
        // Lazy-init the anchor to `ctx.today` on first reload so the
        // tab's notion of "today" honors `FT_TODAY` like the rest of
        // the App. Subsequent `H`/`L`/`T` chords mutate it directly.
        let anchor = *self.anchor.get_or_insert(ctx.today);
        let next = anchor + chrono::Duration::days(1);
        let prev_today_sel = self.today.selection;
        let prev_tomorrow_sel = self.tomorrow.selection;
        self.today = self.load_pane(ctx, anchor);
        self.tomorrow = self.load_pane(ctx, next);
        self.today.selection = prev_today_sel;
        self.tomorrow.selection = prev_tomorrow_sel;
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

    /// After a mutation that changes a block's start time, the
    /// post-sort index of "the block I just edited" may differ from
    /// the pre-mutation index. This helper finds the block whose start
    /// matches `start` and sets the focused-pane selection to its
    /// index, so the cursor follows the user's intent through the sort.
    fn select_by_start(&mut self, pane: Pane, start: NaiveTime) {
        let p = self.pane_mut(pane);
        if let Some(idx) = p.blocks.iter().position(|b| b.start == start) {
            p.selection = idx;
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

    // `handle_key` (the navigation-only no-ctx dispatcher) was removed
    // — its arms now live in `TIMEBLOCKS_KEYMAP` + `dispatch_command`
    // alongside the mutation arms.

    // ── mutation chord handlers ────────────────────────────────────────

    fn selected_block_idx(&self, pane: Pane) -> Option<usize> {
        let p = match pane {
            Pane::Today => &self.today,
            Pane::Tomorrow => &self.tomorrow,
        };
        if p.blocks.is_empty() {
            None
        } else {
            Some(p.selection)
        }
    }

    fn pane_path(&self, pane: Pane) -> Option<PathBuf> {
        match pane {
            Pane::Today => self.today.path.clone(),
            Pane::Tomorrow => self.tomorrow.path.clone(),
        }
    }

    /// Run a time-shift edit on the focused pane's selected block.
    /// `which == 'start'` → shifts start; otherwise shifts end. Negative
    /// values move earlier. Library clamps at 00:00 / 23:59 and enforces
    /// `end > start`.
    fn shift_block_time(&mut self, ctx: &mut TabCtx, shift_minutes: i32, on_end: bool) {
        let pane = self.focus;
        let Some(idx) = self.selected_block_idx(pane) else {
            return;
        };
        let Some(path) = self.pane_path(pane) else {
            queue_toast(ctx, "no daily-note path resolved", ToastStyle::Error);
            return;
        };
        let p = match pane {
            Pane::Today => &self.today,
            Pane::Tomorrow => &self.tomorrow,
        };
        let block = &p.blocks[idx];
        let old_start = block.start;
        let source_line = block.source_line;
        let mutation = if on_end {
            EditMutation {
                end: Some(TimeChange::ShiftMinutes(shift_minutes)),
                ..Default::default()
            }
        } else {
            EditMutation {
                start: Some(TimeChange::ShiftMinutes(shift_minutes)),
                ..Default::default()
            }
        };
        // Use the 1-indexed display line as the selector so blocks with
        // identical start times can still be edited individually.
        // `Selector::Time(start)` would match every block sharing this
        // start and fail with `ambiguous selector`.
        let selector = Selector::Line(source_line);
        match ops::edit_block(&path, &self.heading, &selector, mutation) {
            Ok(_) => {
                self.reload(ctx);
                // End-shift leaves the start unchanged, so the (now
                // preserved) selection index already points at the same
                // block. A start-shift can move the block in the sorted
                // list — re-anchor by the new start time so the cursor
                // tracks the user's intent across the re-sort.
                if !on_end {
                    let new_start = shift_clamped(old_start, shift_minutes);
                    self.select_by_start(pane, new_start);
                }
            }
            Err(e) => queue_toast(ctx, &format!("{e}"), ToastStyle::Error),
        }
    }

    fn shift_end(&mut self, ctx: &mut TabCtx, m: i32) {
        self.shift_block_time(ctx, m, true);
    }

    fn shift_start(&mut self, ctx: &mut TabCtx, m: i32) {
        self.shift_block_time(ctx, m, false);
    }

    /// Move the date window by `delta_days` (positive = forward in
    /// time, negative = back). Re-reads both panes from disk for the
    /// new anchor; per-pane selection indices reset to 0 because the
    /// block lists belong to different days.
    fn shift_anchor(&mut self, ctx: &mut TabCtx, delta_days: i64) {
        let base = self.anchor.unwrap_or(ctx.today);
        let new = base + chrono::Duration::days(delta_days);
        self.anchor = Some(new);
        // Selections from the previous window don't apply to the new
        // day; reset them so the cursor lands at the top of each pane.
        self.today.selection = 0;
        self.tomorrow.selection = 0;
        self.reload(ctx);
    }

    /// Shift both start and end by the same delta, moving the whole
    /// block earlier or later in time without changing its duration.
    /// Bound by the same `[00:00, 23:59]` clamps the library applies to
    /// each endpoint independently — pushing a block past the 23:59
    /// ceiling collapses end onto start and trips the library's
    /// `end > start` validation, surfaced as an error toast.
    fn shift_block(&mut self, ctx: &mut TabCtx, shift_minutes: i32) {
        let pane = self.focus;
        let Some(idx) = self.selected_block_idx(pane) else {
            return;
        };
        let Some(path) = self.pane_path(pane) else {
            queue_toast(ctx, "no daily-note path resolved", ToastStyle::Error);
            return;
        };
        let p = match pane {
            Pane::Today => &self.today,
            Pane::Tomorrow => &self.tomorrow,
        };
        let block = &p.blocks[idx];
        let old_start = block.start;
        let source_line = block.source_line;
        let mutation = EditMutation {
            start: Some(TimeChange::ShiftMinutes(shift_minutes)),
            end: Some(TimeChange::ShiftMinutes(shift_minutes)),
            ..Default::default()
        };
        let selector = Selector::Line(source_line);
        match ops::edit_block(&path, &self.heading, &selector, mutation) {
            Ok(_) => {
                self.reload(ctx);
                // Both endpoints moved by the same delta — the block
                // may re-sort relative to neighbours, so anchor by the
                // new start to keep the cursor on it.
                let new_start = shift_clamped(old_start, shift_minutes);
                self.select_by_start(pane, new_start);
            }
            Err(e) => queue_toast(ctx, &format!("{e}"), ToastStyle::Error),
        }
    }

    /// `c` chord — when the focused pane's daily note doesn't yet exist,
    /// create it via `create_or_get_periodic_path` and re-read. Otherwise
    /// toast "already exists".
    fn handle_create_daily(&mut self, ctx: &mut TabCtx) {
        let pane = self.focus;
        let date = match pane {
            Pane::Today => self.today.date,
            Pane::Tomorrow => self.tomorrow.date,
        };
        let already_present = match pane {
            Pane::Today => self.today.present,
            Pane::Tomorrow => self.tomorrow.present,
        };
        if already_present {
            queue_toast(ctx, "daily note already exists", ToastStyle::Info);
            return;
        }
        let Some(daily_cfg) = ctx.vault.config.config.periodic_notes.daily.as_ref() else {
            queue_toast(
                ctx,
                "no `[periodic_notes.daily]` configured",
                ToastStyle::Error,
            );
            return;
        };
        let (today_n, now_n) = today_now_for_template(ctx, self.clock);
        match ft_core::periodic::create_or_get_periodic_path(
            &ctx.vault.path,
            &ctx.vault.templates_dir(),
            daily_cfg,
            date,
            today_n,
            now_n,
        ) {
            Ok((_path, _created)) => {
                queue_toast(
                    ctx,
                    &format!("created daily note for {date}"),
                    ToastStyle::Success,
                );
                self.reload(ctx);
            }
            Err(e) => queue_toast(ctx, &format!("{e}"), ToastStyle::Error),
        }
    }

    /// First `d` of the `d d` chord. Captures the focused selection so
    /// subsequent navigation doesn't shift the delete target. Toasts
    /// the inter-stroke hint.
    fn start_delete_confirm(&mut self, ctx: &mut TabCtx) {
        let pane = self.focus;
        let Some(idx) = self.selected_block_idx(pane) else {
            queue_toast(ctx, "nothing to delete", ToastStyle::Info);
            return;
        };
        self.mode = Mode::DeleteConfirm {
            pane,
            block_idx: idx,
        };
        queue_toast(
            ctx,
            "press `d` again to delete, Esc to cancel",
            ToastStyle::Info,
        );
    }

    fn handle_delete_confirm(&mut self, k: KeyEvent, ctx: &mut TabCtx) -> EventOutcome {
        let Mode::DeleteConfirm { pane, block_idx } = self.mode else {
            return EventOutcome::NotHandled;
        };
        match k.code {
            KeyCode::Char('d') if k.modifiers == KeyModifiers::NONE => {
                self.mode = Mode::Idle;
                self.commit_delete(ctx, pane, block_idx);
                EventOutcome::Consumed
            }
            KeyCode::Esc => {
                self.mode = Mode::Idle;
                queue_toast(ctx, "delete cancelled", ToastStyle::Info);
                EventOutcome::Consumed
            }
            _ => {
                // Any other key cancels the chord — matches the
                // tasks-tab convention so an accidental `j`/`k` doesn't
                // silently arm the deletion.
                self.mode = Mode::Idle;
                EventOutcome::Consumed
            }
        }
    }

    fn commit_delete(&mut self, ctx: &mut TabCtx, pane: Pane, block_idx: usize) {
        let Some(path) = self.pane_path(pane) else {
            queue_toast(ctx, "no daily-note path resolved", ToastStyle::Error);
            return;
        };
        let p = match pane {
            Pane::Today => &self.today,
            Pane::Tomorrow => &self.tomorrow,
        };
        if block_idx >= p.blocks.len() {
            queue_toast(ctx, "block no longer exists", ToastStyle::Error);
            return;
        }
        let target = p.blocks[block_idx].clone();
        // Selector::Line keeps the operation unambiguous when two
        // blocks share a start time. `source_line` is set by
        // `Document::read` to the 1-indexed display order.
        let selector = Selector::Line(target.source_line);
        match ops::delete_block(&path, &self.heading, &selector) {
            Ok(_) => {
                queue_toast(
                    ctx,
                    &format!(
                        "deleted {} - {} {}",
                        fmt_hhmm(target.start),
                        fmt_hhmm(target.end),
                        target.desc
                    ),
                    ToastStyle::Success,
                );
                // Aim the cursor at the block that took the deleted
                // one's slot. `reload` preserves the index and
                // `clamp_selection` brings it down when we removed the
                // tail — but pinning it here also handles the case
                // where `block_idx` happens to be 0 (no clamp needed
                // but we'd otherwise stay at 0 anyway).
                self.pane_mut(pane).selection = block_idx;
                self.reload(ctx);
            }
            Err(e) => queue_toast(ctx, &format!("{e}"), ToastStyle::Error),
        }
    }

    // ── quickline (`a`) ────────────────────────────────────────────────

    fn handle_quickline(&mut self, k: KeyEvent, ctx: &mut TabCtx) -> EventOutcome {
        let Mode::Quickline(buf) = &mut self.mode else {
            return EventOutcome::NotHandled;
        };
        match k.code {
            KeyCode::Esc => {
                self.mode = Mode::Idle;
                EventOutcome::Consumed
            }
            KeyCode::Enter => {
                let input = buf.text.clone();
                self.commit_quickline(ctx, &input);
                EventOutcome::Consumed
            }
            _ => {
                let _ = buf.handle_event(k);
                EventOutcome::Consumed
            }
        }
    }

    fn commit_quickline(&mut self, ctx: &mut TabCtx, input: &str) {
        let pane = self.focus;
        let Some(path) = self.pane_path(pane) else {
            queue_toast(ctx, "no daily-note path resolved", ToastStyle::Error);
            return;
        };
        let block = match timeblock::parse_line(input) {
            Ok(b) => b,
            Err(e) => {
                // Keep the buffer populated so the user can fix the input.
                queue_toast(ctx, &format!("parse: {e}"), ToastStyle::Error);
                return;
            }
        };
        let summary = format!(
            "+ {} - {} {}",
            fmt_hhmm(block.start),
            fmt_hhmm(block.end),
            block.desc.trim()
        );
        // The daily note might be missing on disk — same behavior as
        // CLI `ft timeblocks add`: render the template first.
        if let Err(e) = self.ensure_pane_file(ctx, pane) {
            queue_toast(ctx, &format!("{e}"), ToastStyle::Error);
            return;
        }
        let new_start = block.start;
        match ops::add_block(&path, &self.heading, block, AddOptions::default()) {
            Ok(_) => {
                self.mode = Mode::Idle;
                queue_toast(ctx, &summary, ToastStyle::Success);
                self.reload(ctx);
                // Pin the cursor to the freshly added block so the next
                // chord (`]`, `[`, `e`, …) operates on what the user
                // just typed, rather than wherever they last navigated.
                self.select_by_start(pane, new_start);
            }
            Err(e) => queue_toast(ctx, &format!("{e}"), ToastStyle::Error),
        }
    }

    /// Render the daily-note template for the focused pane's date when
    /// the file is missing. No-op when the file already exists or when
    /// `[periodic_notes.daily]` isn't configured (the subsequent write
    /// will create the file with just the section heading — same as the
    /// pre-session-5 behavior, surfaced via the existing remedy hint).
    fn ensure_pane_file(&self, ctx: &mut TabCtx, pane: Pane) -> Result<()> {
        let (date, present, path) = match pane {
            Pane::Today => (self.today.date, self.today.present, self.today.path.clone()),
            Pane::Tomorrow => (
                self.tomorrow.date,
                self.tomorrow.present,
                self.tomorrow.path.clone(),
            ),
        };
        if present || path.is_none() {
            return Ok(());
        }
        if ctx.vault.config.config.periodic_notes.daily.is_none() {
            return Ok(());
        }
        let (today_n, now_n) = today_now_for_template(ctx, self.clock);
        ctx.vault
            .ensure_target(date, None, today_n, now_n)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    // ── edit description (`e`) ─────────────────────────────────────────

    fn start_edit_desc(&mut self, ctx: &mut TabCtx) {
        let pane = self.focus;
        let Some(idx) = self.selected_block_idx(pane) else {
            queue_toast(ctx, "nothing to edit", ToastStyle::Info);
            return;
        };
        let block = match pane {
            Pane::Today => &self.today.blocks[idx],
            Pane::Tomorrow => &self.tomorrow.blocks[idx],
        };
        self.mode = Mode::EditDesc {
            pane,
            block_idx: idx,
            buf: EditBuffer::from(&block.desc),
        };
    }

    fn handle_edit_desc(&mut self, k: KeyEvent, ctx: &mut TabCtx) -> EventOutcome {
        let Mode::EditDesc {
            pane,
            block_idx,
            buf,
        } = &mut self.mode
        else {
            return EventOutcome::NotHandled;
        };
        match k.code {
            KeyCode::Esc => {
                self.mode = Mode::Idle;
                EventOutcome::Consumed
            }
            KeyCode::Enter => {
                let new_desc = buf.text.clone();
                let pane = *pane;
                let block_idx = *block_idx;
                self.commit_edit_desc(ctx, pane, block_idx, new_desc);
                EventOutcome::Consumed
            }
            _ => {
                let _ = buf.handle_event(k);
                EventOutcome::Consumed
            }
        }
    }

    fn commit_edit_desc(
        &mut self,
        ctx: &mut TabCtx,
        pane: Pane,
        block_idx: usize,
        new_desc: String,
    ) {
        let Some(path) = self.pane_path(pane) else {
            queue_toast(ctx, "no daily-note path resolved", ToastStyle::Error);
            return;
        };
        let p = match pane {
            Pane::Today => &self.today,
            Pane::Tomorrow => &self.tomorrow,
        };
        if block_idx >= p.blocks.len() {
            queue_toast(ctx, "block no longer exists", ToastStyle::Error);
            return;
        }
        let target = p.blocks[block_idx].clone();
        // Use the display-order line to disambiguate equal-start blocks.
        let selector = Selector::Line(target.source_line);
        let mutation = EditMutation {
            desc: Some(new_desc),
            ..Default::default()
        };
        match ops::edit_block(&path, &self.heading, &selector, mutation) {
            Ok(_) => {
                self.mode = Mode::Idle;
                self.reload(ctx);
                // Desc edits don't touch the block's start time, but
                // anchor by start anyway to make the invariant — "the
                // block you just edited stays selected" — uniform across
                // every mutation chord.
                self.select_by_start(pane, target.start);
            }
            Err(e) => queue_toast(ctx, &format!("{e}"), ToastStyle::Error),
        }
    }

    // ── form (`A`) ─────────────────────────────────────────────────────

    fn default_form(&self) -> FormState {
        let now = (self.clock)();
        // Snap clock time to the nearest 5-minute boundary.
        let total = now.hour() * 60 + now.minute();
        let snapped = (total / 5) * 5;
        let start = NaiveTime::from_hms_opt(snapped / 60, snapped % 60, 0).unwrap();
        let end = start + chrono::Duration::minutes(30);
        FormState {
            start: EditBuffer::from(&fmt_hhmm(start)),
            end: EditBuffer::from(&fmt_hhmm(end)),
            desc: EditBuffer::default(),
            focus: FormField::Start,
        }
    }

    fn handle_form(&mut self, k: KeyEvent, ctx: &mut TabCtx) -> EventOutcome {
        let Mode::Form(state) = &mut self.mode else {
            return EventOutcome::NotHandled;
        };
        match k.code {
            KeyCode::Esc => {
                self.mode = Mode::Idle;
                EventOutcome::Consumed
            }
            KeyCode::Tab | KeyCode::Down => {
                state.focus = next_field(state.focus);
                EventOutcome::Consumed
            }
            KeyCode::BackTab | KeyCode::Up => {
                state.focus = prev_field(state.focus);
                EventOutcome::Consumed
            }
            KeyCode::Enter => {
                if state.focus == FormField::Desc {
                    let start_text = state.start.text.clone();
                    let end_text = state.end.text.clone();
                    let desc = state.desc.text.clone();
                    self.commit_form(ctx, &start_text, &end_text, &desc);
                } else {
                    state.focus = next_field(state.focus);
                }
                EventOutcome::Consumed
            }
            _ => {
                let _ = form_buf_mut(state).handle_event(k);
                EventOutcome::Consumed
            }
        }
    }

    fn commit_form(&mut self, ctx: &mut TabCtx, start: &str, end: &str, desc: &str) {
        // Build a blockstring and reuse the quickline parser so the
        // grammar and error messages stay in one place.
        let blockstring = if desc.trim().is_empty() {
            format!("{} - {}", start, end)
        } else {
            format!("{} - {} {}", start, end, desc)
        };
        self.commit_quickline(ctx, &blockstring);
    }

    // ── tag modal (`t`) ────────────────────────────────────────────────

    fn start_tagging(&mut self, ctx: &mut TabCtx) {
        let pane = self.focus;
        let Some(idx) = self.selected_block_idx(pane) else {
            queue_toast(ctx, "nothing to tag", ToastStyle::Info);
            return;
        };
        self.mode = Mode::Tagging {
            pane,
            block_idx: idx,
            buf: EditBuffer::default(),
        };
    }

    fn handle_tagging(&mut self, k: KeyEvent, ctx: &mut TabCtx) -> EventOutcome {
        let Mode::Tagging {
            pane,
            block_idx,
            buf,
        } = &mut self.mode
        else {
            return EventOutcome::NotHandled;
        };
        match k.code {
            KeyCode::Esc => {
                self.mode = Mode::Idle;
                EventOutcome::Consumed
            }
            KeyCode::Enter => {
                let input = buf.text.clone();
                let pane = *pane;
                let block_idx = *block_idx;
                self.commit_tagging(ctx, pane, block_idx, &input);
                EventOutcome::Consumed
            }
            _ => {
                let _ = buf.handle_event(k);
                EventOutcome::Consumed
            }
        }
    }

    /// Parse a whitespace-separated list of `+@tag` / `-@tag` tokens
    /// and apply them via a single `ops::edit_block` call. Empty input
    /// closes the modal without writing.
    fn commit_tagging(&mut self, ctx: &mut TabCtx, pane: Pane, block_idx: usize, input: &str) {
        if input.trim().is_empty() {
            self.mode = Mode::Idle;
            return;
        }
        let Some(path) = self.pane_path(pane) else {
            queue_toast(ctx, "no daily-note path resolved", ToastStyle::Error);
            return;
        };
        let p = match pane {
            Pane::Today => &self.today,
            Pane::Tomorrow => &self.tomorrow,
        };
        if block_idx >= p.blocks.len() {
            queue_toast(ctx, "block no longer exists", ToastStyle::Error);
            return;
        }
        let target = p.blocks[block_idx].clone();

        let mut add_tags: Vec<Tag> = Vec::new();
        let mut remove_tags: Vec<Tag> = Vec::new();
        for tok in input.split_whitespace() {
            let (sign, rest) = match tok.chars().next() {
                Some('+') => ('+', &tok[1..]),
                Some('-') => ('-', &tok[1..]),
                _ => {
                    queue_toast(
                        ctx,
                        &format!("tag token must start with `+` or `-`, got `{tok}`"),
                        ToastStyle::Error,
                    );
                    return;
                }
            };
            match timeblock::parse_tag_string(rest) {
                Ok(tag) => {
                    if sign == '+' {
                        add_tags.push(tag);
                    } else {
                        remove_tags.push(tag);
                    }
                }
                Err(e) => {
                    queue_toast(ctx, &format!("bad tag `{tok}`: {e}"), ToastStyle::Error);
                    return;
                }
            }
        }

        let selector = Selector::Line(target.source_line);
        let mutation = EditMutation {
            add_tags,
            remove_tags,
            ..Default::default()
        };
        match ops::edit_block(&path, &self.heading, &selector, mutation) {
            Ok(_) => {
                self.mode = Mode::Idle;
                queue_toast(ctx, "tags updated", ToastStyle::Success);
                self.reload(ctx);
                self.select_by_start(pane, target.start);
            }
            Err(e) => queue_toast(ctx, &format!("{e}"), ToastStyle::Error),
        }
    }
}

fn next_field(f: FormField) -> FormField {
    match f {
        FormField::Start => FormField::End,
        FormField::End => FormField::Desc,
        FormField::Desc => FormField::Start,
    }
}

fn prev_field(f: FormField) -> FormField {
    match f {
        FormField::Start => FormField::Desc,
        FormField::End => FormField::Start,
        FormField::Desc => FormField::End,
    }
}

fn form_buf_mut(s: &mut FormState) -> &mut EditBuffer {
    match s.focus {
        FormField::Start => &mut s.start,
        FormField::End => &mut s.end,
        FormField::Desc => &mut s.desc,
    }
}

fn fmt_hhmm(t: NaiveTime) -> String {
    format!("{:02}:{:02}", t.hour(), t.minute())
}

/// Apply the same `±N`-minute shift the library performs on
/// [`TimeChange::ShiftMinutes`], clamping at `00:00` and `23:59` so the
/// "expected new start" computed by the TUI matches what
/// `ops::edit_block` will have written. Kept in sync with
/// `ft_core::timeblock::ops::apply_change`.
fn shift_clamped(t: NaiveTime, delta: i32) -> NaiveTime {
    let cur = (t.hour() as i32) * 60 + (t.minute() as i32);
    let new = (cur + delta).clamp(0, 23 * 60 + 59);
    NaiveTime::from_hms_opt((new / 60) as u32, (new % 60) as u32, 0).unwrap()
}

fn queue_toast(ctx: &TabCtx, text: &str, style: ToastStyle) {
    *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast {
        text: text.to_string(),
        style,
    });
}

/// `(today, now)` for template rendering — honors `FT_TODAY` for tests
/// and falls back to the tab's clock for production.
fn today_now_for_template(
    ctx: &TabCtx,
    clock: ClockFn,
) -> (chrono::NaiveDate, chrono::NaiveDateTime) {
    // FT_TODAY (when set) takes precedence over the injected clock so the
    // CLI and TUI agree on "today" under tests.
    let _ = ctx; // ctx kept in signature for future use (e.g. real-clock-from-app)
    if std::env::var_os("FT_TODAY").is_some() {
        return ft_core::dates::now_pair();
    }
    let now = (clock)();
    (now.date_naive(), now.naive_local())
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

    fn kind(&self) -> TabKind {
        TabKind::Timeblocks
    }

    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()> {
        self.reload(ctx);
        Ok(())
    }

    fn commands(&self) -> &'static [CommandDef] {
        TIMEBLOCKS_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &self.keymap
    }

    fn dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome {
        match cmd.name {
            // Navigation
            "timeblocks.cursor-up" => {
                self.move_selection(-1);
                CommandOutcome::Handled
            }
            "timeblocks.cursor-down" => {
                self.move_selection(1);
                CommandOutcome::Handled
            }
            "timeblocks.cursor-first" => {
                self.jump_selection(false);
                CommandOutcome::Handled
            }
            "timeblocks.cursor-last" => {
                self.jump_selection(true);
                CommandOutcome::Handled
            }
            "timeblocks.toggle-pane-back" => {
                self.toggle_focus(false);
                CommandOutcome::Handled
            }
            "timeblocks.toggle-pane-forward" => {
                self.toggle_focus(true);
                CommandOutcome::Handled
            }
            "timeblocks.toggle-view" => {
                self.view = match self.view {
                    ViewMode::Split => ViewMode::Single,
                    ViewMode::Single => ViewMode::Split,
                };
                CommandOutcome::Handled
            }
            "timeblocks.anchor-back" => {
                self.shift_anchor(ctx, -1);
                CommandOutcome::Handled
            }
            "timeblocks.anchor-forward" => {
                self.shift_anchor(ctx, 1);
                CommandOutcome::Handled
            }
            "timeblocks.anchor-today" => {
                self.anchor = Some(ctx.today);
                self.reload(ctx);
                CommandOutcome::Handled
            }
            "timeblocks.reload" => {
                self.reload(ctx);
                CommandOutcome::Handled
            }
            // Edit times
            "timeblocks.end-later" => {
                self.shift_end(ctx, 5);
                CommandOutcome::Handled
            }
            "timeblocks.end-earlier" => {
                self.shift_end(ctx, -5);
                CommandOutcome::Handled
            }
            "timeblocks.start-later" => {
                self.shift_start(ctx, 5);
                CommandOutcome::Handled
            }
            "timeblocks.start-earlier" => {
                self.shift_start(ctx, -5);
                CommandOutcome::Handled
            }
            "timeblocks.block-later" => {
                self.shift_block(ctx, 5);
                CommandOutcome::Handled
            }
            "timeblocks.block-earlier" => {
                self.shift_block(ctx, -5);
                CommandOutcome::Handled
            }
            // Create / edit / delete
            "timeblocks.create-daily" => {
                self.handle_create_daily(ctx);
                CommandOutcome::Handled
            }
            "timeblocks.add-quickline" => {
                self.mode = Mode::Quickline(EditBuffer::default());
                CommandOutcome::Handled
            }
            "timeblocks.add-form" => {
                self.mode = Mode::Form(self.default_form());
                CommandOutcome::Handled
            }
            "timeblocks.edit-desc" => {
                self.start_edit_desc(ctx);
                CommandOutcome::Handled
            }
            "timeblocks.start-tagging" => {
                self.start_tagging(ctx);
                CommandOutcome::Handled
            }
            "timeblocks.delete-start" => {
                self.start_delete_confirm(ctx);
                CommandOutcome::Handled
            }
            _ => CommandOutcome::NotHandled,
        }
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };

        // Per-mode handlers (DeleteConfirm, Quickline, EditDesc, Form,
        // Tagging) capture raw keys before the Idle keymap is consulted
        // — same pattern as GatherTab's picker-overlay bypass.
        match &mut self.mode {
            Mode::Idle => {}
            Mode::DeleteConfirm { .. } => {
                return Ok(self.handle_delete_confirm(k, ctx));
            }
            Mode::Quickline(_) => {
                return Ok(self.handle_quickline(k, ctx));
            }
            Mode::EditDesc { .. } => {
                return Ok(self.handle_edit_desc(k, ctx));
            }
            Mode::Form(_) => {
                return Ok(self.handle_form(k, ctx));
            }
            Mode::Tagging { .. } => {
                return Ok(self.handle_tagging(k, ctx));
            }
        }

        // Idle keymap.
        let chord = KeyChord::from_key_event(k);
        let Some(cmd) = self.keymap.lookup(chord).cloned() else {
            return Ok(EventOutcome::NotHandled);
        };
        Ok(match self.dispatch_command(&cmd, ctx) {
            CommandOutcome::Handled => EventOutcome::Consumed,
            CommandOutcome::NotHandled => EventOutcome::NotHandled,
        })
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        view::render(self, frame, area, ctx);
    }

    fn refresh(&mut self, ctx: &mut TabCtx) -> Result<()> {
        self.reload(ctx);
        Ok(())
    }

    fn help_sections(&self) -> Vec<HelpSection> {
        vec![
            HelpSection::new(
                "Navigation",
                &[
                    ("↑ / ↓ · j / k", "select prev / next block"),
                    ("g / G", "first / last block"),
                    ("h / l · ← / →", "toggle pane (Split) / switch day (Single)"),
                    ("f", "toggle Split / Single view"),
                    ("Shift+H / Shift+L", "slide anchor day back / forward"),
                    ("Shift+T", "jump anchor to today"),
                    ("r", "refresh from disk"),
                ],
            ),
            HelpSection::new(
                "Edit times",
                &[
                    ("] / [", "end +5m / -5m"),
                    ("} / {", "start +5m / -5m"),
                    ("> / <", "shift block ±5m (duration preserved)"),
                ],
            ),
            HelpSection::new(
                "Create / edit / delete",
                &[
                    ("c", "create missing daily note"),
                    ("a", "add block (quickline)"),
                    ("Shift+A", "add block (form)"),
                    ("e", "edit description (inline)"),
                    ("t", "add / remove @tags"),
                    ("d d", "delete focused block (two-stroke)"),
                ],
            ),
            HelpSection::new(
                "Modals",
                &[
                    ("Enter", "commit"),
                    ("Esc", "cancel / close modal"),
                    ("Tab / Shift+Tab", "next / prev field (form)"),
                    ("Ctrl+W / Ctrl+⌫", "delete previous word"),
                ],
            ),
        ]
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

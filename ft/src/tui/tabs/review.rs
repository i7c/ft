//! `Review` tab — frequency-ranked `[[wikilinks]]` mentioned in a
//! commit/date window. Drives the synthesis ritual's discovery step:
//! user selects N links with `<space>`, hits `<enter>`, and the
//! Journal tab opens in multi-target mode with those targets queued.
//!
//! v1 computes the link review synchronously on focus / window-change.
//! For very large vaults that becomes a UX problem; the codebase's
//! single-threaded + mpsc background-worker pattern (see
//! `journal::load_for` and the `g s` worker) can be applied here if
//! needed — track separately.

use std::collections::HashSet;
use std::sync::LazyLock;

use anyhow::Result;
use chrono::Duration;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use ft_core::git::discover_repo;
use ft_core::graph::{Graph, NodeKind, NoteId};
use ft_core::link_review::{compute_link_review, LinkReview, LinkReviewRow, WindowRange};

use crate::tui::command::{Command, CommandDef, CommandOutcome, CommandScope};
use crate::tui::event::Event;
use crate::tui::help::HelpSection;
use crate::tui::keymap::{KeyChord, KeyMap};
use crate::tui::palette;
use crate::tui::tab::{
    AppRequest, EventOutcome, JournalTarget, JournalWindow, MultiTargetRequest, Tab, TabCtx,
};

// ── Commands ─────────────────────────────────────────────────────────

pub(crate) static REVIEW_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "review.cursor-up",
        description: "Move the cursor up one row",
        scope: CommandScope::Tab("review"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "review.cursor-down",
        description: "Move the cursor down one row",
        scope: CommandScope::Tab("review"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "review.toggle-selection",
        description: "Toggle multi-select on the current row",
        scope: CommandScope::Tab("review"),
        group: "Selection",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "review.handoff-to-journal",
        description: "Open the Journal tab with selected (or cursor) links as multi-targets",
        scope: CommandScope::Tab("review"),
        group: "Handoff",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "review.window-wider",
        description: "Double the window duration (--since-style only)",
        scope: CommandScope::Tab("review"),
        group: "Window",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "review.window-narrower",
        description: "Halve the window duration (--since-style only, minimum 1 day)",
        scope: CommandScope::Tab("review"),
        group: "Window",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "review.reload",
        description: "Recompute the link review",
        scope: CommandScope::Tab("review"),
        group: "Source",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
];

pub(crate) static REVIEW_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("Up", "review.cursor-up")
        .bind("k", "review.cursor-up")
        .bind("Down", "review.cursor-down")
        .bind("j", "review.cursor-down")
        .bind("Space", "review.toggle-selection")
        .bind("Enter", "review.handoff-to-journal")
        .bind("]", "review.window-wider")
        .bind("[", "review.window-narrower")
        .bind("R", "review.reload")
});

pub struct ReviewTab {
    /// Current window — defaults to `--since 7d` on first focus.
    window: WindowRange,
    /// Last-computed rows. Empty when not yet loaded or window is empty.
    rows: Vec<LinkReviewRow>,
    /// Set of selected row indices (multi-select via `<space>`).
    selected: HashSet<usize>,
    /// 0-indexed cursor into `rows`.
    cursor: usize,
    /// Last error message, if any.
    last_error: Option<String>,
    /// `true` after the first load; used so we don't re-load on every
    /// focus (the user can press `R` to force a reload).
    loaded_once: bool,
    keymap: KeyMap,
}

impl Default for ReviewTab {
    fn default() -> Self {
        Self::new()
    }
}

impl ReviewTab {
    pub fn new() -> Self {
        Self {
            window: WindowRange::Since(Duration::days(7)),
            rows: Vec::new(),
            selected: HashSet::new(),
            cursor: 0,
            last_error: None,
            loaded_once: false,
            keymap: REVIEW_KEYMAP.clone(),
        }
    }

    pub fn with_keymap_overlay(mut self, overlay: &crate::tui::keymap::KeymapOverlay) -> Self {
        self.keymap = REVIEW_KEYMAP.with_overlay(overlay);
        self
    }

    fn load(&mut self, ctx: &mut TabCtx) {
        if discover_repo(&ctx.vault.path).is_none() {
            self.last_error =
                Some("vault is not inside a git repository — review needs git history".to_string());
            self.rows.clear();
            return;
        }
        let scan = ctx.vault.scan();
        let graph = match Graph::build(ctx.vault, &scan) {
            Ok(g) => g,
            Err(e) => {
                self.last_error = Some(format!("graph build failed: {e}"));
                return;
            }
        };
        let cfg = ctx.vault.config.config.synth.clone();
        let review =
            match compute_link_review(&graph, ctx.vault, &ctx.vault.path, &self.window, &cfg) {
                Ok(r) => r,
                Err(e) => {
                    self.last_error = Some(format!("compute_link_review failed: {e}"));
                    return;
                }
            };
        self.apply_review(graph, review);
        self.last_error = None;
        self.loaded_once = true;
    }

    fn apply_review(&mut self, _graph: Graph, review: LinkReview) {
        self.rows = review.rows;
        // Clamp cursor and clear selection.
        if self.cursor >= self.rows.len() {
            self.cursor = self.rows.len().saturating_sub(1);
        }
        self.selected.clear();
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let len = self.rows.len() as isize;
        self.cursor = ((self.cursor as isize + delta).clamp(0, len - 1)) as usize;
    }

    fn toggle_selection(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        if !self.selected.remove(&self.cursor) {
            self.selected.insert(self.cursor);
        }
    }

    fn handoff(&mut self, ctx: &mut TabCtx) {
        if self.rows.is_empty() {
            return;
        }
        // Build the target list: selected rows, or the cursor row when
        // nothing is selected.
        let row_indices: Vec<usize> = if self.selected.is_empty() {
            vec![self.cursor]
        } else {
            let mut v: Vec<usize> = self.selected.iter().copied().collect();
            v.sort_unstable();
            v
        };
        // Need a graph to convert each row's target name to a JournalTarget
        // (Note vs Ghost). Re-build the graph at handoff time so the IDs
        // we hand off are valid in the Journal tab's freshly-built graph.
        let scan = ctx.vault.scan();
        let graph = match Graph::build(ctx.vault, &scan) {
            Ok(g) => g,
            Err(e) => {
                self.last_error = Some(format!("handoff graph build failed: {e}"));
                return;
            }
        };
        let mut targets: Vec<JournalTarget> = Vec::new();
        for idx in row_indices {
            let row = &self.rows[idx];
            let target = if row.is_ghost {
                JournalTarget::Ghost(row.target.clone())
            } else if let Some(id) = note_id_by_title(&graph, &row.target) {
                let NodeKind::Note(n) = graph.node(id) else {
                    continue;
                };
                JournalTarget::Note(n.path.clone())
            } else {
                continue;
            };
            targets.push(target);
        }
        if targets.is_empty() {
            return;
        }
        let request = MultiTargetRequest {
            targets,
            window: Some(window_to_journal(&self.window)),
        };
        *ctx.pending_request.borrow_mut() = Some(AppRequest::JournalForMulti { request });
    }

    /// Double the window (Since-style only).
    fn window_wider(&mut self, ctx: &mut TabCtx) {
        if let WindowRange::Since(d) = &self.window {
            let new = *d * 2;
            self.window = WindowRange::Since(new);
            self.load(ctx);
        }
    }

    /// Halve the window (Since-style only, minimum 1 day).
    fn window_narrower(&mut self, ctx: &mut TabCtx) {
        if let WindowRange::Since(d) = &self.window {
            let mut new = *d / 2;
            if new < Duration::days(1) {
                new = Duration::days(1);
            }
            self.window = WindowRange::Since(new);
            self.load(ctx);
        }
    }
}

fn note_id_by_title(graph: &Graph, title: &str) -> Option<NoteId> {
    for (id, node) in graph.nodes() {
        if let NodeKind::Note(n) = node {
            if n.title.eq_ignore_ascii_case(title) {
                return Some(id);
            }
        }
    }
    None
}

fn window_to_journal(window: &WindowRange) -> JournalWindow {
    match window {
        WindowRange::Since(d) => JournalWindow::Since(*d),
        WindowRange::Range { from, to } => JournalWindow::Range {
            from: from.clone(),
            to: to.clone(),
        },
    }
}

fn window_label(w: &WindowRange) -> String {
    match w {
        WindowRange::Since(d) => {
            let days = d.num_days();
            if days >= 1 {
                format!("since {days}d")
            } else {
                let hours = d.num_hours();
                format!("since {hours}h")
            }
        }
        WindowRange::Range { from, to } => format!("range {from}..{to}"),
    }
}

impl Tab for ReviewTab {
    fn title(&self) -> &str {
        "Review"
    }

    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()> {
        if !self.loaded_once {
            self.load(ctx);
            // Always advance loaded_once after the first focus so the
            // UI moves past the loading… placeholder even on error.
            self.loaded_once = true;
        }
        Ok(())
    }

    fn commands(&self) -> &'static [CommandDef] {
        REVIEW_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &self.keymap
    }

    fn dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome {
        match cmd.name {
            "review.cursor-up" => {
                self.move_cursor(-1);
                CommandOutcome::Handled
            }
            "review.cursor-down" => {
                self.move_cursor(1);
                CommandOutcome::Handled
            }
            "review.toggle-selection" => {
                self.toggle_selection();
                CommandOutcome::Handled
            }
            "review.handoff-to-journal" => {
                self.handoff(ctx);
                CommandOutcome::Handled
            }
            "review.window-wider" => {
                self.window_wider(ctx);
                CommandOutcome::Handled
            }
            "review.window-narrower" => {
                self.window_narrower(ctx);
                CommandOutcome::Handled
            }
            "review.reload" => {
                self.load(ctx);
                CommandOutcome::Handled
            }
            _ => CommandOutcome::NotHandled,
        }
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };
        let chord = KeyChord::from_key_event(k);
        let Some(cmd) = self.keymap.lookup(chord).cloned() else {
            return Ok(EventOutcome::NotHandled);
        };
        Ok(match self.dispatch_command(&cmd, ctx) {
            CommandOutcome::Handled => EventOutcome::Consumed,
            CommandOutcome::NotHandled => EventOutcome::NotHandled,
        })
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        let title = format!(
            " Review — {} ({} link{}, {} selected) ",
            window_label(&self.window),
            self.rows.len(),
            if self.rows.len() == 1 { "" } else { "s" },
            self.selected.len()
        );
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Surface errors regardless of `loaded_once` so a failing load
        // doesn't get masked by the placeholder.
        if let Some(err) = self.last_error.as_deref() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("error: {err}"),
                    Style::default().fg(palette::ERROR),
                ))),
                inner,
            );
            return;
        }
        if !self.loaded_once {
            let text = vec![Line::from(Span::styled(
                "loading…",
                Style::default().fg(palette::DIM),
            ))];
            frame.render_widget(Paragraph::new(text), inner);
            return;
        }
        if let Some(err) = self.last_error.as_deref() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("error: {err}"),
                    Style::default().fg(palette::ERROR),
                ))),
                inner,
            );
            return;
        }
        if self.rows.is_empty() {
            let text = vec![Line::from(Span::styled(
                "no new links in window — try `]` to widen",
                Style::default().fg(palette::DIM),
            ))];
            frame.render_widget(Paragraph::new(text), inner);
            return;
        }
        let mut lines: Vec<Line> = Vec::with_capacity(self.rows.len());
        for (i, row) in self.rows.iter().enumerate() {
            let select_marker = if self.selected.contains(&i) {
                "[*] "
            } else {
                "    "
            };
            let ghost = if row.is_ghost { "?" } else { "" };
            let text = format!("{select_marker}({}) [[{}]]{}", row.count, row.target, ghost);
            let style = if i == self.cursor {
                Style::default()
                    .fg(palette::PRIMARY)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else if row.is_ghost {
                Style::default().fg(palette::DIM)
            } else {
                Style::default().fg(palette::PRIMARY)
            };
            lines.push(Line::from(Span::styled(text, style)));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn help_sections(&self) -> Vec<HelpSection> {
        vec![
            HelpSection::new("Navigation", &[("↑ / ↓ · j / k", "select prev / next row")]),
            HelpSection::new(
                "Selection",
                &[("Space", "toggle multi-select on the current row")],
            ),
            HelpSection::new("Window", &[("[", "narrower window"), ("]", "wider window")]),
            HelpSection::new(
                "Handoff",
                &[("Enter", "open Journal tab with selected (or cursor) links")],
            ),
            HelpSection::new("Source", &[("R", "reload the review")]),
        ]
    }
}

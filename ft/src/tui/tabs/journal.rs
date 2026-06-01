//! `Journal` tab — interactive surface for `ft_core::journal::build_journal`.
//!
//! The user picks a note via the shared fuzzy picker, then scrolls the
//! reverse-chronological feed of paragraph mentions. `Enter` opens the
//! source note in `$EDITOR` at the paragraph's first line. State
//! (the target note + its loaded entries) persists across tab switches;
//! `R` reloads, `c` clears back to the empty-state prompt.
//!
//! Cross-tab entry: the graph tab's `Shift+J` keybinding raises
//! [`crate::tui::tab::AppRequest::JournalForNote`]; the App services
//! that by calling [`JournalTab::queue_journal_for`] and switching the
//! active tab. The queued path is consumed on the next `on_focus` and
//! turned into a load.
//!
//! `BlameCache` is held in the tab so subsequent loads in the same
//! session warm up; the on-disk file at `.ft/cache/blame.msgpack` is
//! refreshed best-effort after every successful `build_journal` call.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use ft_core::blame_cache::BlameCache;
use ft_core::git::discover_repo;
use ft_core::graph::Graph;
use ft_core::journal::{build_journal, JournalEntry};
use ft_core::search::Hit;

use crate::tui::event::Event;
use crate::tui::help::HelpSection;
use crate::tui::tab::{AppRequest, EventOutcome, Tab, TabCtx, ToastStyle};
use crate::tui::widgets::picker::{FuzzyPicker, PickerOutcome, VaultFilePickerSource};

pub struct JournalTab {
    /// Vault-relative path of the note currently loaded. `None` puts
    /// the tab in its empty-state prompt.
    target_path: Option<PathBuf>,
    /// The currently-displayed feed.
    entries: Vec<JournalEntry>,
    /// 0-indexed cursor into `entries`. Saturating-clamped on load.
    selected: usize,
    /// 0-indexed scroll offset (in entries, not lines). Adjusted at
    /// render time when `selected` would otherwise fall offscreen.
    scroll_offset: usize,
    /// Active fuzzy picker. `Some` while the picker overlay owns the
    /// keyboard; cleared on selection or `Esc`.
    picker: Option<FuzzyPicker<VaultFilePickerSource>>,
    /// Queued path from a cross-tab jump. Consumed by `on_focus` to
    /// kick off a load.
    queued_for: Option<PathBuf>,
    /// Lazy-loaded blame cache; preserved across loads within the
    /// tab's session.
    cache: Option<BlameCache>,
    /// Last load error, if any. Shown as a single-line banner so the
    /// user knows why the feed didn't change. Cleared on next
    /// successful load or `c`.
    last_error: Option<String>,
}

impl Default for JournalTab {
    fn default() -> Self {
        Self::new()
    }
}

impl JournalTab {
    pub fn new() -> Self {
        Self {
            target_path: None,
            entries: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            picker: None,
            queued_for: None,
            cache: None,
            last_error: None,
        }
    }

    /// Run `build_journal` for `path` (vault-relative) and replace
    /// `entries`. The blame cache is loaded from disk on first use and
    /// saved best-effort after a successful build.
    fn load_for(&mut self, path: PathBuf, ctx: &mut TabCtx) {
        if discover_repo(&ctx.vault.path).is_none() {
            self.last_error = Some(
                "vault is not inside a git repository — journal needs git history".to_string(),
            );
            self.target_path = Some(path);
            self.entries.clear();
            self.selected = 0;
            self.scroll_offset = 0;
            return;
        }

        // Build a fresh graph; the App-level graph belongs to the Graph
        // tab and isn't easily reachable from here.
        let scan = ctx.vault.scan();
        let graph = match Graph::build(ctx.vault, &scan) {
            Ok(g) => g,
            Err(e) => {
                self.last_error = Some(format!("graph build failed: {e}"));
                self.target_path = Some(path);
                self.entries.clear();
                return;
            }
        };

        let Some(note_id) = graph.note_by_path(&path) else {
            self.last_error = Some(format!("note not found in graph: {}", path.display()));
            self.target_path = Some(path);
            self.entries.clear();
            return;
        };

        if self.cache.is_none() {
            self.cache = Some(BlameCache::load(&ctx.vault.path).unwrap_or_default());
        }
        let cache = self.cache.as_mut().expect("just initialized");

        // Pass `vault.path` as the git CWD: paragraph paths are
        // vault-relative, and `git -C <vault>` finds the enclosing repo
        // even when the vault is a subdirectory of it.
        let vault_path = ctx.vault.path.clone();
        match build_journal(&graph, note_id, ctx.vault, &vault_path, cache) {
            Ok(report) => {
                self.last_error = None;
                self.target_path = Some(path);
                self.entries = report.entries;
                self.selected = 0;
                self.scroll_offset = 0;
                if !report.skipped_blame.is_empty() {
                    let first = report
                        .skipped_blame
                        .first()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    let extra = report.skipped_blame.len().saturating_sub(1);
                    let msg = if extra == 0 {
                        format!("blame skipped 1 file: {first}")
                    } else {
                        format!(
                            "blame skipped {} files (e.g. {first})",
                            report.skipped_blame.len()
                        )
                    };
                    crate::tui::notes_actions::queue_toast(ctx, &msg, ToastStyle::Info);
                }
                // Best-effort cache save — failures are logged via toast
                // and otherwise non-fatal.
                if let Err(e) = cache.save(&ctx.vault.path) {
                    crate::tui::notes_actions::queue_toast(
                        ctx,
                        &format!("blame cache save: {e}"),
                        ToastStyle::Info,
                    );
                }
            }
            Err(e) => {
                self.last_error = Some(format!("build_journal failed: {e}"));
                self.target_path = Some(path);
                self.entries.clear();
            }
        }
    }

    fn open_picker(&mut self, ctx: &TabCtx) {
        let source = VaultFilePickerSource::new(Arc::clone(ctx.vault), Arc::clone(ctx.recents));
        self.picker = Some(FuzzyPicker::new(source));
    }

    fn handle_picker_key(&mut self, k: KeyEvent, ctx: &mut TabCtx) -> EventOutcome {
        let Some(mut picker) = self.picker.take() else {
            return EventOutcome::NotHandled;
        };
        match picker.handle_key(k) {
            PickerOutcome::Selected(hit) => {
                let Hit { path, .. } = hit;
                self.load_for(path, ctx);
                EventOutcome::Consumed
            }
            PickerOutcome::Cancelled => EventOutcome::Consumed,
            PickerOutcome::StillOpen => {
                self.picker = Some(picker);
                EventOutcome::Consumed
            }
            PickerOutcome::NotHandled => {
                self.picker = Some(picker);
                EventOutcome::NotHandled
            }
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let len = self.entries.len() as isize;
        let new = (self.selected as isize + delta).clamp(0, len - 1);
        self.selected = new as usize;
    }

    fn jump_first(&mut self) {
        self.selected = 0;
        self.scroll_offset = 0;
    }

    fn jump_last(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = self.entries.len() - 1;
    }

    fn request_open_selected(&self, ctx: &TabCtx) {
        let Some(entry) = self.entries.get(self.selected) else {
            return;
        };
        let abs = ctx.vault.path.join(&entry.source_path);
        ctx.recents.record_open(&entry.source_path);
        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
            path: abs,
            line: entry.line_start as usize,
        });
    }
}

impl Tab for JournalTab {
    fn title(&self) -> &str {
        "Journal"
    }

    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()> {
        if let Some(path) = self.queued_for.take() {
            self.load_for(path, ctx);
        }
        Ok(())
    }

    fn queue_journal_for(&mut self, note_path: &Path) {
        self.queued_for = Some(note_path.to_path_buf());
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };

        // Picker overlay captures the keyboard while open.
        if self.picker.is_some() {
            return Ok(self.handle_picker_key(k, ctx));
        }

        match (k.code, k.modifiers) {
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                self.open_picker(ctx);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('R'), m) if m == KeyModifiers::SHIFT => {
                if let Some(path) = self.target_path.clone() {
                    self.load_for(path, ctx);
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('c'), KeyModifiers::NONE) => {
                self.target_path = None;
                self.entries.clear();
                self.selected = 0;
                self.scroll_offset = 0;
                self.last_error = None;
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.move_selection(-1);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.move_selection(1);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('g'), KeyModifiers::NONE) => {
                self.jump_first();
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('G'), KeyModifiers::SHIFT) => {
                self.jump_last();
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('d'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_selection(10);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.move_selection(-10);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Enter, _) => {
                self.request_open_selected(ctx);
                Ok(EventOutcome::Consumed)
            }
            _ => Ok(EventOutcome::NotHandled),
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        match &self.target_path {
            None => render_empty(frame, area, self.last_error.as_deref()),
            Some(path) => render_loaded(
                frame,
                area,
                path,
                &self.entries,
                self.selected,
                &mut self.scroll_offset,
                self.last_error.as_deref(),
            ),
        }

        if let Some(ref mut picker) = self.picker {
            let popup_area = centered_rect(70, 70, area);
            frame.render_widget(Clear, popup_area);
            picker.render(frame, popup_area);
        }
    }

    fn help_sections(&self) -> Vec<HelpSection> {
        vec![
            HelpSection::new(
                "Source",
                &[
                    ("/", "open the fuzzy note picker"),
                    ("R", "reload the current note's journal"),
                    ("c", "clear back to the picker prompt"),
                ],
            ),
            HelpSection::new(
                "Navigation",
                &[
                    ("↑ / ↓ · j / k", "select prev / next entry"),
                    ("g / G", "first / last entry"),
                    ("Ctrl+D / Ctrl+U", "half-page down / up"),
                ],
            ),
            HelpSection::new(
                "Open",
                &[("Enter", "open selected entry's note in $EDITOR")],
            ),
        ]
    }
}

fn render_empty(frame: &mut Frame, area: Rect, last_error: Option<&str>) {
    let block = Block::default().borders(Borders::ALL).title(" Journal ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "press `/` to pick a note",
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Shift+J in the Graph tab on a Note row jumps straight here.",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    if let Some(err) = last_error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("error: {err}"),
            Style::default().fg(Color::Red),
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_loaded(
    frame: &mut Frame,
    area: Rect,
    target_path: &Path,
    entries: &[JournalEntry],
    selected: usize,
    scroll_offset: &mut usize,
    last_error: Option<&str>,
) {
    let title = format!(
        " Journal — {} ({} entries) ",
        target_path.display(),
        entries.len()
    );
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if entries.is_empty() {
        let mut lines = vec![Line::from(Span::styled(
            "no journal entries for this note",
            Style::default().fg(Color::Gray),
        ))];
        if let Some(err) = last_error {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("error: {err}"),
                Style::default().fg(Color::Red),
            )));
        }
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    // Each entry contributes: 1 header line + N body lines + 1 blank
    // separator. We materialize as Lines and clip via scroll_offset.
    let mut lines: Vec<Line> = Vec::new();
    let mut entry_starts: Vec<usize> = Vec::with_capacity(entries.len());
    for (i, e) in entries.iter().enumerate() {
        entry_starts.push(lines.len());
        let header_style = if i == selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        };
        lines.push(Line::from(Span::styled(
            format!("{}  {}", e.date, e.source_title),
            header_style,
        )));
        for body_line in e.section_text.lines() {
            lines.push(Line::from(Span::raw(body_line.to_string())));
        }
        lines.push(Line::from(""));
    }

    // Scroll so the selected entry's header sits within the viewport.
    let view_height = inner.height as usize;
    let selected_start = *entry_starts.get(selected).copied().get_or_insert(0);
    if selected_start < *scroll_offset {
        *scroll_offset = selected_start;
    } else if selected_start >= scroll_offset.saturating_add(view_height) {
        *scroll_offset = selected_start.saturating_sub(view_height.saturating_sub(2));
    }

    frame.render_widget(
        Paragraph::new(lines).scroll((*scroll_offset as u16, 0)),
        inner,
    );

    if let Some(err) = last_error {
        // Overlay a single-line error banner across the bottom of the inner area.
        let banner_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        frame.render_widget(Clear, banner_area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("error: {err}"),
                Style::default().fg(Color::Red),
            ))),
            banner_area,
        );
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

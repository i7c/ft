//! `ConflictResolve`: pick local / remote / skip for each issue whose
//! status changed on both sides since the last sync, then apply the
//! choices as local note edits (no network; a "local" choice becomes an
//! ordinary pending push).

use std::path::PathBuf;

use crossterm::event::KeyCode;
use ft_core::sources::sync::{resolve_conflict, Take};
use ft_core::task::Status;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::tui::{
    command::CommandDef,
    event::Event,
    keymap::KeyMap,
    modal::{Modal, ModalOutcome},
    modal_commands as mc, palette,
    tab::{AppRequest, TabCtx, ToastStyle},
};

/// One conflicted issue.
#[derive(Debug, Clone)]
pub struct ConflictRow {
    /// Vault-relative issue note.
    pub note: PathBuf,
    pub key: String,
    pub local: Status,
    pub remote: Status,
    pub choice: Option<Take>,
}

#[derive(Debug)]
pub struct ConflictResolveModal {
    pub rows: Vec<ConflictRow>,
    pub selected: usize,
}

impl ConflictResolveModal {
    pub fn new(rows: Vec<ConflictRow>) -> Self {
        Self { rows, selected: 0 }
    }

    fn choose(&mut self, take: Option<Take>) {
        if let Some(r) = self.rows.get_mut(self.selected) {
            r.choice = take;
        }
    }

    /// Apply every chosen resolution; returns `(resolved, failures)`.
    fn apply(&self, ctx: &TabCtx) -> (usize, Vec<String>) {
        let mut resolved = 0;
        let mut failures = Vec::new();
        for row in &self.rows {
            let Some(take) = row.choice else { continue };
            let path = ctx.vault.path.join(&row.note);
            let result = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|c| {
                    resolve_conflict(&c, take, ctx.today, ctx.vault.task_format())
                        .ok_or_else(|| "conflict no longer stored".to_string())
                })
                .and_then(|new| ft_core::fs::write_atomic(&path, &new).map_err(|e| e.to_string()));
            match result {
                Ok(()) => resolved += 1,
                Err(e) => failures.push(format!("{}: {e}", row.key)),
            }
        }
        (resolved, failures)
    }
}

impl Modal for ConflictResolveModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match k.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.selected + 1 < self.rows.len() {
                    self.selected += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Char('l') => self.choose(Some(Take::Local)),
            KeyCode::Char('r') => self.choose(Some(Take::Remote)),
            KeyCode::Char('s') => self.choose(None),
            KeyCode::Esc => return ModalOutcome::Closed,
            KeyCode::Enter => {
                let (resolved, failures) = self.apply(ctx);
                if resolved > 0 {
                    ctx.request_graph_refresh();
                }
                let (text, style) = if failures.is_empty() {
                    (
                        format!("resolved {resolved} conflict(s)"),
                        ToastStyle::Success,
                    )
                } else {
                    (failures.join("; "), ToastStyle::Error)
                };
                *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast { text, style });
                return ModalOutcome::Closed;
            }
            _ => {}
        }
        ModalOutcome::Consumed
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        let height = (self.rows.len() as u16 + 5).min(area.height.saturating_sub(2));
        let width = 72u16.min(area.width.saturating_sub(4));
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" status conflicts · l local · r remote · s skip · Enter apply ")
            .border_style(Style::default().fg(palette::PRIMARY))
            .style(Style::default().bg(palette::BLACK));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let mut lines = vec![Line::from("")];
        for (i, r) in self.rows.iter().enumerate() {
            let choice = match r.choice {
                Some(Take::Local) => "→ local",
                Some(Take::Remote) => "→ remote",
                None => "→ skip",
            };
            let cursor = if i == self.selected { "▶ " } else { "  " };
            let style = if i == self.selected {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "{cursor}{:<14} local={:<10} remote={:<10} {choice}",
                    r.key,
                    r.local.as_str(),
                    r.remote.as_str()
                ),
                style.fg(palette::WHITE),
            )));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn name(&self) -> &'static str {
        "conflict-resolve"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::CONFLICT_RESOLVE_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::CONFLICT_RESOLVE_KEYMAP
    }
}

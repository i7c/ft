//! Notes tab renderer. The idle body is a keymap-style panel; an opt-in
//! help overlay floats above it on `?`; the open-flow picker (when set)
//! covers the body with its own centered rect.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::tui::tab::TabCtx;
use crate::tui::tabs::notes::NotesState;

/// Idle-panel keymap. Each row is `(keys, description)`. Kept identical to
/// the `?` help overlay so users see one canonical list.
const IDLE_KEYS: &[(&str, &str)] = &[
    ("o", "open file / heading"),
    ("m", "move section(s) to another file"),
    ("?", "show this help"),
    ("Esc", "close overlay"),
];

/// Picker-mode keymap shown along the bottom while the open-flow picker
/// is on screen. Mirrors the bindings in `mod.rs`.
const PICKER_KEYS: &[(&str, &str)] = &[
    ("Enter", "open in $EDITOR"),
    ("Ctrl+O", "open in Obsidian"),
    ("Esc", "back to idle"),
];

pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    _ctx: &TabCtx,
    state: &mut NotesState,
    show_help: bool,
) {
    render_idle_body(frame, area);

    match state {
        NotesState::Idle => {
            if show_help {
                render_help_overlay(frame, area);
            }
        }
        NotesState::OpenPicking { picker } => {
            let popup = centered_rect(60, 70, area);
            frame.render_widget(Clear, popup);
            let outer = Block::default()
                .borders(Borders::ALL)
                .title(" open · pick file / heading ")
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::Black));
            let inner = outer.inner(popup);
            frame.render_widget(outer, popup);

            // Split the popup into picker area (top) + keymap footer (bottom).
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(2)])
                .split(inner);

            picker.render(frame, chunks[0]);

            let footer = Line::from(
                PICKER_KEYS
                    .iter()
                    .flat_map(|(k, d)| {
                        vec![
                            Span::styled(
                                format!(" {k} "),
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(format!("{d}  "), Style::default().fg(Color::Gray)),
                        ]
                    })
                    .collect::<Vec<_>>(),
            );
            frame.render_widget(
                Paragraph::new(footer).alignment(Alignment::Center),
                chunks[1],
            );
        }
    }
}

fn render_idle_body(frame: &mut Frame, area: Rect) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(" notes ")
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let mut lines: Vec<Line> = Vec::with_capacity(IDLE_KEYS.len() + 3);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Notes — Obsidian-flavoured editing",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for (key, desc) in IDLE_KEYS {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {key:<6}"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(*desc, Style::default().fg(Color::White)),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(50, 50, area);
    frame.render_widget(Clear, popup);

    let mut lines: Vec<Line> = Vec::with_capacity(IDLE_KEYS.len() + 4);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Notes keybindings",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for (key, desc) in IDLE_KEYS {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {key:<8}"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(*desc, Style::default().fg(Color::White)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  press ? or Esc to close",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" notes · help ")
        .style(Style::default().bg(Color::Black));
    frame.render_widget(Paragraph::new(lines).block(block), popup);
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

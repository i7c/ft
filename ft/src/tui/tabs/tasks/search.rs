use anyhow::Result;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::{
    event::Event,
    tab::{EventOutcome, TabCtx},
    tabs::tasks::view::View,
};

/// Stub Search view. Session 3 wires up: lazy task load, default
/// overdue+upcoming query, row rendering with priority/due/scheduled, the
/// overdue/upcoming divider, navigation, the editable query bar, and `R`.
pub struct SearchView;

impl SearchView {
    pub fn new() -> Self {
        Self
    }
}

impl View for SearchView {
    fn title(&self) -> &str {
        "Search"
    }

    fn handle_event(&mut self, _ev: Event, _ctx: &mut TabCtx) -> Result<EventOutcome> {
        Ok(EventOutcome::NotHandled)
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(area);

        // Query bar placeholder — session 3 makes this editable.
        let query = Paragraph::new(Line::from(vec![
            Span::styled("query: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "(default — session 3 wires this up)",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]))
        .block(Block::default().borders(Borders::ALL).title(" query "));
        frame.render_widget(query, chunks[0]);

        // Task list placeholder.
        let body = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "Search view",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "(no tasks loaded yet — session 3)",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .alignment(ratatui::layout::Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title(" tasks "));
        frame.render_widget(body, chunks[1]);
    }
}

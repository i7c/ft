//! Rendering for the Timeblocks tab (read-only view, plan 015 session 4).

use chrono::Timelike;
use ft_core::timeblock::report::{minutes_to_hours_minutes, time_per_tag, total_minutes};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::tui::tab::TabCtx;

use super::{Pane, TimeblocksTab, SIDEBAR_WIDTH};

pub(super) fn render(tab: &mut TimeblocksTab, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(1)])
        .split(area);

    render_sidebar(tab, frame, chunks[0]);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    render_pane(tab, frame, panes[0], Pane::Today);
    render_pane(tab, frame, panes[1], Pane::Tomorrow);
    // Suppress unused-warning in the no-render path; `ctx` is part of the
    // contract and will be used by session 5's mutation flows.
    let _ = ctx;
}

fn render_sidebar(tab: &TimeblocksTab, frame: &mut Frame, area: Rect) {
    let now = (tab.clock)();
    let date = now.format("%a %d %b").to_string();
    let time = now.format("%H:%M:%S").to_string();

    let mut lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!(" {date}"),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(" {time}"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " ── totals (today) ──",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let total = total_minutes(&tab.today.blocks);
    let (th, tm) = minutes_to_hours_minutes(total);
    lines.push(Line::from(Span::styled(
        format!(" total {th:02}:{tm:02}"),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )));
    for tt in time_per_tag(&tab.today.blocks) {
        let (h, m) = minutes_to_hours_minutes(tt.minutes);
        let style = if tt.tag == "break" {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(Span::styled(
            format!(" @{}  {h:02}:{m:02}", tt.tag),
            style,
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " ── focus ──",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        match tab.focus {
            Pane::Today => " ▶ today",
            Pane::Tomorrow => " ▶ tomorrow",
        },
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" sidebar ")
        .border_style(Style::default().fg(Color::DarkGray));
    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, area);
}

fn render_pane(tab: &TimeblocksTab, frame: &mut Frame, area: Rect, which: Pane) {
    let pane = match which {
        Pane::Today => &tab.today,
        Pane::Tomorrow => &tab.tomorrow,
    };
    let focused = tab.focus == which;
    let title_text = match which {
        Pane::Today => format!(" Today  {} ", pane.date),
        Pane::Tomorrow => format!(" Tomorrow  {} ", pane.date),
    };
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title_text)
        .border_style(border_style);

    if !pane.present {
        // Tomorrow pane (or today) with no daily-note file on disk yet.
        // Session 5 binds `c` to create the file via the daily-template;
        // for now we just surface the placeholder.
        let body = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  no daily note yet.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "  press `c` to create (session 5)",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let para = Paragraph::new(body).block(block);
        frame.render_widget(para, area);
        return;
    }

    if pane.blocks.is_empty() {
        let body = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  no timeblocks for this day yet.",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let para = Paragraph::new(body).block(block);
        frame.render_widget(para, area);
        return;
    }

    let items: Vec<ListItem> = pane
        .blocks
        .iter()
        .map(|b| {
            let line_text = format!("{:>3}  {}  {}", b.source_line, period_str(b), b.desc.trim());
            ListItem::new(line_text)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(pane.selection));

    let highlight_style = if focused {
        Style::default()
            .bg(Color::Cyan)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    };
    let list = List::new(items)
        .block(block)
        .highlight_style(highlight_style)
        .highlight_symbol(if focused { "▶ " } else { "  " });

    frame.render_stateful_widget(list, area, &mut state);
}

fn period_str(b: &ft_core::timeblock::Timeblock) -> String {
    format!(
        "{:02}:{:02} - {:02}:{:02}",
        b.start.hour(),
        b.start.minute(),
        b.end.hour(),
        b.end.minute()
    )
}

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
use ratatui::widgets::Clear;

use super::{FormField, Mode, Pane, TimeblocksTab, ViewMode, SIDEBAR_WIDTH};

pub(super) fn render(tab: &mut TimeblocksTab, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
    // Split off a single-row quickline strip from the bottom when the
    // tab is in Quickline / EditDesc mode. The form (`A`) renders as a
    // centered overlay instead.
    let bottom_strip = matches!(tab.mode, Mode::Quickline(_) | Mode::EditDesc { .. });
    let body_area = if bottom_strip {
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);
        render_quickline_strip(tab, frame, split[1]);
        split[0]
    } else {
        area
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(1)])
        .split(body_area);

    render_sidebar(tab, frame, chunks[0]);

    match tab.view {
        ViewMode::Split => {
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[1]);
            render_pane(tab, frame, ctx, panes[0], Pane::Today);
            render_pane(tab, frame, ctx, panes[1], Pane::Tomorrow);
        }
        ViewMode::Single => {
            // Full-width single pane shows whichever day currently has
            // focus. `h`/`l` flip focus → flips which day is on screen.
            render_pane(tab, frame, ctx, chunks[1], tab.focus);
        }
    }

    if let Mode::Form(_) = &tab.mode {
        render_form_modal(tab, frame, area);
    }
    if let Mode::Tagging { .. } = &tab.mode {
        render_tag_modal(tab, frame, area);
    }
}

fn render_quickline_strip(tab: &TimeblocksTab, frame: &mut Frame, area: Rect) {
    // ASCII-only prefixes so `chars().count()` matches the rendered
    // cell count (see the form-modal fix for the same reason).
    let (prefix, text, cursor) = match &tab.mode {
        Mode::Quickline(buf) => (" + ", buf.text.as_str(), buf.cursor),
        Mode::EditDesc { buf, .. } => (" edit desc > ", buf.text.as_str(), buf.cursor),
        _ => return,
    };
    let line = Line::from(vec![
        Span::styled(prefix, Style::default().fg(Color::Cyan)),
        Span::raw(text),
    ]);
    let para = Paragraph::new(line);
    frame.render_widget(para, area);
    let col = area.x + (prefix.chars().count() as u16) + (cursor as u16);
    let col = col.min(area.x + area.width.saturating_sub(1));
    frame.set_cursor_position((col, area.y));
}

fn render_form_modal(tab: &TimeblocksTab, frame: &mut Frame, area: Rect) {
    let Mode::Form(state) = &tab.mode else {
        return;
    };

    // Center a 50x10 modal inside the tab body area.
    let w = 50u16.min(area.width.saturating_sub(2));
    let h = 9u16.min(area.height.saturating_sub(2));
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let modal = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" New timeblock ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    frame.render_widget(block, modal);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Start row
            Constraint::Length(1), // End row
            Constraint::Length(1), // Desc row
            Constraint::Length(1), // blank
            Constraint::Length(1), // help
        ])
        .split(inner);

    // ASCII prefix so cursor positioning matches what the terminal
    // actually renders (the previous `▸` glyph was 2 cells wide in some
    // fonts, throwing the cursor offset off by one cell).
    let prefix_for = |label: &str, focused: bool| -> String {
        let marker = if focused { '>' } else { ' ' };
        format!("{marker} {label:<6}")
    };
    let row = |label: &str, buf_text: &str, focused: bool| -> Paragraph<'_> {
        let style = if focused {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        Paragraph::new(Line::from(vec![
            Span::styled(prefix_for(label, focused), style),
            Span::raw(buf_text.to_string()),
        ]))
    };

    let start_row = row("start", &state.start.text, state.focus == FormField::Start);
    let end_row = row("end", &state.end.text, state.focus == FormField::End);
    let desc_row = row("desc", &state.desc.text, state.focus == FormField::Desc);

    frame.render_widget(start_row, rows[0]);
    frame.render_widget(end_row, rows[1]);
    frame.render_widget(desc_row, rows[2]);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "Tab / ↑↓ to cycle  ·  Enter on desc to commit  ·  Esc to cancel",
            Style::default().fg(Color::DarkGray),
        )),
        rows[4],
    );

    // Cursor offset = the focused row's actual prefix width. ASCII-only
    // so `chars().count()` equals the rendered cell count.
    let (buf_cursor, row_idx, label) = match state.focus {
        FormField::Start => (state.start.cursor, 0, "start"),
        FormField::End => (state.end.cursor, 1, "end"),
        FormField::Desc => (state.desc.cursor, 2, "desc"),
    };
    let prefix_width = prefix_for(label, true).chars().count() as u16;
    let col = rows[row_idx].x + prefix_width + buf_cursor as u16;
    let col = col.min(rows[row_idx].x + rows[row_idx].width.saturating_sub(1));
    frame.set_cursor_position((col, rows[row_idx].y));
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
            // Totals always summarize the LEFT (anchor) pane — that's
            // the day the user is steering toward with H/L. Label by
            // date so the relationship is unambiguous.
            format!(" ── totals · {} ──", tab.today.date),
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
    let focused_date = match tab.focus {
        Pane::Today => tab.today.date,
        Pane::Tomorrow => tab.tomorrow.date,
    };
    lines.push(Line::from(Span::styled(
        format!(" ▶ {focused_date}"),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    // Show the view mode so users have a visible cue that `f` is doing
    // something — split vs single-day full-width.
    lines.push(Line::from(Span::styled(
        match tab.view {
            ViewMode::Split => " view: split",
            ViewMode::Single => " view: single (f)",
        },
        Style::default().fg(Color::DarkGray),
    )));
    if matches!(tab.mode, Mode::DeleteConfirm { .. }) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " d again = delete",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" sidebar ")
        .border_style(Style::default().fg(Color::DarkGray));
    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, area);
}

fn render_pane(tab: &TimeblocksTab, frame: &mut Frame, ctx: &TabCtx, area: Rect, which: Pane) {
    let pane = match which {
        Pane::Today => &tab.today,
        Pane::Tomorrow => &tab.tomorrow,
    };
    let focused = tab.focus == which;
    let title_text = format!(" {} ", pane_title(pane.date, ctx.today));
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
        // No daily-note file on disk yet. `c` creates it via the
        // configured `[periodic_notes.daily].template`.
        let body = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  no daily note yet.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "  press `c` to create from template",
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

    let items: Vec<ListItem> = pane.blocks.iter().map(build_block_item).collect();

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

/// Number of display lines a block occupies. Mirrors the spec:
/// 1–60 min → 1 line, 61–120 min → 2 lines, 121–180 min → 3 lines, …
/// (`ceil(duration / 60)`, with a saturating floor of 1 for safety).
fn block_lines(b: &ft_core::timeblock::Timeblock) -> usize {
    let mins = duration_minutes(b);
    if mins == 0 {
        1
    } else {
        ((mins - 1) / 60 + 1) as usize
    }
}

fn duration_minutes(b: &ft_core::timeblock::Timeblock) -> u32 {
    let s = b.start.hour() * 60 + b.start.minute();
    let e = b.end.hour() * 60 + b.end.minute();
    e.saturating_sub(s)
}

/// Build a `ListItem` whose visible height scales with the block's
/// duration. The first line carries the timing + description; each
/// extra line draws a `│` continuation marker under the time column so
/// the block reads as one visually tall row.
fn build_block_item(b: &ft_core::timeblock::Timeblock) -> ListItem<'static> {
    let n = block_lines(b);
    let header = format!("{:>3}  {}  {}", b.source_line, period_str(b), b.desc.trim());
    // Continuation lines indent past the "NN  " column (3 + 2 = 5
    // cells) and place a `│` where the time would otherwise be.
    // Indent matches the highlight_symbol's "▶ " prefix width (2) so
    // the bar lines up cleanly under the header in both focused and
    // unfocused states.
    let cont = "        │";
    let mut lines: Vec<Line> = Vec::with_capacity(n);
    lines.push(Line::from(header));
    for _ in 1..n {
        lines.push(Line::from(Span::styled(
            cont.to_string(),
            Style::default().fg(Color::DarkGray),
        )));
    }
    ListItem::new(lines)
}

fn render_tag_modal(tab: &TimeblocksTab, frame: &mut Frame, area: Rect) {
    let Mode::Tagging {
        pane,
        block_idx,
        buf,
    } = &tab.mode
    else {
        return;
    };

    let target = {
        let p = match *pane {
            Pane::Today => &tab.today,
            Pane::Tomorrow => &tab.tomorrow,
        };
        p.blocks.get(*block_idx).cloned()
    };

    let w = 60u16.min(area.width.saturating_sub(2));
    let h = 9u16.min(area.height.saturating_sub(2));
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let modal = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Tags ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    frame.render_widget(block, modal);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // block summary
            Constraint::Length(1), // current tags
            Constraint::Length(1), // blank
            Constraint::Length(1), // input prompt
            Constraint::Length(1), // help
        ])
        .split(inner);

    let summary = target
        .as_ref()
        .map(|b| format!("Block: {} - {} {}", period_str(b), "", b.desc.trim()))
        .unwrap_or_else(|| "Block: (gone)".to_string());
    frame.render_widget(
        Paragraph::new(Span::styled(
            summary,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        rows[0],
    );

    let tags_text = target
        .as_ref()
        .map(|b| {
            if b.tags.is_empty() {
                "Current: (none)".to_string()
            } else {
                let s = b
                    .tags
                    .iter()
                    .map(|t| t.to_string_form())
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("Current: {s}")
            }
        })
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(Span::styled(
            tags_text,
            Style::default().fg(Color::DarkGray),
        )),
        rows[1],
    );

    let prefix = "> ";
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(Color::Cyan)),
            Span::raw(buf.text.clone()),
        ])),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "+@tag to add, -@tag to remove (space-separated)  ·  Enter commits, Esc cancels",
            Style::default().fg(Color::DarkGray),
        )),
        rows[4],
    );

    let col = rows[3].x + (prefix.chars().count() as u16) + buf.cursor as u16;
    let col = col.min(rows[3].x + rows[3].width.saturating_sub(1));
    frame.set_cursor_position((col, rows[3].y));
}

/// Friendly pane title for `date` relative to `today`. Always includes
/// the day-of-week + ISO date; when the date IS today/tomorrow/yesterday
/// (the three the user steers by most often), a parenthetical badge
/// makes that obvious.
fn pane_title(date: chrono::NaiveDate, today: chrono::NaiveDate) -> String {
    let dow = date.format("%a").to_string();
    let badge = match (date - today).num_days() {
        0 => " (today)",
        1 => " (tomorrow)",
        -1 => " (yesterday)",
        _ => "",
    };
    format!("{dow} {date}{badge}")
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

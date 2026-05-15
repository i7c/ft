use chrono::Local;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs},
    Frame,
};

use crate::tui::tab::{Tab, TabCtx};

/// Whether the help overlay is open and which mode tag to render in the
/// status bar's right cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Help,
    /// `g` leader pressed; waiting for the second key (`s` → sync,
    /// any other key → dismiss).
    GitLeader,
    /// `ft_core::git::sync` is running. The overlay is drawn once
    /// before the (blocking) call; the event loop is paused for the
    /// duration so no further keys are processed.
    Syncing,
    /// Sync surfaced a merge or rebase conflict. The conflict-detail
    /// modal stays up until the user presses Esc.
    SyncConflict,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Help => "help",
            Mode::GitLeader => "git",
            Mode::Syncing => "sync",
            Mode::SyncConflict => "conflict",
        }
    }
}

/// Data attached to [`Mode::SyncConflict`]. Stored on the App so the
/// detail modal can render the conflicted file list and remember which
/// strategy produced them.
#[derive(Debug, Clone)]
pub struct SyncConflictInfo {
    pub kind: SyncConflictKind,
    pub files: Vec<std::path::PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncConflictKind {
    Merge,
    Rebase,
}

/// Compute the screen layout: top tab bar (1 line) + body + status bar (1 line).
pub fn split_screen(area: Rect) -> [Rect; 3] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

pub fn render_tab_bar(frame: &mut Frame, area: Rect, titles: &[&str], selected: usize) {
    let spans: Vec<Line> = titles
        .iter()
        .enumerate()
        .map(|(i, t)| Line::from(format!(" {} {} ", i + 1, t)))
        .collect();
    let widget = Tabs::new(spans)
        .select(selected)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .divider("│");
    frame.render_widget(widget, area);
}

pub fn render_status_bar(
    frame: &mut Frame,
    area: Rect,
    vault_name: &str,
    tab_title: &str,
    last_refresh: Option<chrono::DateTime<Local>>,
    toast: Option<&crate::tui::app::Toast>,
    mode: Mode,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(30),
            Constraint::Percentage(20),
        ])
        .split(area);

    let left = Line::from(vec![
        Span::styled(" vault: ", Style::default().fg(Color::DarkGray)),
        Span::styled(vault_name, Style::default().fg(Color::White)),
        Span::raw("  ·  "),
        Span::styled("tab: ", Style::default().fg(Color::DarkGray)),
        Span::styled(tab_title, Style::default().fg(Color::White)),
    ]);

    // Toast takes priority over the refresh timestamp so transient
    // success/error feedback isn't crowded out by the periodic redraw.
    let center = if let Some(t) = toast {
        let color = match t.style {
            crate::tui::tab::ToastStyle::Success => Color::Green,
            crate::tui::tab::ToastStyle::Error => Color::Red,
        };
        Line::from(Span::styled(
            t.text.clone(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center)
    } else {
        let refresh_text = match last_refresh {
            Some(ts) => format!("refreshed {}", ts.format("%H:%M:%S")),
            None => "not yet refreshed".to_string(),
        };
        Line::from(Span::styled(
            refresh_text,
            Style::default().fg(Color::DarkGray),
        ))
        .alignment(Alignment::Center)
    };

    let right = Line::from(vec![
        Span::styled("mode: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            mode.label(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ])
    .alignment(Alignment::Right);

    let bg = Style::default().bg(Color::Rgb(28, 28, 32));
    frame.render_widget(Paragraph::new(left).style(bg), chunks[0]);
    frame.render_widget(Paragraph::new(center).style(bg), chunks[1]);
    frame.render_widget(Paragraph::new(right).style(bg), chunks[2]);
}

pub fn render_body(frame: &mut Frame, area: Rect, tab: &mut dyn Tab, ctx: &TabCtx) {
    tab.render(frame, area, ctx);
}

const HELP_LINES: &[(&str, &str)] = &[
    ("q / Ctrl+C", "quit"),
    ("?", "toggle this help"),
    ("Tab / Shift+Tab", "next / previous tab"),
    ("1 / 2", "jump to tab N"),
    ("g s", "git sync"),
    ("/", "edit query"),
    ("↑ / ↓ · j / k", "select task"),
    ("] / [", "due date +1d / -1d"),
    ("} / {", "scheduled +1d / -1d"),
    ("t", "set due to today"),
    ("p / P", "priority cycle fwd / back"),
    ("x / X", "complete / cancel"),
    ("e", "open edit popup"),
    ("c / Shift+C", "new task (line / form)"),
    ("Ctrl+E", "expand quickline → form"),
    ("Enter (target)", "open file/heading picker"),
    ("Enter", "open task in $EDITOR"),
    ("R", "reload vault"),
    ("Ctrl+W / Ctrl+⌫", "delete previous word"),
    ("Esc", "close overlay"),
];

pub fn render_help_overlay(frame: &mut Frame, area: Rect) {
    // 90% height (was 80%) — the binding list grew past what 80% of a
    // 24-row terminal could contain after plan-004 added `c` for the
    // new-task quickline.
    let popup = centered_rect(60, 90, area);
    frame.render_widget(Clear, popup);

    let mut lines: Vec<Line> = Vec::with_capacity(HELP_LINES.len() + 2);
    lines.push(Line::from(Span::styled(
        "Keybindings",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for (key, desc) in HELP_LINES {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {key:<18}"),
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
        .title(" help ")
        .style(Style::default().bg(Color::Black));
    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, popup);
}

/// Small overlay showing the second-key choices for the `g` leader.
pub fn render_git_leader(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(48, 30, area);
    frame.render_widget(Clear, popup);

    let outer = Block::default()
        .borders(Borders::ALL)
        .title(" git · pick an action ")
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let lines = [
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  s     ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "sync (commit + pull + push)",
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Esc   ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("cancel", Style::default().fg(Color::White)),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines.to_vec()).alignment(Alignment::Left),
        inner,
    );
}

/// Blocking "Syncing…" overlay drawn once before the (synchronous)
/// `ft_core::git::sync` call. The event loop is paused for the
/// duration, so this is a static label — no spinner animation needed.
pub fn render_syncing(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(30, 20, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" syncing ")
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let lines = [
        Line::from(""),
        Line::from(Span::styled(
            "  running ft git sync…",
            Style::default().fg(Color::White),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines.to_vec()).alignment(Alignment::Left),
        inner,
    );
}

/// Persistent conflict-detail modal. Lists the conflicted files and
/// stays up until the user presses Esc.
pub fn render_sync_conflict(frame: &mut Frame, area: Rect, info: &SyncConflictInfo) {
    let popup = centered_rect(60, 50, area);
    frame.render_widget(Clear, popup);

    let title = match info.kind {
        SyncConflictKind::Merge => " merge conflict ",
        SyncConflictKind::Rebase => " rebase conflict ",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Red))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines: Vec<Line> = Vec::with_capacity(info.files.len() + 4);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("  {} conflicted file(s):", info.files.len()),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for f in &info.files {
        lines.push(Line::from(Span::styled(
            format!("    {}", f.display()),
            Style::default().fg(Color::White),
        )));
    }
    lines.push(Line::from(""));
    let hint = match info.kind {
        SyncConflictKind::Merge => "  resolve, commit, and push manually",
        SyncConflictKind::Rebase => "  resolve, then `git rebase --continue` manually",
    };
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        "  press Esc to dismiss",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    )));
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner);
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

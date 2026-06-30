use chrono::Local;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs,
    },
    Frame,
};

use crate::tui::{
    help::HelpSection,
    jobs::JobKind,
    palette,
    tab::{Tab, TabCtx},
};

/// Whether the help overlay is open and which mode tag to render in the
/// status bar's right cell.
///
/// Note: there is no `Syncing` mode — git sync runs on a background
/// worker thread (plan 014). The "a sync is in flight" indicator is
/// rendered as a separate cell in the status bar driven off
/// `App.jobs`, not off `Mode`, so the user can drop into help, switch
/// tabs, or even open the git leader again while a sync is running
/// without the indicator disappearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Help,
    /// `g` leader pressed; waiting for the second key (`s` → sync,
    /// any other key → dismiss).
    GitLeader,
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
        .style(Style::default().fg(palette::DIM))
        .highlight_style(
            Style::default()
                .fg(palette::BLACK)
                .bg(palette::PRIMARY)
                .add_modifier(Modifier::BOLD),
        )
        .divider("│");
    frame.render_widget(widget, area);
}

/// Inputs to [`render_status_bar`]. Grouped into a struct so the
/// function stays under clippy's `too_many_arguments` threshold as
/// the cell composition grows. All fields are `Copy` so the caller
/// can hand the struct over by value without cloning.
#[derive(Clone, Copy)]
pub struct StatusBarState<'a> {
    pub vault_name: &'a str,
    pub tab_title: &'a str,
    pub last_refresh: Option<chrono::DateTime<Local>>,
    pub toast: Option<&'a crate::tui::app::Toast>,
    pub mode: Mode,
    pub in_flight: Option<JobKind>,
    /// Name of the active modal, if any (`App::active_modal_name()`).
    /// When `Some`, the right cell renders `modal: <name>` instead of
    /// `mode: <label>` so users always know which keymap context owns
    /// the keyboard. Added in extract-modal-driver §6.
    pub active_modal: Option<&'a str>,
    /// Up to three `(chord, label)` pairs surfaced from the active
    /// modal's keymap (`CommandDef.is_primary = true`). When non-empty
    /// and no toast is showing, the center cell renders
    /// `chord:label  chord:label  chord:label` instead of the
    /// refresh timestamp so users see the modal's important chords
    /// without opening `?`. Added in commands-and-keymaps §10.2.
    pub modal_hints: &'a [(String, String)],
}

pub fn render_status_bar(frame: &mut Frame, area: Rect, state: StatusBarState<'_>) {
    let StatusBarState {
        vault_name,
        tab_title,
        last_refresh,
        toast,
        mode,
        in_flight,
        active_modal,
        modal_hints,
    } = state;
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(30),
            Constraint::Percentage(20),
        ])
        .split(area);

    let left = Line::from(vec![
        Span::styled(" vault: ", Style::default().fg(palette::DIM)),
        Span::styled(vault_name, Style::default().fg(palette::WHITE)),
        Span::raw("  ·  "),
        Span::styled("tab: ", Style::default().fg(palette::DIM)),
        Span::styled(tab_title, Style::default().fg(palette::WHITE)),
    ]);

    // Toast takes priority over the refresh timestamp so transient
    // success/error feedback isn't crowded out by the periodic redraw.
    // When a modal is active and the modal advertised primary chords
    // (commands-and-keymaps §10.2), the chord hints take the cell
    // ahead of the refresh stamp so users see what the modal accepts.
    let center = if let Some(t) = toast {
        let color = match t.style {
            crate::tui::tab::ToastStyle::Success => palette::SUCCESS,
            crate::tui::tab::ToastStyle::Error => palette::ERROR,
            crate::tui::tab::ToastStyle::Info => palette::PRIMARY,
        };
        Line::from(Span::styled(
            t.text.clone(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center)
    } else if !modal_hints.is_empty() {
        let mut spans: Vec<Span> = Vec::with_capacity(modal_hints.len() * 4);
        for (i, (chord, label)) in modal_hints.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled("  ", Style::default().fg(palette::DIM)));
            }
            spans.push(Span::styled(
                chord.clone(),
                Style::default()
                    .fg(palette::TERTIARY)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(":", Style::default().fg(palette::DIM)));
            spans.push(Span::styled(
                label.clone(),
                Style::default().fg(palette::WHITE),
            ));
        }
        Line::from(spans).alignment(Alignment::Center)
    } else {
        let refresh_text = match last_refresh {
            Some(ts) => format!("refreshed {}", ts.format("%H:%M:%S")),
            None => "not yet refreshed".to_string(),
        };
        Line::from(Span::styled(
            refresh_text,
            Style::default().fg(palette::DIM),
        ))
        .alignment(Alignment::Center)
    };

    // Right cell composes `mode: <label>` (default), `⟳ <job> · <label>`
    // (in-flight), or `modal: <name>` (when a modal is active — extract-
    // modal-driver §6). The "mode:" prefix is dropped when an indicator
    // is present so the line still fits the 16-char right cell at 80
    // cols. The modal indicator persists across help, git leader, and
    // conflict modes so the user always knows which keymap context
    // owns the keyboard. The in-flight indicator takes priority over
    // modal (background jobs are higher-stakes than which modal is up).
    let right = if let Some(kind) = in_flight {
        Line::from(vec![
            Span::styled(
                format!("⟳ {}", kind.indicator_label()),
                Style::default()
                    .fg(palette::PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · ", Style::default().fg(palette::DIM)),
            Span::styled(
                mode.label(),
                Style::default()
                    .fg(palette::SECONDARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ])
    } else if let Some(name) = active_modal {
        Line::from(vec![
            Span::styled("modal: ", Style::default().fg(palette::DIM)),
            Span::styled(
                name,
                Style::default()
                    .fg(palette::TERTIARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ])
    } else {
        Line::from(vec![
            Span::styled("mode: ", Style::default().fg(palette::DIM)),
            Span::styled(
                mode.label(),
                Style::default()
                    .fg(palette::SECONDARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ])
    }
    .alignment(Alignment::Right);

    let bg = Style::default().bg(palette::STATUS_BG);
    frame.render_widget(Paragraph::new(left).style(bg), chunks[0]);
    frame.render_widget(Paragraph::new(center).style(bg), chunks[1]);
    frame.render_widget(Paragraph::new(right).style(bg), chunks[2]);
}

pub fn render_body(frame: &mut Frame, area: Rect, tab: &mut dyn Tab, ctx: &TabCtx) {
    tab.render(frame, area, ctx);
}

/// Render the `?` overlay. The renderer is tab-agnostic: it composes the
/// shared `global` section (always first) with whatever the active tab
/// returns from `Tab::help_sections()`. The header carries the tab title
/// so users can tell which keymap they're looking at.
pub fn render_help_overlay(
    frame: &mut Frame,
    area: Rect,
    tab_title: &str,
    global: &HelpSection,
    tab_sections: &[HelpSection],
    scroll: &mut usize,
    view_height_out: &mut u16,
) {
    // 75% width keeps the key + description columns readable on an
    // 80-col terminal; 95% height gives the popup room, and the
    // scroll + scrollbar below handle the case where the composed
    // lines still overflow (e.g. the Graph tab's ~35 bindings).
    let popup = centered_rect(75, 95, area);
    frame.render_widget(Clear, popup);

    // Pre-compute the longest key column across all sections so the
    // `desc` column lines up no matter which tab is active. The 4-char
    // floor prevents single-char keys from collapsing the column.
    let key_width = std::iter::once(global)
        .chain(tab_sections.iter())
        .flat_map(|s| s.entries.iter())
        .map(|e| e.keys.chars().count())
        .max()
        .unwrap_or(0)
        .max(4);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!("Keybindings — {tab_title}"),
        Style::default()
            .fg(palette::PRIMARY)
            .add_modifier(Modifier::BOLD),
    )));

    for section in std::iter::once(global).chain(tab_sections.iter()) {
        if section.entries.is_empty() {
            continue;
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {}", section.title),
            Style::default()
                .fg(palette::PRIMARY)
                .add_modifier(Modifier::BOLD),
        )));
        for entry in &section.entries {
            let pad = key_width.saturating_sub(entry.keys.chars().count());
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {}{}  ", entry.keys, " ".repeat(pad)),
                    Style::default()
                        .fg(palette::SECONDARY)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(entry.desc.clone(), Style::default().fg(palette::WHITE)),
            ]));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ↑/↓ or j/k scroll · PgUp/PgDn · g/G · ?/Esc/q close",
        Style::default()
            .fg(palette::DIM)
            .add_modifier(Modifier::ITALIC),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" help ")
        .style(Style::default().bg(palette::BLACK));
    frame.render_widget(block, popup);
    let inner = {
        let b = Block::default().borders(Borders::ALL);
        b.inner(popup)
    };

    // Clamp the scroll offset to the valid range for the current
    // layout. `max_scroll` is 0 when the content fits, which also
    // disables the scrollbar below.
    let total = lines.len();
    let max_scroll = total.saturating_sub(inner.height as usize);
    *scroll = (*scroll).min(max_scroll);
    *view_height_out = inner.height;

    let overflows = total > inner.height as usize;
    // Reserve the rightmost column for the scrollbar track on overflow
    // so the thumb never paints over row text — mirrors
    // `widgets::scroll_list`.
    let text_area = if overflows {
        Rect {
            width: inner.width.saturating_sub(1),
            ..inner
        }
    } else {
        inner
    };
    let para = Paragraph::new(lines).scroll((*scroll as u16, 0));
    frame.render_widget(para, text_area);

    if overflows {
        let scrollbar_area = Rect {
            x: inner.x + inner.width.saturating_sub(1),
            width: 1,
            ..inner
        };
        let mut sb_state = ScrollbarState::new(total)
            .viewport_content_length(inner.height as usize)
            .position(*scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::default().fg(palette::PRIMARY))
            .track_style(Style::default().fg(palette::DIM));
        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut sb_state);
    }
}

/// Small overlay showing the second-key choices for the `g` leader.
pub fn render_git_leader(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(48, 30, area);
    frame.render_widget(Clear, popup);

    let outer = Block::default()
        .borders(Borders::ALL)
        .title(" git · pick an action ")
        .border_style(Style::default().fg(palette::PRIMARY))
        .style(Style::default().bg(palette::BLACK));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let lines = [
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  s     ",
                Style::default()
                    .fg(palette::SECONDARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "sync (commit + pull + push)",
                Style::default().fg(palette::WHITE),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  c     ",
                Style::default()
                    .fg(palette::SECONDARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "commit (local only, no pull/push)",
                Style::default().fg(palette::WHITE),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Esc   ",
                Style::default()
                    .fg(palette::SECONDARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("cancel", Style::default().fg(palette::WHITE)),
        ]),
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
        .border_style(Style::default().fg(palette::ERROR))
        .style(Style::default().bg(palette::BLACK));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines: Vec<Line> = Vec::with_capacity(info.files.len() + 4);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("  {} conflicted file(s):", info.files.len()),
        Style::default()
            .fg(palette::WHITE)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for f in &info.files {
        lines.push(Line::from(Span::styled(
            format!("    {}", f.display()),
            Style::default().fg(palette::WHITE),
        )));
    }
    lines.push(Line::from(""));
    let hint = match info.kind {
        SyncConflictKind::Merge => "  resolve, commit, and push manually",
        SyncConflictKind::Rebase => "  resolve, then `git rebase --continue` manually",
    };
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(palette::DIM),
    )));
    lines.push(Line::from(Span::styled(
        "  press Esc to dismiss",
        Style::default()
            .fg(palette::DIM)
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

#[cfg(test)]
mod status_bar_tests {
    //! §10.3 — snapshot the status bar's center cell to lock in the
    //! modal primary-chord hint rendering. Tests render
    //! [`render_status_bar`] directly so they don't depend on a full
    //! `App` or any tab/modal scaffolding.
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn render_bar(state: StatusBarState<'_>, w: u16) -> String {
        let backend = TestBackend::new(w, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = Rect {
                    x: 0,
                    y: 0,
                    width: w,
                    height: 1,
                };
                render_status_bar(f, area, state);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for x in 0..buf.area().width {
            out.push_str(buf[(x, 0)].symbol());
        }
        out
    }

    #[test]
    fn modal_hint_cell_shows_primary_chords() {
        let hints = vec![
            ("Space".to_string(), "toggle".to_string()),
            ("Enter".to_string(), "confirm".to_string()),
            ("Esc".to_string(), "cancel".to_string()),
        ];
        let state = StatusBarState {
            vault_name: "v",
            tab_title: "Graph",
            last_refresh: None,
            toast: None,
            mode: Mode::Normal,
            in_flight: None,
            active_modal: Some("section-move"),
            modal_hints: &hints,
        };
        // 160 cols ⇒ 48-col center cell ⇒ all three hints fit
        // comfortably. Narrower terminals get a graceful truncation
        // (ratatui clips the cell), but the cell-shape spec scenario
        // requires the full list to render somewhere.
        let rendered = render_bar(state, 160);
        // Center cell shows all three primary hints with the
        // `chord:label` shape from the spec scenario.
        assert!(
            rendered.contains("Space:toggle"),
            "expected Space:toggle in center cell, got: {rendered}"
        );
        assert!(
            rendered.contains("Enter:confirm"),
            "expected Enter:confirm in center cell, got: {rendered}"
        );
        assert!(
            rendered.contains("Esc:cancel"),
            "expected Esc:cancel in center cell, got: {rendered}"
        );
        // Right cell still surfaces the modal name (§6 indicator).
        assert!(
            rendered.contains("modal: section-move"),
            "expected modal name in right cell, got: {rendered}"
        );
        // Hints replaced the refresh timestamp.
        assert!(!rendered.contains("refreshed"));
        assert!(!rendered.contains("not yet refreshed"));
    }

    #[test]
    fn modal_hint_cell_empty_when_no_modal() {
        let hints: Vec<(String, String)> = Vec::new();
        let state = StatusBarState {
            vault_name: "v",
            tab_title: "Graph",
            last_refresh: None,
            toast: None,
            mode: Mode::Normal,
            in_flight: None,
            active_modal: None,
            modal_hints: &hints,
        };
        let rendered = render_bar(state, 80);
        // Falls back to the refresh-stamp default; no hint text leaks
        // through, and the right cell shows `mode: normal` (not `modal:`).
        assert!(rendered.contains("not yet refreshed"));
        assert!(!rendered.contains(":toggle"));
        assert!(!rendered.contains(":confirm"));
        assert!(rendered.contains("mode: normal"));
        assert!(!rendered.contains("modal: "));
    }

    #[test]
    fn toast_outranks_modal_hints_in_center_cell() {
        // A toast in flight crowds out everything else — confirming
        // hints don't override transient success/error feedback.
        let hints = vec![("Enter".to_string(), "confirm".to_string())];
        let toast = crate::tui::app::Toast {
            text: "saved".to_string(),
            style: crate::tui::tab::ToastStyle::Success,
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(1),
        };
        let state = StatusBarState {
            vault_name: "v",
            tab_title: "Graph",
            last_refresh: None,
            toast: Some(&toast),
            mode: Mode::Normal,
            in_flight: None,
            active_modal: Some("create"),
            modal_hints: &hints,
        };
        let rendered = render_bar(state, 80);
        assert!(rendered.contains("saved"));
        assert!(!rendered.contains("Enter:confirm"));
    }
}

//! Shared list/preview split widget for the paragraph-feed tabs
//! (Recent, Gather).
//!
//! Both tabs render the same shape — a compact one-line-per-entry list
//! on top and a single-entry paragraph preview on the bottom, like an
//! email client. The two entry types (`RecentEntry`, `GatherEntry`)
//! differ only in which fields they carry, so each tab builds its own
//! compact list rows + preview header + wrapped body and hands them
//! here; this widget owns only the split geometry, the list's
//! cursor-follow + scrollbar (via [`render_scroll_list`]), and the
//! preview pane's non-scrolling header/body render.
//!
//! Contract: the caller renders empty / loading / error states itself
//! (full-pane, no split) and only calls this widget when the feed is
//! non-empty. The preview pane does not scroll independently — long
//! paragraphs are visibly cut off (the user opens the paragraph in
//! `$EDITOR` via `Enter` to read it in full).

use std::collections::HashSet;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::palette;
use crate::tui::widgets::scroll_list::{render_scroll_list, ScrollListOpts};

/// Default height of the list pane. Clamped down to the entry count
/// (so a 3-entry feed doesn't leave a half-empty list) and up to the
/// available area. Kept constant across renders so the preview pane's
/// size stays stable while the cursor moves.
pub const LIST_DEFAULT: usize = 10;

/// Render a list/preview split for a paragraph feed.
///
/// - `list_rows`: one already-styled [`ListItem`] per entry, in feed
///   order. The caller is responsible for the compact `{date} {title}
///   {badge?}` content and the multi-select `●` marker.
/// - `selected`: cursor index into the feed (clamped internally by
///   [`render_scroll_list`]).
/// - `multi_selected`: indices marked via `Space`. Unused by the
///   geometry but accepted so the caller has one less struct to thread;
///   the marker is already baked into `list_rows`.
/// - `preview_header`: distinct header lines (title · date · line
///   range · badges). Rendered with a separating rule below it.
/// - `preview_body`: wrapped paragraph body lines for the selected
///   entry. Rendered non-scrolling; overflow is clipped.
///
/// `multi_selected` is currently unused by the geometry but kept in
/// the signature so future affordances (e.g. a "N selected" footer in
/// the list pane) don't break callers. The marker glyph itself is
/// baked into `list_rows` by the caller.
#[allow(clippy::too_many_arguments)]
pub fn render_feed_split(
    frame: &mut Frame,
    area: Rect,
    list_rows: Vec<ratatui::widgets::ListItem<'_>>,
    selected: usize,
    multi_selected: &HashSet<usize>,
    preview_header: &[Line<'_>],
    preview_body: &[Line<'_>],
) {
    let _ = multi_selected; // marker is baked into list_rows by the caller
    if area.height == 0 || area.width == 0 || list_rows.is_empty() {
        return;
    }

    let n = list_rows.len();
    let list_h = LIST_DEFAULT.min(n).min(area.height as usize);
    let list_h = list_h.max(1) as u16;
    let list_area = Rect {
        height: list_h,
        ..area
    };
    let preview_area = Rect {
        y: area.y + list_h,
        height: area.height.saturating_sub(list_h),
        ..area
    };

    let opts = ScrollListOpts {
        highlight_symbol: "▶ ",
        highlight_style: Style::default()
            .fg(palette::BLACK)
            .bg(palette::PRIMARY)
            .add_modifier(Modifier::BOLD),
        scrollbar: true,
    };
    render_scroll_list(frame, list_area, list_rows, Some(selected), opts);

    if preview_area.height == 0 {
        return;
    }

    // Preview pane: a bordered block so the header reads as distinct
    // from the list, with a rule separating header from body.
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(palette::PRIMARY));
    let inner = block.inner(preview_area);
    frame.render_widget(block, preview_area);
    if inner.height == 0 {
        return;
    }

    // Header lines, then a full-width rule, then the body. The rule
    // is a single line of `─` so the header/body boundary is unambiguous
    // even when the header is multi-line.
    let mut lines: Vec<Line> = Vec::with_capacity(preview_header.len() + 1 + preview_body.len());
    for hl in preview_header {
        lines.push(hl.clone());
    }
    let rule = "─".repeat(inner.width as usize);
    lines.push(Line::from(Span::styled(
        rule,
        Style::default().fg(palette::DIM),
    )));
    for bl in preview_body {
        lines.push(bl.clone());
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::ListItem;
    use ratatui::Terminal;

    fn render_to_string(
        area: Rect,
        rows: Vec<ListItem<'_>>,
        header: &[Line<'_>],
        body: &[Line<'_>],
    ) -> String {
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).unwrap();
        let selected = HashSet::new();
        terminal
            .draw(|f| {
                render_feed_split(f, area, rows, 0, &selected, header, body);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn list_height_clamps_to_entry_count_when_small() {
        // 3 entries, default 10, tall area → list pane should be 3 rows.
        let rows: Vec<ListItem<'_>> =
            vec![ListItem::new("a"), ListItem::new("b"), ListItem::new("c")];
        let area = Rect::new(0, 0, 40, 20);
        let out = render_to_string(area, rows, &[], &[]);
        // The 4th line should be the top border of the preview block
        // (─), proving the list pane was only 3 rows tall.
        assert!(out.lines().nth(3).unwrap().contains('─'));
    }

    #[test]
    fn list_height_clamps_to_area_when_area_smaller_than_default() {
        // 50 entries but only 8 rows of area → list pane is 8, no preview.
        let rows: Vec<ListItem<'_>> = (0..50).map(|_| ListItem::new("x")).collect();
        let area = Rect::new(0, 0, 40, 8);
        let out = render_to_string(area, rows, &[], &[]);
        // No preview border should appear (preview height 0).
        assert!(!out.contains('─'));
    }

    #[test]
    fn empty_rows_is_a_noop() {
        let area = Rect::new(0, 0, 40, 20);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).unwrap();
        let selected = HashSet::new();
        // Should not panic and should render nothing.
        terminal
            .draw(|f| {
                render_feed_split(f, area, vec![], 0, &selected, &[], &[]);
            })
            .unwrap();
    }
}

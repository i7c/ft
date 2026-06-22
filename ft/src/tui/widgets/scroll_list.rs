//! Shared scrollable list widget.
//!
//! Every modal / overlay that renders a vertical list longer than its
//! area used to hand-roll its own windowing (`picker::adjust_scroll`,
//! `notes::view::compute_scroll`, the graph tree's `scroll_to_selection`)
//! — and two of them (the Related modal, the reslice preview) shipped
//! without any, so the selection could scroll off-screen with no way to
//! follow it.
//!
//! [`render_scroll_list`] centralises that: callers build their own
//! styled [`ListItem`]s (row content/colours stay local) and pass a
//! clamped `selected` index. The widget wraps ratatui's [`List`] +
//! [`ListState`] (which auto-scrolls the selection into view) and draws
//! a [`Scrollbar`] on the right edge whenever the content overflows.
//!
//! State is transient: a fresh [`ListState`] is built each render from
//! the caller's `selected` index, so callers only persist a single
//! clamped `usize` (or `Option<usize>` for selection-less viewports like
//! the reslice preview). The list still scrolls the selection into view
//! because [`List`] recomputes the offset from `selected` every frame.

#![allow(dead_code)] // wired up in the Problem-A migration commit

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{
    List, ListItem, ListState, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;

use crate::tui::palette;

/// Rendering options for [`render_scroll_list`].
pub struct ScrollListOpts<'a> {
    /// Gutter prefix drawn on the selected row; an equal-width run of
    /// spaces is reserved on every other row so content stays aligned.
    pub highlight_symbol: &'a str,
    /// Style applied to the selected row.
    pub highlight_style: Style,
    /// Draw a scrollbar on the right edge when the content overflows.
    pub scrollbar: bool,
}

impl Default for ScrollListOpts<'_> {
    fn default() -> Self {
        Self {
            highlight_symbol: "▶ ",
            highlight_style: Style::default().add_modifier(Modifier::REVERSED),
            scrollbar: true,
        }
    }
}

/// Render `items` into `area` (the content rectangle, inside any border
/// the caller already drew), keeping `selected` scrolled into view and
/// drawing a scrollbar on overflow.
///
/// Pass `selected = None` for a selection-less viewport (e.g. a preview
/// pane); in that mode no row is highlighted and the list shows from the
/// top. To keep a particular line visible in a viewport, pass its index
/// as `Some(idx)` with a subtle `highlight_style`.
pub fn render_scroll_list(
    frame: &mut Frame,
    area: Rect,
    items: Vec<ListItem<'_>>,
    selected: Option<usize>,
    opts: ScrollListOpts<'_>,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let total = items.len();
    let needs_scrollbar = opts.scrollbar && total > area.height as usize;

    // Reserve the rightmost column for the scrollbar track when the
    // content overflows, so the bar never paints over row text.
    let list_area = if needs_scrollbar {
        Rect {
            width: area.width.saturating_sub(1),
            ..area
        }
    } else {
        area
    };

    let mut state = ListState::default();
    state.select(selected);

    let list = List::new(items)
        .highlight_symbol(opts.highlight_symbol)
        .highlight_style(opts.highlight_style);
    frame.render_stateful_widget(list, list_area, &mut state);

    if needs_scrollbar {
        let scrollbar_area = Rect {
            x: area.x + area.width.saturating_sub(1),
            width: 1,
            ..area
        };
        let mut sb_state = ScrollbarState::new(total)
            .viewport_content_length(area.height as usize)
            .position(state.offset());
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::default().fg(palette::PRIMARY))
            .track_style(Style::default().fg(palette::DIM));
        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut sb_state);
    }
}

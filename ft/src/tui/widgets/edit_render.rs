//! Shared single-line input rendering for [`EditBuffer`].
//!
//! [`EditBuffer`] is the one input *model* every TUI surface uses, but
//! rendering was hand-rolled at every mount: some sites scrolled the
//! text horizontally (the tasks query bar, the create-subdir prompt),
//! most didn't, and the notes prompts drew a trailing block cursor that
//! ignored the actual cursor index. On a narrow field a long value would
//! run off the right edge with the caret pinned out of sight.
//!
//! [`render_inline_input`] centralises it: given the buffer, an optional
//! prompt prefix, and a [`CursorMode`], it computes a horizontal scroll
//! offset (via [`horizontal_scroll`]) so the caret is always visible,
//! renders the visible slice, and draws the cursor.

#![allow(dead_code)] // wired up in the Problem-B migration commit

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::widgets::EditBuffer;

/// First visible char index so a caret at `cursor` stays within a field
/// `width` cells wide. Returns `0` until the cursor would fall off the
/// right edge, then tracks it, clamped so the tail of the text doesn't
/// scroll further than necessary.
pub fn horizontal_scroll(cursor: usize, total: usize, width: usize) -> usize {
    if width == 0 || cursor < width {
        return 0;
    }
    let max_scroll = total.saturating_sub(width.saturating_sub(1));
    cursor
        .saturating_sub(width.saturating_sub(1))
        .min(max_scroll)
}

/// How the caret is drawn.
pub enum CursorMode {
    /// Inline vertical bar `│` rendered *between* characters at the
    /// cursor (consumes one cell, covers no char). Matches the fuzzy
    /// picker and the tasks query bar.
    Bar(Style),
    /// Inline block over the char at the cursor (a space block at
    /// end-of-text). Matches the create-subdir prompt.
    Block(Style),
    /// Position the terminal's hardware cursor via
    /// [`Frame::set_cursor_position`]. Used where a snapshot test asserts
    /// `get_cursor_position` (the graph query bar) or where the field
    /// shares a row with other widgets (timeblocks form).
    Hardware,
}

/// A single-line input to render.
pub struct InlineInput<'a> {
    pub buf: &'a EditBuffer,
    /// Static prompt drawn left of the text (e.g. `"> "`, `"filename: "`).
    /// Its width is subtracted from the field before scrolling.
    pub prefix: Option<Span<'a>>,
    /// Dim text shown when the buffer is empty.
    pub placeholder: Option<Span<'a>>,
    /// Style for the text glyphs.
    pub text_style: Style,
    pub cursor: CursorMode,
}

impl<'a> InlineInput<'a> {
    /// Plain input: no prefix, no placeholder, default text style.
    pub fn new(buf: &'a EditBuffer, cursor: CursorMode) -> Self {
        Self {
            buf,
            prefix: None,
            placeholder: None,
            text_style: Style::default(),
            cursor,
        }
    }

    pub fn prefix(mut self, span: Span<'a>) -> Self {
        self.prefix = Some(span);
        self
    }

    pub fn placeholder(mut self, span: Span<'a>) -> Self {
        self.placeholder = Some(span);
        self
    }

    pub fn text_style(mut self, style: Style) -> Self {
        self.text_style = style;
        self
    }
}

/// Render `input` into `area` as a single line, scrolling horizontally so
/// the caret stays visible.
pub fn render_inline_input(frame: &mut Frame, area: Rect, input: InlineInput<'_>) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let prefix_width = input.prefix.as_ref().map(|s| s.width() as u16).unwrap_or(0);
    let field_width = area.width.saturating_sub(prefix_width) as usize;

    let mut spans: Vec<Span> = Vec::new();
    if let Some(p) = input.prefix.clone() {
        spans.push(p);
    }

    let chars: Vec<char> = input.buf.text.chars().collect();

    // Empty buffer: show the placeholder (if any) and put the caret at
    // the start of the field.
    if chars.is_empty() {
        push_empty_caret(&mut spans, &input);
        if let Some(ph) = input.placeholder.clone() {
            // Bar/Block already drew one caret cell; the placeholder
            // follows it. Hardware drew nothing, so the placeholder
            // starts at the field origin.
            spans.push(ph);
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        if let CursorMode::Hardware = input.cursor {
            frame.set_cursor_position((area.x + prefix_width, area.y));
        }
        return;
    }

    let cursor = input.buf.cursor.min(chars.len());
    // A `Bar` caret occupies its own cell, so it eats one column of the
    // visible field; `Block`/`Hardware` overlay an existing cell.
    let visible_width = match input.cursor {
        CursorMode::Bar(_) => field_width.saturating_sub(1).max(1),
        _ => field_width.max(1),
    };
    let scroll = horizontal_scroll(cursor, chars.len(), visible_width);
    let visible_end = (scroll + visible_width).min(chars.len());
    let visible: Vec<char> = chars[scroll..visible_end].to_vec();
    let caret_in_visible = cursor.saturating_sub(scroll);

    match input.cursor {
        CursorMode::Bar(style) => {
            let split = caret_in_visible.min(visible.len());
            let left: String = visible[..split].iter().collect();
            let right: String = visible[split..].iter().collect();
            spans.push(Span::styled(left, input.text_style));
            spans.push(Span::styled("│", style));
            spans.push(Span::styled(right, input.text_style));
            frame.render_widget(Paragraph::new(Line::from(spans)), area);
        }
        CursorMode::Block(style) => {
            let split = caret_in_visible.min(visible.len());
            let left: String = visible[..split].iter().collect();
            spans.push(Span::styled(left, input.text_style));
            if split < visible.len() {
                spans.push(Span::styled(visible[split].to_string(), style));
                let right: String = visible[split + 1..].iter().collect();
                spans.push(Span::styled(right, input.text_style));
            } else {
                // Caret past the last visible char — block over a space.
                spans.push(Span::styled(" ", style));
            }
            frame.render_widget(Paragraph::new(Line::from(spans)), area);
        }
        CursorMode::Hardware => {
            let text: String = visible.iter().collect();
            spans.push(Span::styled(text, input.text_style));
            frame.render_widget(Paragraph::new(Line::from(spans)), area);
            let col = area.x + prefix_width + caret_in_visible as u16;
            frame.set_cursor_position((col.min(area.x + area.width.saturating_sub(1)), area.y));
        }
    }
}

/// Draw just the caret for an empty buffer (no text to split around).
fn push_empty_caret(spans: &mut Vec<Span>, input: &InlineInput<'_>) {
    match input.cursor {
        CursorMode::Bar(style) => spans.push(Span::styled("│", style)),
        CursorMode::Block(style) => spans.push(Span::styled(" ", style)),
        CursorMode::Hardware => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_scroll_until_cursor_reaches_width() {
        assert_eq!(horizontal_scroll(0, 10, 5), 0);
        assert_eq!(horizontal_scroll(4, 10, 5), 0);
    }

    #[test]
    fn scrolls_to_keep_caret_in_view() {
        // width 5, cursor at 6 → keep caret visible at the right edge.
        assert_eq!(horizontal_scroll(6, 20, 5), 2);
    }

    #[test]
    fn clamps_at_text_tail() {
        // Cursor at end of a 10-char string in a 5-wide field: the tail
        // shouldn't scroll past what's needed to show the last chars.
        let s = horizontal_scroll(10, 10, 5);
        assert_eq!(s, 10usize.saturating_sub(4));
    }

    #[test]
    fn zero_width_is_safe() {
        assert_eq!(horizontal_scroll(5, 10, 0), 0);
    }
}

//! Shared text formatting for the Search tab's paragraph feed:
//! inline-markdown span styling and column-aware word wrapping.
//! Extracted from the removed Gather tab so the tabs that outlived it
//! keep one rendering source.

use ratatui::{
    style::{Modifier, Style},
    text::Span,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::tui::palette;

/// Word-wrap one logical line to `width` columns, preserving leading
/// whitespace on the first wrapped fragment (so an indented bullet
/// still looks indented). Words longer than `width` are hard-broken on
/// grapheme boundaries; widths are measured in terminal columns (not
/// `char`s): a `📅` is one char but two columns, so char-counting would
/// let a wrapped line overflow the pane and ratatui would clip
/// mid-glyph. A `width` of 0 returns the original line unchanged to
/// avoid an infinite loop.
pub fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![line.to_string()];
    }
    // Expand tabs and drop stray control chars before measuring: a raw
    // `\t` (common in nested markdown lists) is one char that the terminal
    // expands to the next tab stop while ratatui reserves it a single
    // cell, so unsanitized tabs visibly garble the wrapped body.
    let sanitized = sanitize_display_line(line);
    let line = sanitized.as_str();
    if line.is_empty() {
        return vec![String::new()];
    }
    // Preserve any leading indent on the first wrapped fragment. Measure
    // everything in terminal columns (not `char`s): a `📅` is one char but
    // two columns, so char-counting would let a wrapped line overflow the
    // pane and ratatui would clip mid-glyph — the "garbled chars" seen
    // with emoji/CJK/accented content.
    let indent: String = line
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect::<String>();
    let indent_width = str_width(&indent);
    let body = &line[indent.len()..];
    if body.is_empty() {
        // Trailing whitespace-only line: keep the columns (chunked so we
        // never exceed width) so spacing in poetry-style content is
        // preserved.
        return chunk_by_width(line, width);
    }

    let mut out: Vec<String> = Vec::new();
    let mut current = indent.clone();
    let mut current_width = indent_width;
    for word in body.split_whitespace() {
        let word_width = str_width(word);
        if word_width > width {
            // Flush whatever's in the current buffer first, then
            // hard-break the long word across full-width chunks.
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
                current_width = 0;
            }
            for chunk in chunk_by_width(word, width) {
                out.push(chunk);
            }
            continue;
        }
        let needs_space = current_width > 0 && current_width > indent_width;
        let space_width = if needs_space { 1 } else { 0 };
        if current_width + space_width + word_width > width {
            out.push(std::mem::take(&mut current));
            current.push_str(word);
            current_width = word_width;
        } else {
            if needs_space {
                current.push(' ');
                current_width += 1;
            }
            current.push_str(word);
            current_width += word_width;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Build styled spans for one already-wrapped body line, applying
/// minimal inline-markdown styling: `[[wikilinks]]` (gold), `[text](url)`
/// markdown links (orange underlined), `**bold**` (bold), and
/// `` `code` `` (dim). Italic and strikethrough are intentionally
/// skipped — single-asterisk emphasis is ambiguous in prose (often used
/// for multiplication or footnotes).
///
/// Applied AFTER wrap, so a token split across wrap boundaries
/// degrades to plain text on both fragments. Acceptable for a feed of
/// short paragraphs.
pub fn inline_markdown_spans(line: &str) -> Vec<Span<'static>> {
    let mut out: Vec<Span<'static>> = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    let mut plain_start = 0usize;

    let flush_plain = |out: &mut Vec<Span<'static>>, plain_start: &mut usize, end: usize| {
        if end > *plain_start {
            out.push(Span::raw(line[*plain_start..end].to_string()));
        }
        *plain_start = end;
    };

    while i < bytes.len() {
        // [[wikilink]] or [[wikilink|display]]
        if bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            if let Some(end) = find_balanced(line, i, "[[", "]]") {
                flush_plain(&mut out, &mut plain_start, i);
                out.push(Span::styled(
                    line[i..end].to_string(),
                    Style::default().fg(palette::SECONDARY),
                ));
                i = end;
                plain_start = end;
                continue;
            }
        }
        // [text](url) markdown link — keep it simple: a `[` not followed
        // by `[` that has a matching `](...)`.
        if bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] != b'[' {
            if let Some(end) = find_md_link(line, i) {
                flush_plain(&mut out, &mut plain_start, i);
                out.push(Span::styled(
                    line[i..end].to_string(),
                    Style::default()
                        .fg(palette::PRIMARY)
                        .add_modifier(Modifier::UNDERLINED),
                ));
                i = end;
                plain_start = end;
                continue;
            }
        }
        // **bold**
        if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            if let Some(end) = find_balanced(line, i, "**", "**") {
                flush_plain(&mut out, &mut plain_start, i);
                out.push(Span::styled(
                    line[i..end].to_string(),
                    Style::default().add_modifier(Modifier::BOLD),
                ));
                i = end;
                plain_start = end;
                continue;
            }
        }
        // `inline code`
        if bytes[i] == b'`' {
            if let Some(end) = find_balanced(line, i, "`", "`") {
                flush_plain(&mut out, &mut plain_start, i);
                out.push(Span::styled(
                    line[i..end].to_string(),
                    Style::default().fg(palette::DIM),
                ));
                i = end;
                plain_start = end;
                continue;
            }
        }
        i += 1;
    }
    flush_plain(&mut out, &mut plain_start, bytes.len());
    out
}

/// Find the end (exclusive byte offset) of a balanced `open…close`
/// span starting at `start`. Returns `None` when no closing token is
/// found on the same line.
fn find_balanced(line: &str, start: usize, open: &str, close: &str) -> Option<usize> {
    let after_open = start + open.len();
    if after_open > line.len() {
        return None;
    }
    line[after_open..]
        .find(close)
        .map(|rel| after_open + rel + close.len())
}

/// Find the end of a `[text](url)` markdown link starting at `start`.
/// Returns `None` if either bracket isn't balanced on this line.
fn find_md_link(line: &str, start: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    if start >= bytes.len() || bytes[start] != b'[' {
        return None;
    }
    let close_text = line[start + 1..].find(']')? + start + 1;
    if close_text + 1 >= bytes.len() || bytes[close_text + 1] != b'(' {
        return None;
    }
    let close_url = line[close_text + 2..].find(')')? + close_text + 2;
    Some(close_url + 1)
}

/// Expand tabs to spaces and drop stray C0/C1 control characters so the
/// preview panes render vault prose faithfully. Terminals expand a raw
/// `\t` to the next tab stop while ratatui reserves it a single cell, so
/// unsanitized tabs — common in nested markdown lists — visibly garble
/// the wrapped body. Tabs become a fixed four spaces (deterministic, and
/// what most editors show for indent); other controls are dropped.
fn sanitize_display_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for c in line.chars() {
        match c {
            '\t' => out.push_str("    "),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Display width of `s` in terminal columns, matching ratatui's own
/// measurement (both go through `unicode-width`).
fn str_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Split `s` into chunks that each fit within `width` columns, breaking on
/// grapheme boundaries so a multi-column glyph is never sliced. A single
/// grapheme wider than `width` gets its own chunk (it cannot be split).
fn chunk_by_width(s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![s.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for g in s.graphemes(true) {
        let w = grapheme_width(g);
        if current_width + w > width && !current.is_empty() {
            out.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push_str(g);
        current_width += w;
    }
    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Column width of a single grapheme cluster. `unicode-width` measures per
/// `char`; for a cluster (e.g. a base plus combining marks, or a ZWJ emoji
/// sequence) the base carries the width and zero-width joiners/marks add 0,
/// so summing the chars' widths yields the cluster's rendered width.
fn grapheme_width(g: &str) -> usize {
    g.chars()
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{inline_markdown_spans, wrap_line};

    fn rendered_text(spans: &[ratatui::text::Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn inline_markdown_plain_passes_through() {
        let spans = inline_markdown_spans("just some prose, nothing special.");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "just some prose, nothing special.");
    }

    #[test]
    fn inline_markdown_wikilink_is_styled_and_text_preserved() {
        let spans = inline_markdown_spans("see [[Foo]] for context");
        assert_eq!(
            rendered_text(&spans),
            "see [[Foo]] for context",
            "rendered text must round-trip verbatim"
        );
        // The wikilink span exists and is colored.
        assert!(spans
            .iter()
            .any(|s| s.content == "[[Foo]]" && s.style.fg == Some(crate::tui::palette::SECONDARY)));
    }

    #[test]
    fn inline_markdown_bold_and_code_and_md_link() {
        let line = "**urgent**: read `config.toml` and see [docs](https://x.dev)";
        let spans = inline_markdown_spans(line);
        assert_eq!(rendered_text(&spans), line);
        let bold = spans.iter().any(|s| s.content == "**urgent**");
        let code = spans.iter().any(|s| s.content == "`config.toml`");
        let link = spans.iter().any(|s| s.content == "[docs](https://x.dev)");
        assert!(bold, "missing bold span: {spans:?}");
        assert!(code, "missing code span: {spans:?}");
        assert!(link, "missing md-link span: {spans:?}");
    }

    #[test]
    fn inline_markdown_unterminated_token_stays_plain() {
        // No closing `]]` → must not panic and must not eat the rest of the line.
        let spans = inline_markdown_spans("see [[Foo without close");
        assert_eq!(rendered_text(&spans), "see [[Foo without close");
        // Whole line is one plain span (no styled match found).
        assert!(spans
            .iter()
            .all(|s| s.style.fg.is_none() && s.style.add_modifier.is_empty()));
    }

    #[test]
    fn wrap_line_short_fits_on_one_line() {
        assert_eq!(wrap_line("hello world", 40), vec!["hello world"]);
    }

    #[test]
    fn wrap_line_breaks_on_word_boundary() {
        let out = wrap_line("the quick brown fox jumps over the lazy dog", 20);
        assert_eq!(
            out,
            vec!["the quick brown fox", "jumps over the lazy", "dog"]
        );
        for line in &out {
            assert!(line.chars().count() <= 20, "overflow: {line:?}");
        }
    }

    #[test]
    fn wrap_line_preserves_leading_indent_on_first_fragment() {
        // Continuation lines start at column 0, matching how the
        // journal already renders bullet bodies. The indent is kept on
        // the first fragment so the bullet visually leads the wrap.
        let out = wrap_line("  - this is a bullet point that wraps", 20);
        assert_eq!(out[0], "  - this is a bullet");
        assert_eq!(out[1], "point that wraps");
    }

    #[test]
    fn wrap_line_hard_breaks_word_longer_than_width() {
        let out = wrap_line("supercalifragilisticexpialidocious tail", 10);
        // First three chunks are 10-char slices of the long word;
        // remainder + tail wrap accordingly.
        assert_eq!(out[0], "supercalif");
        assert_eq!(out[1], "ragilistic");
        assert_eq!(out[2], "expialidoc");
        assert_eq!(out[3], "ious");
        assert_eq!(out[4], "tail");
    }

    #[test]
    fn wrap_line_empty_input_yields_single_empty_line() {
        assert_eq!(wrap_line("", 20), vec![""]);
    }

    #[test]
    fn wrap_line_width_zero_is_a_no_op() {
        // Defensive: degenerate width should not loop forever.
        assert_eq!(
            wrap_line("anything goes here", 0),
            vec!["anything goes here"]
        );
    }

    /// Display width of a wrapped line, matching ratatui's own measure.
    fn cols(s: &str) -> usize {
        unicode_width::UnicodeWidthStr::width(s)
    }

    #[test]
    fn wrap_line_expands_tabs_and_drops_control_chars() {
        // Nested markdown list bodies carry raw tabs; a terminal expands
        // them to a tab stop while ratatui gives them a single cell, so
        // unsanitized tabs garble the preview. They must become spaces,
        // and stray controls (here a carriage return) must be dropped.
        let out = wrap_line("\t\t- deep item\r", 40);
        assert_eq!(out, vec!["        - deep item"]);
        assert!(
            !out.iter().any(|l| l.contains('\t') || l.contains('\r')),
            "no raw control chars may survive: {out:?}"
        );
    }

    #[test]
    fn wrap_line_measures_wide_glyphs_by_columns_not_chars() {
        // `📅` is one `char` but two terminal columns. Counting chars let
        // wrapped lines run wider than the pane, so ratatui clipped the
        // overflow mid-glyph — the "garbled chars" bug. Every fragment
        // must stay within the column budget, and no token may be lost.
        let line = "eigen 📅 aa 📅 bb 📅 cc 📅 dd 📅 ee 📅 ff 📅 gg";
        let out = wrap_line(line, 20);
        for frag in &out {
            assert!(cols(frag) <= 20, "fragment overflows 20 cols: {frag:?}");
        }
        // Every space-separated token survives the wrap (join the
        // fragments, re-split on whitespace, compare token multisets).
        let mut got: Vec<&str> = out.iter().flat_map(|f| f.split_whitespace()).collect();
        let mut want: Vec<&str> = line.split_whitespace().collect();
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(got, want, "a token was dropped or garbled by wrapping");
    }

    #[test]
    fn wrap_line_hard_break_keeps_wide_glyphs_within_width() {
        // A long unbroken run of wide glyphs must hard-break on grapheme
        // boundaries without any chunk exceeding the column budget.
        let word = "📅📅📅📅📅📅📅📅"; // 8 glyphs × 2 cols = 16 cols
        let out = wrap_line(word, 6);
        for chunk in &out {
            assert!(cols(chunk) <= 6, "chunk overflows 6 cols: {chunk:?}");
        }
        // Reassembled, the glyphs round-trip verbatim (none sliced).
        assert_eq!(out.concat(), word);
    }
}

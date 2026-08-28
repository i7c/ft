//! Markdown structure parsers used by the search layer.
//!
//! Today this module only ships a heading extractor used by
//! [`crate::search`]. The task line parser lives in [`crate::task::emoji`] —
//! the two are kept separate because they answer different questions
//! (`- [ ]` lines vs `#` headings) and a future contributor wiring up,
//! say, a backlink resolver should be able to add markdown helpers here
//! without touching the task code.

/// A markdown heading found inside a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub text: String,
    /// ATX level — 1 for `#`, 2 for `##`, … up to 6.
    pub level: u8,
    /// 1-indexed line number within the source file.
    pub line: usize,
}

/// Extract every ATX heading (`#` … `######`) from `content`.
///
/// Headings inside fenced code blocks (``` and ~~~), inside indented
/// code blocks (4-space indent at column 0), and inside the leading
/// YAML/TOML frontmatter (the `---` block at the very top of the file)
/// are skipped. Setext headings (`===` / `---` underlines) are out of
/// scope — they're rare in modern Obsidian vaults.
pub fn extract_headings(content: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut state = LineSkipState::new();

    for (idx, line) in content.lines().enumerate() {
        let lineno = idx + 1;
        if state.skip_line(line) {
            continue;
        }
        if let Some(h) = parse_atx(line, lineno) {
            out.push(h);
        }
    }
    out
}

/// A paragraph-sized section of markdown content.
///
/// Boundaries: one or more blank lines, a Markdown heading line (which
/// itself starts a new paragraph), or a horizontal-rule separator
/// (`--` or more dashes on a line by themselves). Frontmatter and
/// fenced / indented code blocks are skipped via [`LineSkipState`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Paragraph {
    /// 1-indexed line number of the first line in the paragraph.
    pub line_start: u32,
    /// 1-indexed line number of the last line in the paragraph.
    pub line_end: u32,
    /// Paragraph lines joined with `\n` — no trailing newline.
    pub text: String,
}

/// Extract every paragraph from `content` in document order.
///
/// See [`Paragraph`] for boundary rules.
pub fn extract_paragraphs(content: &str) -> Vec<Paragraph> {
    let mut out = Vec::new();
    let mut state = LineSkipState::new();
    let mut buf: Option<(u32, u32, Vec<String>)> = None;

    fn flush(out: &mut Vec<Paragraph>, buf: &mut Option<(u32, u32, Vec<String>)>) {
        if let Some((line_start, line_end, lines)) = buf.take() {
            out.push(Paragraph {
                line_start,
                line_end,
                text: lines.join("\n"),
            });
        }
    }

    for (idx, line) in content.lines().enumerate() {
        let lineno = (idx + 1) as u32;
        if state.skip_line(line) {
            flush(&mut out, &mut buf);
            continue;
        }
        if line.trim().is_empty() {
            flush(&mut out, &mut buf);
            continue;
        }
        if is_atx_heading(line) {
            flush(&mut out, &mut buf);
            buf = Some((lineno, lineno, vec![line.to_string()]));
            continue;
        }
        if is_rule_separator(line) {
            flush(&mut out, &mut buf);
            continue;
        }
        match &mut buf {
            Some((_, end, lines)) => {
                *end = lineno;
                lines.push(line.to_string());
            }
            None => {
                buf = Some((lineno, lineno, vec![line.to_string()]));
            }
        }
    }
    flush(&mut out, &mut buf);
    out
}

fn is_atx_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return false;
    }
    let after = &trimmed[level..];
    after.is_empty() || after.starts_with(|c: char| c.is_whitespace())
}

/// Horizontal-rule separator: a line whose non-whitespace content is
/// two or more `-` characters. CommonMark's stricter rule (3+ matching
/// `-`/`*`/`_`) isn't enforced — we accept the wider Obsidian-friendly
/// form including the spec's `--` separator.
pub(crate) fn is_rule_separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 2 && trimmed.chars().all(|c| c == '-')
}

/// Line-buffer helpers shared by `task::ops`, `timeblock::doc`, and any
/// other module that needs to splice into a markdown file.
///
/// All functions are line-oriented (work on `Vec<String>`). The
/// canonical round-trip is:
/// 1. `let mut lines = lines::split(&content);`
/// 2. ... edit lines in place ...
/// 3. `let new_content = lines::join_with_newline(&lines);`
pub mod lines {
    use std::io;
    use std::path::Path;

    /// Split `content` into newline-stripped lines (`\n` and `\r\n` both
    /// trimmed). Empty input produces an empty vector — *not* a vector
    /// with one empty element.
    pub fn split(content: &str) -> Vec<String> {
        if content.is_empty() {
            Vec::new()
        } else {
            content
                .split_inclusive('\n')
                .map(|s| s.trim_end_matches('\n').trim_end_matches('\r').to_string())
                .collect()
        }
    }

    /// Join `lines` with `\n` and append a trailing `\n`. Empty input
    /// produces an empty string.
    pub fn join_with_newline(lines: &[String]) -> String {
        if lines.is_empty() {
            String::new()
        } else {
            let mut s = lines.join("\n");
            s.push('\n');
            s
        }
    }

    /// Read `path` to string, treating `NotFound` as an empty file. Any
    /// other I/O error is returned verbatim — callers wrap into their
    /// own error type.
    pub fn read_or_empty(path: &Path) -> io::Result<String> {
        match std::fs::read_to_string(path) {
            Ok(s) => Ok(s),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e),
        }
    }

    /// Parse an ATX heading line, returning `(level, text)` when the
    /// line is a heading. Level is the number of leading `#` chars
    /// (1..=6). The required space after the hashes is consumed.
    pub fn parse_heading(line: &str) -> Option<(usize, &str)> {
        let trimmed = line.trim_start();
        let hashes = trimmed.bytes().take_while(|b| *b == b'#').count();
        if hashes == 0 || hashes > 6 {
            return None;
        }
        let after = &trimmed[hashes..];
        let after = after.strip_prefix(' ')?;
        Some((hashes, after.trim_end()))
    }

    /// Find the first heading whose text exactly matches `target`.
    /// Returns `(line_index, level)` where the index is 0-based.
    pub fn find_heading(lines: &[String], target: &str) -> Option<(usize, usize)> {
        for (i, l) in lines.iter().enumerate() {
            if let Some((level, text)) = parse_heading(l) {
                if text == target {
                    return Some((i, level));
                }
            }
        }
        None
    }

    /// Index *just after* the last line of the section opened by
    /// `heading_idx` at `level`. The section ends at the next heading
    /// whose level is `<= level`, or at end of file. Trailing blank
    /// lines belong to the *next* section, not this one — we step
    /// back over them so callers inserting at the boundary land
    /// before the blanks.
    pub fn section_end(lines: &[String], heading_idx: usize, level: usize) -> usize {
        let mut end = lines.len();
        for (i, l) in lines.iter().enumerate().skip(heading_idx + 1) {
            if let Some((lvl, _)) = parse_heading(l) {
                if lvl <= level {
                    end = i;
                    break;
                }
            }
        }
        while end > heading_idx + 1 && lines[end - 1].is_empty() {
            end -= 1;
        }
        end
    }
}

/// Tracks frontmatter / fenced code block / indented code block state
/// across a line-by-line scan of a markdown file. Both the heading
/// extractor (above) and the link parser (`crate::graph::parser`) use
/// this so the "what counts as content vs. structure" rules stay in
/// one place.
///
/// Inline code spans (single/double/triple backticks within a line)
/// are *not* handled here — they're a within-line concern that each
/// consumer handles with its own intra-line scanner. This struct only
/// answers the per-line question "should I skip this whole line?"
#[derive(Debug, Default)]
pub(crate) struct LineSkipState {
    /// Are we still inside the leading frontmatter block? Set on the
    /// first line if it's `---`; cleared when we hit the closing `---`.
    in_frontmatter: bool,
    /// Have we seen any line yet? Used to detect the frontmatter opener
    /// — frontmatter only counts when `---` is the very first line.
    started: bool,
    /// Fence character active for a fenced code block: `'`'` or `'~'`.
    /// `None` when we're not inside a fenced block.
    fence: Option<char>,
    /// Number of fence chars the opener used. The closer needs to match
    /// or exceed this count (per CommonMark).
    fence_len: usize,
    /// Was the previous line blank (or the document start)? An indented
    /// code block can only *begin* at such a block boundary — per
    /// CommonMark it cannot interrupt a paragraph or a list item's
    /// content. Initialised `true` so a code block at the very top of a
    /// file is still recognised.
    prev_blank: bool,
    /// Are we inside an indented code block? Set when one begins at a
    /// block boundary; cleared by a non-blank, non-indented line.
    in_indent_code: bool,
    /// Was the most recently advanced line code (fence delimiter,
    /// inside a fence, or an indented-code-block line)? Consumers that
    /// need to distinguish "code" from other structural lines (the
    /// export driver) read this after [`LineSkipState::skip_line`].
    last_code: bool,
    /// Was the most recently advanced line the *opening* delimiter of
    /// a fenced code block? True only on the opener line itself;
    /// false on closing delimiters, content inside fences, and
    /// non-fence lines. Consumers that need to distinguish "opened a
    /// fence" from "inside a fence" (the export driver, for fence
    /// normalization) read this after [`LineSkipState::skip_line`].
    last_opened_fence: bool,
}

impl LineSkipState {
    pub(crate) fn new() -> Self {
        Self {
            prev_blank: true,
            ..Self::default()
        }
    }

    /// Advance one line. Returns `true` when this line is structural
    /// (frontmatter delimiter, frontmatter body, code-fence delimiter,
    /// inside a fenced code block, or an indented code block) and
    /// should be skipped by the consumer; `false` when this line
    /// carries content the consumer should examine.
    pub(crate) fn skip_line(&mut self, line: &str) -> bool {
        let is_blank = line.trim().is_empty();
        let result = self.classify(line, is_blank);
        self.prev_blank = is_blank;
        result
    }

    /// Whether the most recently [`skip_line`](LineSkipState::skip_line)'d
    /// line was code: a fence delimiter, a line inside a fence, or an
    /// indented-code-block line. Frontmatter and plain content lines
    /// return `false`.
    pub(crate) fn last_was_code(&self) -> bool {
        self.last_code
    }

    /// Whether the most recently [`skip_line`](LineSkipState::skip_line)'d
    /// line opened a fenced code block (its opening delimiter).
    pub(crate) fn opened_fence(&self) -> bool {
        self.last_opened_fence
    }

    /// The fence char active *after* the most recently
    /// [`skip_line`](LineSkipState::skip_line)'d line: `'`'` or `'~'`
    /// when the line is inside a fenced block (including the opening
    /// delimiter line), `None` otherwise — plain lines, indented code,
    /// and the closing delimiter line (which ends the block).
    pub(crate) fn fence_char(&self) -> Option<char> {
        self.fence
    }

    fn classify(&mut self, line: &str, is_blank: bool) -> bool {
        self.last_opened_fence = false;
        // Frontmatter handling: only relevant on line 1 and during the
        // block. CommonMark doesn't define frontmatter; we follow the
        // Obsidian / Jekyll convention of a `---` block at the very top.
        if !self.started {
            self.started = true;
            if line.trim_end() == "---" {
                self.in_frontmatter = true;
                self.last_code = false;
                return true;
            }
            self.last_code = false;
        } else if self.in_frontmatter {
            if line.trim_end() == "---" || line.trim_end() == "..." {
                self.in_frontmatter = false;
            }
            self.last_code = false;
            return true;
        }

        // Fenced code blocks: opening fence pattern is N≥3 of `'`'` or
        // `'~'` chars at the start of the line (possibly preceded by up
        // to 3 spaces of indent, per CommonMark — we accept any leading
        // whitespace for robustness).
        let trimmed = line.trim_start();
        if let Some(fence_char) = self.fence {
            // Inside a fence — only the matching close fence ends it.
            if let Some((c, n)) = leading_fence(trimmed) {
                // Per CommonMark a closing fence is `n >= fence_len`
                // fence chars followed by nothing but spaces/tabs — a
                // line like ` ```js ` inside a fence is content, not a
                // closer.
                if c == fence_char
                    && n >= self.fence_len
                    && trimmed[n..].chars().all(|c| c == ' ' || c == '\t')
                {
                    self.fence = None;
                    self.fence_len = 0;
                }
            }
            self.last_code = true;
            return true;
        }
        if let Some((c, n)) = leading_fence(trimmed) {
            self.fence = Some(c);
            self.fence_len = n;
            self.last_code = true;
            self.last_opened_fence = true;
            return true;
        }

        // Indented code block: 4+ leading spaces (or a tab). Per
        // CommonMark such a block cannot *interrupt* a paragraph or a
        // list item's content — it only begins at a block boundary
        // (after a blank line or at the document start). So an indented
        // line that continues preceding content (e.g. a nested list
        // item) is treated as content, not code. Once a block has begun,
        // it continues through blank and further-indented lines until a
        // non-blank, non-indented line ends it.
        if self.in_indent_code {
            if is_blank || starts_with_indent(line, 4) {
                self.last_code = true;
                return true;
            }
            self.in_indent_code = false;
            self.last_code = false;
        } else if self.prev_blank && !is_blank && starts_with_indent(line, 4) {
            self.in_indent_code = true;
            self.last_code = true;
            return true;
        }

        self.last_code = false;
        false
    }
}

/// True when `line` is a blockquote continuation. Blockquote lines
/// start with `>` after optional whitespace, matching both simple
/// blockquotes and Obsidian callout syntax (`> [!note]`).
pub(crate) fn is_blockquote_line(line: &str) -> bool {
    line.trim_start().starts_with('>')
}

/// Detect a fenced code block opener / closer at the start of `s`. Returns
/// the fence char (`'`'` or `'~'`) and the number of consecutive fence
/// chars when 3 or more are present, otherwise `None`.
pub(crate) fn leading_fence(s: &str) -> Option<(char, usize)> {
    let first = s.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let n = s.chars().take_while(|c| *c == first).count();
    (n >= 3).then_some((first, n))
}

/// True if `line` starts with at least `n` columns of whitespace (a tab
/// counts as advancing to the next multiple of 4, per CommonMark).
fn starts_with_indent(line: &str, n: usize) -> bool {
    let mut col = 0usize;
    for c in line.chars() {
        match c {
            ' ' => col += 1,
            '\t' => col = (col / 4 + 1) * 4,
            _ => return col >= n,
        }
        if col >= n {
            return true;
        }
    }
    false
}

/// Leading whitespace of `line` as `(width_in_columns, byte_offset)`, where
/// `byte_offset` points at the first non-space/tab char (`line.len()` when
/// the line is all whitespace). Tabs advance to the next multiple of 4
/// columns, per the same rule as [`starts_with_indent`].
pub(crate) fn leading_ws(line: &str) -> (usize, usize) {
    let mut col = 0usize;
    for (i, c) in line.char_indices() {
        match c {
            ' ' => col += 1,
            '\t' => col = (col / 4 + 1) * 4,
            _ => return (col, i),
        }
    }
    (col, line.len())
}

/// True when `rest` — the part of a line after its leading whitespace —
/// starts a list item: a `-`/`*`/`+` bullet or an ordered `N.` marker,
/// each followed by whitespace. Shared by [`ListDepthTracker`] (depth
/// derivation) and the export driver's Slack re-indent (defensive
/// marker re-check), so both agree on what counts as a list item.
pub(crate) fn is_list_item_marker(rest: &str) -> bool {
    let mut chars = rest.chars();
    match chars.next() {
        Some('-' | '*' | '+') => matches!(chars.next(), Some(' ') | Some('\t')),
        Some(c) if c.is_ascii_digit() => {
            let digits = chars
                .as_str()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .count();
            let after_digits = &chars.as_str()[digits..];
            let mut after_digits = after_digits.chars();
            after_digits.next() == Some('.')
                && matches!(after_digits.next(), Some(' ') | Some('\t'))
        }
        _ => false,
    }
}

/// Tracks list-item nesting depth across consecutive lines, for
/// consumers that need a per-line level (the export driver, for the
/// Slack target's 4-space-per-level re-indentation).
///
/// The tracker is deliberately **not** code-aware: the caller advances
/// it only on non-code lines (fence content and indented code blocks
/// are opaque and must not feed the stack).
///
/// Depth is derived from a stack of source-indent widths, one per open
/// level. An item indented deeper than its predecessor nests one level;
/// equal indentation stays at the same level; lesser indentation moves
/// up to the matching level. A non-list, non-blank line with **no**
/// leading whitespace resets the stack — per CommonMark such a line
/// (heading, paragraph, blockquote) interrupts the list, so a following
/// item starts a new top-level list. Blank lines do not reset (loose
/// lists continue). A first-line item with a nonzero indent (e.g. a
/// range export starting mid-list) is treated as a new top level.
#[derive(Debug, Default)]
pub(crate) struct ListDepthTracker {
    /// Source indent widths (in columns) of the currently open levels,
    /// deepest last. Empty = no list in progress.
    stack: Vec<usize>,
}

impl ListDepthTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Advance one non-code line. Returns `Some(depth)` when `line` is
    /// a list item (0 = top level), `None` otherwise.
    pub(crate) fn advance(&mut self, line: &str) -> Option<usize> {
        let (w, off) = leading_ws(line);
        let rest = &line[off..];
        if is_list_item_marker(rest) {
            let depth = match self.stack.last() {
                None => {
                    self.stack.push(w);
                    0
                }
                Some(&top) if w > top => {
                    self.stack.push(w);
                    self.stack.len() - 1
                }
                Some(&top) if w == top => self.stack.len() - 1,
                Some(_) => {
                    // w < top: pop to the matching level; a width
                    // between two levels starts a new level; an empty
                    // stack starts a new top level.
                    while self.stack.last().is_some_and(|&l| l > w) {
                        self.stack.pop();
                    }
                    match self.stack.last() {
                        None => {
                            self.stack.push(w);
                            0
                        }
                        Some(&l) if l == w => self.stack.len() - 1,
                        Some(_) => {
                            self.stack.push(w);
                            self.stack.len() - 1
                        }
                    }
                }
            };
            Some(depth)
        } else {
            if w == 0 && !line.trim().is_empty() {
                self.stack.clear();
            }
            None
        }
    }
}

/// Parse an ATX heading from `line` if it matches the pattern; `lineno`
/// is the 1-indexed source line.
pub(crate) fn parse_atx(line: &str, lineno: usize) -> Option<Heading> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let after = &trimmed[level..];
    // CommonMark requires a space or end-of-line after the `#` run.
    if !after.is_empty() && !after.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let mut text = after.trim().to_string();
    // CommonMark: closing `#`s are optional and stripped (along with the
    // single space that separates them from the heading text).
    while text.ends_with('#') {
        text.pop();
    }
    let text = text.trim_end().to_string();
    Some(Heading {
        text,
        level: level as u8,
        line: lineno,
    })
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opened_fence_and_fence_char_sequence() {
        // `opened_fence` is true only on the opening delimiter;
        // `fence_char` is the active fence after the line.
        let mut s = LineSkipState::new();

        s.skip_line("before");
        assert!(!s.opened_fence());
        assert_eq!(s.fence_char(), None);

        s.skip_line("```rust"); // opens
        assert!(s.opened_fence());
        assert_eq!(s.fence_char(), Some('`'));

        s.skip_line("```js"); // content: 3 backticks + text is not a closer
        assert!(!s.opened_fence());
        assert_eq!(s.fence_char(), Some('`'));

        s.skip_line("code"); // content
        assert!(!s.opened_fence());
        assert_eq!(s.fence_char(), Some('`'));

        s.skip_line("```"); // closes
        assert!(!s.opened_fence());
        assert_eq!(s.fence_char(), None);

        s.skip_line("~~~"); // opens a tilde fence
        assert!(s.opened_fence());
        assert_eq!(s.fence_char(), Some('~'));

        s.skip_line("~~~"); // closes it
        assert!(!s.opened_fence());
        assert_eq!(s.fence_char(), None);
    }

    #[test]
    fn opened_fence_false_for_plain_and_indented_code() {
        let mut s = LineSkipState::new();
        s.skip_line(""); // blank boundary
        s.skip_line("    indented");
        assert!(!s.opened_fence());
        assert_eq!(s.fence_char(), None);
    }

    #[test]
    fn extracts_atx_levels_one_through_six() {
        let body = "\
# H1
## H2
### H3
#### H4
##### H5
###### H6
####### not a heading
";
        let headings = extract_headings(body);
        assert_eq!(headings.len(), 6);
        for (i, h) in headings.iter().enumerate() {
            assert_eq!(h.level as usize, i + 1);
            assert_eq!(h.text, format!("H{}", i + 1));
            assert_eq!(h.line, i + 1);
        }
    }

    #[test]
    fn skips_headings_in_fenced_code_blocks_backticks() {
        let body = "\
# Real heading
```rust
# fake heading inside backtick fence
## also fake
```
## Real again
";
        let headings = extract_headings(body);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "Real heading");
        assert_eq!(headings[1].text, "Real again");
        assert_eq!(headings[1].line, 6);
    }

    #[test]
    fn skips_headings_in_fenced_code_blocks_tildes() {
        let body = "\
~~~
# fake
~~~
# real
";
        let headings = extract_headings(body);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].text, "real");
    }

    #[test]
    fn skips_indented_code_blocks() {
        // NB: don't use `"\<newline>"` continuation here — it eats the
        // leading whitespace of the next line, defeating the test.
        let body = "    # not a heading (4-space indent)\n\
                    \t# also not a heading (tab indent)\n\
                    # real heading\n";
        let headings = extract_headings(body);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].text, "real heading");
    }

    #[test]
    fn skips_frontmatter_block() {
        let body = "\
---
title: Foo
# this is yaml, not a heading
---
# Actual heading
";
        let headings = extract_headings(body);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].text, "Actual heading");
        assert_eq!(headings[0].line, 5);
    }

    #[test]
    fn frontmatter_only_counts_at_file_top() {
        let body = "\
some prose
---
title: not frontmatter
---
# heading
";
        let headings = extract_headings(body);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].text, "heading");
    }

    #[test]
    fn rejects_hash_without_space() {
        let body = "\
#nospace not a heading
# spaced is a heading
";
        let headings = extract_headings(body);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].text, "spaced is a heading");
    }

    #[test]
    fn strips_trailing_hashes() {
        let body = "\
# Hello ###
## Goodbye ##
";
        let headings = extract_headings(body);
        assert_eq!(headings[0].text, "Hello");
        assert_eq!(headings[1].text, "Goodbye");
    }

    #[test]
    fn empty_input_returns_empty_vec() {
        assert_eq!(extract_headings(""), Vec::<Heading>::new());
    }

    #[test]
    fn heading_with_no_text_is_kept_as_empty_string() {
        let body = "# \n## also empty\n";
        let headings = extract_headings(body);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "");
        assert_eq!(headings[1].text, "also empty");
    }

    // ── extract_paragraphs ─────────────────────────────────────────────

    fn p(line_start: u32, line_end: u32, text: &str) -> Paragraph {
        Paragraph {
            line_start,
            line_end,
            text: text.to_string(),
        }
    }

    #[test]
    fn paragraphs_empty_input() {
        assert_eq!(extract_paragraphs(""), Vec::<Paragraph>::new());
    }

    #[test]
    fn paragraphs_single_paragraph() {
        assert_eq!(
            extract_paragraphs("only line\n"),
            vec![p(1, 1, "only line")]
        );
    }

    #[test]
    fn paragraphs_blank_line_boundary() {
        let body = "line one\nline two\n\nline three\n";
        assert_eq!(
            extract_paragraphs(body),
            vec![p(1, 2, "line one\nline two"), p(4, 4, "line three")]
        );
    }

    #[test]
    fn paragraphs_multiple_blank_lines_collapse() {
        let body = "a\n\n\n\nb\n";
        assert_eq!(extract_paragraphs(body), vec![p(1, 1, "a"), p(5, 5, "b")]);
    }

    #[test]
    fn paragraphs_heading_boundary() {
        let body = "intro text\n## Section\nbody\n";
        assert_eq!(
            extract_paragraphs(body),
            vec![p(1, 1, "intro text"), p(2, 3, "## Section\nbody")]
        );
    }

    #[test]
    fn paragraphs_consecutive_headings_each_start_paragraph() {
        let body = "## H1\n### H2\nbody\n";
        assert_eq!(
            extract_paragraphs(body),
            vec![p(1, 1, "## H1"), p(2, 3, "### H2\nbody")]
        );
    }

    #[test]
    fn paragraphs_rule_separator_double_dash() {
        let body = "a\n--\nb\n";
        assert_eq!(extract_paragraphs(body), vec![p(1, 1, "a"), p(3, 3, "b")]);
    }

    #[test]
    fn paragraphs_rule_separator_triple_dash() {
        let body = "a\n---\nb\n";
        assert_eq!(extract_paragraphs(body), vec![p(1, 1, "a"), p(3, 3, "b")]);
    }

    #[test]
    fn paragraphs_skip_frontmatter() {
        let body = "---\ntitle: Foo\n---\nbody\n";
        assert_eq!(extract_paragraphs(body), vec![p(4, 4, "body")]);
    }

    #[test]
    fn paragraphs_skip_fenced_code_block() {
        let body = "before\n\n```rust\nlet x = 1;\nlet y = 2;\n```\n\nafter\n";
        assert_eq!(
            extract_paragraphs(body),
            vec![p(1, 1, "before"), p(8, 8, "after")]
        );
    }

    #[test]
    fn paragraphs_trailing_blank_lines_ignored() {
        let body = "a\n\n\n";
        assert_eq!(extract_paragraphs(body), vec![p(1, 1, "a")]);
    }

    #[test]
    fn paragraphs_no_trailing_newline() {
        let body = "single";
        assert_eq!(extract_paragraphs(body), vec![p(1, 1, "single")]);
    }

    #[test]
    fn paragraphs_heading_alone() {
        let body = "## Just a heading\n\nnext\n";
        assert_eq!(
            extract_paragraphs(body),
            vec![p(1, 1, "## Just a heading"), p(3, 3, "next")]
        );
    }

    #[test]
    fn paragraphs_nested_list_stays_one_block() {
        // Regression: indented nested list items (4-space / tab — the
        // Obsidian default) must NOT be mistaken for an indented code
        // block. An indented code block can only begin at a block
        // boundary, not interrupt the list/paragraph started by `# Foo`.
        // The whole heading-through-`Bar` run is one paragraph; before
        // the fix it split into [1-2] and [5-5] with the indented lines
        // dropped, so `[[Foo]]`'s two mentions landed in two blocks.
        for indent in ["    ", "\t"] {
            let body = format!(
                "# [[Foo]]\n- A\n{i}- B\n{i}- C\n- [[Foo]]\n{i}- Bar\n\nXYZ\n",
                i = indent
            );
            let ps = extract_paragraphs(&body);
            assert_eq!(
                ps.len(),
                2,
                "indent {indent:?} should yield 2 paragraphs, got {ps:#?}"
            );
            assert_eq!((ps[0].line_start, ps[0].line_end), (1, 6));
            assert!(
                ps[0].text.matches("[[Foo]]").count() == 2,
                "both [[Foo]] mentions belong to one block: {:?}",
                ps[0].text
            );
            assert_eq!(ps[1], p(8, 8, "XYZ"));
        }
    }

    #[test]
    fn paragraphs_real_indented_code_block_after_blank_still_skipped() {
        // The fix must not regress genuine indented code blocks, which
        // begin at a block boundary (after a blank line) and span their
        // indented + blank lines.
        let body = "intro\n\n    code one\n    code two\n\nafter\n";
        assert_eq!(
            extract_paragraphs(body),
            vec![p(1, 1, "intro"), p(6, 6, "after")]
        );
    }

    // ── ListDepthTracker ────────────────────────────────────────────

    /// Advance a fresh tracker over every line, returning the depth
    /// each line was assigned.
    fn depths(lines: &[&str]) -> Vec<Option<usize>> {
        let mut t = ListDepthTracker::new();
        lines.iter().map(|l| t.advance(l)).collect()
    }

    #[test]
    fn list_depth_two_space_source_walk() {
        // The user's case: 2-space-per-level source indentation.
        assert_eq!(
            depths(&["- foo", "  - bar", "    - lol", "- baz"]),
            vec![Some(0), Some(1), Some(2), Some(0)]
        );
    }

    #[test]
    fn list_depth_four_space_source_walk() {
        // Already-4-space sources yield the same levels, so re-indent
        // is idempotent.
        assert_eq!(
            depths(&["- foo", "    - bar", "        - lol"]),
            vec![Some(0), Some(1), Some(2)]
        );
    }

    #[test]
    fn list_depth_deep_nesting_scales_by_level() {
        assert_eq!(
            depths(&["- a", "  - b", "    - c", "      - d"]),
            vec![Some(0), Some(1), Some(2), Some(3)]
        );
    }

    #[test]
    fn list_depth_pop_to_matching_level() {
        assert_eq!(
            depths(&["- a", "  - b", "    - c", "  - d", "- e"]),
            vec![Some(0), Some(1), Some(2), Some(1), Some(0)]
        );
    }

    #[test]
    fn list_depth_all_marker_kinds_alike() {
        assert_eq!(
            depths(&["- one", "  * two", "    + three", "      1. four"]),
            vec![Some(0), Some(1), Some(2), Some(3)]
        );
        assert_eq!(depths(&["10. ten"]), vec![Some(0)]);
    }

    #[test]
    fn list_depth_non_markers_return_none() {
        assert_eq!(
            depths(&["text", "> quote", "-foo", "1.4 prose"]),
            vec![None, None, None, None]
        );
        // A bullet with no content after the marker is not an item by
        // our detection (needs whitespace after the marker).
        assert_eq!(depths(&["-"]), vec![None]);
    }

    #[test]
    fn list_depth_unindented_content_line_resets() {
        // Heading interrupts the list — the later indented item starts
        // a fresh top-level list.
        assert_eq!(
            depths(&["- a", "# H", "  - b"]),
            vec![Some(0), None, Some(0)]
        );
        // Unindented paragraph and blockquote interrupt too.
        assert_eq!(
            depths(&["- a", "text", "  - b"]),
            vec![Some(0), None, Some(0)]
        );
        assert_eq!(
            depths(&["- a", "> quote", "  - b"]),
            vec![Some(0), None, Some(0)]
        );
    }

    #[test]
    fn list_depth_blank_lines_do_not_reset() {
        assert_eq!(
            depths(&["- a", "", "  - b", "- c"]),
            vec![Some(0), None, Some(1), Some(0)]
        );
        // Whitespace-only lines are blank too.
        assert_eq!(
            depths(&["- a", "   ", "  - b"]),
            vec![Some(0), None, Some(1)]
        );
    }

    #[test]
    fn list_depth_indented_continuation_keeps_stack() {
        // A 2-space-indented non-marker line is item content, not an
        // interruption — the next item still nests under the parent.
        assert_eq!(
            depths(&["- a", "  continuation", "  - b"]),
            vec![Some(0), None, Some(1)]
        );
    }

    #[test]
    fn list_depth_tabs_count_as_four_columns() {
        assert_eq!(depths(&["- a", "\t- b"]), vec![Some(0), Some(1)]);
        // A tab (4 cols) after a 2-space item is deeper by one level.
        assert_eq!(
            depths(&["- a", "  - b", "\t- c"]),
            vec![Some(0), Some(1), Some(2)]
        );
    }

    #[test]
    fn list_depth_indented_fragment_starts_at_top() {
        // A range export starting mid-list: the first item is the new
        // top level; a later 0-indent item pops back to level 0.
        assert_eq!(depths(&["  - b", "- a"]), vec![Some(0), Some(0)]);
    }

    #[test]
    fn list_depth_between_levels_width_starts_new_level() {
        assert_eq!(
            depths(&["- a", "      - deep", "    - mid"]),
            vec![Some(0), Some(1), Some(1)]
        );
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(64))]

        /// Extracted paragraphs have non-overlapping line ranges, in
        /// strictly ascending order, and every paragraph's `text` joins
        /// the lines verbatim from `content`.
        #[test]
        fn paragraphs_ranges_disjoint_and_ordered(content in "[a-zA-Z0-9 #\\-\\n]{0,200}") {
            let paragraphs = extract_paragraphs(&content);
            let lines: Vec<&str> = content.lines().collect();
            let mut last_end: u32 = 0;
            for p in &paragraphs {
                proptest::prop_assert!(p.line_start <= p.line_end);
                proptest::prop_assert!(p.line_start > last_end,
                    "paragraph at {}..{} overlaps prior end {}", p.line_start, p.line_end, last_end);
                last_end = p.line_end;
                let start = p.line_start as usize - 1;
                let end = p.line_end as usize - 1;
                proptest::prop_assert!(end < lines.len());
                let reconstructed: String = lines[start..=end].join("\n");
                proptest::prop_assert_eq!(&p.text, &reconstructed);
            }
        }
    }
}

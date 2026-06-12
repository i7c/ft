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
fn is_rule_separator(line: &str) -> bool {
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
}

impl LineSkipState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Advance one line. Returns `true` when this line is structural
    /// (frontmatter delimiter, frontmatter body, code-fence delimiter,
    /// inside a fenced code block, or an indented code block) and
    /// should be skipped by the consumer; `false` when this line
    /// carries content the consumer should examine.
    pub(crate) fn skip_line(&mut self, line: &str) -> bool {
        // Frontmatter handling: only relevant on line 1 and during the
        // block. CommonMark doesn't define frontmatter; we follow the
        // Obsidian / Jekyll convention of a `---` block at the very top.
        if !self.started {
            self.started = true;
            if line.trim_end() == "---" {
                self.in_frontmatter = true;
                return true;
            }
        } else if self.in_frontmatter {
            if line.trim_end() == "---" || line.trim_end() == "..." {
                self.in_frontmatter = false;
            }
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
                if c == fence_char && n >= self.fence_len {
                    self.fence = None;
                    self.fence_len = 0;
                }
            }
            return true;
        }
        if let Some((c, n)) = leading_fence(trimmed) {
            self.fence = Some(c);
            self.fence_len = n;
            return true;
        }

        // Indented code block: 4+ leading spaces (or a tab) and we're
        // not inside a list context. Without a full block parser we
        // approximate by skipping any 4-space-indented line. False
        // positives on deeply-nested list items are accepted in v1;
        // they would never start with `#` to begin with.
        if starts_with_indent(line, 4) {
            return true;
        }

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

/// Parse an ATX heading from `line` if it matches the pattern; `lineno`
/// is the 1-indexed source line.
fn parse_atx(line: &str, lineno: usize) -> Option<Heading> {
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

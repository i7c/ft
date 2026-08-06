//! Exporting note content for targets outside the vault.
//!
//! `ft notes export` renders a note (or an original-file line range of
//! it) as clean, portable markdown — stripping the vault-specific
//! structure: the frontmatter block, `[!ft-source]` callout header
//! lines (the `> body` lines survive as blockquotes), and wikilinks
//! (converted to plain text / CommonMark images).
//!
//! The per-line stripping rules live behind [`ExportTarget`] so new
//! targets (plain text, Slack) are new impls; the frontmatter clamp
//! and range slicing in [`export_content`] are target-independent.

use crate::frontmatter::frontmatter_end_line;
use crate::synth::callout::header_regex;
use crate::synth::slice::{count_lines, slice_lines};

/// Per-line markdown context computed by the driver, passed to
/// [`ExportTarget::transform_line`]. Target-independent structure:
/// any target rendering decisions that depend on "is this line code?"
/// read it here rather than re-deriving fence state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LineContext {
    /// The line is a code-fence delimiter, inside a fenced code block,
    /// or part of an indented code block. Code lines are content, not
    /// vault structure — the CommonMark target copies them verbatim.
    pub in_code: bool,
}

/// One export target: renders a single source line (no trailing
/// newline) into its exported form, or drops it (`None`).
///
/// Mirror of the `task::format::TaskFormat` seam: consumers take
/// `&dyn ExportTarget`; `CommonMarkExport` is the v1 impl, and future
/// targets (plain text, Slack) add impls without touching the driver.
pub trait ExportTarget {
    /// Stable name for `--format` and diagnostics.
    fn name(&self) -> &'static str;
    /// Render one line for this target. `None` drops the line.
    fn transform_line(&self, line: &str, ctx: LineContext) -> Option<String>;
}

/// v1 target: clean CommonMark.
pub struct CommonMarkExport;

impl ExportTarget for CommonMarkExport {
    fn name(&self) -> &'static str {
        "commonmark"
    }

    fn transform_line(&self, line: &str, ctx: LineContext) -> Option<String> {
        // Code lines are content, not vault structure — never drop a
        // header or convert a link inside them.
        if ctx.in_code {
            return Some(line.to_string());
        }
        // Canonical `[!ft-source]` headers are provenance plumbing —
        // meaningless outside the vault, so they drop. Their `> body`
        // lines are already valid CommonMark blockquotes and survive
        // as ordinary lines. Malformed headers (missing a token) do
        // not match and stay as plain blockquotes.
        if header_regex().is_match(line) {
            return None;
        }
        Some(convert_wikilinks(line))
    }
}

/// Result of an export: the transformed text, lines joined with `\n`
/// and no trailing newline. An empty `text` is a valid (empty) export
/// — e.g. a range fully inside the frontmatter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOutcome {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    /// Requested range end `B` beyond the file's raw line count.
    #[error("range end {requested_end} exceeds file line count {file_lines}")]
    RangePastEnd { file_lines: u32, requested_end: u32 },
}

/// Export `content` for `target`.
///
/// `range` is 1-indexed inclusive in **original-file** line numbers;
/// `None` means the whole file. The range start is clamped to the
/// first line after the leading frontmatter block (line 1 when there
/// is none), so frontmatter lines can never leak into the export and a
/// range fully inside the frontmatter yields empty output. `B` beyond
/// the file's raw line count is an error ([`ExportError::RangePastEnd`]
/// carries the raw count for the diagnostic) — only the start clamps,
/// because only frontmatter is a vault element being stripped.
pub fn export_content(
    content: &str,
    range: Option<(u32, u32)>,
    target: &dyn ExportTarget,
) -> Result<ExportOutcome, ExportError> {
    let first_body = frontmatter_end_line(content)
        .map(|close| close + 1)
        .unwrap_or(1);
    let total = count_lines(content);

    let (start, end) = match range {
        Some((a, b)) => {
            if b > total {
                return Err(ExportError::RangePastEnd {
                    file_lines: total,
                    requested_end: b,
                });
            }
            (a.max(1), b)
        }
        None => (1, total),
    };

    let start = start.max(first_body);
    if start > end {
        // The whole range was frontmatter (or empty) — a deliberate
        // empty export, not an error.
        return Ok(ExportOutcome {
            text: String::new(),
        });
    }

    // `start <= end <= total` and `start >= 1` are guaranteed above,
    // so the shared slice primitive cannot return `None` here.
    let body = slice_lines(content, start, end).expect("export range validated before slicing");
    let mut out_lines: Vec<String> = Vec::new();
    // Fence / indented-code tracking is target-independent markdown
    // structure (same rules as the graph parser and heading extractor).
    let mut ls = crate::markdown::LineSkipState::new();
    for line in body.split('\n') {
        ls.skip_line(line);
        let ctx = LineContext {
            in_code: ls.last_was_code(),
        };
        if let Some(t) = target.transform_line(line, ctx) {
            out_lines.push(t);
        }
    }
    Ok(ExportOutcome {
        text: out_lines.join("\n"),
    })
}

/// Rewrite every `[[…]]` / `![[…]]` on `line` for the CommonMark
/// target. Inline code spans (backtick runs, closed by a matching
/// same-length run, per CommonMark) are copied verbatim; an
/// unterminated run is literal text, so links after it still convert.
///
/// Markdown links `[x](y)` and images `![alt](src)` are never touched —
/// only the Obsidian `[[` / `![[` forms trigger a rewrite.
fn convert_wikilinks(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut copy_start = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'`' {
            let run = run_len(bytes, i);
            if let Some(close_end) = closing_run_end(bytes, i + run, run) {
                // Copy the whole closed span verbatim (keeps `copy_start`
                // pinned so the flush below includes it untouched).
                i = close_end;
            } else {
                // Unterminated — backticks are literal; keep scanning so
                // links after them still convert.
                i += run;
            }
            continue;
        }
        if b == b'!' && i + 2 < bytes.len() && bytes[i + 1] == b'[' && bytes[i + 2] == b'[' {
            if let Some(end) = wikilink_end(bytes, i + 1) {
                out.push_str(&line[copy_start..i]);
                out.push_str(&embed_replacement(&line[i..end]));
                copy_start = end;
                i = end;
                continue;
            }
            i += 1;
            continue;
        }
        if b == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            if let Some(end) = wikilink_end(bytes, i) {
                out.push_str(&line[copy_start..i]);
                out.push_str(&wikilink_replacement(&line[i..end]));
                copy_start = end;
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&line[copy_start..]);
    out
}

/// Byte offset just past the closing `]]` of a wikilink whose opening
/// `[[` starts at `start`. `None` when the line ends before the close
/// or the body is empty. Mirrors the graph parser's `parse_wikilink`.
fn wikilink_end(bytes: &[u8], start: usize) -> Option<usize> {
    debug_assert!(bytes[start] == b'[' && bytes[start + 1] == b'[');
    let mut i = start + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b']' {
            if i == start + 2 {
                return None; // `[[]]` — empty body, not a link
            }
            return Some(i + 2);
        }
        if bytes[i] == b'\n' {
            return None; // lines are scanned individually
        }
        i += 1;
    }
    None
}

/// Number of consecutive backticks starting at `i`.
fn run_len(bytes: &[u8], i: usize) -> usize {
    bytes[i..].iter().take_while(|&&b| b == b'`').count()
}

/// End offset of the first backtick run of exactly `len` starting at
/// or after `start` (the CommonMark closing rule), or `None`.
fn closing_run_end(bytes: &[u8], start: usize, len: usize) -> Option<usize> {
    let mut j = start;
    while j < bytes.len() {
        if bytes[j] == b'`' {
            let n = run_len(bytes, j);
            if n == len {
                return Some(j + n);
            }
            j += n;
        } else {
            j += 1;
        }
    }
    None
}

/// Replacement for a wikilink `[[body]]` (no leading `!`). The display
/// text wins when present — it is the text the author chose to show;
/// otherwise the trimmed target survives; a same-file anchor
/// (`[[#A]]`) keeps its `#` and the anchor. Not-a-real-link bodies
/// (`[[]]`, `[[|D]]`) return the raw text unchanged.
fn wikilink_replacement(raw: &str) -> String {
    let body = &raw[2..raw.len() - 2];
    match split_wiki(body) {
        Some((target, anchor, display)) => {
            if let Some(d) = display {
                d
            } else if !target.is_empty() {
                target
            } else {
                // `[[#A]]` — same-file heading link, brackets stripped.
                format!("#{anchor}")
            }
        }
        None => raw.to_string(),
    }
}

/// Replacement for an embed `![[body]]`: a CommonMark image
/// `![alt](href)` where `alt` is the display text (or the target) and
/// `href` is the target — angle-bracketed when it contains whitespace,
/// per the CommonMark URL rule. Anchors on embeds are dropped (an
/// anchor does not address a file). A body without a target is not a
/// real embed and stays verbatim.
fn embed_replacement(raw: &str) -> String {
    let body = &raw[3..raw.len() - 2];
    match split_wiki(body) {
        Some((target, _anchor, display)) if !target.is_empty() => {
            let href = if target.chars().any(|c| c.is_whitespace()) {
                format!("<{target}>")
            } else {
                target.clone()
            };
            let alt = display.unwrap_or(target);
            format!("![{alt}]({href})")
        }
        _ => raw.to_string(),
    }
}

/// Split a wikilink body into `(target, anchor, display)`. The target
/// is trimmed (mirroring the graph parser); anchor and display keep
/// their whitespace. An empty display is treated as absent. Returns
/// `None` for bodies that are not a real link — empty target **and** no
/// anchor (`[[]]`, `[[|D]]`).
fn split_wiki(body: &str) -> Option<(String, String, Option<String>)> {
    let (lhs, display) = match body.find('|') {
        Some(idx) => (&body[..idx], Some(body[idx + 1..].to_string())),
        None => (body, None),
    };
    let display = display.filter(|d| !d.trim().is_empty());
    let (target, anchor) = match lhs.find('#') {
        Some(idx) => (lhs[..idx].trim().to_string(), lhs[idx + 1..].to_string()),
        None => (lhs.trim().to_string(), String::new()),
    };
    if target.is_empty() && anchor.is_empty() {
        return None;
    }
    Some((target, anchor, display))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cm(line: &str) -> Option<String> {
        CommonMarkExport.transform_line(line, LineContext::default())
    }

    fn cm_code(line: &str) -> Option<String> {
        CommonMarkExport.transform_line(line, LineContext { in_code: true })
    }

    // ── transform_line: ft-source headers ──────────────────────────

    #[test]
    fn canonical_header_dropped() {
        assert_eq!(
            cm(r#"> [!ft-source] "notes/foo.md" L42-44 @abc1234 #7f3a91"#),
            None
        );
    }

    #[test]
    fn body_lines_kept_verbatim() {
        assert_eq!(
            cm("> Some original paragraph"),
            Some("> Some original paragraph".into())
        );
        assert_eq!(
            cm("> spanning two lines."),
            Some("> spanning two lines.".into())
        );
        assert_eq!(cm(">"), Some(">".into()));
    }

    #[test]
    fn malformed_header_kept() {
        // Missing line range / SHA / hash — not canonical.
        assert_eq!(
            cm(r#"> [!ft-source] "notes/foo.md""#),
            Some(r#"> [!ft-source] "notes/foo.md""#.into())
        );
    }

    #[test]
    fn nested_blockquote_header_kept() {
        assert_eq!(
            cm(r#"> > [!ft-source] "a.md" L1-1 @aaaaaaa #aaaaaa"#),
            Some(r#"> > [!ft-source] "a.md" L1-1 @aaaaaaa #aaaaaa"#.into())
        );
    }

    #[test]
    fn code_context_lines_verbatim() {
        // Fence delimiters, in-fence content, and indented code lines
        // are content — headers and links survive untouched.
        assert_eq!(cm_code("```rust"), Some("```rust".into()));
        assert_eq!(cm_code("[[Bar]]"), Some("[[Bar]]".into()));
        assert_eq!(
            cm_code("> [!ft-source] \"a.md\" L1-1 @aaaaaaa #aaaaaa"),
            Some("> [!ft-source] \"a.md\" L1-1 @aaaaaaa #aaaaaa".into())
        );
    }

    #[test]
    fn unrelated_callouts_kept() {
        assert_eq!(
            cm("> [!note] some other callout"),
            Some("> [!note] some other callout".into())
        );
    }

    // ── wikilink conversion table ──────────────────────────────────

    #[test]
    fn plain_wikilink() {
        assert_eq!(
            cm("see [[Some other file]] now"),
            Some("see Some other file now".into())
        );
    }

    #[test]
    fn alias_uses_display() {
        assert_eq!(cm("[[Foo|Bar]]"), Some("Bar".into()));
        assert_eq!(cm("[[Foo#H|Baz]]"), Some("Baz".into()));
        assert_eq!(cm("[[#H|Baz]]"), Some("Baz".into()));
    }

    #[test]
    fn anchor_dropped_on_cross_file_links() {
        assert_eq!(cm("[[Foo#Heading]]"), Some("Foo".into()));
    }

    #[test]
    fn same_file_anchor_strips_brackets() {
        assert_eq!(cm("[[#Heading]]"), Some("#Heading".into()));
    }

    #[test]
    fn embed_becomes_image() {
        assert_eq!(cm("![[image.png]]"), Some("![image.png](image.png)".into()));
        assert_eq!(
            cm("![[image.png|alt text]]"),
            Some("![alt text](image.png)".into())
        );
        assert_eq!(
            cm("![[img.png#anchor]]"),
            Some("![img.png](img.png)".into())
        );
    }

    #[test]
    fn embed_href_with_whitespace_angle_bracketed() {
        assert_eq!(
            cm("![[my image.png]]"),
            Some("![my image.png](<my image.png>)".into())
        );
    }

    #[test]
    fn target_trimmed() {
        assert_eq!(cm("[[  Foo  ]]"), Some("Foo".into()));
    }

    #[test]
    fn not_real_links_left_verbatim() {
        assert_eq!(cm("[[]]"), Some("[[]]".into()));
        assert_eq!(cm("[[|D]]"), Some("[[|D]]".into()));
        assert_eq!(cm("[[unterminated"), Some("[[unterminated".into()));
        assert_eq!(cm("![[unterminated"), Some("![[unterminated".into()));
    }

    // ── scanner edges ──────────────────────────────────────────────

    #[test]
    fn code_spans_untouched() {
        assert_eq!(
            cm("`[[Foo]]` and ``[[Bar]]`` real [[Baz]]"),
            Some("`[[Foo]]` and ``[[Bar]]`` real Baz".into())
        );
    }

    #[test]
    fn unterminated_code_span_does_not_swallow_links() {
        assert_eq!(
            cm("`unterminated [[Foo]]"),
            Some("`unterminated Foo".into())
        );
    }

    #[test]
    fn blockquote_lines_converted() {
        assert_eq!(cm("> See [[Foo]]"), Some("> See Foo".into()));
        assert_eq!(cm("> [!note] Title"), Some("> [!note] Title".into()));
    }

    #[test]
    fn markdown_links_and_images_preserved() {
        assert_eq!(
            cm("[text](foo.md) and ![alt](img.png)"),
            Some("[text](foo.md) and ![alt](img.png)".into())
        );
    }

    #[test]
    fn multiple_links_on_one_line() {
        assert_eq!(
            cm("[[A]] and [[B|bee]] and ![[c.png]]"),
            Some("A and bee and ![c.png](c.png)".into())
        );
    }

    #[test]
    fn task_lines_and_headings_preserved() {
        assert_eq!(
            cm("- [ ] ⏫ 📅 2026-08-05 Finish the report"),
            Some("- [ ] ⏫ 📅 2026-08-05 Finish the report".into())
        );
        assert_eq!(cm("# [[Foo]]"), Some("# Foo".into()));
    }

    #[test]
    fn unicode_surrounding_wikilinks() {
        assert_eq!(
            cm("émoji 🚀 [[Foo]] done"),
            Some("émoji 🚀 Foo done".into())
        );
    }

    // ── export_content: clamp, range, validation ───────────────────

    /// A 9-line document: frontmatter on 1-5, body lines 6-9.
    fn doc() -> &'static str {
        "---\nft:\n  synth:\n    enabled: true\n---\nL6\nL7\nL8\nL9\n"
    }

    fn export(doc: &str, range: Option<(u32, u32)>) -> Result<ExportOutcome, ExportError> {
        export_content(doc, range, &CommonMarkExport)
    }

    #[test]
    fn whole_file_clamps_to_body() {
        assert_eq!(export(doc(), None).unwrap().text, "L6\nL7\nL8\nL9");
    }

    #[test]
    fn range_after_frontmatter_is_verbatim() {
        assert_eq!(export(doc(), Some((6, 7))).unwrap().text, "L6\nL7");
    }

    #[test]
    fn mixed_range_clamps_start() {
        assert_eq!(export(doc(), Some((1, 7))).unwrap().text, "L6\nL7");
    }

    #[test]
    fn range_fully_inside_frontmatter_is_empty() {
        assert_eq!(export(doc(), Some((1, 3))).unwrap().text, "");
    }

    #[test]
    fn no_frontmatter_no_clamp() {
        let c = "a\nb\n";
        assert_eq!(export(c, Some((1, 2))).unwrap().text, "a\nb");
        assert_eq!(export(c, None).unwrap().text, "a\nb");
    }

    #[test]
    fn trailing_newline_is_not_a_line() {
        // `a\nb\n` = 2 lines; L1-2 is the whole file.
        assert_eq!(export("a\nb\n", Some((1, 2))).unwrap().text, "a\nb");
    }

    #[test]
    fn range_past_end_errors_with_raw_count() {
        assert_eq!(
            export(doc(), Some((6, 99))),
            Err(ExportError::RangePastEnd {
                file_lines: 9,
                requested_end: 99,
            })
        );
    }

    #[test]
    fn empty_file_export() {
        // Whole-file export of an empty file is empty; an explicit
        // range errors like quote (the file has 0 lines).
        assert_eq!(export("", None).unwrap().text, "");
        assert_eq!(
            export("", Some((1, 1))),
            Err(ExportError::RangePastEnd {
                file_lines: 0,
                requested_end: 1,
            })
        );
    }

    #[test]
    fn all_headers_dropped_gives_empty_text() {
        let c = "---\n---\n> [!ft-source] \"a.md\" L1-1 @aaaaaaa #aaaaaa\n";
        // frontmatter lines 1-2, header line 3 → both dropped.
        assert_eq!(export(c, Some((1, 3))).unwrap().text, "");
    }

    #[test]
    fn callout_conversion_in_document() {
        let c =
            "intro\n> [!ft-source] \"a.md\" L1-2 @aaaaaaa #aaaaaa\n> quoted [[Foo]]\n> more\nend\n";
        assert_eq!(
            export(c, None).unwrap().text,
            "intro\n> quoted Foo\n> more\nend"
        );
    }

    #[test]
    fn fenced_code_blocks_preserved_verbatim() {
        let c = "intro\n```rust\n[[Bar]]\n> [!ft-source] \"x.md\" L1-1 @aaaaaaa #aaaaaa\n```\nend [[Real]]\n";
        assert_eq!(
            export(c, None).unwrap().text,
            "intro\n```rust\n[[Bar]]\n> [!ft-source] \"x.md\" L1-1 @aaaaaaa #aaaaaa\n```\nend Real"
        );
    }

    #[test]
    fn indented_code_blocks_preserved_verbatim() {
        let c = "intro\n\n    [[Indented]]\n    more code\n\nafter [[Real]]\n";
        assert_eq!(
            export(c, None).unwrap().text,
            "intro\n\n    [[Indented]]\n    more code\n\nafter Real"
        );
    }

    #[test]
    fn tilde_fences_preserved_verbatim() {
        let c = "~~~\n[[Tilde]]\n~~~\n";
        assert_eq!(export(c, None).unwrap().text, "~~~\n[[Tilde]]\n~~~");
    }

    #[test]
    fn blank_line_after_frontmatter_respected() {
        let c = "---\na: 1\n---\n\nL7\n";
        // frontmatter on 1-3, line 4 blank, line 5 = L7.
        assert_eq!(export(c, Some((4, 5))).unwrap().text, "\nL7");
    }

    #[test]
    fn target_name() {
        assert_eq!(CommonMarkExport.name(), "commonmark");
    }
}

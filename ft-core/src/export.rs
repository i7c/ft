//! Exporting note content for targets outside the vault.
//!
//! `ft notes export` renders a note (or an original-file line range of
//! it) as clean, portable markdown — stripping the vault-specific
//! structure: the frontmatter block, `[!ft-source]` callout header
//! lines (the `> body` lines survive as blockquotes), and wikilinks
//! (converted to plain text / CommonMark images).
//!
//! The per-line stripping rules live behind [`ExportTarget`] so new
//! targets (plain text) are new impls; the frontmatter clamp
//! and range slicing in [`export_content`] are target-independent.
//! `CommonMarkExport` is the v1 target; `SlackExport` renders
//! Slack's mrkdwn dialect.

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
    /// The line is the *opening* delimiter of a fenced code block.
    /// Fence normalization targets (Slack: language tags, tilde
    /// fences) read this to distinguish the opener from closing
    /// delimiters and from content lines that merely look like
    /// delimiters. False on every other line.
    pub opened_fence: bool,
    /// The fence char active after this line — `'`'` or `'~'` while
    /// inside a fenced block (including the opening delimiter line),
    /// `None` otherwise. Lets a target tell a true closing delimiter
    /// (fence ends: `None`) from content inside a fence (char stays).
    pub fence_char: Option<char>,
    /// Nesting depth of a list-item line — 0 is the top level — `None`
    /// on every other line (non-list content, code). Computed by the
    /// driver from the raw line via [`crate::markdown::ListDepthTracker`];
    /// code lines always carry `None` (the tracker is never advanced
    /// on them). Only the Slack target consumes it, for its
    /// 4-space-per-level list re-indentation.
    pub list_depth: Option<usize>,
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
    /// Whether this target resolves hard-wrapped source lines
    /// (CommonMark soft breaks) into single logical lines. Slack
    /// defaults to true — its mrkdwn renders every newline as a line
    /// break, so wrapped paragraphs and list items must be joined to
    /// paste cleanly. CommonMark stays verbatim (wrapped source is
    /// idiomatic when the receiver is a markdown tool). The CLI
    /// `--unwrap` / `--no-unwrap` flags override the target default.
    fn unwrap_soft_wraps(&self) -> bool {
        false
    }
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

/// v2 target: Slack mrkdwn. `*bold*`, `_italic_`, `~strike~`,
/// `` `code` ``, ` ``` ` blocks, `>` blockquotes, `- ` lists and
/// `<url|text>` links survive; CommonMark syntax Slack renders as
/// literal text — `#` headings, `**bold**`, `[text](url)`,
/// `![alt](src)`, `- [ ]` checkboxes, `[!type]` callout markers,
/// language tags on code fences, `~~~` fences — is rewritten to a
/// Slack-native or plain-text form. `&`, `<`, `>` pass through raw:
/// the output targets the Slack composer, which does not decode HTML
/// entities.
pub struct SlackExport;

impl ExportTarget for SlackExport {
    fn name(&self) -> &'static str {
        "slack"
    }

    fn unwrap_soft_wraps(&self) -> bool {
        true
    }

    fn transform_line(&self, line: &str, ctx: LineContext) -> Option<String> {
        // Code lines are content — but fence delimiter lines are
        // normalized to Slack's syntax (language tags dropped,
        // `~~~` → ` ``` `).
        if ctx.in_code {
            return Some(normalize_fence_line(line, ctx));
        }
        // Canonical `[!ft-source]` headers are provenance plumbing —
        // dropped, same as the CommonMark target. Their `> body`
        // lines survive as blockquotes.
        if header_regex().is_match(line) {
            return None;
        }
        // Inline pass first, structural rewrites second, so generated
        // markup (heading `*H*` wrappers, stripped markers) is never
        // re-scanned as inline content. The list re-indent runs last:
        // a checkbox-dropped task line is re-indented with its marker
        // intact.
        let converted = structural_rewrites(&convert_slack_inline(line));
        Some(reindent_list_item(&converted, ctx.list_depth))
    }
}

/// Rewrite every inline element on `line` for the Slack target in one
/// left-to-right pass: code spans verbatim, then wikilinks, embeds,
/// markdown links/images and emphasis — each rewritten exactly once so
/// no rewrite re-triggers another. Link labels are re-scanned
/// recursively so `[**bold**](u)` keeps its bold; a label is strictly
/// shorter than its line, so recursion terminates.
fn convert_slack_inline(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut copy_start = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'`' {
            let run = run_len(bytes, i);
            if let Some(close_end) = closing_run_end(bytes, i + run, run) {
                // Copy the whole closed span verbatim.
                i = close_end;
            } else {
                i += run;
            }
            continue;
        }
        if b == b'!' && i + 2 < bytes.len() && bytes[i + 1] == b'[' && bytes[i + 2] == b'[' {
            if let Some(end) = wikilink_end(bytes, i + 1) {
                out.push_str(&line[copy_start..i]);
                out.push_str(&slack_embed_replacement(&line[i..end]));
                copy_start = end;
                i = end;
                continue;
            }
            i += 1;
            continue;
        }
        if b == b'!' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            if let Some((label, dest, end)) = markdown_link(bytes, i + 1) {
                out.push_str(&line[copy_start..i]);
                out.push_str(&slack_image_replacement(&label, &dest));
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
            i += 1;
            continue;
        }
        if b == b'[' {
            if let Some((label, dest, end)) = markdown_link(bytes, i) {
                out.push_str(&line[copy_start..i]);
                out.push_str(&slack_link_replacement(&label, &dest));
                copy_start = end;
                i = end;
                continue;
            }
            i += 1;
            continue;
        }
        if b == b'*' || b == b'_' || b == b'~' {
            if let Some((end, rep)) = emphasis_replacement(bytes, i) {
                out.push_str(&line[copy_start..i]);
                out.push_str(&rep);
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

/// Parse a CommonMark link `[label](dest)` whose opening `[` is at
/// `start`. Returns `(label, dest, end)` with `end` just past the
/// closing `)`, or `None`. Label scanning skips inline code spans
/// (first-match `]`); the dest is the raw text between the parens
/// (angle-bracket/title forms are cleaned by the caller).
fn markdown_link(bytes: &[u8], start: usize) -> Option<(String, String, usize)> {
    debug_assert!(bytes[start] == b'[');
    let label_end = scan_until(bytes, start + 1, b']')?;
    if bytes.get(label_end + 1) != Some(&b'(') {
        return None;
    }
    let dest_end = scan_until(bytes, label_end + 2, b')')?;
    let label = String::from_utf8_lossy(&bytes[start + 1..label_end]).into_owned();
    let dest = String::from_utf8_lossy(&bytes[label_end + 2..dest_end]).into_owned();
    Some((label, dest, dest_end + 1))
}

/// Byte offset of the first `needle` at or after `start`, skipping
/// closed inline code spans (backtick runs). `None` at end-of-line.
fn scan_until(bytes: &[u8], mut i: usize, needle: u8) -> Option<usize> {
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'`' {
            let run = run_len(bytes, i);
            if let Some(close_end) = closing_run_end(bytes, i + run, run) {
                i = close_end;
                continue;
            }
        }
        if b == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Slack form of a markdown link. http(s)/mailto destinations become
/// `<url|text>` (markdown titles dropped); anything else (internal
/// note links — a relative path is not a URL and would break Slack's
/// link syntax) degrades to the display text, inline-converted.
fn slack_link_replacement(label: &str, dest: &str) -> String {
    let label_slack = convert_slack_inline(label);
    let dest = clean_destination(dest);
    if is_web_destination(dest) {
        format!("<{dest}|{label_slack}>")
    } else {
        label_slack
    }
}

/// Slack form of a markdown image. A remote source becomes the bare
/// URL — Slack autolinks it and unfurls a preview thumbnail. A local
/// source (unreachable from Slack) becomes the alt text.
fn slack_image_replacement(label: &str, dest: &str) -> String {
    let dest = clean_destination(dest);
    if dest.starts_with("http://") || dest.starts_with("https://") {
        dest.to_string()
    } else {
        convert_slack_inline(label)
    }
}

/// Slack form of an embed `![[body]]`: plain text — display text when
/// present, else the trimmed target; anchors dropped. Vault-local
/// files are unreachable from Slack, so no image syntax survives. A
/// body without a target is not a real embed and stays verbatim.
fn slack_embed_replacement(raw: &str) -> String {
    let body = &raw[3..raw.len() - 2];
    match split_wiki(body) {
        Some((target, _anchor, display)) if !target.is_empty() => display.unwrap_or(target),
        _ => raw.to_string(),
    }
}

/// Clean a markdown link/image destination: strip surrounding
/// whitespace and angle brackets, and drop a trailing ` "title"` /
/// ` 'title'` component (cut at the space before the opening quote).
fn clean_destination(dest: &str) -> &str {
    let d = dest.trim().trim_matches(|c| c == '<' || c == '>');
    for q in ['"', '\''] {
        if let Some(pos) = d.find(q) {
            let core = d[..pos].trim_end();
            if !core.is_empty() {
                return core;
            }
        }
    }
    d
}

fn is_web_destination(dest: &str) -> bool {
    dest.starts_with("http://") || dest.starts_with("https://") || dest.starts_with("mailto:")
}

/// Try to match an emphasis delimiter run at `i` — `*`, `_`, `~` in
/// runs of `***`/`___`, `**`/`__`, `*`/`_`, `~~` — against a closing
/// run, and return `(end, slack_form)` when both sides are valid
/// delimiters. Simplified CommonMark flanking: a run opens only when
/// not followed by whitespace (and, for `_`, only at a word boundary
/// — keeps `snake_case` literal); a closer must not be preceded by
/// whitespace. The closer scan skips inline code spans, and the inner
/// text is re-scanned so nested emphasis survives (`*a **b** c*`).
/// Returns `None` for literal runs (single `~`, runs longer than the
/// table, `2 * 3`, `snake_case`, unmatched openers).
fn emphasis_replacement(bytes: &[u8], i: usize) -> Option<(usize, String)> {
    let c = bytes[i];
    let run = char_run_len(bytes, i);
    let (open_len, open_rep, close_rep) = match (c, run) {
        (b'*', 3) | (b'_', 3) => (3, "*_", "_*"),
        (b'*', 2) | (b'_', 2) => (2, "*", "*"),
        (b'*', 1) | (b'_', 1) => (1, "_", "_"),
        (b'~', 2) => (2, "~", "~"),
        _ => return None,
    };
    if !is_opener(bytes, i, c) {
        return None;
    }
    let close = find_closer(bytes, i + run, c, open_len)?;
    let inner = std::str::from_utf8(&bytes[i + run..close]).unwrap_or("");
    Some((
        close + open_len,
        format!("{open_rep}{}{close_rep}", convert_slack_inline(inner)),
    ))
}

/// Number of consecutive copies of `bytes[i]` starting at `i` (the
/// generic form of [`run_len`], which is backtick-specific for code
/// spans).
fn char_run_len(bytes: &[u8], i: usize) -> usize {
    let c = bytes[i];
    bytes[i..].iter().take_while(|&&b| b == c).count()
}

/// Simplified CommonMark flanking check for an emphasis opener at `i`:
/// followed by non-whitespace; `_` additionally only at a word
/// boundary (start / whitespace / punctuation before).
fn is_opener(bytes: &[u8], i: usize, c: u8) -> bool {
    match bytes.get(i + 1) {
        Some(a) if a.is_ascii_whitespace() => return false,
        None => return false, // EOL — nothing to wrap
        _ => {}
    }
    if c == b'_' {
        match i.checked_sub(1).map(|p| bytes[p]) {
            Some(prev) => !prev.is_ascii_alphanumeric(),
            None => true,
        }
    } else {
        true
    }
}

/// Byte offset of a closing run of exactly `len` of char `c` at or
/// after `from`, skipping inline code spans. A closer must not be
/// preceded by whitespace. `None` when no such run exists.
fn find_closer(bytes: &[u8], from: usize, c: u8, len: usize) -> Option<usize> {
    let mut j = from;
    while j < bytes.len() {
        let b = bytes[j];
        if b == b'`' {
            let run = run_len(bytes, j);
            if let Some(close_end) = closing_run_end(bytes, j + run, run) {
                j = close_end;
                continue;
            }
        }
        if b == c {
            let n = char_run_len(bytes, j);
            if n == len && !bytes[j - 1].is_ascii_whitespace() {
                return Some(j);
            }
            j += n;
        } else {
            j += 1;
        }
    }
    None
}

/// Line-level rewrites that run *after* the inline pass so generated
/// markup (heading `*H*` wrappers, stripped markers) is never
/// re-scanned as inline content: callout `[!type]` marker strip,
/// task-checkbox drop, then ATX heading → bold.
fn structural_rewrites(line: &str) -> String {
    let line = strip_callout_marker(line);
    let line = drop_task_checkbox(&line);
    boldify_heading(&line)
}

/// Strip an Obsidian callout marker (`[!type]`) that starts the
/// content of a blockquote line: `> [!note] Title` → `> Title`,
/// `> > [!warning] x` → `> > x`. The marker is vault chrome; the
/// title survives as the quote's first line. Only the first marker
/// token is removed; non-callout blockquotes are untouched.
fn strip_callout_marker(line: &str) -> String {
    let Some((prefix_end, marker_end)) = callout_marker(line) else {
        return line.to_string();
    };
    let after = &line[marker_end..];
    let remainder = after.strip_prefix([' ', '\t']).unwrap_or(after);
    let mut out = String::with_capacity(line.len());
    out.push_str(&line[..prefix_end]);
    if !line[..prefix_end].ends_with(' ') && !remainder.is_empty() {
        out.push(' ');
    }
    out.push_str(remainder);
    out
}

/// Detect a callout marker (`[!type]`) at the start of a blockquote
/// line's content. Returns `(prefix_end, marker_end)` — the byte
/// offset just past the trailing `>` prefixes (where the marker
/// starts) and just past the marker's closing `]`. `None` for
/// non-blockquote lines, blockquotes without a leading `[!type]`
/// token, and tokens that are not a callout type (empty, or
/// containing characters outside `[A-Za-z0-9_-]`). Shared by
/// [`strip_callout_marker`] and the join pass's callout-title
/// detection.
fn callout_marker(line: &str) -> Option<(usize, usize)> {
    let prefix_end = blockquote_prefix_end(line);
    if prefix_end == 0 {
        return None;
    }
    let bytes = line.as_bytes();
    let rest = &bytes[prefix_end..];
    if !rest.starts_with(b"[!") {
        return None;
    }
    let close = rest.iter().position(|&b| b == b']')?;
    let token = &rest[2..close];
    let is_type = !token.is_empty()
        && token[0].is_ascii_alphabetic()
        && token
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !is_type {
        return None;
    }
    Some((prefix_end, prefix_end + close + 1))
}

/// Byte offset just past the leading whitespace and `>` prefixes of a
/// blockquote line — each `>` followed by optional spaces/tabs, per
/// the callout grammar. 0 when the line is not a blockquote. Also the
/// offset of the content a blockquote continuation contributes to a
/// joined logical line.
fn blockquote_prefix_end(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    loop {
        if bytes.get(i) == Some(&b'>') {
            i += 1;
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
        } else {
            break;
        }
    }
    i
}

/// Re-indent a list-item line to Slack's 4-space-per-level rule: a
/// line at depth `d` gets exactly `4d` leading spaces (depth 0 stays
/// unindented). Runs after the inline/structural rewrites, so a
/// checkbox-dropped task line is re-indented cleanly; defensively
/// re-checks that the rewritten line still starts with a list marker
/// (the rewrites preserve markers today, but a malformed line must
/// never gain an indent it does not deserve).
fn reindent_list_item(line: &str, depth: Option<usize>) -> String {
    let Some(d) = depth else {
        return line.to_string();
    };
    let (_, off) = crate::markdown::leading_ws(line);
    if !crate::markdown::is_list_item_marker(&line[off..]) {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len() + 4 * d);
    for _ in 0..(4 * d) {
        out.push(' ');
    }
    out.push_str(&line[off..]);
    out
}

/// Drop the checkbox from a CommonMark task list item:
/// `- [ ] ⏫ …` → `- ⏫ …`, `  - [x] done` → `  - done`. The bullet
/// char, indentation and the rest of the line survive — Slack has no
/// checkbox syntax but renders `- ` as a bullet. The checkbox is
/// recognized after *any* leading whitespace (spaces or tabs), so
/// nested items indented 4+ spaces drop it too.
fn drop_task_checkbox(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= bytes.len() || !matches!(bytes[i], b'-' | b'*' | b'+') {
        return line.to_string();
    }
    i += 1;
    if i >= bytes.len() || !(bytes[i] == b' ' || bytes[i] == b'\t') {
        return line.to_string();
    }
    let space_start = i;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i + 3 > bytes.len()
        || bytes[i] != b'['
        || !matches!(bytes[i + 1], b' ' | b'x' | b'X')
        || bytes[i + 2] != b']'
    {
        return line.to_string();
    }
    let after = i + 3;
    if after < bytes.len() && !(bytes[after] == b' ' || bytes[after] == b'\t') {
        return line.to_string(); // `[x]foo` is not a task item
    }
    let mut out = String::with_capacity(line.len());
    out.push_str(&line[..space_start]);
    out.push_str(&line[after..]);
    out
}

/// Rewrite an ATX heading to Slack bold — `# Title` → `*Title*`. The
/// level is lost (Slack has no headings); the cleaned heading text
/// reuses the crate's ATX parser (closing `#`s stripped).
fn boldify_heading(line: &str) -> String {
    match crate::markdown::parse_atx(line, 0) {
        Some(h) => format!("*{}*", h.text),
        None => line.to_string(),
    }
}

/// Normalize a fenced-code delimiter line to Slack's syntax: opening
/// delimiters collapse to ``` (language tags dropped — Slack has no
/// highlighting), closing delimiters collapse to ```, and tilde
/// delimiters convert to backticks (Slack does not know tilde
/// fences). Only actual delimiters are touched: a content line that
/// merely looks like one (a ```js line inside a fence, a tilde-only
/// line inside a backtick fence) is left alone via `ctx.fence_char`.
fn normalize_fence_line(line: &str, ctx: LineContext) -> String {
    if ctx.opened_fence {
        return "```".to_string();
    }
    match crate::markdown::leading_fence(line.trim_start()) {
        Some(('`', _)) if ctx.fence_char.is_none() => "```".to_string(),
        Some(('~', _)) if ctx.fence_char.is_none() => "```".to_string(),
        _ => line.to_string(),
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

// ── soft-break resolution (the join pass) ──────────────────────────
//
// Hard-wrapped source lines — a bare `\n` inside a paragraph or list
// item — are CommonMark *soft breaks*: they render as a space. The
// Slack target's receiver renders every newline as a line break, so
// wrapped content pastes shattered. The join pass below resolves soft
// breaks into single logical lines, matching what a CommonMark
// renderer produces. It runs in the driver *after* the per-line
// transform but classifies the *source* line: target rewrites
// (heading → `*H*`, checkbox drop) make the transformed text
// ambiguous, so the join decision must see the real structure.

/// Coarse block kind of a source line, for the join state machine.
/// Unlike [`LineContext`] (per-line transform context), this answers
/// the cross-line question "what may continue what".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    /// Blank (or whitespace-only) line — a paragraph boundary.
    Blank,
    /// Fence delimiter, in-fence content, or indented code — opaque,
    /// never joined.
    Code,
    /// ATX heading (`# …` through `###### …`).
    Heading,
    /// List-item marker line (`-`, `*`, `+`, `N.`) at any depth.
    ListItem,
    /// Blockquote line (`> …`). `callout_title` marks a `[!type]`
    /// callout title — its body must not join into it; `inner_list`
    /// marks a line whose content (after the `>` prefixes) starts a
    /// list item, so two items inside a quote never join.
    Blockquote {
        callout_title: bool,
        inner_list: bool,
    },
    /// Ordinary paragraph text.
    Paragraph,
    /// Thematic rule (`--`, `---`, …).
    Break,
}

/// Classify `line` for the join pass. `in_code` comes from
/// [`LineSkipState`](crate::markdown::LineSkipState): fences and
/// indented code are opaque and never join.
fn classify(line: &str, in_code: bool) -> BlockKind {
    if in_code {
        return BlockKind::Code;
    }
    if line.trim().is_empty() {
        return BlockKind::Blank;
    }
    let trimmed = line.trim_start();
    if crate::markdown::is_rule_separator(trimmed) {
        return BlockKind::Break;
    }
    if crate::markdown::parse_atx(line, 0).is_some() {
        return BlockKind::Heading;
    }
    if crate::markdown::is_blockquote_line(line) {
        let inner = blockquote_content(line);
        return BlockKind::Blockquote {
            callout_title: callout_marker(line).is_some(),
            inner_list: crate::markdown::is_list_item_marker(inner),
        };
    }
    let (_, off) = crate::markdown::leading_ws(line);
    if crate::markdown::is_list_item_marker(&line[off..]) {
        return BlockKind::ListItem;
    }
    BlockKind::Paragraph
}

/// The open logical line of the join pass.
struct PendingLine {
    /// Joined output so far, with no trailing whitespace.
    text: String,
    /// Block kind of the *first* line in this logical line — the
    /// flags the join decision reads (a merged continuation must not
    /// change what the line is).
    kind: BlockKind,
    /// The last appended source line ended in a CommonMark hard break
    /// (trailing lone `\` or two+ spaces) — the next line must not
    /// join.
    hard_break: bool,
}

/// Advance the join state machine with one source line (already
/// transformed to `transformed`): merge it into the pending logical
/// line, flush the pending line and start a new one, or — for dropped
/// lines — flush and contribute nothing.
fn join_line(
    out_lines: &mut Vec<String>,
    pending: &mut Option<PendingLine>,
    line: &str,
    in_code: bool,
    transformed: Option<String>,
) {
    let kind = classify(line, in_code);
    let Some(t) = transformed else {
        // Dropped lines (ft-source callout headers) are boundaries:
        // flush any open logical line, contribute nothing. (In
        // CommonMark a `>` line interrupts a paragraph anyway.)
        if let Some(p) = pending.take() {
            out_lines.push(p.text);
        }
        return;
    };
    let hard_break = has_hard_break(line);
    if pending.as_ref().is_some_and(|p| can_join(p, kind, line)) {
        let p = pending.as_mut().unwrap();
        // Join with a single space; drop any trailing whitespace the
        // pending text picked up from its source lines.
        let len = p.text.trim_end().len();
        p.text.truncate(len);
        p.text.push(' ');
        p.text.push_str(continuation_content(line, kind));
        p.hard_break = hard_break;
    } else {
        if let Some(p) = pending.take() {
            out_lines.push(p.text);
        }
        // Boundary lines (blank, code, headings, thematic rules, and
        // empty `>` spacers) pass through verbatim and never open a
        // pending line — nothing may join into them.
        let opens = matches!(
            kind,
            BlockKind::ListItem | BlockKind::Paragraph | BlockKind::Blockquote { .. }
        ) && !(matches!(kind, BlockKind::Blockquote { .. })
            && blockquote_content(line).is_empty());
        if opens {
            *pending = Some(PendingLine {
                text: t,
                kind,
                hard_break,
            });
        } else {
            out_lines.push(t);
        }
    }
}

/// Whether `line` (kind `kind`) may merge into the open pending
/// logical line. Implements the merge table: paragraph→paragraph and
/// indented-paragraph→list-item join (soft breaks); blockquote→
/// blockquote joins except into a callout title, across a list item,
/// or across an empty `>` line; everything else — blank lines, code,
/// headings, thematic rules, list markers, block boundary changes —
/// does not join.
fn can_join(pending: &PendingLine, kind: BlockKind, line: &str) -> bool {
    if pending.hard_break {
        return false;
    }
    match (pending.kind, kind) {
        (BlockKind::Paragraph, BlockKind::Paragraph) => true,
        (BlockKind::ListItem, BlockKind::Paragraph) => {
            // An indented line continues the open item's text; an
            // unindented one is a new paragraph (the same heuristic
            // `ListDepthTracker` uses for list interruption).
            let (w, _) = crate::markdown::leading_ws(line);
            w > 0
        }
        (
            BlockKind::Blockquote {
                callout_title: false,
                ..
            },
            BlockKind::Blockquote {
                inner_list: false, ..
            },
        ) => !blockquote_content(line).is_empty(),
        _ => false,
    }
}

/// Content of a blockquote line after its `>` prefixes and leading
/// whitespace — empty for spacer lines like `>` or `> `.
fn blockquote_content(line: &str) -> &str {
    &line[blockquote_prefix_end(line)..]
}

/// Content appended when `line` joins a pending logical line: for a
/// blockquote continuation the `>` prefixes are dropped (the first
/// line's prefix structure wins), otherwise just the leading
/// whitespace.
fn continuation_content(line: &str, kind: BlockKind) -> &str {
    if matches!(kind, BlockKind::Blockquote { .. }) {
        blockquote_content(line)
    } else {
        let (_, off) = crate::markdown::leading_ws(line);
        &line[off..]
    }
}

/// True when `line` ends in a CommonMark hard break: a lone trailing
/// `\` or two or more trailing spaces. Hard breaks are real line
/// breaks — the next line must not join into this one.
fn has_hard_break(line: &str) -> bool {
    if line.ends_with('\\') {
        return true;
    }
    let trailing_spaces = line.bytes().rev().take_while(|&b| b == b' ').count();
    trailing_spaces >= 2
}

/// Export `content` for `target`, applying the target's default
/// soft-break policy ([`ExportTarget::unwrap_soft_wraps`]).
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
    export_content_with(content, range, target, None)
}

/// Like [`export_content`], with an explicit soft-break policy:
/// `Some(true)` resolves hard-wrapped lines (soft breaks become
/// spaces), `Some(false)` keeps source line breaks verbatim, and
/// `None` falls back to the target's
/// [`ExportTarget::unwrap_soft_wraps`] default.
pub fn export_content_with(
    content: &str,
    range: Option<(u32, u32)>,
    target: &dyn ExportTarget,
    unwrap: Option<bool>,
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
    let join = unwrap.unwrap_or_else(|| target.unwrap_soft_wraps());
    let mut out_lines: Vec<String> = Vec::new();
    // Fence / indented-code tracking is target-independent markdown
    // structure (same rules as the graph parser and heading extractor).
    let mut ls = crate::markdown::LineSkipState::new();
    // List nesting is the same category of structure: the tracker is
    // advanced on every non-code line (fence content and indented code
    // blocks are opaque and must not feed the stack) and the resulting
    // depth feeds the Slack target's re-indent.
    let mut lists = crate::markdown::ListDepthTracker::new();
    // One open logical line while soft-break resolution is active.
    let mut pending: Option<PendingLine> = None;
    for line in body.split('\n') {
        ls.skip_line(line);
        let in_code = ls.last_was_code();
        let list_depth = if in_code { None } else { lists.advance(line) };
        let ctx = LineContext {
            in_code,
            opened_fence: ls.opened_fence(),
            fence_char: ls.fence_char(),
            list_depth,
        };
        let transformed = target.transform_line(line, ctx);
        if join {
            join_line(&mut out_lines, &mut pending, line, in_code, transformed);
        } else if let Some(t) = transformed {
            out_lines.push(t);
        }
    }
    if let Some(p) = pending.take() {
        out_lines.push(p.text);
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
        CommonMarkExport.transform_line(
            line,
            LineContext {
                in_code: true,
                ..Default::default()
            },
        )
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
        assert_eq!(SlackExport.name(), "slack");
    }

    // ── slack: helpers ─────────────────────────────────────────────

    fn sl(line: &str) -> Option<String> {
        SlackExport.transform_line(line, LineContext::default())
    }

    /// Transform with an explicit list depth, as the driver would
    /// compute it for a list-item line.
    fn sl_at(line: &str, depth: Option<usize>) -> Option<String> {
        SlackExport.transform_line(
            line,
            LineContext {
                list_depth: depth,
                ..Default::default()
            },
        )
    }

    fn sl_code(opened: bool, fence: Option<char>, line: &str) -> Option<String> {
        SlackExport.transform_line(
            line,
            LineContext {
                in_code: true,
                opened_fence: opened,
                fence_char: fence,
                list_depth: None,
            },
        )
    }

    // ── slack: shared stripping ────────────────────────────────────

    #[test]
    fn slack_canonical_header_dropped() {
        assert_eq!(
            sl(r#"> [!ft-source] "notes/foo.md" L42-44 @abc1234 #7f3a91"#),
            None
        );
    }

    #[test]
    fn slack_body_lines_kept() {
        assert_eq!(
            sl("> Some original paragraph"),
            Some("> Some original paragraph".into())
        );
    }

    #[test]
    fn slack_malformed_header_marker_stripped() {
        // Not canonical — survives, but the `[!ft-source]` marker is
        // vault chrome and gets stripped like any callout marker.
        assert_eq!(
            sl(r#"> [!ft-source] "notes/foo.md""#),
            Some(r#"> "notes/foo.md""#.into())
        );
    }

    #[test]
    fn slack_code_context_lines_verbatim() {
        assert_eq!(sl_code(false, None, "```rust"), Some("```".into()));
        assert_eq!(sl_code(false, Some('`'), "[[Bar]]"), Some("[[Bar]]".into()));
    }

    // ── slack: headings ────────────────────────────────────────────

    #[test]
    fn heading_becomes_bold() {
        assert_eq!(sl("# Title"), Some("*Title*".into()));
        assert_eq!(sl("## Subtitle"), Some("*Subtitle*".into()));
        assert_eq!(sl("# Title ##"), Some("*Title*".into()));
    }

    #[test]
    fn heading_with_wikilink() {
        assert_eq!(sl("# [[Foo]]"), Some("*Foo*".into()));
    }

    #[test]
    fn non_heading_kept() {
        assert_eq!(
            sl("####### not a heading"),
            Some("####### not a heading".into())
        );
        assert_eq!(sl("no hash"), Some("no hash".into()));
        assert_eq!(sl("#no-space"), Some("#no-space".into()));
    }

    // ── slack: emphasis ────────────────────────────────────────────

    #[test]
    fn emphasis_converts_to_slack_dialect() {
        assert_eq!(sl("**bold**"), Some("*bold*".into()));
        assert_eq!(sl("*italic*"), Some("_italic_".into()));
        assert_eq!(sl("_under_"), Some("_under_".into()));
        assert_eq!(sl("~~strike~~"), Some("~strike~".into()));
        assert_eq!(sl("***both***"), Some("*_both_*".into()));
        assert_eq!(sl("__strong__"), Some("*strong*".into()));
    }

    #[test]
    fn emphasis_code_spans_untouched() {
        assert_eq!(sl("`**not bold**`"), Some("`**not bold**`".into()));
        assert_eq!(
            sl("`a *b* c` and **bold**"),
            Some("`a *b* c` and *bold*".into())
        );
    }

    #[test]
    fn flanking_rules_keep_prose_literal() {
        assert_eq!(sl("snake_case"), Some("snake_case".into()));
        assert_eq!(sl("2 * 3"), Some("2 * 3".into()));
        assert_eq!(sl("*unmatched"), Some("*unmatched".into()));
        assert_eq!(sl("a ~ single"), Some("a ~ single".into()));
    }

    #[test]
    fn nested_emphasis_survives() {
        assert_eq!(sl("*a **b** c*"), Some("_a *b* c_".into()));
        assert_eq!(sl("**a _b_ c**"), Some("*a _b_ c*".into()));
    }

    // ── slack: links ───────────────────────────────────────────────

    #[test]
    fn markdown_link_becomes_slack_link() {
        assert_eq!(
            sl("see [docs](https://docs.example.com/x)"),
            Some("see <https://docs.example.com/x|docs>".into())
        );
        assert_eq!(
            sl("[mail](mailto:per@ex.com)"),
            Some("<mailto:per@ex.com|mail>".into())
        );
    }

    #[test]
    fn internal_markdown_link_loses_link() {
        assert_eq!(
            sl("see [other note](notes/other.md)"),
            Some("see other note".into())
        );
    }

    #[test]
    fn link_title_dropped_and_angle_dest_cleaned() {
        assert_eq!(
            sl("[x](https://ex.com \"title\")"),
            Some("<https://ex.com|x>".into())
        );
        assert_eq!(
            sl("[x](<https://ex.com>)"),
            Some("<https://ex.com|x>".into())
        );
    }

    #[test]
    fn link_label_emphasis_converted() {
        assert_eq!(
            sl("[**bold**](https://ex.com)"),
            Some("<https://ex.com|*bold*>".into())
        );
    }

    #[test]
    fn ampersand_in_url_raw() {
        assert_eq!(
            sl("[x](https://a.com?a=1&b=2)"),
            Some("<https://a.com?a=1&b=2|x>".into())
        );
    }

    // ── slack: images ──────────────────────────────────────────────

    #[test]
    fn remote_image_becomes_bare_url() {
        assert_eq!(
            sl("![diagram](https://ex.com/img.png)"),
            Some("https://ex.com/img.png".into())
        );
    }

    #[test]
    fn local_image_becomes_alt_text() {
        assert_eq!(sl("![screenshot](local.png)"), Some("screenshot".into()));
    }

    #[test]
    fn embed_becomes_plain_text() {
        assert_eq!(sl("![[image.png]]"), Some("image.png".into()));
        assert_eq!(sl("![[image.png|alt text]]"), Some("alt text".into()));
        assert_eq!(sl("![[img.png#anchor]]"), Some("img.png".into()));
        assert_eq!(sl("![[#A]]"), Some("![[#A]]".into()));
    }

    #[test]
    fn slack_wikilink_conversion_matches_commonmark() {
        assert_eq!(
            sl("see [[Some other file]]"),
            Some("see Some other file".into())
        );
        assert_eq!(sl("[[Foo|Bar]]"), Some("Bar".into()));
        assert_eq!(sl("[[#Heading]]"), Some("#Heading".into()));
        assert_eq!(sl("[[Foo]] and [[B|bee]]"), Some("Foo and bee".into()));
    }

    #[test]
    fn slack_non_links_verbatim() {
        assert_eq!(sl("[[]]"), Some("[[]]".into()));
        assert_eq!(sl("[[unterminated"), Some("[[unterminated".into()));
        assert_eq!(sl("`[[Foo]]`"), Some("`[[Foo]]`".into()));
    }

    // ── slack: callouts ────────────────────────────────────────────

    #[test]
    fn callout_marker_stripped() {
        assert_eq!(sl("> [!note] Keep this"), Some("> Keep this".into()));
        assert_eq!(sl("> > [!warning] nested"), Some("> > nested".into()));
        assert_eq!(sl("> [!note]"), Some("> ".into()));
        assert_eq!(sl(">[!note]Title"), Some("> Title".into()));
    }

    #[test]
    fn non_callout_blockquotes_kept() {
        assert_eq!(sl("> plain quote"), Some("> plain quote".into()));
        assert_eq!(sl("> text [!note] mid"), Some("> text [!note] mid".into()));
        assert_eq!(sl("> [!] nothing"), Some("> [!] nothing".into()));
    }

    #[test]
    fn callout_inline_content_converted() {
        assert_eq!(sl("> [!note] **Hi**"), Some("> *Hi*".into()));
        assert_eq!(sl("> see [[Baz]]"), Some("> see Baz".into()));
    }

    // ── slack: task lines ──────────────────────────────────────────

    #[test]
    fn task_checkbox_dropped() {
        assert_eq!(
            sl("- [ ] ⏫ 📅 2026-08-05 Finish"),
            Some("- ⏫ 📅 2026-08-05 Finish".into())
        );
        assert_eq!(sl("  - [x] done"), Some("  - done".into()));
        assert_eq!(sl("* [ ] star"), Some("* star".into()));
        assert_eq!(sl("- [ ]"), Some("-".into()));
    }

    #[test]
    fn task_checkbox_dropped_at_any_depth() {
        // The checkbox is recognized after *any* leading whitespace,
        // not just the 0-3 spaces of a top-level item.
        assert_eq!(sl("    - [ ] foo"), Some("    - foo".into()));
        assert_eq!(sl("        - [x] done"), Some("        - done".into()));
        assert_eq!(sl("\t- [ ] tab"), Some("\t- tab".into()));
    }

    #[test]
    fn non_task_lines_kept() {
        assert_eq!(sl("- [foo]"), Some("- [foo]".into()));
        assert_eq!(sl("- item"), Some("- item".into()));
        assert_eq!(sl("1. item"), Some("1. item".into()));
        // Deeply indented non-task lines are untouched too.
        assert_eq!(sl("    - [foo]"), Some("    - [foo]".into()));
        assert_eq!(sl("    - [x]foo"), Some("    - [x]foo".into()));
    }

    // ── slack: raw specials ────────────────────────────────────────

    #[test]
    fn specials_not_escaped() {
        assert_eq!(
            sl("AT&T and <value> and a > b"),
            Some("AT&T and <value> and a > b".into())
        );
    }

    // ── slack: fence normalization (needs driver context) ──────────

    #[test]
    fn fence_openers_normalized() {
        assert_eq!(sl_code(true, Some('`'), "```rust"), Some("```".into()));
        assert_eq!(sl_code(true, Some('`'), "``` "), Some("```".into()));
        assert_eq!(sl_code(true, Some('~'), "~~~"), Some("```".into()));
        assert_eq!(sl_code(true, Some('~'), "~~~rust"), Some("```".into()));
    }

    #[test]
    fn fence_closers_normalized() {
        assert_eq!(sl_code(false, None, "```"), Some("```".into()));
        assert_eq!(sl_code(false, None, "~~~"), Some("```".into()));
    }

    #[test]
    fn fence_content_lines_verbatim() {
        assert_eq!(sl_code(false, Some('`'), "[[Bar]]"), Some("[[Bar]]".into()));
        assert_eq!(sl_code(false, Some('~'), "  code"), Some("  code".into()));
    }

    #[test]
    fn fence_looking_content_not_normalized() {
        // A ```js line inside a backtick fence is content, not a
        // delimiter — the block must stay intact.
        assert_eq!(sl_code(false, Some('`'), "```js"), Some("```js".into()));
        // A tilde-only line inside a backtick fence is content too.
        assert_eq!(sl_code(false, Some('`'), "~~~"), Some("~~~".into()));
    }

    // ── slack: list re-indentation (4 spaces per level) ────────────

    #[test]
    fn list_item_reindented_by_depth() {
        assert_eq!(sl_at("- foo", Some(0)), Some("- foo".into()));
        assert_eq!(sl_at("  - bar", Some(1)), Some("    - bar".into()));
        assert_eq!(sl_at("    - lol", Some(2)), Some("        - lol".into()));
        assert_eq!(sl_at("      - d", Some(3)), Some("            - d".into()));
        assert_eq!(sl_at("1. item", Some(1)), Some("    1. item".into()));
        assert_eq!(sl_at("* star", Some(1)), Some("    * star".into()));
    }

    #[test]
    fn list_item_inline_and_structural_rewrites_survive() {
        // Inline conversion happens before re-indent: the content is
        // converted, then the whole line is re-indented.
        assert_eq!(sl_at("  - **bold**", Some(1)), Some("    - *bold*".into()));
        // Checkbox drop happens before re-indent too.
        assert_eq!(sl_at("- [ ] foo", Some(1)), Some("    - foo".into()));
        assert_eq!(sl_at("    - [x] done", Some(1)), Some("    - done".into()));
    }

    #[test]
    fn non_list_lines_never_reindented() {
        // Depth is None for every non-list line; the defensive marker
        // re-check also refuses to indent a line that stopped looking
        // like a list item (e.g. a heading boldified to `*H*`).
        assert_eq!(sl_at("plain text", None), Some("plain text".into()));
        assert_eq!(sl_at("> - foo", None), Some("> - foo".into()));
        assert_eq!(sl_at("plain text", Some(1)), Some("plain text".into()));
        assert_eq!(sl_at("# Title", Some(1)), Some("*Title*".into()));
    }

    // ── slack: whole-document driver tests ─────────────────────────

    fn slack_export(doc: &str, range: Option<(u32, u32)>) -> Result<ExportOutcome, ExportError> {
        export_content(doc, range, &SlackExport)
    }

    #[test]
    fn slack_fenced_blocks_normalized_in_document() {
        let c = "intro\n```rust\nfn main() {}\n```\n\n~~~\ncode\n~~~\nend\n";
        assert_eq!(
            slack_export(c, None).unwrap().text,
            "intro\n```\nfn main() {}\n```\n\n```\ncode\n```\nend"
        );
    }

    #[test]
    fn slack_fence_looking_content_stays_in_block() {
        // ` ```js ` inside a 3-backtick fence is content — it must not
        // be treated as an opening delimiter (the block opens once and
        // closes once).
        let c = "```\n```js\ncode\n```\nafter\n";
        assert_eq!(
            slack_export(c, None).unwrap().text,
            "```\n```js\ncode\n```\nafter"
        );
    }

    #[test]
    fn slack_whole_document_composition() {
        let c = "---\ntitle: x\n---\n\n# Heading [[Foo]]\n\n**bold** and [link](https://ex.com)\n\n- [ ] ⏫ task\n\n> [!note] Title\n\n![[img.png|pic]]\n";
        assert_eq!(
            slack_export(c, None).unwrap().text,
            "\n*Heading Foo*\n\n*bold* and <https://ex.com|link>\n\n- ⏫ task\n\n> Title\n\npic"
        );
    }

    // ── slack: list re-indentation in documents ────────────────────

    #[test]
    fn slack_two_space_list_reindented_to_four() {
        // The user's case: a 2-space-per-level source list becomes
        // 4 spaces per level (depth-based, not ceil-to-multiple-of-4 —
        // the 4-space `- lol` becomes 8, two levels deep).
        let c = "- foo\n  - bar\n    - lol\n- baz\n";
        assert_eq!(
            slack_export(c, None).unwrap().text,
            "- foo\n    - bar\n        - lol\n- baz"
        );
    }

    #[test]
    fn slack_deep_nesting_scales_by_level() {
        let c = "- a\n  - b\n    - c\n      - d\n";
        assert_eq!(
            slack_export(c, None).unwrap().text,
            "- a\n    - b\n        - c\n            - d"
        );
    }

    #[test]
    fn slack_all_marker_kinds_normalized() {
        let c = "- one\n  * two\n    + three\n      1. four\n";
        assert_eq!(
            slack_export(c, None).unwrap().text,
            "- one\n    * two\n        + three\n            1. four"
        );
    }

    #[test]
    fn slack_four_space_sources_unchanged() {
        let c = "- foo\n    - bar\n        - lol\n";
        assert_eq!(
            slack_export(c, None).unwrap().text,
            "- foo\n    - bar\n        - lol"
        );
    }

    #[test]
    fn slack_list_looking_lines_in_code_untouched() {
        // A `- item` inside a fence and one inside an indented code
        // block are code content, not lists — indentation verbatim.
        let c = "```\n  - item\n```\n\n    - code item\n\nafter\n";
        assert_eq!(
            slack_export(c, None).unwrap().text,
            "```\n  - item\n```\n\n    - code item\n\nafter"
        );
    }

    #[test]
    fn slack_heading_interrupts_list_reset() {
        let c = "- a\n# Heading\n  - b\n";
        assert_eq!(slack_export(c, None).unwrap().text, "- a\n*Heading*\n- b");
    }

    #[test]
    fn slack_nested_task_checkbox_dropped_in_document() {
        let c = "- parent\n    - [x] done\n";
        assert_eq!(slack_export(c, None).unwrap().text, "- parent\n    - done");
    }

    #[test]
    fn commonmark_list_unchanged_by_list_tracking() {
        // The tracker runs for every target, but only Slack consumes
        // it — a 2-space nested list stays verbatim in commonmark.
        let c = "- foo\n  - bar\n    - lol\n- baz\n";
        assert_eq!(
            export_content(c, None, &CommonMarkExport).unwrap().text,
            "- foo\n  - bar\n    - lol\n- baz"
        );
    }

    // ── soft-break resolution: classifier ────────────────────────────

    #[test]
    fn classify_every_kind() {
        use BlockKind::{Blank, Blockquote, Break, Code, Heading, ListItem, Paragraph};
        assert_eq!(classify("", false), Blank);
        assert_eq!(classify("   ", false), Blank);
        assert_eq!(classify("# H", false), Heading);
        assert_eq!(classify("###", false), Heading);
        assert_eq!(classify("- item", false), ListItem);
        assert_eq!(classify("1. item", false), ListItem);
        assert_eq!(classify("  - nested", false), ListItem);
        assert_eq!(
            classify("> quote", false),
            Blockquote {
                callout_title: false,
                inner_list: false
            }
        );
        assert_eq!(
            classify("> [!note] Title", false),
            Blockquote {
                callout_title: true,
                inner_list: false
            }
        );
        assert_eq!(
            classify("> - item", false),
            Blockquote {
                callout_title: false,
                inner_list: true
            }
        );
        assert_eq!(
            classify(">> deep", false),
            Blockquote {
                callout_title: false,
                inner_list: false
            }
        );
        assert_eq!(classify("---", false), Break);
        assert_eq!(classify("--", false), Break);
        assert_eq!(classify("plain text", false), Paragraph);
        assert_eq!(classify("  continuation", false), Paragraph);
        assert_eq!(classify("```rust", true), Code);
        assert_eq!(classify("    indented", true), Code);
    }

    #[test]
    fn unwrap_policy_per_target() {
        assert!(SlackExport.unwrap_soft_wraps());
        assert!(!CommonMarkExport.unwrap_soft_wraps());
    }

    // ── soft-break resolution: driver ────────────────────────────────

    fn slack_join(doc: &str) -> String {
        export_content(doc, None, &SlackExport).unwrap().text
    }

    fn cm_join(doc: &str) -> String {
        export_content_with(doc, None, &CommonMarkExport, Some(true))
            .unwrap()
            .text
    }

    #[test]
    fn wrapped_paragraph_joins_slack_default() {
        assert_eq!(
            slack_join("first line\nsecond line\n\nafter\n"),
            "first line second line\n\nafter"
        );
    }

    #[test]
    fn wrapped_list_item_joins_user_example() {
        let c = "- line items that are longer than the column width and\n  thus are broken with an indent on the following line\n  to continue\n  - we can still indent and have a sub item that\n    follows the same rules\n- and return to first level\n";
        assert_eq!(
            slack_join(c),
            "- line items that are longer than the column width and thus are broken with an indent on the following line to continue\n    - we can still indent and have a sub item that follows the same rules\n- and return to first level"
        );
    }

    #[test]
    fn wrapped_task_item_joins() {
        assert_eq!(
            slack_join("- [ ] ⏫ task that\n  continues here\n"),
            "- ⏫ task that continues here"
        );
    }

    #[test]
    fn nested_items_do_not_join() {
        assert_eq!(slack_join("- a\n  - b\n"), "- a\n    - b");
    }

    #[test]
    fn unindented_paragraph_after_item_is_new_paragraph() {
        assert_eq!(
            slack_join("- item\nnew paragraph\n"),
            "- item\nnew paragraph"
        );
    }

    #[test]
    fn blank_lines_separate_paragraphs() {
        assert_eq!(
            slack_join("one\ntwo\n\nthree\nfour\n"),
            "one two\n\nthree four"
        );
    }

    #[test]
    fn hard_breaks_preserved_backslash() {
        assert_eq!(slack_join("line one\\\nline two\n"), "line one\\\nline two");
    }

    #[test]
    fn hard_breaks_preserved_trailing_spaces() {
        assert_eq!(
            slack_join("line three  \nline four\n"),
            "line three  \nline four"
        );
    }

    #[test]
    fn code_blocks_never_joined() {
        assert_eq!(
            slack_join("```\nline one\nline two\n```\n\nafter\n"),
            "```\nline one\nline two\n```\n\nafter"
        );
        assert_eq!(
            slack_join("intro\n\n    code one\n    code two\n\nafter\n"),
            "intro\n\n    code one\n    code two\n\nafter"
        );
    }

    #[test]
    fn callout_title_does_not_absorb_body() {
        assert_eq!(
            slack_join("> [!note] Title\n> body line one\n> body line two\n"),
            "> Title\n> body line one body line two"
        );
    }

    #[test]
    fn quoted_paragraph_joins() {
        assert_eq!(
            slack_join("> quoted line one\n> quoted line two\n"),
            "> quoted line one quoted line two"
        );
    }

    #[test]
    fn quote_inner_list_items_do_not_join() {
        assert_eq!(
            slack_join("> - item one\n> - item two\n"),
            "> - item one\n> - item two"
        );
        // An item's indented continuation joins into it.
        assert_eq!(
            slack_join("> - item one\n>   continuation\n"),
            "> - item one continuation"
        );
    }

    #[test]
    fn empty_quote_line_is_boundary() {
        assert_eq!(slack_join("> one\n>\n> two\n"), "> one\n>\n> two");
    }

    #[test]
    fn heading_does_not_absorb_following_paragraph() {
        assert_eq!(
            slack_join("# Heading\nwrapped first\nwrapped second\n"),
            "*Heading*\nwrapped first wrapped second"
        );
    }

    #[test]
    fn commonmark_verbatim_by_default() {
        assert_eq!(
            export_content("wrapped first\nwrapped second\n", None, &CommonMarkExport)
                .unwrap()
                .text,
            "wrapped first\nwrapped second"
        );
    }

    #[test]
    fn commonmark_unwrap_joins() {
        assert_eq!(
            cm_join("wrapped first\nwrapped second\n"),
            "wrapped first wrapped second"
        );
    }

    #[test]
    fn slack_no_unwrap_restores_verbatim() {
        assert_eq!(
            export_content_with(
                "- item that is long\n  and continues\n",
                None,
                &SlackExport,
                Some(false)
            )
            .unwrap()
            .text,
            "- item that is long\n  and continues"
        );
    }

    #[test]
    fn mid_block_range_starts_fresh() {
        let c = "first paragraph line\nsecond paragraph line\n\nafter\n";
        assert_eq!(
            export_content(c, Some((2, 2)), &SlackExport).unwrap().text,
            "second paragraph line"
        );
    }

    #[test]
    fn dropped_header_acts_as_boundary_under_join() {
        let c = "para one\n> [!ft-source] \"a.md\" L1-1 @aaaaaaa #aaaaaa\npara two\n";
        assert_eq!(cm_join(c), "para one\npara two");
    }
}

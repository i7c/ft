//! Hierarchical `ft:` frontmatter namespace.
//!
//! All ft-owned frontmatter keys live under one YAML map:
//!
//! ```yaml
//! ---
//! ft:
//!   tasks:
//!     section: Tasks        # was ft-tasks-section
//!   append:
//!     section: Sessions     # was ft-append-section
//!   synth:
//!     enabled: true         # was ft-synth: true
//!     targets: ["[[Foo]]"]  # was ft-synth-targets
//! ---
//! ```
//!
//! Readers are nested-only: the legacy flat `ft-*` top-level keys are
//! NOT recognized (a deliberate breaking cutover). Writers emit the
//! nested form and clean up legacy flat keys they own (see
//! [`crate::synth::callout::upsert_synth_frontmatter`]).
//!
//! This module is a deliberately lightweight string-level extractor —
//! ft only ever reads ~4 known keys, so a full YAML dependency
//! (`serde_yaml`) isn't justified. It is indentation-aware: a child
//! key is recognized when indented strictly more than its parent
//! `key:` line. Tabs and spaces both count as indentation (the indent
//! width is compared by leading-whitespace byte length).

/// The 1-indexed line number of the closing fence of a well-formed
/// leading frontmatter block (`---` … `---` at the very top of the
/// file), or `None` when there is none.
///
/// Line-based recognition, matching [`crate::markdown::LineSkipState`]:
/// the opener is line 1 trimming to `---`; the closer is the first
/// later line trimming to `---` or `...` (YAML doc-end). Used by the
/// export pipeline to clamp line ranges to the first body line.
pub fn frontmatter_end_line(content: &str) -> Option<u32> {
    let mut lines = content.split('\n');
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut lineno = 1u32;
    for line in lines {
        lineno += 1;
        let t = line.trim_end();
        if t == "---" || t == "..." {
            return Some(lineno);
        }
    }
    None
}

/// Read `ft.tasks.section` (the heading new tasks land under).
///
/// Returns `None` when there is no frontmatter, no `ft:` map, or no
/// `ft.tasks.section` key. The legacy flat `ft-tasks-section` key is
/// ignored.
pub fn ft_tasks_section(content: &str) -> Option<String> {
    let fm = block(content)?;
    ft_tasks_section_in(fm)
}

/// Same as [`ft_tasks_section`] but takes the frontmatter block (as
/// returned by [`block`] / captured by the scan in
/// `crate::scan::ParsedFile::frontmatter`) instead of full content — no
/// re-scanning needed.
pub fn ft_tasks_section_in(fm: &str) -> Option<String> {
    let ft = nested_value(fm, "ft")?;
    let tasks = nested_value(ft, "tasks")?;
    scalar_value(tasks, "section")
}

/// Read `ft.tasks.anchor`: the `🆔` of the task that stands for this
/// whole note when another task links `[[note]]` as its parent.
pub fn ft_tasks_anchor(content: &str) -> Option<String> {
    let fm = block(content)?;
    ft_tasks_anchor_in(fm)
}

/// Same as [`ft_tasks_anchor`] but takes the frontmatter block directly.
pub fn ft_tasks_anchor_in(fm: &str) -> Option<String> {
    let ft = nested_value(fm, "ft")?;
    let tasks = nested_value(ft, "tasks")?;
    scalar_value(tasks, "anchor")
}

/// Read `ft.append.section` (the default append-section heading).
///
/// Returns `None` when there is no frontmatter, no `ft:` map, or no
/// `ft.append.section` key. The legacy flat `ft-append-section` key is
/// ignored.
pub fn ft_append_section(content: &str) -> Option<String> {
    let fm = block(content)?;
    ft_append_section_in(fm)
}

/// Same as [`ft_append_section`] but takes the frontmatter block
/// directly.
pub fn ft_append_section_in(fm: &str) -> Option<String> {
    let ft = nested_value(fm, "ft")?;
    let append = nested_value(ft, "append")?;
    scalar_value(append, "section")
}

/// Read `ft.synth.enabled` (the synth-note marker).
///
/// Returns `Some(true)` only when the key is present and its value
/// is `true` (lenient on quotes/whitespace/case). `false`, absent,
/// or no frontmatter all yield `None` (treated as "not a synth note"
/// by [`crate::synth::callout::is_synth_note`]). The legacy flat
/// `ft-synth:` key is ignored.
pub fn ft_synth_enabled(content: &str) -> Option<bool> {
    let fm = block(content)?;
    ft_synth_enabled_in(fm)
}

/// Same as [`ft_synth_enabled`] but takes the frontmatter block
/// directly.
pub fn ft_synth_enabled_in(fm: &str) -> Option<bool> {
    let ft = nested_value(fm, "ft")?;
    let synth = nested_value(ft, "synth")?;
    let raw = scalar_value(synth, "enabled")?;
    let v = raw
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_lowercase();
    match v.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Read `ft.synth.targets` (the synth-note source set) as a list of
/// raw wikilink strings.
///
/// Returns `None` when the key is absent (or there is no frontmatter /
/// `ft:` map / `ft.synth` map). Returns `Some(vec![])` for an empty
/// sequence. Lenient on quote styles: accepts `"[[Foo]]"`, `'[[Foo]]'`,
/// `[[Foo]]`, `Foo`, and a mix, in either flow-sequence
/// (`["[[Foo]]", "[[Bar]]"]`) or block-sequence (`- "[[Foo]]"`) form.
/// The legacy flat `ft-synth-targets` key is ignored.
pub fn ft_synth_targets(content: &str) -> Option<Vec<String>> {
    let fm = block(content)?;
    ft_synth_targets_in(fm)
}

/// Same as [`ft_synth_targets`] but takes the frontmatter block
/// directly.
pub fn ft_synth_targets_in(fm: &str) -> Option<Vec<String>> {
    let ft = nested_value(fm, "ft")?;
    let synth = nested_value(ft, "synth")?;
    // First check for an inline flow sequence on the `targets:` key line
    // (e.g. `targets: ["[[Foo]]", "[[Bar]]"]`).
    if let Some(inline) = inline_scalar(synth, "targets") {
        let t = inline.trim();
        if let Some(rest) = t.strip_prefix('[') {
            let close = rest.rfind(']')?;
            return Some(parse_flow_seq_items(&rest[..close]));
        }
        // An inline non-sequence value is not a valid targets list.
        return None;
    }
    // Otherwise parse the block-sequence children.
    let block = nested_block(synth, "targets")?;
    parse_sequence(&block)
}

// ── core extraction ──────────────────────────────────────────────────

/// The raw frontmatter body text (between the opening and closing
/// `---` fences), or `None` if `content` doesn't start with a
/// well-formed `---\n…\n---` block.
///
/// The block is the text the [`*_in`](frontmatter::ft_tasks_section_in)
/// reader variants operate on: a scan captures it once per file
/// (`ParsedFile::frontmatter`) so consumers resolve keys without
/// re-reading or re-scanning the file.
pub fn block(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("---")?;
    // The opening `---` must be followed by `\n` (Obsidian also accepts
    // `---\r\n`).
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// Find the `key:` line at the parent's child-indent level and return
/// `(byte_start_of_children, byte_end_of_children, key_indent)`.
///
/// `parent` is the text of a parent map (already sliced to its indented
/// children). The parent's children are the lines at the first non-empty
/// line's indent. A `key:` at exactly that indent is a direct child; its
/// value block is the run of following lines indented strictly more than
/// the `key:` line (blank lines tolerated within).
///
/// Returns `None` when `key:` is not present at the child indent.
fn child_block_bounds(parent: &str, key: &str) -> Option<(usize, usize, usize)> {
    let needle = format!("{key}:");
    let lines: Vec<&str> = parent.lines().collect();
    let child_indent = lines
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| leading_ws(l).len())?;
    // Find the first line at child_indent that is `key:` or `key: ...`.
    let key_line_idx = lines.iter().position(|l| {
        if leading_ws(l).len() != child_indent {
            return false;
        }
        let lt = l.trim_start();
        lt.strip_prefix(&needle)
            .is_some_and(|r| r.is_empty() || r.starts_with(' '))
    })?;
    let key_indent_len = leading_ws(lines[key_line_idx]).len();
    let start = line_byte_start(parent, key_line_idx + 1);
    let mut end = parent.len();
    for (i, line) in lines.iter().enumerate().skip(key_line_idx + 1) {
        if line.trim().is_empty() {
            continue;
        }
        if leading_ws(line).len() <= key_indent_len {
            end = line_byte_start(parent, i);
            break;
        }
    }
    Some((start, end, key_indent_len))
}

/// Borrowed slice of a child map's block (descend one level).
fn nested_value<'a>(parent: &'a str, key: &str) -> Option<&'a str> {
    let (start, end, _) = child_block_bounds(parent, key)?;
    Some(&parent[start..end])
}

/// Owned, quote/blank-trimmed block of a child map's block — used when the
/// caller needs to parse a sequence value out of the indented children.
fn nested_block(parent: &str, key: &str) -> Option<String> {
    let (start, end, _) = child_block_bounds(parent, key)?;
    let mut block = &parent[start..end];
    // Trim trailing whitespace/newlines so empty-block checks work.
    while block.ends_with('\n') || block.ends_with('\r') {
        block = &block[..block.len() - 1];
    }
    Some(block.to_string())
}

/// Read the inline value (text after `key:`) from a `key:` line at the
/// parent's child-indent level. Returns `None` when the key is absent or
/// has no inline value (block form). Does NOT strip quotes — the caller
/// decides how to interpret the raw value.
fn inline_scalar(parent: &str, key: &str) -> Option<String> {
    let needle = format!("{key}:");
    let lines: Vec<&str> = parent.lines().collect();
    let child_indent = lines
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| leading_ws(l).len())?;
    for line in &lines {
        if leading_ws(line).len() != child_indent {
            continue;
        }
        let lt = line.trim_start();
        if let Some(rest) = lt.strip_prefix(&needle) {
            let val = rest.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

/// Read a scalar `key: value` from a parent map block, looking only at
/// direct children (the parent's child-indent level). Strips surrounding
/// quotes. Returns `None` when the key is absent or its value is empty.
fn scalar_value(parent: &str, key: &str) -> Option<String> {
    let needle = format!("{key}:");
    let lines: Vec<&str> = parent.lines().collect();
    let child_indent = lines
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| leading_ws(l).len())?;
    for line in &lines {
        if leading_ws(line).len() != child_indent {
            continue;
        }
        let lt = line.trim_start();
        if let Some(rest) = lt.strip_prefix(&needle) {
            let val = rest.trim();
            let val = val.trim_matches('"').trim_matches('\'').trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

/// Parse a YAML sequence value (flow or block form) from the block
/// belonging to a `targets:` key.
///
/// `block` is the joined lines that are children of the `targets:` key.
/// The first non-blank line may be a flow sequence `[a, b]` (inline on
/// the key line is handled by the caller passing the post-colon text)
/// — but since [`ft_synth_targets`] pulls the child block, a flow
/// sequence appears as a single child line starting with `[`.
fn parse_sequence(block: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = block.lines().collect();
    // Flow sequence: a single line (possibly the key's inline value)
    // starting with `[`.
    for line in &lines {
        let t = line.trim_start();
        if t.is_empty() {
            continue;
        }
        if let Some(rest) = t.strip_prefix('[') {
            let close = rest.rfind(']')?;
            let inner = &rest[..close];
            let items = parse_flow_seq_items(inner);
            return Some(items);
        }
        // Block sequence: lines starting with `-`.
        break;
    }
    // Block sequence: collect `- <item>` lines.
    let mut items: Vec<String> = Vec::new();
    for line in &lines {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix('-') {
            let val = rest.trim();
            if val.is_empty() {
                continue;
            }
            items.push(strip_yaml_scalar_quotes(val).to_string());
        } else if t.is_empty() {
            continue;
        } else {
            break;
        }
    }
    Some(items)
}

/// Parse the inner portion of a YAML flow sequence `[a, b, c]` into
/// scalar strings, stripping surrounding quotes. Commas inside quotes
/// are respected.
fn parse_flow_seq_items(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote: Option<char> = None;
    for c in inner.chars() {
        match in_quote {
            Some(q) => {
                if c == q {
                    in_quote = None;
                } else {
                    cur.push(c);
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    in_quote = Some(c);
                } else if c == ',' {
                    let trimmed = cur.trim();
                    if !trimmed.is_empty() {
                        out.push(strip_yaml_scalar_quotes(trimmed).to_string());
                    }
                    cur.clear();
                } else {
                    cur.push(c);
                }
            }
        }
    }
    let trimmed = cur.trim();
    if !trimmed.is_empty() {
        out.push(strip_yaml_scalar_quotes(trimmed).to_string());
    }
    out
}

/// Strip surrounding single or double quotes from a YAML scalar value.
/// Internal quotes are preserved.
fn strip_yaml_scalar_quotes(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        let first = bytes[0] as char;
        let last = bytes[s.len() - 1] as char;
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// The leading whitespace (spaces and tabs) of a line, as a `&str`.
fn leading_ws(line: &str) -> &str {
    let idx = line
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(i, _)| i)
        .unwrap_or(line.len());
    &line[..idx]
}

/// Byte offset of the start of line `line_idx` (0-indexed) within
/// `text`, where lines are split on `\n`.
fn line_byte_start(text: &str, line_idx: usize) -> usize {
    let mut start = 0;
    let mut current = 0;
    for (i, c) in text.char_indices() {
        if current == line_idx {
            return i;
        }
        if c == '\n' {
            current += 1;
            start = i + 1;
        }
    }
    if current == line_idx {
        start
    } else {
        text.len()
    }
}

// ── writing ft.* keys ────────────────────────────────────────────────

/// A value written under an `ft.<sub>` key: a scalar or a nested map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FmValue {
    Scalar(String),
    Map(Vec<(String, FmValue)>),
}

impl FmValue {
    pub fn scalar(v: impl Into<String>) -> Self {
        FmValue::Scalar(v.into())
    }
}

/// Set (`Some`) or remove (`None`) keys of the `ft.<sub>` map in `content`'s
/// frontmatter, leaving every other line untouched. Creates the frontmatter
/// fence, the `ft:` map, and the `<sub>:` map as needed; new keys are
/// appended after existing ones in `updates` order. Scalars are quoted only
/// when YAML needs it.
pub fn upsert_ft_keys(content: &str, sub: &str, updates: &[(&str, Option<FmValue>)]) -> String {
    let mut lines: Vec<String> = content.split('\n').map(str::to_string).collect();
    if lines.first().map(String::as_str) != Some("---") || fence_close(&lines).is_none() {
        let mut fresh = vec!["---".to_string(), "---".to_string()];
        if !(lines.len() == 1 && lines[0].is_empty()) {
            fresh.append(&mut lines);
        } else {
            fresh.push(String::new());
        }
        lines = fresh;
    }
    let mut end = fence_close(&lines).expect("fence ensured above");

    let ft = match (1..end).find(|&i| lines[i] == "ft:" || lines[i].starts_with("ft: ")) {
        Some(i) => i,
        None => {
            lines.insert(end, "ft:".to_string());
            end += 1;
            end - 1
        }
    };
    let ft_child = child_indent(&lines, ft, end).unwrap_or(2);
    let sub_line = format!("{}{sub}:", " ".repeat(ft_child));
    let sub_idx = match (ft + 1..block_end(&lines, ft, 0, end)).find(|&i| lines[i] == sub_line) {
        Some(i) => i,
        None => {
            let at = block_end(&lines, ft, 0, end);
            lines.insert(at, sub_line);
            end += 1;
            at
        }
    };
    let key_indent = child_indent(&lines, sub_idx, end).unwrap_or(ft_child + 2);

    for (key, value) in updates {
        let sub_end = block_end(&lines, sub_idx, ft_child, end);
        let prefix = format!("{}{key}:", " ".repeat(key_indent));
        let existing = (sub_idx + 1..sub_end)
            .find(|&i| lines[i] == prefix || lines[i].starts_with(&format!("{prefix} ")));
        let rendered = value
            .as_ref()
            .map(|v| render_entry(key, v, key_indent))
            .unwrap_or_default();
        let (at, remove) = match existing {
            Some(i) => (i, block_end(&lines, i, key_indent, end) - i),
            None => (sub_end, 0),
        };
        let added = rendered.len();
        lines.splice(at..at + remove, rendered);
        end = end + added - remove;
    }
    lines.join("\n")
}

/// Index of the closing `---` fence (the first after line 0).
fn fence_close(lines: &[String]) -> Option<usize> {
    lines.iter().skip(1).position(|l| l == "---").map(|p| p + 1)
}

/// Indent of the first child line of the map opened at `idx`, if any.
fn child_indent(lines: &[String], idx: usize, end: usize) -> Option<usize> {
    let own = leading_ws(&lines[idx]).len();
    lines[idx + 1..end]
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| leading_ws(l).len())
        .filter(|&n| n > own)
}

/// One past the last line belonging to the entry at `idx` (indented `indent`):
/// following lines indented deeper (blank lines inside count).
fn block_end(lines: &[String], idx: usize, indent: usize, end: usize) -> usize {
    let mut j = idx + 1;
    let mut last = idx + 1;
    while j < end {
        let l = &lines[j];
        if l.trim().is_empty() {
            j += 1;
            continue;
        }
        if leading_ws(l).len() <= indent {
            break;
        }
        j += 1;
        last = j;
    }
    last
}

fn render_entry(key: &str, value: &FmValue, indent: usize) -> Vec<String> {
    let pad = " ".repeat(indent);
    match value {
        FmValue::Scalar(v) => vec![format!("{pad}{key}: {}", yaml_scalar(v))],
        FmValue::Map(entries) => {
            let mut out = vec![format!("{pad}{key}:")];
            for (k, v) in entries {
                out.extend(render_entry(k, v, indent + 2));
            }
            out
        }
    }
}

/// `v` as a YAML scalar, double-quoted when a plain scalar would be read
/// differently (empty, leading indicator, `: ` / ` #`, booleans/null).
fn yaml_scalar(v: &str) -> String {
    let needs_quotes = v.is_empty()
        || v != v.trim()
        || v.starts_with(|c: char| "[]{},&*!|>'\"%@`#-?:".contains(c))
        || v.contains(": ")
        || v.contains(" #")
        || v.contains('"')
        || v.ends_with(':')
        || matches!(
            v.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "null" | "~" | "on" | "off"
        );
    if needs_quotes {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        v.to_string()
    }
}

/// The `ft.source` block of an issue note.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FtSource {
    pub name: String,
    pub key: String,
    pub url: Option<String>,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub state: Option<String>,
    pub updated: Option<String>,
    pub synced_status: Option<String>,
    /// Remote status recorded when both sides changed (a conflict).
    pub conflict: Option<String>,
}

/// Read `ft.source` (requires `name` and `key`).
pub fn ft_source_in(fm: &str) -> Option<FtSource> {
    let ft = nested_value(fm, "ft")?;
    let src = nested_value(ft, "source")?;
    let get = |k: &str| inline_scalar(src, k).and_then(|v| parse_scalar(&v));
    Some(FtSource {
        name: get("name")?,
        key: get("key")?,
        url: get("url"),
        status: get("status"),
        assignee: get("assignee"),
        state: get("state"),
        updated: get("updated"),
        conflict: get("conflict"),
        synced_status: nested_value(src, "synced")
            .and_then(|s| inline_scalar(s, "status"))
            .and_then(|v| parse_scalar(&v)),
    })
}

/// Read `ft.tasks.parent` (the parent issue's `🆔`, e.g. `EPIC-7`).
pub fn ft_tasks_parent_in(fm: &str) -> Option<String> {
    let ft = nested_value(fm, "ft")?;
    let tasks = nested_value(ft, "tasks")?;
    inline_scalar(tasks, "parent").and_then(|v| parse_scalar(&v))
}

/// Parse a raw inline YAML scalar: double-quoted (with `\\` / `\"`
/// escapes), single-quoted (`''` escape), or plain. Empty values are `None`.
fn parse_scalar(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let v = if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        let mut out = String::new();
        let mut chars = raw[1..raw.len() - 1].chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else {
                out.push(c);
            }
        }
        out
    } else if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        raw[1..raw.len() - 1].replace("''", "'")
    } else {
        raw.to_string()
    };
    (!v.is_empty()).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fm(body: &str) -> String {
        format!("---\n{body}\n---\n\n# Body\n")
    }

    // ── block-taking readers (`*_in`) ──────────────────────────────

    #[test]
    fn block_in_variants_agree_with_content_readers() {
        let c = fm(
            "ft:\n  tasks:\n    section: Tasks\n  append:\n    section: Sessions\n  synth:\n    enabled: true\n    targets: [\"[[Foo]]\", \"[[Bar]]\"]\n",
        );
        let b = block(&c).unwrap();
        assert_eq!(ft_tasks_section(&c), ft_tasks_section_in(b));
        assert_eq!(ft_append_section(&c), ft_append_section_in(b));
        assert_eq!(ft_synth_enabled(&c), ft_synth_enabled_in(b));
        assert_eq!(ft_synth_targets(&c), ft_synth_targets_in(b));
        assert_eq!(ft_synth_enabled_in(b), Some(true));
    }

    #[test]
    fn block_in_handles_block_without_content() {
        let c = fm("ft:\n  synth:\n    enabled: true\n");
        let b = block(&c).unwrap();
        assert_eq!(ft_synth_enabled_in(b), Some(true));
        assert_eq!(ft_tasks_section_in(b), None);
    }

    #[test]
    fn block_returns_none_without_frontmatter() {
        assert_eq!(block("no frontmatter"), None);
    }

    // ── ft_tasks_section ───────────────────────────────────────────

    #[test]
    fn tasks_anchor_nested_beside_section() {
        let c = fm("ft:\n  tasks:\n    anchor: ABC-123\n    section: Tasks\n");
        assert_eq!(ft_tasks_anchor(&c), Some("ABC-123".to_string()));
        assert_eq!(ft_tasks_section(&c), Some("Tasks".to_string()));
    }

    #[test]
    fn tasks_anchor_flat_key_ignored() {
        let c = fm("ft-tasks-anchor: ABC-123\n");
        assert_eq!(ft_tasks_anchor(&c), None);
    }

    #[test]
    fn tasks_section_nested() {
        let c = fm("ft:\n  tasks:\n    section: Tasks\n");
        assert_eq!(ft_tasks_section(&c), Some("Tasks".to_string()));
    }

    #[test]
    fn tasks_section_quoted() {
        let c = fm("ft:\n  tasks:\n    section: \"My Tasks\"\n");
        assert_eq!(ft_tasks_section(&c), Some("My Tasks".to_string()));
    }

    #[test]
    fn tasks_section_absent() {
        let c = fm("ft:\n  append:\n    section: Log\n");
        assert_eq!(ft_tasks_section(&c), None);
    }

    #[test]
    fn tasks_section_no_ft_map() {
        let c = fm("title: Foo\n");
        assert_eq!(ft_tasks_section(&c), None);
    }

    #[test]
    fn tasks_section_no_frontmatter() {
        assert_eq!(ft_tasks_section("# just a heading\n"), None);
    }

    #[test]
    fn tasks_section_legacy_flat_ignored() {
        let c = "---\nft-tasks-section: Tasks\n---\n# body\n";
        assert_eq!(ft_tasks_section(c), None);
    }

    // ── ft_append_section ──────────────────────────────────────────

    #[test]
    fn append_section_nested() {
        let c = fm("ft:\n  append:\n    section: Sessions\n");
        assert_eq!(ft_append_section(&c), Some("Sessions".to_string()));
    }

    #[test]
    fn append_section_legacy_flat_ignored() {
        let c = "---\nft-append-section: Sessions\n---\n# body\n";
        assert_eq!(ft_append_section(c), None);
    }

    // ── ft_synth_enabled ────────────────────────────────────────────

    #[test]
    fn synth_enabled_true() {
        let c = fm("ft:\n  synth:\n    enabled: true\n");
        assert_eq!(ft_synth_enabled(&c), Some(true));
    }

    #[test]
    fn synth_enabled_quoted_true() {
        let c = fm("ft:\n  synth:\n    enabled: \"true\"\n");
        assert_eq!(ft_synth_enabled(&c), Some(true));
    }

    #[test]
    fn synth_enabled_false() {
        let c = fm("ft:\n  synth:\n    enabled: false\n");
        assert_eq!(ft_synth_enabled(&c), Some(false));
    }

    #[test]
    fn synth_enabled_absent() {
        let c = fm("ft:\n  tasks:\n    section: Tasks\n");
        assert_eq!(ft_synth_enabled(&c), None);
    }

    #[test]
    fn synth_enabled_legacy_flat_ignored() {
        let c = "---\nft-synth: true\n---\n# body\n";
        assert_eq!(ft_synth_enabled(c), None);
    }

    #[test]
    fn synth_enabled_no_frontmatter() {
        assert_eq!(ft_synth_enabled("# heading\n"), None);
    }

    // ── ft_synth_targets ────────────────────────────────────────────

    #[test]
    fn synth_targets_flow_quoted() {
        let c = fm("ft:\n  synth:\n    enabled: true\n    targets: [\"[[Foo]]\", \"[[Bar]]\"]\n");
        assert_eq!(
            ft_synth_targets(&c),
            Some(vec!["[[Foo]]".into(), "[[Bar]]".into()])
        );
    }

    #[test]
    fn synth_targets_flow_bare() {
        let c = fm("ft:\n  synth:\n    targets: [Foo, Bar]\n");
        assert_eq!(ft_synth_targets(&c), Some(vec!["Foo".into(), "Bar".into()]));
    }

    #[test]
    fn synth_targets_block_sequence() {
        let c = fm(
            "ft:\n  synth:\n    enabled: true\n    targets:\n      - \"[[Foo]]\"\n      - Bar\n",
        );
        assert_eq!(
            ft_synth_targets(&c),
            Some(vec!["[[Foo]]".into(), "Bar".into()])
        );
    }

    #[test]
    fn synth_targets_empty_flow() {
        let c = fm("ft:\n  synth:\n    targets: []\n");
        assert_eq!(ft_synth_targets(&c), Some(Vec::new()));
    }

    #[test]
    fn synth_targets_absent() {
        let c = fm("ft:\n  synth:\n    enabled: true\n");
        assert_eq!(ft_synth_targets(&c), None);
    }

    #[test]
    fn synth_targets_legacy_flat_ignored() {
        let c = "---\nft-synth-targets: [\"[[Foo]]\"]\n---\n# body\n";
        assert_eq!(ft_synth_targets(c), None);
    }

    // ── coexistence / mixed ────────────────────────────────────────

    #[test]
    fn all_four_keys_together() {
        let c = fm(
            "ft:\n  tasks:\n    section: Tasks\n  append:\n    section: Log\n  synth:\n    enabled: true\n    targets: [\"[[Foo]]\"]\n",
        );
        assert_eq!(ft_tasks_section(&c), Some("Tasks".to_string()));
        assert_eq!(ft_append_section(&c), Some("Log".to_string()));
        assert_eq!(ft_synth_enabled(&c), Some(true));
        assert_eq!(ft_synth_targets(&c), Some(vec!["[[Foo]]".into()]));
    }

    #[test]
    fn preserves_unrelated_frontmatter() {
        // Unrelated keys at top level don't break parsing.
        let c = fm("title: My Note\ntags: [a, b]\nft:\n  synth:\n    enabled: true\n");
        assert_eq!(ft_synth_enabled(&c), Some(true));
        assert_eq!(ft_tasks_section(&c), None);
    }

    // ── quirks ─────────────────────────────────────────────────────

    #[test]
    fn crlf_line_endings() {
        let c = "---\r\nft:\r\n  synth:\r\n    enabled: true\r\n---\r\n\r\nbody\r\n";
        assert_eq!(ft_synth_enabled(c), Some(true));
    }

    #[test]
    fn tab_indentation() {
        // Tabs are valid YAML indentation in Obsidian's frontmatter.
        let c = "---\nft:\n\tsynth:\n\t\tenabled: true\n---\n";
        assert_eq!(ft_synth_enabled(c), Some(true));
    }

    #[test]
    fn extra_whitespace() {
        let c = fm("ft:\n  synth:\n    enabled:    true\n");
        assert_eq!(ft_synth_enabled(&c), Some(true));
    }

    // ── frontmatter_end_line ───────────────────────────────────────

    #[test]
    fn end_line_canonical_block() {
        let c = "---\nft:\n  synth:\n    enabled: true\n---\n# Body\n";
        assert_eq!(frontmatter_end_line(c), Some(5));
    }

    #[test]
    fn end_line_no_frontmatter() {
        assert_eq!(frontmatter_end_line("# Body\n"), None);
        assert_eq!(frontmatter_end_line(""), None);
    }

    #[test]
    fn end_line_ellipsis_closer() {
        let c = "---\ntitle: Foo\n...\n# Body\n";
        assert_eq!(frontmatter_end_line(c), Some(3));
    }

    #[test]
    fn end_line_crlf() {
        let c = "---\r\nft:\r\n  synth:\r\n    enabled: true\r\n---\r\nbody\r\n";
        assert_eq!(frontmatter_end_line(c), Some(5));
    }

    #[test]
    fn end_line_mid_file_dashes_not_frontmatter() {
        let c = "some prose\n---\nnot frontmatter\n";
        assert_eq!(frontmatter_end_line(c), None);
    }

    #[test]
    fn end_line_unterminated_block() {
        assert_eq!(frontmatter_end_line("---\nno close\n"), None);
    }

    #[test]
    fn end_line_empty_block() {
        let c = "---\n---\n# Body\n";
        assert_eq!(frontmatter_end_line(c), Some(2));
    }

    // ── upsert_ft_keys ─────────────────────────────────────────────

    fn src(v: &str) -> Option<FmValue> {
        Some(FmValue::scalar(v))
    }

    #[test]
    fn upsert_creates_fence_and_maps() {
        let out = upsert_ft_keys("# Note\n", "tasks", &[("anchor", src("ABC-123"))]);
        assert_eq!(
            out,
            "---\nft:\n  tasks:\n    anchor: ABC-123\n---\n# Note\n"
        );
        assert_eq!(ft_tasks_anchor(&out), Some("ABC-123".into()));
        let empty = upsert_ft_keys("", "tasks", &[("anchor", src("X-1"))]);
        assert_eq!(empty, "---\nft:\n  tasks:\n    anchor: X-1\n---\n");
    }

    #[test]
    fn upsert_preserves_siblings_and_updates_in_place() {
        let c = "---\ntags: [a]\nft:\n  tasks:\n    section: Tasks\n    anchor: OLD\n  append:\n    section: Log\naliases: x\n---\nbody\n";
        let out = upsert_ft_keys(
            c,
            "tasks",
            &[("anchor", src("ABC-123")), ("parent", src("[[EPIC-7]]"))],
        );
        assert_eq!(
            out,
            "---\ntags: [a]\nft:\n  tasks:\n    section: Tasks\n    anchor: ABC-123\n    parent: \"[[EPIC-7]]\"\n  append:\n    section: Log\naliases: x\n---\nbody\n"
        );
        assert_eq!(
            ft_tasks_parent_in(block(&out).unwrap()),
            Some("[[EPIC-7]]".into())
        );
        assert_eq!(ft_tasks_section(&out), Some("Tasks".into()));
        let removed = upsert_ft_keys(&out, "tasks", &[("parent", None)]);
        assert!(!removed.contains("parent:"));
    }

    #[test]
    fn upsert_nested_map_and_new_sub_block() {
        let c = "---\nft:\n  tasks:\n    anchor: ABC-123\n---\n";
        let synced = FmValue::Map(vec![("status".into(), FmValue::scalar("InProgress"))]);
        let out = upsert_ft_keys(
            c,
            "source",
            &[
                ("name", src("jira")),
                ("key", src("ABC-123")),
                ("status", src("In Review")),
                ("updated", src("2026-10-21T09:12:44.000+0000")),
                ("synced", Some(synced.clone())),
            ],
        );
        assert_eq!(
            out,
            "---\nft:\n  tasks:\n    anchor: ABC-123\n  source:\n    name: jira\n    key: ABC-123\n    status: In Review\n    updated: 2026-10-21T09:12:44.000+0000\n    synced:\n      status: InProgress\n---\n"
        );
        let s = ft_source_in(block(&out).unwrap()).unwrap();
        assert_eq!(s.synced_status.as_deref(), Some("InProgress"));
        assert_eq!(s.status.as_deref(), Some("In Review"));
        // Re-upserting the same values is a no-op.
        let again = upsert_ft_keys(&out, "source", &[("synced", Some(synced))]);
        assert_eq!(again, out);
    }

    #[test]
    fn yaml_quoting_round_trips() {
        for v in [
            "@mention",
            "a: b",
            "x #y",
            "true",
            "",
            "say \"hi\"",
            "- dash",
            "plain text",
        ] {
            let out = upsert_ft_keys("", "source", &[("name", src("n")), ("key", src(v))]);
            let s = ft_source_in(block(&out).unwrap());
            if v.is_empty() {
                assert!(s.is_none(), "empty key is treated as absent");
            } else {
                assert_eq!(s.unwrap().key, v, "{out}");
            }
        }
    }
}

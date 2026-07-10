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

/// Read `ft.tasks.section` (the heading new tasks land under).
///
/// Returns `None` when there is no frontmatter, no `ft:` map, or no
/// `ft.tasks.section` key. The legacy flat `ft-tasks-section` key is
/// ignored.
pub fn ft_tasks_section(content: &str) -> Option<String> {
    let fm = frontmatter_block(content)?;
    let ft = nested_value(fm, "ft")?;
    let tasks = nested_value(ft, "tasks")?;
    scalar_value(tasks, "section")
}

/// Read `ft.append.section` (the default append-section heading).
///
/// Returns `None` when there is no frontmatter, no `ft:` map, or no
/// `ft.append.section` key. The legacy flat `ft-append-section` key is
/// ignored.
pub fn ft_append_section(content: &str) -> Option<String> {
    let fm = frontmatter_block(content)?;
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
    let fm = frontmatter_block(content)?;
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
    let fm = frontmatter_block(content)?;
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
fn frontmatter_block(content: &str) -> Option<&str> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fm(body: &str) -> String {
        format!("---\n{body}\n---\n\n# Body\n")
    }

    // ── ft_tasks_section ───────────────────────────────────────────

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
}

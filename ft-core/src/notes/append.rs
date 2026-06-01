//! Append a rendered template into an existing markdown note.
//!
//! Two insertion strategies:
//! - **End of file** (the default) — `append_template(content, rendered, None)`.
//! - **After a named section** — `append_template(content, rendered, Some("Sessions"))`
//!   finds the first ATX heading whose trimmed text case-insensitively matches
//!   the supplied string (any heading level) and inserts after that section's
//!   body — i.e. immediately before the next heading of equal-or-higher level
//!   (or end of file).
//!
//! A per-note frontmatter key `ft-append-section` provides the default section
//! name when the caller doesn't supply an explicit override. Use
//! [`frontmatter_append_section`] to read it, then pass the result to
//! [`append_template`] as `section_heading`.

use crate::error::{Error, Result};
use crate::markdown::extract_headings;
use crate::notes::line_byte_offsets;

/// Append `rendered` to `content`.
///
/// Returns `(new_content, line_number)` where `line_number` is the
/// 1-indexed line where the first byte of `rendered` lands in
/// `new_content` — suitable for feeding to an editor `+<N>` argument.
///
/// When `section_heading` is `None`, the rendered text is appended to
/// the end of the file. A `\n` separator is prepended if the file
/// doesn't already end with one.
///
/// When `section_heading` is `Some(name)`, the function searches for
/// a heading whose text matches `name` (case-insensitive, trimmed) at
/// any ATX level, then inserts after that section's body. If no heading
/// matches, an error is returned.
pub fn append_template(
    content: &str,
    rendered: &str,
    section_heading: Option<&str>,
) -> Result<(String, usize)> {
    match section_heading {
        None => Ok(append_to_end(content, rendered)),
        Some(name) => append_to_section(content, rendered, name),
    }
}

/// Read the `ft-append-section` value from YAML frontmatter.
///
/// Returns `None` when there is no frontmatter block or the key is absent.
/// This is a lightweight string-level extraction — we don't pull in a
/// full YAML parser for a single key. The frontmatter block is defined as
/// the leading `---\n...\n---` region.
pub fn frontmatter_append_section(content: &str) -> Option<String> {
    let fm = extract_frontmatter_block(content)?;
    for line in fm.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("ft-append-section:") {
            let val = val.trim();
            // Strip optional surrounding quotes.
            let val = val.strip_prefix('"').unwrap_or(val);
            let val = val.strip_suffix('"').unwrap_or(val);
            let val = val.strip_prefix('\'').unwrap_or(val);
            let val = val.strip_suffix('\'').unwrap_or(val);
            let val = val.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

// ── helpers ───────────────────────────────────────────────────────────

/// Extract the raw frontmatter block from `content` if it starts with
/// `---\n`. Returns the inner text between the opening and closing `---`.
/// Returns `None` if the first line is not `---` or there is no closing
/// `---`.
fn extract_frontmatter_block(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("---")?;
    // The opening `---` must be the first line or immediately followed
    // by `\n` (Obsidian also accepts `---\r\n`).
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// Append `rendered` to the end of `content`. Returns `(new_content, line_number)`.
fn append_to_end(content: &str, rendered: &str) -> (String, usize) {
    let needs_sep = !content.is_empty() && !content.ends_with('\n');
    let base_lines = if content.is_empty() {
        0
    } else {
        content.lines().count()
    };
    let insert_line = if base_lines == 0 {
        1
    } else if needs_sep {
        // The `\n` separator adds a line; `rendered` starts on the next.
        base_lines + 1
    } else {
        // File already ends with `\n`; `rendered` starts on the next line.
        base_lines + 1
    };
    let mut out = String::with_capacity(content.len() + rendered.len() + 2);
    out.push_str(content);
    if needs_sep {
        out.push('\n');
    }
    out.push_str(rendered);
    (out, insert_line)
}

/// Find the section named `heading_name` and insert `rendered` after its
/// body. Returns `(new_content, line_number)` or an error if the heading
/// is not found.
fn append_to_section(content: &str, rendered: &str, heading_name: &str) -> Result<(String, usize)> {
    let needle = heading_name.trim().to_lowercase();
    let headings = extract_headings(content);

    // Find the first heading whose trimmed text matches case-insensitively.
    let target_idx = headings
        .iter()
        .position(|h| h.text.trim().to_lowercase() == needle)
        .ok_or_else(|| {
            Error::Notes(format!(
                "section heading {:?} not found in the target file",
                heading_name
            ))
        })?;

    let target = &headings[target_idx];
    let offsets = line_byte_offsets(content);

    // Where does this section end? Look for the next heading of equal or
    // higher level, or end of file.
    let insertion_byte = headings[target_idx + 1..]
        .iter()
        .find(|next| next.level <= target.level)
        .map(|next| offsets[next.line - 1])
        .unwrap_or(content.len());

    // Compute the 1-indexed line where the insertion lands.
    // Count lines up to `insertion_byte`, then `rendered` starts on the
    // next line.
    let lines_before = content[..insertion_byte].lines().count();
    // If the section body ends with a newline, the insertion point is
    // already at the start of the next line. If it doesn't, we're at
    // the last char — insertion_line = lines_before + 1 either way.
    let insert_line = if insertion_byte == content.len()
        && content
            .as_bytes()
            .get(insertion_byte.wrapping_sub(1))
            .is_none_or(|&b| b != b'\n')
    {
        // File doesn't end with newline and we're at EOF — rendered
        // starts on the next line after what's there.
        lines_before + 1
    } else if insertion_byte < content.len() {
        // We're at the start of the next heading line.
        lines_before + 1
    } else {
        // At EOF with trailing newline.
        lines_before + 1
    };

    // Build the new content: before insertion point + rendered + after.
    let mut out = String::with_capacity(content.len() + rendered.len() + 2);
    out.push_str(&content[..insertion_byte]);

    // Ensure a newline boundary before the rendered content if not at
    // start of file and the insertion point isn't already after a
    // newline.
    if insertion_byte > 0 && !content.as_bytes()[..insertion_byte].ends_with(b"\n") {
        out.push('\n');
    }

    out.push_str(rendered);

    // Ensure a newline boundary between rendered and the following content.
    let after = &content[insertion_byte..];
    if !rendered.ends_with('\n') && !after.is_empty() && !after.starts_with('\n') {
        out.push('\n');
    }

    out.push_str(after);
    Ok((out, insert_line))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── append_to_end ──────────────────────────────────────────────────

    #[test]
    fn append_to_end_normal() {
        let (new, line) = append_template("# Title\nbody\n", "## Added\nmore\n", None).unwrap();
        assert_eq!(new, "# Title\nbody\n## Added\nmore\n");
        assert_eq!(line, 3); // "## Added" is line 3
    }

    #[test]
    fn append_to_end_missing_trailing_newline() {
        let (new, line) = append_template("# Title\nfinal line", "## Added\n", None).unwrap();
        assert_eq!(new, "# Title\nfinal line\n## Added\n");
        // base_lines = 2, needs_sep = true → insert_line = 2 + 1 = 3
        assert_eq!(line, 3);
    }

    #[test]
    fn append_to_end_empty_file() {
        let (new, line) = append_template("", "# First\n", None).unwrap();
        assert_eq!(new, "# First\n");
        assert_eq!(line, 1);
    }

    #[test]
    fn append_to_end_already_ends_with_newline() {
        let (new, line) = append_template("text\n", "more\n", None).unwrap();
        assert_eq!(new, "text\nmore\n");
        assert_eq!(line, 2);
    }

    // ── append_to_section ──────────────────────────────────────────────

    #[test]
    fn append_to_section_basic() {
        let content =
            "# Intro\nintro body\n## Log\nlog line 1\nlog line 2\n## Footer\nfooter text\n";
        let (new, line) = append_template(content, "log line 3\n", Some("Log")).unwrap();
        assert_eq!(
            new,
            "# Intro\nintro body\n## Log\nlog line 1\nlog line 2\nlog line 3\n## Footer\nfooter text\n"
        );
        assert_eq!(line, 6); // "log line 3" should be line 6
    }

    #[test]
    fn append_to_section_different_level() {
        // Matches heading text regardless of ATX level.
        let content = "### Sessions\nsession content\n";
        let (new, line) = append_template(content, "new entry\n", Some("Sessions")).unwrap();
        assert_eq!(new, "### Sessions\nsession content\nnew entry\n");
        assert_eq!(line, 3);
    }

    #[test]
    fn append_to_section_case_insensitive() {
        let content = "## Log\nentry\n";
        let (new, _line) = append_template(content, "more\n", Some("log")).unwrap();
        assert_eq!(new, "## Log\nentry\nmore\n");
    }

    #[test]
    fn append_to_section_not_found() {
        let content = "# Title\nbody\n";
        let err = append_template(content, "x\n", Some("Nonexistent")).unwrap_err();
        assert!(matches!(err, Error::Notes(msg) if msg.contains("Nonexistent")));
    }

    #[test]
    fn append_to_section_first_match_wins() {
        // When multiple headings share the same text, use the first one.
        // "### Log" (level 3) is nested under "## Log" (level 2), so
        // the first "## Log" section extends through "### Log" to EOF.
        let content = "## Log\na\n### Log\nb\n";
        let (new, line) = append_template(content, "c\n", Some("Log")).unwrap();
        assert_eq!(new, "## Log\na\n### Log\nb\nc\n");
        assert_eq!(line, 5);
    }

    #[test]
    fn append_to_section_single_section_file() {
        // Only one heading in the file — section extends to EOF.
        let content = "# Note\njust one section\n";
        let (new, line) = append_template(content, "appended\n", Some("Note")).unwrap();
        assert_eq!(new, "# Note\njust one section\nappended\n");
        assert_eq!(line, 3);
    }

    #[test]
    fn append_to_section_with_nested_headings() {
        // Section includes nested headings until a sibling-or-higher.
        let content = "## Parent\nparent body\n### Child\nchild body\n## Sibling\nsib body\n";
        let (new, line) = append_template(content, "inserted\n", Some("Parent")).unwrap();
        assert_eq!(
            new,
            "## Parent\nparent body\n### Child\nchild body\ninserted\n## Sibling\nsib body\n"
        );
        assert_eq!(line, 5);
    }

    // ── frontmatter_append_section ─────────────────────────────────────

    #[test]
    fn frontmatter_extracts_section() {
        let content = "---\nft-append-section: Daily Log\n---\n# Title\n";
        assert_eq!(
            frontmatter_append_section(content),
            Some("Daily Log".to_string())
        );
    }

    #[test]
    fn frontmatter_quoted_value() {
        let content = "---\nft-append-section: \"Daily Log\"\n---\n# Title\n";
        assert_eq!(
            frontmatter_append_section(content),
            Some("Daily Log".to_string())
        );
    }

    #[test]
    fn frontmatter_single_quoted_value() {
        let content = "---\nft-append-section: 'Daily Log'\n---\n# Title\n";
        assert_eq!(
            frontmatter_append_section(content),
            Some("Daily Log".to_string())
        );
    }

    #[test]
    fn frontmatter_no_block() {
        let content = "# Just a heading\n";
        assert_eq!(frontmatter_append_section(content), None);
    }

    #[test]
    fn frontmatter_key_absent() {
        let content = "---\ntitle: My Note\n---\n# Title\n";
        assert_eq!(frontmatter_append_section(content), None);
    }

    #[test]
    fn frontmatter_empty_value_returns_none() {
        let content = "---\nft-append-section:\n---\n# Title\n";
        assert_eq!(frontmatter_append_section(content), None);
    }

    #[test]
    fn frontmatter_dash_in_value() {
        let content = "---\nft-append-section: Multi-word Section\n---\n# Title\n";
        assert_eq!(
            frontmatter_append_section(content),
            Some("Multi-word Section".to_string())
        );
    }
}

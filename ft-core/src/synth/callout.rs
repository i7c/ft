//! Grammar for `[!ft-source]` protected-section callouts.
//!
//! A protected section is a CommonMark-style blockquote that begins
//! with an Obsidian-style callout header literal `[!ft-source]` and is
//! followed by the verbatim quoted source paragraph. Round-trip
//! property: `parse(serialize(s)) == s` for any well-formed
//! `ProtectedSection`.
//!
//! See [`crate::synth`] for the higher-level context.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

/// Number of hex chars used for the short commit SHA in callout headers.
pub const SHORT_SHA_LEN: usize = 7;

/// Number of hex chars used for the blake3 content-hash prefix.
pub const CONTENT_HASH_PREFIX_LEN: usize = 6;

/// One protected section, scaffold-side. Carries the four header tokens
/// plus the verbatim source-paragraph body. Body is *unquoted* — the
/// `>` prefix that wraps each line in the on-disk markdown is added by
/// [`serialize`] and stripped by [`parse`]. Lines are joined with `\n`;
/// no trailing newline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedSection {
    /// Vault-relative path of the source file.
    pub source_path: PathBuf,
    /// 1-indexed inclusive start line in the source file at `commit_sha`.
    pub line_start: u32,
    /// 1-indexed inclusive end line.
    pub line_end: u32,
    /// 7-char (or longer) short commit SHA the section is pinned to.
    pub commit_sha: String,
    /// 6-char (or longer) blake3 content-hash prefix of `body`.
    pub content_hash: String,
    /// The source paragraph text, line-by-line joined with `\n`, no
    /// trailing newline.
    pub body: String,
}

/// A parsed callout found in some markdown source. Carries the same
/// header tokens as [`ProtectedSection`] plus the byte range in the
/// source for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCallout {
    pub source_path: PathBuf,
    pub line_start: u32,
    pub line_end: u32,
    pub commit_sha: String,
    pub content_hash: String,
    pub body: String,
    /// Byte range in the source where the entire callout (header line
    /// through last body line) lives. `&source[byte_range]` is the
    /// raw callout text including the trailing `\n` of the last line,
    /// when present.
    pub byte_range: std::ops::Range<usize>,
    /// 1-indexed line number of the callout's header in the source.
    pub header_line: u32,
}

/// Compute the blake3 content-hash prefix for a body string.
///
/// The body is hashed as-is (no normalization). Returns the first
/// [`CONTENT_HASH_PREFIX_LEN`] hex chars of the digest.
pub fn compute_section_hash(body: &str) -> String {
    let digest = blake3::hash(body.as_bytes());
    let hex = digest.to_hex();
    hex.as_str()[..CONTENT_HASH_PREFIX_LEN].to_string()
}

/// Render a [`ProtectedSection`] to its canonical markdown form.
///
/// Paths are always wrapped in double-quotes so the grammar stays
/// unambiguous regardless of whether the path contains spaces.
/// Literal `"` characters in the path are not supported (they would
/// break the grammar); callers should not pass such paths.
///
/// The output has no trailing newline; callers control how sections
/// are joined into the surrounding document.
pub fn serialize(section: &ProtectedSection) -> String {
    let mut out = String::new();
    out.push_str("> [!ft-source] \"");
    out.push_str(&section.source_path.to_string_lossy());
    out.push_str(&format!(
        "\" L{}-{} @{} #{}",
        section.line_start, section.line_end, section.commit_sha, section.content_hash
    ));
    for line in section.body.split('\n') {
        out.push('\n');
        if line.is_empty() {
            // Preserve blank-quoted line as `>` with no trailing space.
            out.push('>');
        } else {
            out.push_str("> ");
            out.push_str(line);
        }
    }
    out
}

/// Find every well-formed `[!ft-source]` callout in `text` and return
/// one [`ParsedCallout`] per occurrence in document order.
///
/// A well-formed callout is a header line matching the canonical
/// grammar (see [`HEADER_RE`]) followed by one or more contiguous
/// `>`-prefixed body lines. The callout ends at the first line that is
/// not a `>`-quoted continuation.
///
/// Malformed headers (e.g. missing a token) are silently skipped here —
/// the verifier reports them separately so this parser stays simple.
pub fn parse(text: &str) -> Vec<ParsedCallout> {
    let header_re = header_regex();
    let mut out = Vec::new();

    // Pre-compute (byte_offset, line_no) per line for fast lookup.
    let mut line_offsets: Vec<usize> = vec![0];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            line_offsets.push(i + 1);
        }
    }

    let lines: Vec<&str> = text.split('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(cap) = header_re.captures(line) {
            let source_path = PathBuf::from(&cap[1]);
            let line_start: u32 = cap[2].parse().unwrap_or(0);
            let line_end: u32 = cap[3].parse().unwrap_or(0);
            let commit_sha = cap[4].to_string();
            let content_hash = cap[5].to_string();

            // Collect body lines: every following line that starts
            // with `>` (with or without a single trailing space).
            // A line matching another `[!ft-source]` header starts the
            // next callout, not a body line.
            let body_start_idx = i + 1;
            let mut body_lines: Vec<String> = Vec::new();
            let mut j = body_start_idx;
            while j < lines.len() {
                if header_re.is_match(lines[j]) {
                    break;
                }
                if let Some(rest) = strip_quote_prefix(lines[j]) {
                    body_lines.push(rest.to_string());
                    j += 1;
                } else {
                    break;
                }
            }
            let body = body_lines.join("\n");

            // Byte range: from start of header line through end of
            // last body line (or end of header if no body).
            let header_off = line_offsets[i];
            let last_idx = if j > body_start_idx { j - 1 } else { i };
            // End offset: start of line after last_idx, or text.len()
            // if last_idx is the final line.
            let end_off = if last_idx + 1 < line_offsets.len() {
                line_offsets[last_idx + 1].saturating_sub(1).max(header_off)
            } else {
                text.len()
            };

            out.push(ParsedCallout {
                source_path,
                line_start,
                line_end,
                commit_sha,
                content_hash,
                body,
                byte_range: header_off..end_off,
                header_line: (i + 1) as u32,
            });

            i = j;
            continue;
        }
        i += 1;
    }
    out
}

/// Strip the leading `> ` (or bare `>`) from a quoted-continuation
/// line. Returns the line content without the prefix when the line is
/// a blockquote continuation; `None` otherwise.
fn strip_quote_prefix(line: &str) -> Option<&str> {
    // A continuation line must start with `>`. The canonical
    // serializer writes `> <content>` for non-empty content and bare
    // `>` for an empty body line. Be lenient about a missing trailing
    // space.
    let after = line.strip_prefix('>')?;
    Some(after.strip_prefix(' ').unwrap_or(after))
}

/// The canonical header regex. Captures `source_path`, `line_start`,
/// `line_end`, `commit_sha`, `content_hash` in that order.
///
/// Tolerates longer hash prefixes than the canonical 7/6 in case a
/// user widens them by hand.
pub fn header_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"^>\s*\[!ft-source\]\s+"([^"]+)"\s+L(\d+)-(\d+)\s+@([0-9a-f]{7,40})\s+#([0-9a-f]{6,})\s*$"#,
        )
        .expect("ft-source header regex must compile")
    })
}

/// Check if a path-and-line falls inside any parsed callout's body.
/// Returns `true` when `line` (1-indexed) is in the line range
/// `header_line+1..=header_line+body_line_count` of any callout in
/// `callouts`.
///
/// This is the predicate the link-review uses to skip wikilinks that
/// were quoted from elsewhere (so they don't double-count in the next pulse).
pub fn line_is_inside_callout(line: u32, callouts: &[ParsedCallout]) -> bool {
    callouts.iter().any(|c| {
        // Number of body lines = number of `\n`-separated chunks in body.
        // An empty body string still counts as zero body lines.
        let body_line_count = if c.body.is_empty() {
            0
        } else {
            c.body.split('\n').count() as u32
        };
        let body_first = c.header_line + 1;
        let body_last = c.header_line + body_line_count;
        body_first <= line && line <= body_last
    })
}

/// Detect whether a `.md` file's content marks it as a synth note —
/// `ft-synth: true` somewhere in the YAML frontmatter at the top.
///
/// The check is lenient on whitespace and quote styles around `true`.
pub fn is_synth_note(content: &str) -> bool {
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return false;
    }
    for line in lines {
        if line == "---" {
            return false;
        }
        let trimmed = line.trim();
        // Match `ft-synth: true`, `ft-synth:true`, `ft-synth: "true"`, etc.
        if let Some(val) = trimmed.strip_prefix("ft-synth:") {
            let v = val
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_lowercase();
            return v == "true";
        }
    }
    false
}

/// Parse the optional `ft-synth-targets` frontmatter key into a list
/// of raw wikilink strings. Returns `None` when the key is absent (or
/// the file has no frontmatter, or the value is not a YAML sequence).
///
/// Lenient on quote styles: accepts `"[[Foo]]"`, `'[[Foo]]'`, `[[Foo]]`,
/// `Foo`, and a mix. The value is returned verbatim (with surrounding
/// quotes stripped) — resolution to a `NoteId` happens at the call site
/// via the same `resolve_link_to_id` path the CLI uses for `--link`.
///
/// Supports both flow-sequence form (`["[[Foo]]", "[[Bar]]"]` on one
/// line) and block-sequence form (`- "[[Foo]]"` lines), since users may
/// hand-author either.
pub fn parse_synth_targets(content: &str) -> Option<Vec<String>> {
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return None;
    }
    // Collect the frontmatter body (between the fences).
    let mut fm: Vec<&str> = Vec::new();
    for line in lines {
        if line == "---" {
            break;
        }
        fm.push(line);
    }
    // Find the `ft-synth-targets:` key line.
    let key_idx = fm
        .iter()
        .position(|l| l.trim_start().starts_with("ft-synth-targets:"))?;
    let key_line = fm[key_idx];
    let after_key = key_line
        .trim_start()
        .strip_prefix("ft-synth-targets:")
        .unwrap_or("");

    // Flow sequence on the same line: `["[[Foo]]", "[[Bar]]"]`.
    let trimmed_inline = after_key.trim();
    if trimmed_inline.starts_with('[') {
        let close = trimmed_inline.rfind(']')?;
        let inner = &trimmed_inline[1..close];
        let items = parse_flow_seq_items(inner);
        if items.is_empty() {
            return Some(Vec::new());
        }
        return Some(items);
    }
    // Block sequence: subsequent `- <item>` lines until a non-dash line.
    let mut items: Vec<String> = Vec::new();
    for line in fm.iter().skip(key_idx + 1) {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("-") {
            let val = rest.trim();
            if val.is_empty() {
                // An empty dash entry — skip rather than push an empty target.
                continue;
            }
            items.push(strip_yaml_scalar_quotes(val).to_string());
        } else if t.is_empty() {
            // Blank lines within a block sequence are tolerated.
            continue;
        } else {
            // First non-dash, non-blank line ends the sequence.
            break;
        }
    }
    if items.is_empty() {
        return Some(Vec::new());
    }
    Some(items)
}

/// Parse the inner portion of a YAML flow sequence `[a, b, c]` into
/// scalar strings, stripping surrounding quotes. Commas inside quotes
/// are respected.
fn parse_flow_seq_items(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars = inner.chars();
    let mut cur = String::new();
    let mut in_quote: Option<char> = None;
    for c in chars {
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
                        out.push(trimmed.to_string());
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

/// Pure transform: idempotently ensure the result has `ft-synth: true`
/// in YAML frontmatter and, when `targets` is `Some`, an
/// `ft-synth-targets` key whose value is the YAML flow sequence of the
/// given strings. Existing frontmatter keys (including an existing
/// `ft-synth-targets`) are preserved or replaced in place; the marker is
/// replaced/inserted when missing or false. Unrelated body content is
/// unchanged. When `targets` is `None`, the `ft-synth-targets` key is
/// left untouched (not removed) — callers wanting removal handle that
/// separately.
///
/// This supersedes the older `upsert_ft_synth_marker` in the TUI layer,
/// which is refactored to delegate here so the marker and targets keys
/// compose without clobbering each other.
pub fn upsert_synth_frontmatter(content: &str, targets: Option<&[String]>) -> String {
    // Serialize the targets as a YAML flow sequence, e.g. `["[[Foo]]", "[[Bar]]"]`.
    let targets_line = targets.map(|ts| {
        let items: String = ts
            .iter()
            .map(|t| {
                // Escape any embedded double-quote, then wrap in quotes.
                let escaped = t.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{escaped}\"")
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("ft-synth-targets: [{items}]")
    });

    let lines: Vec<&str> = content.split('\n').collect();
    let has_fm = lines.first() == Some(&"---");
    if !has_fm {
        // No frontmatter: build a fresh block with both keys (if present).
        let mut fm = String::from("---\nft-synth: true");
        if let Some(tl) = &targets_line {
            fm.push('\n');
            fm.push_str(tl);
        }
        fm.push_str("\n---\n");
        if !content.starts_with('\n') {
            fm.push('\n');
        }
        fm.push_str(content);
        return fm;
    }
    // Find the closing `---` (first line equal to `---` after the opener).
    let end_idx = fm_close_index(&lines);
    let Some(end_idx) = end_idx else {
        // Unterminated frontmatter — bail and just prepend a fresh block.
        let mut fm = String::from("---\nft-synth: true");
        if let Some(tl) = &targets_line {
            fm.push('\n');
            fm.push_str(tl);
        }
        fm.push_str("\n---\n\n");
        fm.push_str(content);
        return fm;
    };
    let mut new_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();

    // --- ft-synth: true ---
    let marker_idx = new_lines
        .iter()
        .take(end_idx)
        .skip(1)
        .position(|l| l.trim_start().starts_with("ft-synth:"));
    match marker_idx {
        Some(i) => {
            // i is offset from skip(1), so the real index is i+1.
            new_lines[i + 1] = "ft-synth: true".to_string();
        }
        None => {
            new_lines.insert(end_idx, "ft-synth: true".to_string());
        }
    }

    // --- ft-synth-targets --- (only when `Some`)
    if let Some(tl) = targets_line {
        // Recompute the frontmatter close after the potential marker insert.
        let end_idx2 = fm_close_index(&new_lines).unwrap_or(end_idx);
        let targets_idx = new_lines
            .iter()
            .take(end_idx2)
            .skip(1)
            .position(|l| l.trim_start().starts_with("ft-synth-targets:"));
        match targets_idx {
            Some(i) => {
                let real = i + 1;
                new_lines[real] = tl;
                // Drop any immediately-following indented `- item` lines
                // that belonged to the old block-sequence value.
                let j = real + 1;
                while j < new_lines.len() {
                    let line = &new_lines[j];
                    let t = line.trim_start();
                    if t.strip_prefix('-').is_some()
                        && (line.starts_with(' ') || line.starts_with('\t'))
                    {
                        new_lines.remove(j);
                    } else {
                        break;
                    }
                }
            }
            None => {
                let end = fm_close_index(&new_lines).unwrap_or(end_idx);
                new_lines.insert(end, tl);
            }
        }
    }

    new_lines.join("\n")
}

/// Index (in the split-by-`\n` lines vec) of the frontmatter closing
/// `---` — the first line equal to `---` after the opening fence.
/// Returns `None` when the frontmatter is unterminated.
fn fm_close_index(lines: &[impl AsRef<str>]) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| l.as_ref() == "---")
        .map(|(i, _)| i)
}

/// Convenience: return the path-prefix exclusion check, treating
/// `prefixes` as plain path-string prefixes (vault-relative).
pub fn path_excluded(path: &Path, prefixes: &[String]) -> bool {
    let s = path.to_string_lossy();
    prefixes.iter().any(|p| s.starts_with(p.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ProtectedSection {
        ProtectedSection {
            source_path: PathBuf::from("notes/foo.md"),
            line_start: 42,
            line_end: 44,
            commit_sha: "abc1234".to_string(),
            content_hash: "7f3a91".to_string(),
            body: "Some original paragraph\nspanning two lines.".to_string(),
        }
    }

    #[test]
    fn serialize_canonical_form() {
        let s = sample();
        let out = serialize(&s);
        let expected = "\
> [!ft-source] \"notes/foo.md\" L42-44 @abc1234 #7f3a91
> Some original paragraph
> spanning two lines.";
        assert_eq!(out, expected);
    }

    #[test]
    fn parse_single_callout() {
        let text = "\
intro line

> [!ft-source] \"notes/foo.md\" L42-44 @abc1234 #7f3a91
> Some original paragraph
> spanning two lines.

after text
";
        let got = parse(text);
        assert_eq!(got.len(), 1);
        let p = &got[0];
        assert_eq!(p.source_path, PathBuf::from("notes/foo.md"));
        assert_eq!(p.line_start, 42);
        assert_eq!(p.line_end, 44);
        assert_eq!(p.commit_sha, "abc1234");
        assert_eq!(p.content_hash, "7f3a91");
        assert_eq!(p.body, "Some original paragraph\nspanning two lines.");
        assert_eq!(p.header_line, 3);
    }

    #[test]
    fn parse_round_trips_serialize() {
        let s = sample();
        let out = serialize(&s);
        let parsed = parse(&out);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].source_path, s.source_path);
        assert_eq!(parsed[0].line_start, s.line_start);
        assert_eq!(parsed[0].line_end, s.line_end);
        assert_eq!(parsed[0].commit_sha, s.commit_sha);
        assert_eq!(parsed[0].content_hash, s.content_hash);
        assert_eq!(parsed[0].body, s.body);
    }

    #[test]
    fn parse_round_trips_path_with_spaces() {
        // Regression: vaults commonly have folders like `My Notes/` or
        // `Areas/Personal Notes/`. The scaffold serializer writes them
        // verbatim; verify must be able to read them back.
        let s = ProtectedSection {
            source_path: PathBuf::from("My Notes/sub folder/foo.md"),
            line_start: 42,
            line_end: 44,
            commit_sha: "abc1234".to_string(),
            content_hash: "7f3a91".to_string(),
            body: "Some original paragraph\nspanning two lines.".to_string(),
        };
        let out = serialize(&s);
        let parsed = parse(&out);
        assert_eq!(
            parsed.len(),
            1,
            "callout with spaces in source path should round-trip; serialized:\n{out}\nparsed: {parsed:?}"
        );
        assert_eq!(parsed[0].source_path, s.source_path);
        assert_eq!(parsed[0].line_start, s.line_start);
        assert_eq!(parsed[0].line_end, s.line_end);
        assert_eq!(parsed[0].commit_sha, s.commit_sha);
        assert_eq!(parsed[0].content_hash, s.content_hash);
        assert_eq!(parsed[0].body, s.body);
    }

    #[test]
    fn parse_two_adjacent_callouts() {
        let text = "\
> [!ft-source] \"a.md\" L1-1 @aaaaaaa #aaaaaa
> first body
> [!ft-source] \"b.md\" L5-7 @bbbbbbb #bbbbbb
> second body line 1
> second body line 2
";
        let got = parse(text);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].body, "first body");
        assert_eq!(got[1].body, "second body line 1\nsecond body line 2");
    }

    #[test]
    fn parse_ignores_malformed_header() {
        // Missing the content-hash → not a valid header. Parser skips.
        let text = "\
> [!ft-source] \"a.md\" L1-1 @aaaaaaa
> not a real callout body
";
        let got = parse(text);
        assert!(got.is_empty());
    }

    #[test]
    fn parse_skips_unrelated_callouts() {
        let text = "\
> [!note] some other callout
> body here

> [!ft-source] \"real.md\" L1-2 @ccccccc #cccccc
> real body
";
        let got = parse(text);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].source_path, PathBuf::from("real.md"));
    }

    #[test]
    fn compute_section_hash_is_deterministic() {
        let h1 = compute_section_hash("hello world");
        let h2 = compute_section_hash("hello world");
        let h3 = compute_section_hash("hello world!");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), CONTENT_HASH_PREFIX_LEN);
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn line_is_inside_callout_detects_body_lines() {
        let text = "\
some intro

> [!ft-source] \"a.md\" L1-1 @aaaaaaa #aaaaaa
> body line 1
> body line 2

after
";
        let callouts = parse(text);
        // Header is at line 3; body lines at 4, 5.
        assert!(!line_is_inside_callout(3, &callouts));
        assert!(line_is_inside_callout(4, &callouts));
        assert!(line_is_inside_callout(5, &callouts));
        assert!(!line_is_inside_callout(6, &callouts));
    }

    #[test]
    fn is_synth_note_detects_marker() {
        let with_marker = "---\nft-synth: true\n---\n# body\n";
        assert!(is_synth_note(with_marker));
        let with_quoted = "---\nft-synth: \"true\"\n---\n";
        assert!(is_synth_note(with_quoted));
        let false_value = "---\nft-synth: false\n---\n";
        assert!(!is_synth_note(false_value));
        let no_marker = "---\nfoo: bar\n---\n";
        assert!(!is_synth_note(no_marker));
        let no_frontmatter = "# just a body\n";
        assert!(!is_synth_note(no_frontmatter));
    }

    #[test]
    fn path_excluded_prefix_match() {
        let prefixes = vec!["Periodic/".to_string(), "Inbox/".to_string()];
        assert!(path_excluded(
            Path::new("Periodic/2025-03-14.md"),
            &prefixes
        ));
        assert!(path_excluded(Path::new("Inbox/quick.md"), &prefixes));
        assert!(!path_excluded(Path::new("Notes/foo.md"), &prefixes));
        assert!(!path_excluded(Path::new("PeriodicX/foo.md"), &prefixes));
    }

    // ── ft-synth-targets frontmatter ──────────────────────────────────

    #[test]
    fn parse_synth_targets_absent_returns_none() {
        let content = "---\nft-synth: true\n---\nbody\n";
        assert!(parse_synth_targets(content).is_none());
    }

    #[test]
    fn parse_synth_targets_no_frontmatter_returns_none() {
        assert!(parse_synth_targets("# heading\n").is_none());
    }

    #[test]
    fn parse_synth_targets_flow_sequence_quoted() {
        let content = "---\nft-synth: true\nft-synth-targets: [\"[[Foo]]\", \"[[Bar]]\"]\n---\n";
        let got = parse_synth_targets(content).unwrap();
        assert_eq!(got, vec!["[[Foo]]", "[[Bar]]"]);
    }

    #[test]
    fn parse_synth_targets_flow_sequence_bare() {
        // Bare (unquoted) values are accepted.
        let content = "---\nft-synth-targets: [Foo, Bar]\n---\n";
        let got = parse_synth_targets(content).unwrap();
        assert_eq!(got, vec!["Foo", "Bar"]);
    }

    #[test]
    fn parse_synth_targets_block_sequence() {
        let content = "---\nft-synth: true\nft-synth-targets:\n  - \"[[Foo]]\"\n  - Bar\n---\n";
        let got = parse_synth_targets(content).unwrap();
        assert_eq!(got, vec!["[[Foo]]", "Bar"]);
    }

    #[test]
    fn parse_synth_targets_empty_flow_sequence() {
        let content = "---\nft-synth-targets: []\n---\n";
        assert_eq!(parse_synth_targets(content), Some(Vec::new()));
    }

    #[test]
    fn upsert_adds_frontmatter_when_missing_with_targets() {
        let content = "# heading\nbody\n";
        let targets = vec!["[[Foo]]".to_string(), "[[Bar]]".to_string()];
        let out = upsert_synth_frontmatter(content, Some(&targets));
        assert!(out.starts_with(
            "---\nft-synth: true\nft-synth-targets: [\"[[Foo]]\", \"[[Bar]]\"]\n---\n"
        ));
        assert!(out.contains("# heading"));
    }

    #[test]
    fn upsert_inserts_marker_into_existing_frontmatter_no_targets() {
        let content = "---\ntitle: Foo\n---\n\nbody\n";
        let out = upsert_synth_frontmatter(content, None);
        assert!(out.contains("ft-synth: true"));
        assert!(out.contains("title: Foo"));
        assert!(out.contains("body"));
        // No targets key requested → none added.
        assert!(!out.contains("ft-synth-targets"));
    }

    #[test]
    fn upsert_replaces_false_marker() {
        let content = "---\nft-synth: false\n---\n";
        let out = upsert_synth_frontmatter(content, None);
        assert!(out.contains("ft-synth: true"));
        assert!(!out.contains("ft-synth: false"));
    }

    #[test]
    fn upsert_adds_targets_key_when_absent() {
        let content = "---\nft-synth: true\ntitle: T\n---\nbody\n";
        let targets = vec!["[[Baz]]".to_string()];
        let out = upsert_synth_frontmatter(content, Some(&targets));
        assert!(out.contains("ft-synth-targets: [\"[[Baz]]\"]"));
        assert!(out.contains("title: T"));
        assert!(out.contains("body"));
    }

    #[test]
    fn upsert_replaces_existing_targets() {
        let content = "---\nft-synth: true\nft-synth-targets: [\"[[Old]]\"]\n---\nbody\n";
        let targets = vec!["[[New]]".to_string()];
        let out = upsert_synth_frontmatter(content, Some(&targets));
        assert!(out.contains("ft-synth-targets: [\"[[New]]\"]"));
        assert!(!out.contains("[[Old]]"));
    }

    #[test]
    fn upsert_replaces_existing_block_targets() {
        // Old block-sequence form collapses to flow form on replace.
        let content = "---\nft-synth: true\nft-synth-targets:\n  - \"[[Old]]\"\n  - \"[[AlsoOld]]\"\n---\nbody\n";
        let targets = vec!["[[New]]".to_string()];
        let out = upsert_synth_frontmatter(content, Some(&targets));
        assert!(out.contains("ft-synth-targets: [\"[[New]]\"]"));
        assert!(!out.contains("[[Old]]"));
        assert!(!out.contains("[[AlsoOld]]"));
        assert!(out.contains("body"));
    }

    #[test]
    fn upsert_preserves_unrelated_frontmatter_keys() {
        let content = "---\ntitle: My Note\ntags: [a, b]\n---\nbody\n";
        let targets = vec!["[[Foo]]".to_string()];
        let out = upsert_synth_frontmatter(content, Some(&targets));
        assert!(out.contains("title: My Note"));
        assert!(out.contains("tags: [a, b]"));
        assert!(out.contains("ft-synth: true"));
        assert!(out.contains("ft-synth-targets: [\"[[Foo]]\"]"));
    }

    #[test]
    fn upsert_is_idempotent() {
        let content = "---\nft-synth: true\nft-synth-targets: [\"[[Foo]]\"]\n---\nbody\n";
        let targets = vec!["[[Foo]]".to_string()];
        let out = upsert_synth_frontmatter(content, Some(&targets));
        assert_eq!(out, content);
    }

    // ── Property: parse(serialize(s)) preserves all fields ────────────
    use proptest::prelude::*;

    fn arb_section() -> impl Strategy<Value = ProtectedSection> {
        // Path: nonempty, no `"` (would break the quoted form), no
        // newlines; spaces ARE allowed since the quoted grammar accepts
        // them. Always ends in `.md`.
        let path_strat = "[a-zA-Z][a-zA-Z0-9_/ -]{0,40}\\.md".prop_map(PathBuf::from);
        // Line numbers: small u32, line_end >= line_start.
        let lines_strat = (1u32..=10_000u32, 0u32..=200u32)
            .prop_map(|(start, span)| (start, start.saturating_add(span)));
        // SHA: exactly SHORT_SHA_LEN hex chars.
        let sha_strat = "[0-9a-f]{7}";
        // Hash: exactly CONTENT_HASH_PREFIX_LEN hex chars.
        let hash_strat = "[0-9a-f]{6}";
        // Body: 1..6 lines, each nonempty, no newlines, no leading `>`
        // (would collide with a continuation line), reasonable charset.
        let line_strat = "[a-zA-Z0-9 .,;:'\"!\\?\\[\\]\\(\\)\\-_]{1,60}";
        let body_strat =
            proptest::collection::vec(line_strat, 1..=6).prop_map(|lines| lines.join("\n"));

        (path_strat, lines_strat, sha_strat, hash_strat, body_strat).prop_map(
            |(source_path, (line_start, line_end), commit_sha, content_hash, body)| {
                ProtectedSection {
                    source_path,
                    line_start,
                    line_end,
                    commit_sha,
                    content_hash,
                    body,
                }
            },
        )
    }

    proptest! {
        #[test]
        fn parse_serialize_round_trip(s in arb_section()) {
            let serialized = serialize(&s);
            let parsed = parse(&serialized);
            prop_assert_eq!(parsed.len(), 1, "exactly one callout should parse back");
            let p = &parsed[0];
            prop_assert_eq!(&p.source_path, &s.source_path);
            prop_assert_eq!(p.line_start, s.line_start);
            prop_assert_eq!(p.line_end, s.line_end);
            prop_assert_eq!(&p.commit_sha, &s.commit_sha);
            prop_assert_eq!(&p.content_hash, &s.content_hash);
            prop_assert_eq!(&p.body, &s.body);
        }
    }
}

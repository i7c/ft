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
/// The output has no trailing newline; callers control how sections
/// are joined into the surrounding document.
pub fn serialize(section: &ProtectedSection) -> String {
    let mut out = String::new();
    out.push_str("> [!ft-source] ");
    out.push_str(&section.source_path.to_string_lossy());
    out.push_str(&format!(
        " L{}-{} @{} #{}",
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
            r"^>\s*\[!ft-source\]\s+(\S+)\s+L(\d+)-(\d+)\s+@([0-9a-f]{7,40})\s+#([0-9a-f]{6,})\s*$",
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
/// were quoted from elsewhere (so they don't double-count next ritual).
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
> [!ft-source] notes/foo.md L42-44 @abc1234 #7f3a91
> Some original paragraph
> spanning two lines.";
        assert_eq!(out, expected);
    }

    #[test]
    fn parse_single_callout() {
        let text = "\
intro line

> [!ft-source] notes/foo.md L42-44 @abc1234 #7f3a91
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
    fn parse_two_adjacent_callouts() {
        let text = "\
> [!ft-source] a.md L1-1 @aaaaaaa #aaaaaa
> first body
> [!ft-source] b.md L5-7 @bbbbbbb #bbbbbb
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
> [!ft-source] a.md L1-1 @aaaaaaa
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

> [!ft-source] real.md L1-2 @ccccccc #cccccc
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

> [!ft-source] a.md L1-1 @aaaaaaa #aaaaaa
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

    // ── Property: parse(serialize(s)) preserves all fields ────────────
    use proptest::prelude::*;

    fn arb_section() -> impl Strategy<Value = ProtectedSection> {
        // Path: nonempty, no whitespace, no newlines, ends in `.md`.
        let path_strat = "[a-zA-Z][a-zA-Z0-9_/-]{0,40}\\.md".prop_map(PathBuf::from);
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

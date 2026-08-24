//! Paragraph-search query DSL.
//!
//! One parser drives the `ft notes search` CLI argument, the Search TUI
//! input line, and `ft notes synth scaffold --search`. Grammar (per
//! clause):
//!
//! - `[[…]]` — wikilink target. Atomic: may contain spaces. `#anchor`
//!   is stripped and `[[X|Alias]]` matches on `Alias`.
//! - `"…"` — phrase: the quoted text as a contiguous substring.
//! - `~term` — fuzzy: levenshtein over the token dictionary.
//! - `=term` — word: whole token, or a `[[…]]` link target.
//! - `term` — substring (the default mode).
//! - `-` prefix on any clause — exclude: paragraphs matching it are
//!   dropped after the positive clauses have been applied.
//!
//! Clauses are ANDed by default; `any: true` switches the query to OR.
//! There is no `OR` keyword. An unterminated `[[` or `"` degrades to a
//! literal substring term so partial typing never errors.
//!
//! Parsing is total — it never fails; an empty or whitespace-only input
//! yields a query with no clauses.

/// One clause's match mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Default: the term appears as a contiguous substring.
    Substring,
    /// `=term`: whole token (or link target).
    Word,
    /// `~term`: levenshtein over dictionary tokens.
    Fuzzy,
    /// `"…"`: the quoted string as a contiguous substring.
    Phrase,
    /// `[[…]]`: link-target token.
    Link,
}

/// One parsed clause: `[-][mode-prefix]term`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clause {
    /// The verbatim clause text as typed, used as the result label
    /// (e.g. `[[foo]]`, `=eigen`, `~memoizaton`, `-task`).
    pub raw: String,
    /// `true` for an exclude clause (`-` prefix).
    pub negated: bool,
    pub mode: Mode,
    /// The match term, case-folded. For `[[…]]` clauses this is the
    /// anchor-stripped / alias-resolved target.
    pub term: String,
}

/// A parsed query: an `any` flag plus an ordered clause list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub any: bool,
    pub clauses: Vec<Clause>,
}

impl SearchQuery {
    /// True when the input had no parseable clauses.
    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }

    /// The positive (non-exclude) clauses.
    pub fn positives(&self) -> impl Iterator<Item = &Clause> {
        self.clauses.iter().filter(|c| !c.negated)
    }

    /// The exclude clauses.
    pub fn excludes(&self) -> impl Iterator<Item = &Clause> {
        self.clauses.iter().filter(|c| c.negated)
    }

    /// Reconstruct the canonical query text: clauses joined by single
    /// spaces. Re-parsing this string (with the same `any` flag) yields
    /// the same query, which is the round-trip invariant the proptests
    /// assert. The `any` flag is not part of the string — it is a
    /// separate parse parameter.
    pub fn render(&self) -> String {
        self.clauses
            .iter()
            .map(|c| c.raw.clone())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Parse `input` into a [`SearchQuery`]. `any` selects OR (any clause
/// matches) instead of the default AND. Never fails: unrecognized or
/// unterminated fragments degrade to literal substring clauses.
pub fn parse(input: &str, any: bool) -> SearchQuery {
    let mut clauses = Vec::new();
    let mut rest = input.trim();

    while !rest.is_empty() {
        let (negated, after_neg) = match rest.strip_prefix('-') {
            Some(t) if !t.is_empty() => (true, t),
            _ => (false, rest),
        };
        let neg = if negated { "-" } else { "" };

        // `[[…]]` — atomic, spaces allowed. Scan for the closing `]]`.
        if let Some(inner) = after_neg.strip_prefix("[[") {
            if let Some(end) = inner.find("]]") {
                let content = &inner[..end];
                let term = normalize_link_target(content);
                if !term.is_empty() {
                    clauses.push(Clause {
                        raw: format!("{neg}[[{content}]]"),
                        negated,
                        mode: Mode::Link,
                        term,
                    });
                }
                rest = inner[end + 2..].trim_start();
                continue;
            }
            // Unterminated `[[` → falls through to substring below.
        }

        // `"…"` — phrase.
        if let Some(inner) = after_neg.strip_prefix('"') {
            if let Some(end) = inner.find('"') {
                let content = &inner[..end];
                clauses.push(Clause {
                    raw: format!("{neg}\"{content}\""),
                    negated,
                    mode: Mode::Phrase,
                    term: content.to_lowercase(),
                });
                rest = inner[end + 1..].trim_start();
                continue;
            }
            // Unterminated `"` → falls through to substring.
        }

        // Mode prefixes `~` (fuzzy) and `=` (word); else substring.
        let (mode, term_start) = if let Some(t) = after_neg.strip_prefix('~') {
            (Mode::Fuzzy, t)
        } else if let Some(t) = after_neg.strip_prefix('=') {
            (Mode::Word, t)
        } else {
            (Mode::Substring, after_neg)
        };

        let end = term_start
            .find(char::is_whitespace)
            .unwrap_or(term_start.len());
        let raw_term = &term_start[..end];
        if raw_term.is_empty() {
            // Bare `-`, `~`, or `=` with nothing after it: skip.
            rest = term_start[end..].trim_start();
            continue;
        }
        let mode_prefix = match mode {
            Mode::Substring => "",
            Mode::Word => "=",
            Mode::Fuzzy => "~",
            Mode::Phrase => "",
            Mode::Link => "",
        };
        clauses.push(Clause {
            raw: format!(
                "{}{}{}",
                if negated { "-" } else { "" },
                mode_prefix,
                raw_term
            ),
            negated,
            mode,
            term: raw_term.to_lowercase(),
        });
        rest = term_start[end..].trim_start();
    }

    SearchQuery { any, clauses }
}

/// Fold a `[[…]]` content into its match target: strip `#anchor`, use
/// the alias after `|`, trim, lowercase.
fn normalize_link_target(content: &str) -> String {
    let no_anchor = content.split('#').next().unwrap_or(content);
    let target = no_anchor.rsplit('|').next().unwrap_or(no_anchor);
    target.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn empty_and_whitespace_parse_to_no_clauses() {
        assert!(parse("", false).clauses.is_empty());
        assert!(parse("   ", false).clauses.is_empty());
    }

    #[test]
    fn default_substring_and_and() {
        let q = parse("eigen memoization", false);
        assert!(!q.any);
        assert_eq!(q.clauses.len(), 2);
        assert_eq!(q.clauses[0].mode, Mode::Substring);
        assert_eq!(q.clauses[0].term, "eigen");
        assert_eq!(q.clauses[1].term, "memoization");
        assert!(!q.clauses.iter().any(|c| c.negated));
    }

    #[test]
    fn mode_prefixes() {
        let q = parse("=word ~fuzzy \"exact phrase\"", false);
        assert_eq!(q.clauses.len(), 3);
        assert_eq!(q.clauses[0].mode, Mode::Word);
        assert_eq!(q.clauses[0].term, "word");
        assert_eq!(q.clauses[1].mode, Mode::Fuzzy);
        assert_eq!(q.clauses[1].term, "fuzzy");
        assert_eq!(q.clauses[2].mode, Mode::Phrase);
        assert_eq!(q.clauses[2].term, "exact phrase");
    }

    #[test]
    fn wikilink_with_spaces_is_atomic() {
        let q = parse("[[Bar Foo]]", false);
        assert_eq!(q.clauses.len(), 1);
        assert_eq!(q.clauses[0].mode, Mode::Link);
        assert_eq!(q.clauses[0].term, "bar foo");
    }

    #[test]
    fn link_anchor_stripped_and_alias_used() {
        let q = parse("[[Baz#Section]] [[Qux|Alias]]", false);
        assert_eq!(q.clauses[0].term, "baz");
        assert_eq!(q.clauses[1].term, "alias");
    }

    #[test]
    fn exclude_prefix() {
        let q = parse("eigen -task", false);
        assert_eq!(q.clauses.len(), 2);
        assert!(!q.clauses[0].negated);
        assert!(q.clauses[1].negated);
        assert_eq!(q.clauses[1].mode, Mode::Substring);
        assert_eq!(q.clauses[1].raw, "-task");
    }

    #[test]
    fn exclude_on_other_modes() {
        let q = parse("-~fuz -[[Bar]] -=word", false);
        assert_eq!(q.clauses.len(), 3);
        assert!(q.clauses.iter().all(|c| c.negated));
        assert_eq!(q.clauses[0].mode, Mode::Fuzzy);
        assert_eq!(q.clauses[1].mode, Mode::Link);
        assert_eq!(q.clauses[2].mode, Mode::Word);
    }

    #[test]
    fn negated_phrase_and_link_keep_negation_in_raw() {
        // Regression: the phrase and `[[…]]` branches built `raw`
        // without the `-` prefix, so a negated phrase/link rendered
        // back without the negation and broke the round-trip.
        for (input, any) in [
            ("-\"exact phrase\"", false),
            ("-\"\"", false),
            ("-[[Bar Foo]]", false),
            ("eigen -\"two words\" -[[link]]", false),
        ] {
            let q = parse(input, any);
            let rendered = q.render();
            let q2 = parse(&rendered, any);
            assert_eq!(q, q2, "render/parse round-trip for {input:?}");
        }
    }

    #[test]
    fn unterminated_constructs_degrade_to_substring() {
        let q = parse("[[unterminated \"phrase", false);
        assert_eq!(q.clauses.len(), 2);
        assert_eq!(q.clauses[0].mode, Mode::Substring);
        assert_eq!(q.clauses[0].term, "[[unterminated");
        assert_eq!(q.clauses[1].mode, Mode::Substring);
        assert_eq!(q.clauses[1].term, "\"phrase");
    }

    #[test]
    fn any_flag_round_trips_through_render() {
        for (input, any) in [
            ("eigen memoization", false),
            ("[[foo]] [[bar]]", true),
            ("=word ~fuzzy -task", false),
            ("[[Bar Foo]] \"exact phrase\"", true),
        ] {
            let q = parse(input, any);
            let rendered = q.render();
            let q2 = parse(&rendered, any);
            assert_eq!(q, q2, "render/parse round-trip for {input:?}");
        }
    }

    #[test]
    fn bare_mode_chars_are_skipped() {
        let q = parse("- ~ =", false);
        assert!(q.clauses.is_empty());
    }

    #[test]
    fn case_folding_applied() {
        let q = parse("EIGEN [[Memoization]]", false);
        assert_eq!(q.clauses[0].term, "eigen");
        assert_eq!(q.clauses[1].term, "memoization");
    }

    proptest::proptest! {
        /// Round-trip: re-parsing a parsed query's canonical rendering
        /// yields the same query — parsing is idempotent for any input.
        #[test]
        fn parse_render_round_trip(s in ".{0,80}", any in proptest::bool::ANY) {
            let q = parse(&s, any);
            let q2 = parse(&q.render(), any);
            prop_assert_eq!(q, q2);
        }
    }
}

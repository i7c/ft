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
//! - `=term` — word: whole token, stem-mate, or link-target word.
//! - `term` — substring (the default mode).
//! - `path:<prefix>`, `tag:<tag>`, `after:<date>`, `before:<date>`,
//!   `scope:<graph-preset>` — filters; the value may be `"…"`-quoted.
//! - `near:[[Note]]` — boost by link-graph proximity to a note.
//! - `-` prefix on any text clause or filter — exclude / invert.
//!
//! Text clauses are ANDed by default; `any: true` switches them to OR.
//! Filters always AND. There is no `OR` keyword. An unterminated `[[`
//! or `"`, an unknown `key:`, an empty filter value, and an unparseable
//! date all degrade to a literal substring term, so partial typing never
//! errors.
//!
//! Terms are folded with [`text::fold`] (case and diacritics), the same
//! fold the index applies. Parsing is total — it never fails; an empty
//! or whitespace-only input yields a query with no clauses.

use chrono::NaiveDate;

use crate::search::text;

/// A filter clause's key (and resolved value, for dates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    /// `path:<prefix>` — vault-relative path prefix (folded).
    Path,
    /// `tag:<tag>` — tag or descendant tag (folded, `#` stripped).
    Tag,
    /// `after:<date>` — blame date on or after.
    After(NaiveDate),
    /// `before:<date>` — blame date on or before.
    Before(NaiveDate),
    /// `scope:<preset>` — notes selected by a graph preset.
    Scope,
}

/// One clause's match mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Default: the term appears as a contiguous substring.
    Substring,
    /// `=term`: whole token, stem-mate, or link-target word.
    Word,
    /// `~term`: levenshtein over dictionary tokens.
    Fuzzy,
    /// `"…"`: the quoted string as a contiguous substring.
    Phrase,
    /// `[[…]]`: link-target token.
    Link,
    /// `key:value`: restricts the result set, never scores.
    Filter(Filter),
    /// `near:[[…]]`: boosts by graph distance, never filters.
    Near,
}

/// One parsed clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clause {
    /// The verbatim clause text as typed, used as the result label
    /// (e.g. `[[foo]]`, `=eigen`, `~memoizaton`, `-task`, `path:know/`).
    pub raw: String,
    /// `true` for an exclude clause or inverted filter (`-` prefix).
    pub negated: bool,
    pub mode: Mode,
    /// The match term, folded. For `[[…]]` / `near:` clauses this is the
    /// anchor-stripped / alias-resolved target; for `scope:` the preset
    /// name as typed; for dates the value as typed.
    pub term: String,
}

impl Clause {
    /// True for clauses that match paragraph text (not filters or boosts).
    pub fn is_text(&self) -> bool {
        !matches!(self.mode, Mode::Filter(_) | Mode::Near)
    }
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

    /// The positive (non-exclude) text clauses.
    pub fn positives(&self) -> impl Iterator<Item = &Clause> {
        self.clauses.iter().filter(|c| c.is_text() && !c.negated)
    }

    /// The exclude text clauses.
    pub fn excludes(&self) -> impl Iterator<Item = &Clause> {
        self.clauses.iter().filter(|c| c.is_text() && c.negated)
    }

    /// The filter clauses (inverted ones included).
    pub fn filters(&self) -> impl Iterator<Item = &Clause> {
        self.clauses
            .iter()
            .filter(|c| matches!(c.mode, Mode::Filter(_)))
    }

    /// The `near:` boost clauses.
    pub fn nears(&self) -> impl Iterator<Item = &Clause> {
        self.clauses.iter().filter(|c| c.mode == Mode::Near)
    }

    /// True when an `after:` / `before:` filter needs blame dates.
    pub fn has_date_filters(&self) -> bool {
        self.clauses.iter().any(|c| {
            matches!(
                c.mode,
                Mode::Filter(Filter::After(_)) | Mode::Filter(Filter::Before(_))
            )
        })
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

/// Filter keys recognized before a `:`.
const FILTER_KEYS: &[&str] = &["path", "tag", "after", "before", "scope"];

/// Parse `input` into a [`SearchQuery`]. `any` selects OR over the text
/// clauses instead of the default AND. Never fails: unrecognized or
/// unterminated fragments degrade to literal substring clauses.
pub fn parse(input: &str, any: bool) -> SearchQuery {
    parse_on(input, any, crate::dates::today())
}

/// [`parse`] with an explicit "today" for relative date values.
fn parse_on(input: &str, any: bool, today: NaiveDate) -> SearchQuery {
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
                if let Some(term) = text::link_target(content) {
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

        // `near:[[…]]` — boost. Never negated: `-near:` stays literal.
        if !negated {
            if let Some(inner) = after_neg.strip_prefix("near:[[") {
                if let Some(end) = inner.find("]]") {
                    let content = &inner[..end];
                    if let Some(term) = text::link_target(content) {
                        clauses.push(Clause {
                            raw: format!("near:[[{content}]]"),
                            negated: false,
                            mode: Mode::Near,
                            term,
                        });
                        rest = inner[end + 2..].trim_start();
                        continue;
                    }
                }
            }
        }

        // `key:value` filters.
        if let Some((clause, after)) = parse_filter(after_neg, negated, today) {
            clauses.push(clause);
            rest = after.trim_start();
            continue;
        }

        // `"…"` — phrase.
        if let Some(inner) = after_neg.strip_prefix('"') {
            if let Some(end) = inner.find('"') {
                let content = &inner[..end];
                clauses.push(Clause {
                    raw: format!("{neg}\"{content}\""),
                    negated,
                    mode: Mode::Phrase,
                    term: text::fold(content),
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
            Mode::Word => "=",
            Mode::Fuzzy => "~",
            _ => "",
        };
        clauses.push(Clause {
            raw: format!("{neg}{mode_prefix}{raw_term}"),
            negated,
            mode,
            term: text::fold(raw_term),
        });
        rest = term_start[end..].trim_start();
    }

    SearchQuery { any, clauses }
}

/// Parse a `key:value` filter at the start of `s`. Returns the clause and
/// the unconsumed rest, or `None` when `s` is not a well-formed filter
/// (unknown key, empty value, unterminated quote, invalid date) — the
/// caller then treats the text as a literal substring.
fn parse_filter(s: &str, negated: bool, today: NaiveDate) -> Option<(Clause, &str)> {
    let (key, after_key) = s.split_once(':')?;
    if !FILTER_KEYS.contains(&key) {
        return None;
    }
    let (value, raw_value, after) = if let Some(inner) = after_key.strip_prefix('"') {
        let end = inner.find('"')?;
        (&inner[..end], &after_key[..end + 2], &inner[end + 1..])
    } else {
        let end = after_key
            .find(char::is_whitespace)
            .unwrap_or(after_key.len());
        (&after_key[..end], &after_key[..end], &after_key[end..])
    };
    if value.trim().is_empty() {
        return None;
    }
    let (filter, term) = match key {
        "path" => (Filter::Path, text::fold(value)),
        "tag" => {
            let t = text::fold(value.trim_start_matches('#'));
            if t.is_empty() {
                return None;
            }
            (Filter::Tag, t)
        }
        "after" | "before" => {
            let d = crate::dates::parse_date_value(value, today)?;
            let f = if key == "after" {
                Filter::After(d)
            } else {
                Filter::Before(d)
            };
            (f, value.to_string())
        }
        "scope" => (Filter::Scope, value.to_string()),
        _ => return None,
    };
    let neg = if negated { "-" } else { "" };
    Some((
        Clause {
            raw: format!("{neg}{key}:{raw_value}"),
            negated,
            mode: Mode::Filter(filter),
            term,
        },
        after,
    ))
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

    #[test]
    fn filter_and_boost_clauses() {
        let q = parse(
            "cache path:\"Projects/Big Idea/\" -tag:archive after:2026-01-01 near:[[Caching]]",
            false,
        );
        assert_eq!(q.clauses.len(), 5);
        assert_eq!(q.clauses[0].mode, Mode::Substring);
        assert_eq!(q.clauses[1].mode, Mode::Filter(Filter::Path));
        assert_eq!(q.clauses[1].term, "projects/big idea/");
        assert_eq!(q.clauses[1].raw, "path:\"Projects/Big Idea/\"");
        assert_eq!(q.clauses[2].mode, Mode::Filter(Filter::Tag));
        assert!(q.clauses[2].negated);
        assert_eq!(
            q.clauses[3].mode,
            Mode::Filter(Filter::After(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()))
        );
        assert_eq!(q.clauses[4].mode, Mode::Near);
        assert_eq!(q.clauses[4].term, "caching");
        assert_eq!(q.positives().count(), 1);
        assert_eq!(q.filters().count(), 3);
        assert_eq!(q.nears().count(), 1);
        assert!(q.has_date_filters());
    }

    #[test]
    fn unknown_keys_and_empty_values_stay_literal() {
        let q = parse(
            "http://example.com foo:bar path: tag:# after:someday",
            false,
        );
        assert!(q.clauses.iter().all(|c| c.mode == Mode::Substring), "{q:?}");
        assert_eq!(q.clauses[0].term, "http://example.com");
        assert_eq!(q.clauses[2].term, "path:");
        assert_eq!(q.clauses[4].term, "after:someday");
    }

    #[test]
    fn relative_dates_resolve_against_ft_today() {
        let q = parse_at("after:-7d", "2026-10-23");
        assert_eq!(
            q.clauses[0].mode,
            Mode::Filter(Filter::After(
                NaiveDate::from_ymd_opt(2026, 10, 16).unwrap()
            ))
        );
    }

    fn parse_at(input: &str, today: &str) -> SearchQuery {
        parse_on(input, false, today.parse().unwrap())
    }

    #[test]
    fn tag_filter_strips_hash_and_folds() {
        let q = parse("tag:#Proj/Busão", false);
        assert_eq!(q.clauses[0].mode, Mode::Filter(Filter::Tag));
        assert_eq!(q.clauses[0].term, "proj/busao");
    }

    #[test]
    fn negated_near_is_literal() {
        let q = parse("-near:[[X]]", false);
        assert_eq!(q.clauses.len(), 1);
        assert_eq!(q.clauses[0].mode, Mode::Substring);
        assert!(q.clauses[0].negated);
    }

    #[test]
    fn terms_fold_diacritics() {
        let q = parse("=Ônibus \"Café Naïve\" [[Busão]]", false);
        assert_eq!(q.clauses[0].term, "onibus");
        assert_eq!(q.clauses[1].term, "cafe naive");
        assert_eq!(q.clauses[2].term, "busao");
    }

    #[test]
    fn filters_round_trip_through_render() {
        for input in [
            "cache path:\"Projects/Big Idea/\" -tag:archive near:[[Caching]]",
            "scope:work -path:archive/ before:2026-01-01",
            "path:\"unterminated tag:",
            "near:[[unterminated",
        ] {
            let q = parse(input, false);
            assert_eq!(parse(&q.render(), false), q, "round-trip for {input:?}");
        }
    }

    proptest::proptest! {
        /// Round-trip over structured clauses: filters, quoted values,
        /// near boosts, and negations mixed with text clauses.
        #[test]
        fn structured_clauses_round_trip(
            parts in proptest::collection::vec(
                prop_oneof![
                    "[a-zA-Zôé]{1,8}",
                    "=[a-z]{1,6}",
                    "~[a-z]{1,6}",
                    "\"[a-z ]{0,8}\"",
                    "\\[\\[[A-Za-z ]{1,8}\\]\\]",
                    "-?path:[a-z/]{0,6}",
                    "-?path:\"[a-z /]{0,6}\"",
                    "-?tag:#?[a-z/]{0,6}",
                    "-?(after|before):(2026-0[1-9]-1[0-9]|today|-[0-9]d|bogus)",
                    "-?scope:[a-z]{0,5}",
                    "-?near:\\[\\[[A-Za-z ]{0,6}\\]\\]",
                ],
                0..6,
            ),
            any in proptest::bool::ANY,
        ) {
            let input = parts.join(" ");
            let q = parse_on(&input, any, NaiveDate::from_ymd_opt(2026, 10, 23).unwrap());
            let q2 = parse_on(&q.render(), any, NaiveDate::from_ymd_opt(2026, 10, 23).unwrap());
            prop_assert_eq!(q, q2);
        }
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

//! Text layer for paragraph search: folding, tokenization, stemming.
//!
//! Index and query text go through the same [`fold`] — lowercase, then
//! NFD with combining marks dropped — so `Ônibus`, `ônibus`, and
//! `onibus` are one token. Tokens are cut from the *original* text so
//! their byte ranges double as highlight ranges without an offset map;
//! only multi-token substring/phrase highlights need
//! [`fold_with_offsets`].

use std::ops::Range;

use rust_stemmers::{Algorithm, Stemmer};
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Snowball algorithm names accepted by `[search] stemmers`.
pub const STEMMER_NAMES: &[&str] = &[
    "arabic",
    "danish",
    "dutch",
    "english",
    "finnish",
    "french",
    "german",
    "greek",
    "hungarian",
    "italian",
    "norwegian",
    "portuguese",
    "romanian",
    "russian",
    "spanish",
    "swedish",
    "tamil",
    "turkish",
];

/// The Snowball algorithm for a config name (case-insensitive).
pub fn stemmer_algorithm(name: &str) -> Option<Algorithm> {
    Some(match name.to_lowercase().as_str() {
        "arabic" => Algorithm::Arabic,
        "danish" => Algorithm::Danish,
        "dutch" => Algorithm::Dutch,
        "english" => Algorithm::English,
        "finnish" => Algorithm::Finnish,
        "french" => Algorithm::French,
        "german" => Algorithm::German,
        "greek" => Algorithm::Greek,
        "hungarian" => Algorithm::Hungarian,
        "italian" => Algorithm::Italian,
        "norwegian" => Algorithm::Norwegian,
        "portuguese" => Algorithm::Portuguese,
        "romanian" => Algorithm::Romanian,
        "russian" => Algorithm::Russian,
        "spanish" => Algorithm::Spanish,
        "swedish" => Algorithm::Swedish,
        "tamil" => Algorithm::Tamil,
        "turkish" => Algorithm::Turkish,
        _ => return None,
    })
}

/// Stem a word under `algo`, returning the folded stem. The stemmer sees
/// the lowercase but *unfolded* word: Portuguese and German suffix rules
/// need their accents.
pub fn stem(algo: Algorithm, word: &str) -> String {
    let lower = word.to_lowercase();
    fold(&Stemmer::create(algo).stem(&lower))
}

/// Case- and diacritic-fold `s`: lowercase, NFD, combining marks dropped.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        push_folded(c, &mut out);
    }
    out
}

fn push_folded(c: char, out: &mut String) {
    if c.is_ascii() {
        out.push(c.to_ascii_lowercase());
        return;
    }
    for d in c.to_lowercase().nfd() {
        if !is_combining_mark(d) {
            out.push(d);
        }
    }
}

/// [`fold`] plus, for every byte of the folded string, the byte offset
/// of the original char it came from. Used to map a match in folded
/// text back to a range of the original text.
pub struct Folded {
    pub text: String,
    origin: Vec<usize>,
}

pub fn fold_with_offsets(s: &str) -> Folded {
    let mut text = String::with_capacity(s.len());
    let mut origin = Vec::with_capacity(s.len());
    for (i, c) in s.char_indices() {
        let before = text.len();
        push_folded(c, &mut text);
        origin.extend(std::iter::repeat_n(i, text.len() - before));
    }
    Folded { text, origin }
}

impl Folded {
    /// Map a byte range of the folded text to the original text range
    /// covering every source char that contributed to it.
    pub fn to_original(&self, r: Range<usize>, original: &str) -> Range<usize> {
        if r.start >= r.end || r.end > self.origin.len() {
            return 0..0;
        }
        let start = self.origin[r.start];
        let last = self.origin[r.end - 1];
        let end = last + original[last..].chars().next().map_or(0, char::len_utf8);
        start..end
    }
}

/// Kind of a token cut from paragraph text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A run of alphanumeric chars (combining marks kept inside).
    Word,
    /// The target of a `[[…]]` span: anchor stripped, alias resolved.
    /// The range covers the whole `[[…]]` span.
    Link,
    /// An inline `#tag` (without the `#`). The range covers `#tag`.
    Tag,
}

/// One token: folded text plus its byte range in the original text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    /// Folded token text (for `Word`/`Tag`) or folded link target.
    pub text: String,
    pub range: Range<usize>,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || is_combining_mark(c)
}

fn is_tag_char(c: char) -> bool {
    is_word_char(c) || matches!(c, '_' | '/' | '-')
}

/// Tokenize original paragraph text. Words inside a `[[…]]` span are
/// emitted as `Word` tokens too (so `=symmetric` matches
/// `[[Symmetric Multiprocessing]]`), and a `#tag` also yields the words
/// it contains.
pub fn tokenize(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut prev: Option<char> = None;
    while i < text.len() {
        let rest = &text[i..];
        if let Some(inner) = rest.strip_prefix("[[") {
            if let Some(end) = inner.find("]]") {
                let content = &inner[..end];
                let span = i..i + 2 + end + 2;
                if let Some(target) = link_target(content) {
                    out.push(Token {
                        kind: TokenKind::Link,
                        text: target,
                        range: span.clone(),
                    });
                }
                words(content, i + 2, &mut out);
                i = span.end;
                prev = Some(']');
                continue;
            }
        }
        let c = rest.chars().next().expect("non-empty rest");
        if c == '#' && prev.is_none_or(char::is_whitespace) {
            let body: usize = rest[1..]
                .chars()
                .take_while(|&c| is_tag_char(c))
                .map(char::len_utf8)
                .sum();
            let tag = &rest[1..1 + body];
            if tag.chars().any(|c| !c.is_ascii_digit() && is_word_char(c)) {
                out.push(Token {
                    kind: TokenKind::Tag,
                    text: fold(tag),
                    range: i..i + 1 + body,
                });
            }
            i += 1;
            prev = Some('#');
            continue;
        }
        if is_word_char(c) {
            let len: usize = rest
                .chars()
                .take_while(|&c| is_word_char(c))
                .map(char::len_utf8)
                .sum();
            out.push(Token {
                kind: TokenKind::Word,
                text: fold(&rest[..len]),
                range: i..i + len,
            });
            prev = rest[..len].chars().last();
            i += len;
            continue;
        }
        prev = Some(c);
        i += c.len_utf8();
    }
    out
}

/// Word tokens of `s`, ranges offset by `base`.
pub fn words(s: &str, base: usize, out: &mut Vec<Token>) {
    let mut start: Option<usize> = None;
    for (j, c) in s.char_indices().chain(std::iter::once((s.len(), ' '))) {
        match (start, is_word_char(c)) {
            (None, true) => start = Some(j),
            (Some(st), false) => {
                out.push(Token {
                    kind: TokenKind::Word,
                    text: fold(&s[st..j]),
                    range: base + st..base + j,
                });
                start = None;
            }
            _ => {}
        }
    }
}

/// Folded word tokens of `s` (no ranges) — for context fields.
pub fn word_texts(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    words(s, 0, &mut out);
    out.into_iter().map(|t| t.text).collect()
}

/// Fold `[[…]]` content into its match target: strip `#anchor`, use the
/// alias after `|`. `None` for empty content (`[[]]`, `[[#x]]`).
pub fn link_target(content: &str) -> Option<String> {
    let no_anchor = content.split('#').next().unwrap_or(content);
    let target = no_anchor.rsplit('|').next().unwrap_or(no_anchor).trim();
    (!target.is_empty()).then(|| fold(target))
}

/// True when `term` is a single word run (no separators): such terms
/// match through the token dictionary; others scan folded text.
pub fn is_single_run(term: &str) -> bool {
    !term.is_empty() && term.chars().all(is_word_char)
}

/// All 3-grams of a folded term (length ≥ 3 chars), counted in chars.
pub fn trigrams_of(term: &str) -> Vec<String> {
    let chars: Vec<char> = term.chars().collect();
    if chars.len() < 3 {
        return Vec::new();
    }
    chars.windows(3).map(|w| w.iter().collect()).collect()
}

/// Character-counted levenshtein distance.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Fuzzy distance threshold: 1 up to 4 chars, else `chars / 4`.
pub fn levenshtein_threshold(term: &str) -> usize {
    let n = term.chars().count();
    if n <= 4 {
        1
    } else {
        n / 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_strips_case_and_diacritics() {
        assert_eq!(fold("Ônibus"), "onibus");
        assert_eq!(fold("estações"), "estacoes");
        assert_eq!(fold("Naïve Café"), "naive cafe");
        assert_eq!(fold("Straße"), "straße");
        assert_eq!(fold("plain ascii"), "plain ascii");
        assert_eq!(fold("emoji 🚌 ok"), "emoji 🚌 ok");
    }

    #[test]
    fn fold_with_offsets_maps_back_to_original() {
        let original = "O Ônibus chegou";
        let f = fold_with_offsets(original);
        assert_eq!(f.text, fold(original));
        let at = f.text.find("onibus").unwrap();
        let r = f.to_original(at..at + "onibus".len(), original);
        assert_eq!(&original[r], "Ônibus");
    }

    #[test]
    fn offsets_cover_multi_char_lowercase_expansion() {
        // `İ` lowercases to `i` + U+0307; the combining dot is dropped.
        let original = "xİy";
        let f = fold_with_offsets(original);
        assert_eq!(f.text, "xiy");
        let r = f.to_original(1..2, original);
        assert_eq!(&original[r], "İ");
    }

    fn texts(tokens: &[Token], kind: TokenKind) -> Vec<&str> {
        tokens
            .iter()
            .filter(|t| t.kind == kind)
            .map(|t| t.text.as_str())
            .collect()
    }

    #[test]
    fn accented_words_are_whole_tokens() {
        let toks = tokenize("o ônibus da manhã");
        assert_eq!(
            texts(&toks, TokenKind::Word),
            vec!["o", "onibus", "da", "manha"]
        );
        let r = toks[1].range.clone();
        assert_eq!(&"o ônibus da manhã"[r], "ônibus");
    }

    #[test]
    fn link_targets_and_their_words() {
        let text = "See [[Foo Bar]], [[Baz#Section]], and [[Qux|Alias]].";
        let toks = tokenize(text);
        assert_eq!(
            texts(&toks, TokenKind::Link),
            vec!["foo bar", "baz", "alias"]
        );
        let words = texts(&toks, TokenKind::Word);
        for w in ["see", "foo", "bar", "baz", "section", "qux", "alias", "and"] {
            assert!(words.contains(&w), "missing word {w:?} in {words:?}");
        }
        let link = toks.iter().find(|t| t.kind == TokenKind::Link).unwrap();
        assert_eq!(&text[link.range.clone()], "[[Foo Bar]]");
    }

    #[test]
    fn empty_link_targets_are_skipped() {
        let toks = tokenize("[[]] and [[#only-anchor]]");
        assert!(texts(&toks, TokenKind::Link).is_empty());
    }

    #[test]
    fn inline_tags() {
        let text = "#idea Share the cache, see #proj/busao and #123 but not a#b or # heading";
        let toks = tokenize(text);
        assert_eq!(texts(&toks, TokenKind::Tag), vec!["idea", "proj/busao"]);
        // The tag's words are still words.
        assert!(texts(&toks, TokenKind::Word).contains(&"idea"));
        let tag = toks.iter().find(|t| t.kind == TokenKind::Tag).unwrap();
        assert_eq!(&text[tag.range.clone()], "#idea");
    }

    #[test]
    fn heading_marker_is_not_a_tag() {
        let toks = tokenize("# Heading");
        assert!(texts(&toks, TokenKind::Tag).is_empty());
        assert_eq!(texts(&toks, TokenKind::Word), vec!["heading"]);
    }

    #[test]
    fn underscores_split_words() {
        let toks = tokenize("cgroup_memory=1");
        assert_eq!(texts(&toks, TokenKind::Word), vec!["cgroup", "memory", "1"]);
    }

    #[test]
    fn single_run_detection() {
        assert!(is_single_run("onibus"));
        assert!(is_single_run("eigen2"));
        assert!(!is_single_run("eigen-dec"));
        assert!(!is_single_run("shared memory"));
        assert!(!is_single_run(""));
    }

    #[test]
    fn stems_share_across_inflections() {
        assert_eq!(
            stem(Algorithm::English, "memories"),
            stem(Algorithm::English, "memory")
        );
        assert_eq!(
            stem(Algorithm::Portuguese, "casas"),
            stem(Algorithm::Portuguese, "casa")
        );
    }

    #[test]
    fn stemmer_names_resolve() {
        for name in STEMMER_NAMES {
            assert!(stemmer_algorithm(name).is_some(), "{name}");
        }
        assert!(stemmer_algorithm("English").is_some());
        assert!(stemmer_algorithm("klingon").is_none());
    }

    #[test]
    fn trigrams_split_on_chars_not_bytes() {
        assert_eq!(trigrams_of("móv"), vec!["móv".to_string()]);
        assert_eq!(
            trigrams_of("móve"),
            vec!["móv".to_string(), "óve".to_string()]
        );
        assert!(trigrams_of("óé").is_empty());
    }

    #[test]
    fn levenshtein_counts_chars() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("ônibus", "onibus"), 1);
        assert_eq!(levenshtein_threshold("memoizaton"), 2);
        assert_eq!(levenshtein_threshold("ôni"), 1);
    }
}

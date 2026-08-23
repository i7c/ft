//! Matching, ranking, and orchestration for paragraph search.
//!
//! Clause matching uses the index's fast paths where possible:
//! word/link clauses resolve straight to postings, fuzzy clauses go
//! through the trigram candidate filter + levenshtein verification, and
//! substring/phrase clauses scan the folded text. Each clause's matched
//! document set is computed once; AND/OR/exclude combination and
//! scoring then work on those sets, so the expensive dictionary work
//! (fuzzy levenshtein, postings unions) happens exactly once per query.
//!
//! Relevance scoring is a heuristic, deliberately simple: matched-clause
//! mode weights (phrase 3, word/link 2, substring 1.5, fuzzy 1) scaled
//! by an occurrence boost (1 + 0.5 × (occ − 1), capped at ×3) plus a
//! position bonus for earlier first hits. Ties break on vault-relative
//! path then line start, so identical queries over an identical index
//! order identically.

use std::collections::HashSet;
use std::path::PathBuf;

use chrono::NaiveDate;

use crate::blame_cache::{paragraph_date, BlameCache};
use crate::error::Result;
use crate::git;
use crate::search::index::{trigrams_of, SearchIndex};
use crate::search::query::{Clause, Mode, SearchQuery};
use crate::vault::Vault;

/// Result sort order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    /// Relevance score descending (default).
    Relevance,
    /// `git blame` date descending (newest edit first), ties by score.
    Date,
}

impl Sort {
    pub fn parse(s: &str) -> Option<Sort> {
        match s {
            "relevance" => Some(Sort::Relevance),
            "date" => Some(Sort::Date),
            _ => None,
        }
    }
}

/// One search result row: a matching paragraph plus its ranking data.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Vault-relative source path.
    pub path: PathBuf,
    /// 1-indexed line of the paragraph's first line.
    pub line_start: u32,
    /// 1-indexed line of the paragraph's last line.
    pub line_end: u32,
    /// Verbatim paragraph text.
    pub body: String,
    /// Raw clause labels of the positive clauses that matched.
    pub matched: Vec<String>,
    /// Relevance score.
    pub score: f64,
    /// Blame date — populated only by the date-sort path.
    pub date: Option<NaiveDate>,
}

/// Run a query against the index, relevance-sorted.
pub fn search(index: &SearchIndex, query: &SearchQuery) -> Vec<SearchResult> {
    let Some((sets, docs)) = match_query(index, query) else {
        return Vec::new();
    };

    let mut results: Vec<SearchResult> = docs
        .into_iter()
        .map(|doc_id| {
            let (score, matched) = score_doc(index, doc_id, &query.clauses, &sets);
            let doc = &index.docs()[doc_id as usize];
            SearchResult {
                path: doc.path.clone(),
                line_start: doc.line_start,
                line_end: doc.line_end,
                body: doc.text.clone(),
                matched,
                score,
                date: None,
            }
        })
        .collect();

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line_start.cmp(&b.line_start))
    });
    results
}

/// Run a query, ordering by the paragraph's blame date (newest first),
/// blaming only the result-set files via the shared cache. Ties fall
/// back to relevance then path/line.
pub fn search_with_dates(
    index: &SearchIndex,
    query: &SearchQuery,
    vault: &Vault,
    cache: &mut BlameCache,
) -> Result<Vec<SearchResult>> {
    let mut results = search(index, query);
    if results.is_empty() {
        return Ok(results);
    }

    let repo = git::RepoMap::discover(&vault.path)?;
    let head = git::head_hash(repo.root())?;

    // Per-file blame, loaded once per file; per-paragraph dates derived
    // from the cached blame (line ranges differ within a file).
    let mut blame_by_path: std::collections::HashMap<String, Option<Vec<git::LineBlame>>> =
        std::collections::HashMap::new();
    for r in results.iter_mut() {
        let path_str = r.path.to_string_lossy().into_owned();
        let blame = blame_by_path.entry(path_str.clone()).or_insert_with(|| {
            if cache.get(&path_str, &head).is_none() {
                if let Ok(blame) = git::blame_file(repo.root(), &repo.to_repo(&r.path)) {
                    cache.insert(path_str.clone(), head.clone(), blame);
                }
            }
            cache.get(&path_str, &head).cloned()
        });
        r.date = blame
            .as_deref()
            .and_then(|b| paragraph_date(b, r.line_start, r.line_end));
    }
    let _ = cache.save(&vault.path);

    results.sort_by(|a, b| {
        date_of(b)
            .cmp(&date_of(a))
            .then_with(|| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line_start.cmp(&b.line_start))
    });
    Ok(results)
}

/// Blame-failure and untracked paragraphs sort as the oldest possible
/// (no date → last under newest-first).
fn date_of(r: &SearchResult) -> NaiveDate {
    r.date.unwrap_or(NaiveDate::MIN)
}

/// Match every clause once, then combine: positives AND (or OR, when
/// `query.any`), excludes filter the survivors. Returns the per-clause
/// matched sets (parallel to `query.clauses`) and the surviving doc
/// ids. `None` when the query has no positive clauses.
fn match_query(index: &SearchIndex, query: &SearchQuery) -> Option<(Vec<HashSet<u32>>, Vec<u32>)> {
    let sets: Vec<HashSet<u32>> = query
        .clauses
        .iter()
        .map(|c| clause_matches(index, c).into_iter().collect())
        .collect();

    let positives: Vec<usize> = query
        .clauses
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.negated)
        .map(|(i, _)| i)
        .collect();
    let first = *positives.first()?;

    let mut working: HashSet<u32> = sets[first].clone();
    if query.any {
        for i in positives.iter().skip(1) {
            working.extend(sets[*i].iter().copied());
        }
    } else {
        for i in positives.iter().skip(1) {
            if working.is_empty() {
                break;
            }
            let set = &sets[*i];
            working.retain(|id| set.contains(id));
        }
    }

    for (i, c) in query.clauses.iter().enumerate() {
        if c.negated {
            let set = &sets[i];
            working.retain(|id| !set.contains(id));
        }
    }

    Some((sets, working.into_iter().collect()))
}

/// Paragraph ids matching one clause. Word/link resolve straight to
/// postings; fuzzy goes through trigram candidates + levenshtein;
/// substring/phrase scan the folded text.
fn clause_matches(index: &SearchIndex, clause: &Clause) -> Vec<u32> {
    match clause.mode {
        Mode::Word => index
            .token_ids
            .get(&clause.term)
            .into_iter()
            .flat_map(|id| index.postings[*id as usize].iter().copied())
            .collect(),
        Mode::Link => index
            .link_postings
            .get(&clause.term)
            .into_iter()
            .flat_map(|v| v.iter().copied())
            .collect(),
        Mode::Fuzzy => fuzzy_postings(index, &clause.term),
        Mode::Substring | Mode::Phrase => (0..index.paragraph_count() as u32)
            .filter(|id| text_matches(index, *id, clause))
            .collect(),
    }
}

/// Fuzzy postings: trigram candidates (or prefix scan for short terms)
/// narrowed by levenshtein, then postings.
fn fuzzy_postings(index: &SearchIndex, term: &str) -> Vec<u32> {
    let mut candidate_ids: HashSet<u32> = HashSet::new();
    if term.chars().count() <= 3 {
        for (tok, id) in &index.token_ids {
            if tok.starts_with(term) {
                candidate_ids.insert(*id);
            }
        }
    } else {
        for trig in trigrams_of(term) {
            if let Some(ids) = index.trigrams.get(&trig) {
                candidate_ids.extend(ids.iter().copied());
            }
        }
    }

    let threshold = levenshtein_threshold(term);
    let mut out: Vec<u32> = candidate_ids
        .into_iter()
        .filter(|id| {
            let tok = &index.tokens[*id as usize];
            levenshtein(term, tok) <= threshold
        })
        .flat_map(|id| index.postings[id as usize].iter().copied())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Levenshtein distance threshold: 1 for terms up to 4 chars, else
/// `len / 4`.
fn levenshtein_threshold(term: &str) -> usize {
    if term.len() <= 4 {
        1
    } else {
        term.len() / 4
    }
}

/// Classic two-row DP levenshtein distance.
fn levenshtein(a: &str, b: &str) -> usize {
    let a = a.as_bytes();
    let b = b.as_bytes();
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

/// Substring/phrase text test against a document's folded text.
fn text_matches(index: &SearchIndex, doc_id: u32, clause: &Clause) -> bool {
    index.docs()[doc_id as usize].folded.contains(&clause.term)
}

/// Score one document against the clauses using the precomputed matched
/// sets; returns `(score, matched labels)`. Excludes never contribute.
fn score_doc(
    index: &SearchIndex,
    doc_id: u32,
    clauses: &[Clause],
    sets: &[HashSet<u32>],
) -> (f64, Vec<String>) {
    let folded = &index.docs()[doc_id as usize].folded;
    let mut score = 0.0;
    let mut matched = Vec::new();
    for (i, clause) in clauses.iter().enumerate() {
        if clause.negated || !sets[i].contains(&doc_id) {
            continue;
        }
        matched.push(clause.raw.clone());
        let (occ, first) = occurrences_and_first(folded, &clause.term, clause.mode);
        let weight = match clause.mode {
            Mode::Phrase => 3.0,
            Mode::Word | Mode::Link => 2.0,
            Mode::Substring => 1.5,
            Mode::Fuzzy => 1.0,
        };
        let boost = 1.0 + 0.5 * (occ.saturating_sub(1) as f64).min(4.0);
        let position = if first == usize::MAX {
            0.0
        } else {
            1.0 / (1.0 + first as f64)
        };
        score += weight * boost + position;
    }
    (score, matched)
}

/// Count non-overlapping occurrences of the term in the folded text and
/// the byte offset of the first. For fuzzy clauses the typed term is
/// usually absent (it is a typo), so we count 1 and report no position
/// unless the term itself appears.
fn occurrences_and_first(folded: &str, term: &str, mode: Mode) -> (u32, usize) {
    if mode == Mode::Fuzzy {
        let first = folded.find(term).unwrap_or(usize::MAX);
        return (1, first);
    }
    if term.is_empty() {
        return (0, usize::MAX);
    }
    let mut count = 0u32;
    let mut first = None;
    let mut idx = 0;
    while let Some(rel) = folded[idx..].find(term) {
        let abs = idx + rel;
        if first.is_none() {
            first = Some(abs);
        }
        count += 1;
        idx = abs + term.len();
    }
    (count, first.unwrap_or(usize::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::query::parse;

    fn idx(files: &[(&str, &str)]) -> SearchIndex {
        let tmp = assert_fs::TempDir::new().unwrap();
        for (name, content) in files {
            let path = tmp.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
        let scan = crate::scan::scan_vault(tmp.path(), &[]);
        SearchIndex::build(&scan, &[])
    }

    fn bodies(results: &[SearchResult]) -> Vec<&str> {
        results.iter().map(|r| r.body.as_str()).collect()
    }

    #[test]
    fn substring_matches_fragments() {
        let index = idx(&[("a.md", "The eigen decomposition is central.\n")]);
        let q = parse("eigen", false);
        assert_eq!(
            bodies(&search(&index, &q)),
            vec!["The eigen decomposition is central."]
        );
        let q = parse("eigenval", false);
        assert!(search(&index, &q).is_empty());
        let index = idx(&[("a.md", "The eigenvalue is large.\n")]);
        let q = parse("eigen", false);
        assert_eq!(
            bodies(&search(&index, &q)).len(),
            1,
            "eigen matches eigenvalue"
        );
    }

    #[test]
    fn word_mode_requires_whole_token() {
        let index = idx(&[("a.md", "eigenvalue and eigen.\n")]);
        let q = parse("=eigen", false);
        let results = search(&index, &q);
        assert_eq!(results.len(), 1, "whole-token eigen present");
    }

    #[test]
    fn word_mode_does_not_match_fragment_only() {
        let index = idx(&[("a.md", "eigenvalue only.\n")]);
        let q = parse("=eigen", false);
        assert!(search(&index, &q).is_empty(), "no whole-token eigen");
    }

    #[test]
    fn word_mode_matches_link_target() {
        let index = idx(&[("a.md", "See [[memoization]] here.\n")]);
        let q = parse("=memoization", false);
        assert_eq!(search(&index, &q).len(), 1);
    }

    #[test]
    fn link_clause_restricts_to_link_targets() {
        let index = idx(&[
            ("a.md", "[[memoization]] is the word.\n"),
            ("b.md", "prose memoization only\n"),
        ]);
        let q = parse("[[memoization]]", false);
        let results = search(&index, &q);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, PathBuf::from("a.md"));
    }

    #[test]
    fn fuzzy_tolerates_typos() {
        let index = idx(&[("a.md", "memoization pays off.\n")]);
        let q = parse("~memoizaton", false);
        assert_eq!(search(&index, &q).len(), 1);
        // A clearly unrelated term never matches, whatever the threshold.
        let q = parse("~bananas", false);
        assert!(search(&index, &q).is_empty());
    }

    #[test]
    fn fuzzy_short_term_prefix_scan() {
        let index = idx(&[("a.md", "foo bar baz.\n")]);
        let q = parse("~fo", false);
        assert_eq!(search(&index, &q).len(), 1, "fo → foo via prefix scan");
    }

    #[test]
    fn phrase_requires_contiguous() {
        let index = idx(&[("a.md", "the eigen decomposition matters\n")]);
        let q = parse("\"eigen decomposition\"", false);
        assert_eq!(search(&index, &q).len(), 1);
        let q = parse("\"decomposition eigen\"", false);
        assert!(search(&index, &q).is_empty());
    }

    #[test]
    fn and_requires_all_terms_in_same_paragraph() {
        let index = idx(&[
            ("a.md", "eigen and memoization together.\n"),
            ("b.md", "only eigen here.\n"),
            ("c.md", "only memoization here.\n"),
        ]);
        let q = parse("eigen memoization", false);
        assert_eq!(
            bodies(&search(&index, &q)),
            vec!["eigen and memoization together."]
        );
    }

    #[test]
    fn any_mode_unions() {
        let index = idx(&[("a.md", "eigen only.\n"), ("b.md", "memoization only.\n")]);
        let q = parse("eigen memoization", true);
        assert_eq!(search(&index, &q).len(), 2);
    }

    #[test]
    fn exclude_filters_after_matching() {
        let index = idx(&[("a.md", "eigen and task.\n"), ("b.md", "eigen only.\n")]);
        let q = parse("eigen -task", false);
        let results = search(&index, &q);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, PathBuf::from("b.md"));
    }

    #[test]
    fn exclude_link_clause() {
        let index = idx(&[
            ("a.md", "[[foo]] and [[bar]].\n"),
            ("b.md", "[[foo]] only.\n"),
        ]);
        let q = parse("[[foo]] -[[bar]]", false);
        let results = search(&index, &q);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, PathBuf::from("b.md"));
    }

    #[test]
    fn no_match_is_empty() {
        let index = idx(&[("a.md", "nothing here.\n")]);
        let q = parse("zebra", false);
        assert!(search(&index, &q).is_empty());
    }

    #[test]
    fn empty_query_is_empty() {
        let index = idx(&[("a.md", "something.\n")]);
        let q = parse("", false);
        assert!(search(&index, &q).is_empty());
    }

    #[test]
    fn more_clauses_rank_higher() {
        let index = idx(&[
            ("a.md", "eigen and memoization.\n"),
            ("b.md", "eigen only.\n"),
        ]);
        let q = parse("eigen memoization", false);
        let results = search(&index, &q);
        assert_eq!(results[0].body, "eigen and memoization.");
    }

    #[test]
    fn deterministic_tiebreak_path_then_line() {
        let index = idx(&[("b.md", "shared term.\n"), ("a.md", "shared term.\n")]);
        let q = parse("shared term", false);
        let results = search(&index, &q);
        assert_eq!(results[0].path, PathBuf::from("a.md"), "path asc");
        assert_eq!(results[1].path, PathBuf::from("b.md"));
    }

    #[test]
    fn wikilink_with_spaces_matches() {
        let index = idx(&[
            ("a.md", "See [[Bar Foo]] today.\n"),
            ("b.md", "Bar Foo prose.\n"),
        ]);
        let q = parse("[[Bar Foo]]", false);
        let results = search(&index, &q);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].path,
            PathBuf::from("a.md"),
            "link clause only matches the link"
        );
    }

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("memoizaton", "memoization"), 1);
        assert_eq!(levenshtein("foo", "foo"), 0);
        assert_eq!(levenshtein("", "abc"), 3);
    }

    #[test]
    fn fuzzy_matches_link_target_tokens() {
        let index = idx(&[("a.md", "See [[memoization]].\n")]);
        let q = parse("~memoizaton", false);
        assert_eq!(
            search(&index, &q).len(),
            1,
            "fuzzy over the link-target token"
        );
    }

    #[test]
    fn fuzzy_over_non_ascii_term_does_not_panic() {
        // Both the indexed link target and the query term carry
        // multi-byte scalars; trigram generation used to slice bytes.
        let index = idx(&[("a.md", "See [[móveis]] and [[problems with móveis ii]].\n")]);
        assert_eq!(search(&index, &parse("~móveis", false)).len(), 1);
        // Short non-ASCII term takes the prefix-scan branch.
        assert!(search(&index, &parse("~óé", false)).is_empty());
    }

    // ── perf budget (gated; run with FT_PERF_TESTS=1) ─────────────────

    fn perf_enabled() -> bool {
        std::env::var("FT_PERF_TESTS").as_deref() == Ok("1")
    }

    /// Build a synthetic 5,000-paragraph index in memory (no filesystem
    /// writes): 500 files × 10 paragraphs, with a spread of topics so
    /// substring and fuzzy queries have real work to do.
    fn synthetic_5k_index() -> SearchIndex {
        use crate::markdown::Paragraph;
        use crate::scan::ParsedFile;

        let mut files = Vec::new();
        for f in 0..500usize {
            let mut paragraphs = Vec::new();
            for p in 0..10u32 {
                let topic = match (p + f as u32) % 5 {
                    0 => "eigenvalue decomposition",
                    1 => "memoization strategy",
                    2 => "levenshtein threshold",
                    3 => "trigram candidate filter",
                    _ => "provenance pinning",
                };
                let text = format!(
                    "Paragraph {p} of file {f}: about {topic}, with extra words to pad the text"
                );
                paragraphs.push(Paragraph {
                    line_start: p * 2 + 1,
                    line_end: p * 2 + 1,
                    text,
                });
            }
            files.push(ParsedFile {
                rel: PathBuf::from(format!("notes/file-{f:04}.md")),
                links: Vec::new(),
                paragraphs,
                headings: Vec::new(),
                frontmatter: None,
                mtime: std::time::UNIX_EPOCH,
                line_count: 20,
            });
        }
        let scan = crate::scan::Scan {
            files,
            ..Default::default()
        };
        SearchIndex::build(&scan, &[])
    }

    #[test]
    fn perf_budget_holds() {
        if !perf_enabled() {
            return;
        }
        let build_start = std::time::Instant::now();
        let index = synthetic_5k_index();
        assert_eq!(index.paragraph_count(), 5000);
        let build_ms = build_start.elapsed().as_secs_f64() * 1000.0;
        assert!(
            build_ms < 500.0,
            "index build over 5k paragraphs took {build_ms:.1}ms (budget 500ms)"
        );

        for query in [
            "eigenvalue",
            "memoization",
            "~memoizaton",
            "=threshold",
            "\"trigram candidate\"",
            "eigenvalue -strategy",
        ] {
            let q = parse(query, false);
            let start = std::time::Instant::now();
            let results = search(&index, &q);
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            assert!(
                ms < 10.0,
                "query {query:?} over 5k paragraphs took {ms:.2}ms (budget 10ms)"
            );
            assert!(
                !results.is_empty(),
                "query {query:?} should match synthetic corpus"
            );
        }
    }
}

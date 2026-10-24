//! Matching, ranking, and orchestration for paragraph search.
//!
//! Every text clause resolves to a set of dictionary tokens — word
//! (exact + stem-mates), substring (tokens containing the term), fuzzy
//! (trigram candidates within the levenshtein threshold) — or, for
//! multi-token substrings and phrases, to occurrence counts from a scan
//! of the folded field text; link clauses read the body link postings.
//! Positive clauses match the body or any context field (title, aliases,
//! heading path, tags); excludes and links read the body only.
//!
//! A clause's contribution to a paragraph is its mode weight times the
//! best BM25F score among its resolved tokens: per-field term frequency,
//! weighted and length-normalized per field, saturated by `K1`, times the
//! highest per-field inverse document frequency, damped when only
//! context fields matched. The paragraph score is
//! the sum over matched positive clauses, multiplied by the prior —
//! note in-degree plus paragraph recency (blame day when the query-time
//! [`RecencyTable`] has it, else file mtime), `[1, 1.25]` — and the
//! `near:` boost. Ties
//! break on vault-relative path then line start, so identical queries
//! over an identical index order identically.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::path::PathBuf;

use chrono::NaiveDate;

use crate::blame_cache::{paragraph_date, BlameCache, BlameValidity};
use crate::git;
use crate::graph::query::{Profile, WalkNode, WalkOptions};
use crate::graph::{Graph, NodeKind};
use crate::search::index::{FastMap, Field, SearchIndex, FIELDS};
use crate::search::query::{Clause, Filter, Mode, SearchQuery};
use crate::search::recency::{recency_part, RecencyTable};
use crate::search::semantic::SemanticQuery;
use crate::search::text::{self, TokenKind};
use crate::vault::Vault;

/// BM25 term-frequency saturation.
const K1: f64 = 1.2;
/// Per-field weights, indexed by `Field as usize`
/// (body, title, alias, heading, tag).
const FIELD_WEIGHT: [f64; 5] = [1.0, 3.0, 2.5, 2.0, 2.0];
/// Per-field length normalization strength.
const FIELD_B: [f64; 5] = [0.75, 0.0, 0.0, 0.3, 0.0];
/// Reciprocal Rank Fusion constant: `1 / (RRF_K + rank)`.
const RRF_K: f64 = 60.0;
/// Damping for a clause that matches a paragraph only through context
/// fields: inherited title/alias/heading/tag text is weaker evidence than
/// the paragraph's own words, and without it every paragraph of a note
/// whose title matches would flood the top of the list.
const CONTEXT_ONLY: f64 = 0.4;
/// Boost for `near:` by link distance from the seed: 0, 1, 2 hops.
const NEAR_BOOST: [f64; 3] = [1.5, 1.3, 1.15];

/// Clause weight by mode: exact intent above approximate.
fn mode_weight(mode: Mode) -> f64 {
    match mode {
        Mode::Phrase => 1.2,
        Mode::Word | Mode::Link => 1.0,
        Mode::Substring => 0.85,
        Mode::Fuzzy => 0.6,
        Mode::Filter(_) | Mode::Near => 0.0,
    }
}

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

/// Why a paragraph matched: one positive clause and one field it hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchReason {
    /// The clause label as typed.
    pub clause: String,
    pub field: Field,
}

/// One search result row: a matching paragraph plus its ranking and
/// presentation data.
#[derive(Debug, Clone, Default)]
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
    /// Blame date — populated by [`search_with_dates`] when sorting by
    /// date or filtering by date.
    pub date: Option<NaiveDate>,
    /// The note's file stem.
    pub title: String,
    /// Enclosing headings, outermost first.
    pub heading_path: Vec<String>,
    /// Byte ranges of `body` to emphasize, sorted and merged. Filled by
    /// [`fill_highlights`], empty until then.
    pub highlights: Vec<Range<usize>>,
    /// Per-clause, per-field match reasons.
    pub reasons: Vec<MatchReason>,
}

/// What a query may consult beyond the index: the link graph (for
/// `scope:`), the user's graph presets (shadowing built-ins), and the
/// paragraph recency table.
#[derive(Debug, Clone, Copy, Default)]
pub struct SearchCtx<'a> {
    pub graph: Option<&'a Graph>,
    pub presets: Option<&'a HashMap<String, String>>,
    /// Paragraph blame days; without it recency uses file mtimes.
    pub recency: Option<&'a RecencyTable>,
    /// Embeddings and model for hybrid retrieval; `None` is lexical only.
    pub semantic: Option<SemanticQuery<'a>>,
    /// More-like-this source: the paragraph at `(path, line_start)`.
    /// Needs `semantic`.
    pub like: Option<(&'a std::path::Path, u32)>,
}

/// Errors a query can raise at search time.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("unknown scope preset `{name}` (available: {})", .available.join(", "))]
    UnknownScope {
        name: String,
        available: Vec<String>,
    },
    #[error("scope preset `{name}` is not a valid graph query: {message}")]
    InvalidScope { name: String, message: String },
    #[error("`scope:` needs the link graph")]
    NoGraph,
    #[error("`after:` / `before:` need blame dates (search_with_dates)")]
    NeedsDates,
    #[error("more-like-this needs paragraph embeddings (run `ft notes embed fetch` and `ft notes embed build`)")]
    NeedsEmbeddings,
    #[error("no indexed paragraph starts at {path}:{line}")]
    NoSuchParagraph { path: String, line: u32 },
    #[error(transparent)]
    Core(#[from] crate::error::Error),
}

/// Run a query against the index, relevance-sorted. Date filters need
/// [`search_with_dates`] and fail here with [`SearchError::NeedsDates`].
pub fn search(
    index: &SearchIndex,
    query: &SearchQuery,
    ctx: SearchCtx<'_>,
) -> Result<Vec<SearchResult>, SearchError> {
    if query.has_date_filters() {
        return Err(SearchError::NeedsDates);
    }
    let mut results = run(index, query, ctx)?;
    sort_by_relevance(&mut results);
    Ok(results)
}

/// [`search_with_dates`] output: the results plus how many otherwise
/// matching paragraphs a date filter dropped for having no blame date.
#[derive(Debug, Default)]
pub struct DatedSearch {
    pub results: Vec<SearchResult>,
    pub undated_dropped: usize,
}

/// Run a query that needs blame dates — a date sort or `after:` /
/// `before:` filters — blaming only the files of the paragraphs that
/// survive every other clause, through the shared cache (saved under
/// `vault` when anything was blamed). `validity` is the caller's
/// [`BlameValidity`] snapshot, `None` outside git. Paragraphs without a
/// blame date (untracked, no git) never pass a date filter and sort as
/// oldest.
pub fn search_with_dates(
    index: &SearchIndex,
    query: &SearchQuery,
    ctx: SearchCtx<'_>,
    vault: &Vault,
    validity: Option<&BlameValidity>,
    cache: &mut BlameCache,
    sort: Sort,
) -> Result<DatedSearch, SearchError> {
    let mut results = run(index, query, ctx)?;
    sort_by_relevance(&mut results);
    let date_filters: Vec<&Clause> = query
        .filters()
        .filter(|c| {
            matches!(
                c.mode,
                Mode::Filter(Filter::After(_)) | Mode::Filter(Filter::Before(_))
            )
        })
        .collect();
    if results.is_empty() || (sort == Sort::Relevance && date_filters.is_empty()) {
        return Ok(DatedSearch {
            results,
            undated_dropped: 0,
        });
    }

    if let Some(validity) = validity {
        // Per-file blame, loaded once per file; per-paragraph dates
        // derived from it (line ranges differ within a file).
        let mut blame_by_path: HashMap<PathBuf, Option<Vec<git::LineBlame>>> = HashMap::new();
        let mut blamed = false;
        for r in results.iter_mut() {
            let blame = blame_by_path.entry(r.path.clone()).or_insert_with(|| {
                let lines = index
                    .note_by_path
                    .get(&r.path)
                    .map_or(0, |&n| index.notes[n as usize].line_count);
                let (blame, ran) = cache.ensure(&r.path, lines, validity);
                blamed |= ran;
                blame
            });
            r.date = blame
                .as_deref()
                .and_then(|b| paragraph_date(b, r.line_start, r.line_end));
        }
        if blamed {
            let _ = cache.save(&vault.path);
        }
    }

    let mut undated_dropped = 0;
    if !date_filters.is_empty() {
        results.retain(|r| {
            let Some(d) = r.date else {
                undated_dropped += 1;
                return false;
            };
            date_filters.iter().all(|c| {
                let pass = match c.mode {
                    Mode::Filter(Filter::After(x)) => d >= x,
                    Mode::Filter(Filter::Before(x)) => d <= x,
                    _ => true,
                };
                pass != c.negated
            })
        });
    }

    if sort == Sort::Date {
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
    }
    Ok(DatedSearch {
        results,
        undated_dropped,
    })
}

/// Blame-failure and untracked paragraphs sort as the oldest possible
/// (no date → last under newest-first).
fn date_of(r: &SearchResult) -> NaiveDate {
    r.date.unwrap_or(NaiveDate::MIN)
}

fn sort_by_relevance(results: &mut [SearchResult]) {
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line_start.cmp(&b.line_start))
    });
}

/// Per-doc clause hit: BM25F score and the fields that matched (bitmask).
type Hits = FastMap<u32, (f64, u8)>;

/// Match, filter, and score — everything except date filters and sorting.
fn run(
    index: &SearchIndex,
    query: &SearchQuery,
    ctx: SearchCtx<'_>,
) -> Result<Vec<SearchResult>, SearchError> {
    let positives: Vec<&Clause> = query.positives().collect();
    let has_filters = query.filters().next().is_some();
    let semantic = ctx.semantic.filter(|s| s.index.fits(index.docs.len()));
    let like = match ctx.like {
        None => None,
        Some((path, line)) => {
            if semantic.is_none() {
                return Err(SearchError::NeedsEmbeddings);
            }
            Some(
                index
                    .doc_at(path, line)
                    .ok_or_else(|| SearchError::NoSuchParagraph {
                        path: path.display().to_string(),
                        line,
                    })?,
            )
        }
    };
    if positives.is_empty() && !has_filters && like.is_none() {
        return Ok(Vec::new());
    }

    // Resolve scopes first so an unknown preset errors even when the
    // text clauses match nothing.
    let mut scopes: Vec<(&Clause, HashSet<u32>)> = Vec::new();
    for c in query.filters() {
        if c.mode == Mode::Filter(Filter::Scope) {
            scopes.push((c, scope_notes(index, &c.term, ctx)?));
        }
    }

    // Candidate docs with per-clause hits (`(clause index, score, mask)`).
    let mut cands: FastMap<u32, Vec<(usize, f64, u8)>> = FastMap::default();
    if positives.is_empty() {
        // Filter-only: every paragraph; more-like-this: semantic only.
        if like.is_none() {
            for d in 0..index.docs.len() as u32 {
                cands.insert(d, Vec::new());
            }
        }
    } else {
        let hits: Vec<Hits> = positives
            .iter()
            .map(|c| match_clause(index, c, true))
            .collect();
        if query.any {
            for (i, h) in hits.iter().enumerate() {
                for (&doc, &(s, m)) in h {
                    cands.entry(doc).or_default().push((i, s, m));
                }
            }
        } else {
            let smallest = (0..hits.len())
                .min_by_key(|&i| hits[i].len())
                .expect("positives is non-empty");
            for &doc in hits[smallest].keys() {
                if hits.iter().all(|h| h.contains_key(&doc)) {
                    let per: Vec<(usize, f64, u8)> = hits
                        .iter()
                        .enumerate()
                        .map(|(i, h)| (i, h[&doc].0, h[&doc].1))
                        .collect();
                    cands.insert(doc, per);
                }
            }
        }
    }

    let mut excluded: HashSet<u32> = HashSet::new();
    for c in query.excludes() {
        excluded.extend(match_clause(index, c, false).keys());
    }
    let passes = |doc: u32| {
        if excluded.contains(&doc) {
            return false;
        }
        let d = &index.docs[doc as usize];
        let note = &index.notes[d.note as usize];
        query.filters().all(|c| {
            let pass = match c.mode {
                Mode::Filter(Filter::Path) => note.folded_path.starts_with(&c.term),
                Mode::Filter(Filter::Tag) => d
                    .tags
                    .iter()
                    .chain(note.tags.iter())
                    .any(|t| tag_matches(t, &c.term)),
                Mode::Filter(Filter::Scope) => scopes
                    .iter()
                    .find(|(sc, _)| std::ptr::eq(*sc, c))
                    .is_some_and(|(_, set)| set.contains(&d.note)),
                // Date filters apply in `search_with_dates`.
                _ => return true,
            };
            pass != c.negated
        })
    };
    cands.retain(|&doc, _| passes(doc));

    let base_of = |per: &[(usize, f64, u8)]| -> f64 {
        if positives.is_empty() {
            1.0
        } else {
            per.iter()
                .map(|&(i, s, _)| mode_weight(positives[i].mode) * s)
                .sum()
        }
    };

    // Semantic candidates join the set; both rankings fuse by RRF.
    let mut fused: Option<FastMap<u32, f64>> = None;
    if let Some(sq) = semantic {
        let qv = match like {
            Some(doc) => sq.index.vector(doc).map(<[f32]>::to_vec),
            None if !positives.is_empty() => Some(sq.embed_query(&semantic_text(&positives))),
            None => None,
        };
        if let Some(qv) = qv {
            let sem: Vec<u32> = sq
                .index
                .top_k(&qv, sq.candidates, sq.min_similarity, like)
                .into_iter()
                .map(|(doc, _)| doc)
                .filter(|&doc| passes(doc))
                .collect();
            let mut lexical: Vec<(u32, f64)> = cands
                .iter()
                .filter(|(_, per)| !per.is_empty())
                .map(|(&doc, per)| (doc, base_of(per)))
                .collect();
            lexical.sort_by(|a, b| {
                b.1.total_cmp(&a.1)
                    .then_with(|| {
                        index.docs[a.0 as usize]
                            .path
                            .cmp(&index.docs[b.0 as usize].path)
                    })
                    .then_with(|| a.0.cmp(&b.0))
            });
            let mut scores: FastMap<u32, f64> = FastMap::default();
            for (rank, (doc, _)) in lexical.iter().enumerate() {
                *scores.entry(*doc).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
            }
            for (rank, doc) in sem.iter().enumerate() {
                *scores.entry(*doc).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
                cands.entry(*doc).or_default();
            }
            fused = Some(scores);
        }
    }
    if like.is_some() && fused.is_none() {
        // The source paragraph has no vector yet.
        return Err(SearchError::NeedsEmbeddings);
    }

    let near = near_boosts(index, query);
    let results = cands
        .into_iter()
        .filter_map(|(doc, per)| {
            let d = &index.docs[doc as usize];
            let note = &index.notes[d.note as usize];
            let base = match &fused {
                Some(scores) => *scores.get(&doc)?,
                None => base_of(&per),
            };
            let boost = near.get(&d.note).copied().unwrap_or(1.0);
            let mut reasons = Vec::new();
            for &(i, _, mask) in &per {
                for f in FIELDS {
                    if mask & f.bit() != 0 {
                        reasons.push(MatchReason {
                            clause: positives[i].raw.clone(),
                            field: f,
                        });
                    }
                }
            }
            if per.is_empty() && fused.is_some() {
                reasons.push(MatchReason {
                    clause: SEMANTIC_LABEL.to_string(),
                    field: Field::Semantic,
                });
            }
            let mut matched_idx: Vec<usize> = per.iter().map(|&(i, _, _)| i).collect();
            matched_idx.sort_unstable();
            Some(SearchResult {
                path: d.path.clone(),
                line_start: d.line_start,
                line_end: d.line_end,
                body: d.text.clone(),
                matched: matched_idx
                    .into_iter()
                    .map(|i| positives[i].raw.clone())
                    .collect(),
                score: base * (note.prior + recency_part(index, ctx.recency, doc)) * boost,
                date: None,
                title: note.title.clone(),
                heading_path: index.sections[d.section as usize].heading_path.clone(),
                highlights: Vec::new(),
                reasons,
            })
        })
        .collect();
    Ok(results)
}

/// Clause label for results found only by meaning.
pub const SEMANTIC_LABEL: &str = "≈";

/// The text a query is embedded as: its positive text clauses as typed,
/// without DSL markup.
fn semantic_text(positives: &[&Clause]) -> String {
    positives
        .iter()
        .map(|c| {
            let raw = c.raw.as_str();
            match c.mode {
                Mode::Word | Mode::Fuzzy => raw.get(1..).unwrap_or(raw),
                Mode::Phrase => raw.trim_matches('"'),
                Mode::Link => raw.trim_start_matches("[[").trim_end_matches("]]"),
                _ => raw,
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `tag:x` keeps `x` and its descendants `x/…`.
fn tag_matches(tag: &str, want: &str) -> bool {
    tag == want
        || tag
            .strip_prefix(want)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The notes a graph preset selects, walked with default options so the
/// scope equals what the Graph tab shows for that preset.
fn scope_notes(
    index: &SearchIndex,
    name: &str,
    ctx: SearchCtx<'_>,
) -> Result<HashSet<u32>, SearchError> {
    let dsl = ctx
        .presets
        .and_then(|p| p.get(name))
        .map(String::as_str)
        .or_else(|| crate::graph::preset::builtin(name))
        .ok_or_else(|| {
            let mut available: Vec<String> = crate::graph::preset::builtin_names()
                .iter()
                .map(|s| s.to_string())
                .chain(ctx.presets.into_iter().flat_map(|p| p.keys().cloned()))
                .collect();
            available.sort();
            available.dedup();
            SearchError::UnknownScope {
                name: name.to_string(),
                available,
            }
        })?;
    let graph = ctx.graph.ok_or(SearchError::NoGraph)?;
    let q = crate::graph::query::parse_with(dsl, Profile::Default, crate::dates::today()).map_err(
        |e| SearchError::InvalidScope {
            name: name.to_string(),
            message: e.to_string(),
        },
    )?;
    let mut out = HashSet::new();
    let mut stack: Vec<&WalkNode> = Vec::new();
    let walk = q.walk(graph, &WalkOptions::default());
    stack.extend(walk.iter());
    while let Some(n) = stack.pop() {
        if let NodeKind::Note(d) = graph.node(n.id) {
            if let Some(&note) = index.note_by_path.get(&d.path) {
                out.insert(note);
            }
        }
        stack.extend(n.children.iter());
    }
    Ok(out)
}

/// Per-note boost from every `near:` clause: breadth-first over the
/// undirected note-link adjacency to depth 2; the strongest boost wins.
fn near_boosts(index: &SearchIndex, query: &SearchQuery) -> HashMap<u32, f64> {
    let mut out: HashMap<u32, f64> = HashMap::new();
    for c in query.nears() {
        let mut seen: HashSet<u32> = HashSet::new();
        let mut queue: VecDeque<(u32, usize)> = VecDeque::new();
        for &seed in index.notes_named(&c.term) {
            if seen.insert(seed) {
                queue.push_back((seed, 0));
            }
        }
        while let Some((note, dist)) = queue.pop_front() {
            let b = NEAR_BOOST[dist];
            let e = out.entry(note).or_insert(1.0);
            if b > *e {
                *e = b;
            }
            if dist + 1 >= NEAR_BOOST.len() {
                continue;
            }
            for &next in index.adjacency.get(note as usize).into_iter().flatten() {
                if seen.insert(next) {
                    queue.push_back((next, dist + 1));
                }
            }
        }
    }
    out
}

// ── Clause matching ──────────────────────────────────────────────────

/// Docs matching one text clause with their scores. `context` lets the
/// clause match context fields (positives); excludes pass `false` and
/// read the body only.
fn match_clause(index: &SearchIndex, clause: &Clause, context: bool) -> Hits {
    let allowed: u8 = if context { 0b11111 } else { Field::Body.bit() };
    match clause.mode {
        Mode::Word => score_tokens(index, &word_tokens(index, clause), allowed),
        Mode::Fuzzy => score_tokens(index, &fuzzy_tokens(index, &clause.term), allowed),
        Mode::Substring if text::is_single_run(&clause.term) => {
            score_tokens(index, &substring_tokens(index, &clause.term), allowed)
        }
        Mode::Substring | Mode::Phrase => text_hits(index, &clause.term, allowed),
        Mode::Link => link_hits(index, &clause.term),
        Mode::Filter(_) | Mode::Near => Hits::default(),
    }
}

/// Exact token plus its stem-mates under every configured stemmer.
fn word_tokens(index: &SearchIndex, clause: &Clause) -> Vec<u32> {
    let mut ids: Vec<u32> = index
        .token_ids
        .get(&clause.term)
        .copied()
        .into_iter()
        .collect();
    let surface = clause.raw.trim_start_matches('-').trim_start_matches('=');
    if text::is_single_run(&clause.term) {
        for (algo, map) in &index.stems {
            if let Some(mates) = map.get(&text::stem(*algo, surface)) {
                ids.extend(mates.iter().copied());
            }
        }
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Dictionary tokens containing `term`: trigram-narrowed for terms of
/// three chars or more, a dictionary scan otherwise.
fn substring_tokens(index: &SearchIndex, term: &str) -> Vec<u32> {
    let trigrams = text::trigrams_of(term);
    if trigrams.is_empty() {
        return (0..index.tokens.len() as u32)
            .filter(|&id| index.tokens[id as usize].contains(term))
            .collect();
    }
    let mut lists: Vec<&Vec<u32>> = Vec::with_capacity(trigrams.len());
    for t in &trigrams {
        match index.trigrams.get(t) {
            Some(ids) => lists.push(ids),
            None => return Vec::new(),
        }
    }
    lists.sort_by_key(|l| l.len());
    lists[0]
        .iter()
        .copied()
        .filter(|id| lists[1..].iter().all(|l| l.binary_search(id).is_ok()))
        .filter(|&id| index.tokens[id as usize].contains(term))
        .collect()
}

/// Dictionary tokens within the levenshtein threshold: trigram
/// candidates (or a prefix scan for terms of three chars or fewer).
fn fuzzy_tokens(index: &SearchIndex, term: &str) -> Vec<u32> {
    let mut candidates: HashSet<u32> = HashSet::new();
    if term.chars().count() <= 3 {
        for (tok, id) in &index.token_ids {
            if tok.starts_with(term) {
                candidates.insert(*id);
            }
        }
    } else {
        for trig in text::trigrams_of(term) {
            if let Some(ids) = index.trigrams.get(&trig) {
                candidates.extend(ids.iter().copied());
            }
        }
    }
    let threshold = text::levenshtein_threshold(term);
    let mut out: Vec<u32> = candidates
        .into_iter()
        .filter(|&id| text::levenshtein(term, &index.tokens[id as usize]) <= threshold)
        .collect();
    out.sort_unstable();
    out
}

/// Inverse document frequency over `units` with `df` containing units.
fn idf(df: u32, units: u32) -> f64 {
    let (df, n) = (df as f64, units as f64);
    (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
}

/// BM25F score of one doc given per-field term frequencies and
/// per-field idf; returns the score and the matched-field mask.
fn bm25f(index: &SearchIndex, doc: u32, tf: &[u32; 5], idfs: &[f64; 5]) -> (f64, u8) {
    let d = &index.docs[doc as usize];
    let mut tfn = 0.0;
    let mut best_idf: f64 = 0.0;
    let mut mask = 0u8;
    for f in FIELDS {
        let k = f as usize;
        if tf[k] == 0 {
            continue;
        }
        let unit = match f {
            Field::Body | Field::Semantic => doc,
            Field::Heading => d.section,
            Field::Title | Field::Alias | Field::Tag => d.note,
        };
        let len = index.unit_len(f, unit) as f64;
        let norm = 1.0 - FIELD_B[k] + FIELD_B[k] * len / index.avg_len[k];
        tfn += FIELD_WEIGHT[k] * tf[k] as f64 / norm.max(f64::EPSILON);
        best_idf = best_idf.max(idfs[k]);
        mask |= f.bit();
    }
    let damp = if tf[Field::Body as usize] == 0 {
        CONTEXT_ONLY
    } else {
        1.0
    };
    (damp * best_idf * tfn / (K1 + tfn), mask)
}

/// Keep the better score per doc and union the field masks.
fn merge_hit(out: &mut Hits, doc: u32, (s, m): (f64, u8)) {
    let e = out.entry(doc).or_insert((0.0, 0));
    if s > e.0 {
        e.0 = s;
    }
    e.1 |= m;
}

/// Score docs over resolved tokens: each token's postings in the allowed
/// fields, expanded from note/section units to their docs.
fn score_tokens(index: &SearchIndex, ids: &[u32], allowed: u8) -> Hits {
    let mut out = Hits::default();
    let mut per_doc: FastMap<u32, [u32; 5]> = FastMap::default();
    for &id in ids {
        let postings = &index.postings[id as usize];
        let mut df = [0u32; 5];
        for p in postings {
            if allowed & p.field.bit() != 0 {
                df[p.field as usize] += 1;
            }
        }
        let idfs: [f64; 5] = std::array::from_fn(|k| idf(df[k], index.field_units[k]));
        per_doc.clear();
        for p in postings {
            if allowed & p.field.bit() == 0 {
                continue;
            }
            for doc in index.unit_docs(p.field, p.unit) {
                per_doc.entry(doc).or_insert([0; 5])[p.field as usize] = p.tf as u32;
            }
        }
        for (&doc, tf) in &per_doc {
            merge_hit(&mut out, doc, bm25f(index, doc, tf, &idfs));
        }
    }
    out
}

/// Non-overlapping occurrences of `term` in `hay`.
fn count(hay: &str, term: &str) -> u32 {
    if term.is_empty() {
        return 0;
    }
    hay.matches(term).count() as u32
}

/// Multi-token substring / phrase: occurrence counts from the folded
/// text of each allowed field.
fn text_hits(index: &SearchIndex, term: &str, allowed: u8) -> Hits {
    let mut per_doc: FastMap<u32, [u32; 5]> = FastMap::default();
    let mut df = [0u32; 5];
    let mut add = |field: Field, unit: u32, tf: u32, per_doc: &mut FastMap<u32, [u32; 5]>| {
        if tf == 0 {
            return;
        }
        df[field as usize] += 1;
        for doc in index.unit_docs(field, unit) {
            per_doc.entry(doc).or_insert([0; 5])[field as usize] = tf;
        }
    };
    if allowed & Field::Body.bit() != 0 {
        for (i, d) in index.docs.iter().enumerate() {
            add(Field::Body, i as u32, count(&d.folded, term), &mut per_doc);
        }
    }
    if allowed & !Field::Body.bit() != 0 {
        for (i, n) in index.notes.iter().enumerate() {
            let i = i as u32;
            add(Field::Title, i, count(&n.folded_title, term), &mut per_doc);
            let alias: u32 = n.aliases.iter().map(|a| count(a, term)).sum();
            add(Field::Alias, i, alias, &mut per_doc);
            add(Field::Tag, i, count(&n.tags.join(" "), term), &mut per_doc);
        }
        for (i, s) in index.sections.iter().enumerate() {
            add(
                Field::Heading,
                i as u32,
                count(&s.folded, term),
                &mut per_doc,
            );
        }
    }
    let idfs: [f64; 5] = std::array::from_fn(|k| idf(df[k], index.field_units[k]));
    let mut out = Hits::default();
    for (doc, tf) in per_doc {
        merge_hit(&mut out, doc, bm25f(index, doc, &tf, &idfs));
    }
    out
}

/// Link clause: body link postings, one occurrence per doc.
fn link_hits(index: &SearchIndex, target: &str) -> Hits {
    let Some(docs) = index.link_postings.get(target) else {
        return Hits::default();
    };
    let mut idfs = [0.0; 5];
    idfs[Field::Body as usize] = idf(docs.len() as u32, index.field_units[Field::Body as usize]);
    let mut tf = [0u32; 5];
    tf[Field::Body as usize] = 1;
    docs.iter()
        .map(|&doc| (doc, bm25f(index, doc, &tf, &idfs)))
        .collect()
}

// ── Presentation ─────────────────────────────────────────────────────

/// One highlight rule per positive text clause.
enum Highlighter {
    /// Whole word tokens whose folded text is in the set.
    Tokens(HashSet<String>),
    /// The `term` portion of word tokens containing it.
    Within(String),
    /// `[[…]]` spans with this folded target.
    Link(String),
    /// Occurrences of the term in the folded paragraph text.
    Text(String),
}

/// Fill `highlights` on each result (call after any limit is applied:
/// highlighting re-tokenizes the paragraph). Results whose paragraph is
/// not in the index keep empty highlights.
pub fn fill_highlights(index: &SearchIndex, query: &SearchQuery, results: &mut [SearchResult]) {
    let rules: Vec<Highlighter> = query
        .positives()
        .map(|c| match c.mode {
            Mode::Word => Highlighter::Tokens(
                word_tokens(index, c)
                    .into_iter()
                    .map(|id| index.tokens[id as usize].clone())
                    .collect(),
            ),
            Mode::Fuzzy => Highlighter::Tokens(
                fuzzy_tokens(index, &c.term)
                    .into_iter()
                    .map(|id| index.tokens[id as usize].clone())
                    .collect(),
            ),
            Mode::Substring if text::is_single_run(&c.term) => Highlighter::Within(c.term.clone()),
            Mode::Link => Highlighter::Link(c.term.clone()),
            _ => Highlighter::Text(c.term.clone()),
        })
        .collect();
    for r in results.iter_mut() {
        if index.doc_at(&r.path, r.line_start).is_none() {
            continue;
        }
        r.highlights = highlight_ranges(&r.body, &rules);
    }
}

fn highlight_ranges(body: &str, rules: &[Highlighter]) -> Vec<Range<usize>> {
    let tokens = text::tokenize(body);
    let mut folded_body: Option<text::Folded> = None;
    let mut out: Vec<Range<usize>> = Vec::new();
    for rule in rules {
        match rule {
            Highlighter::Tokens(set) => out.extend(
                tokens
                    .iter()
                    .filter(|t| t.kind == TokenKind::Word && set.contains(&t.text))
                    .map(|t| t.range.clone()),
            ),
            Highlighter::Within(term) => {
                for t in tokens
                    .iter()
                    .filter(|t| t.kind == TokenKind::Word && t.text.contains(term.as_str()))
                {
                    let surface = &body[t.range.clone()];
                    let f = text::fold_with_offsets(surface);
                    for (at, m) in f.text.match_indices(term.as_str()) {
                        let r = f.to_original(at..at + m.len(), surface);
                        out.push(t.range.start + r.start..t.range.start + r.end);
                    }
                }
            }
            Highlighter::Link(target) => out.extend(
                tokens
                    .iter()
                    .filter(|t| t.kind == TokenKind::Link && &t.text == target)
                    .map(|t| t.range.clone()),
            ),
            Highlighter::Text(term) => {
                if term.is_empty() {
                    continue;
                }
                let f = folded_body.get_or_insert_with(|| text::fold_with_offsets(body));
                for (at, m) in f.text.match_indices(term.as_str()) {
                    out.push(f.to_original(at..at + m.len(), body));
                }
            }
        }
    }
    out.retain(|r| r.start < r.end);
    out.sort_by_key(|r| (r.start, r.end));
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(out.len());
    for r in out {
        match merged.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
}

/// Results of one note, in result order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteGroup {
    pub path: PathBuf,
    pub title: String,
    /// Indices into the result slice.
    pub members: Vec<usize>,
}

/// Group results by note: groups in order of their best (first) member,
/// members in result order. CLI and TUI share this ordering rule.
pub fn group(results: &[SearchResult]) -> Vec<NoteGroup> {
    let mut out: Vec<NoteGroup> = Vec::new();
    let mut at: HashMap<&PathBuf, usize> = HashMap::new();
    for (i, r) in results.iter().enumerate() {
        match at.get(&r.path) {
            Some(&g) => out[g].members.push(i),
            None => {
                at.insert(&r.path, out.len());
                out.push(NoteGroup {
                    path: r.path.clone(),
                    title: r.title.clone(),
                    members: vec![i],
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SearchCfg;
    use crate::search::query::parse;
    use std::path::Path;

    fn scan_of(files: &[(&str, &str)]) -> (assert_fs::TempDir, crate::scan::Scan) {
        let tmp = assert_fs::TempDir::new().unwrap();
        for (name, content) in files {
            let path = tmp.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
        let scan = crate::scan::scan_vault(tmp.path(), &[]);
        (tmp, scan)
    }

    fn idx(files: &[(&str, &str)]) -> SearchIndex {
        let (_tmp, scan) = scan_of(files);
        SearchIndex::build(&scan, &[])
    }

    fn idx_cfg(files: &[(&str, &str)], stemmers: &[&str]) -> SearchIndex {
        let (_tmp, scan) = scan_of(files);
        let cfg = SearchCfg {
            stemmers: stemmers.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        SearchIndex::build_with(&scan, &[], &cfg)
    }

    fn with_graph(files: &[(&str, &str)]) -> (SearchIndex, Graph) {
        let (_tmp, scan) = scan_of(files);
        let graph = Graph::build(&scan).unwrap();
        let index = SearchIndex::build(&scan, &[]);
        (index, graph)
    }

    fn run_q(index: &SearchIndex, q: &str) -> Vec<SearchResult> {
        search(index, &parse(q, false), SearchCtx::default()).unwrap()
    }

    fn run_any(index: &SearchIndex, q: &str) -> Vec<SearchResult> {
        search(index, &parse(q, true), SearchCtx::default()).unwrap()
    }

    fn bodies(results: &[SearchResult]) -> Vec<&str> {
        results.iter().map(|r| r.body.as_str()).collect()
    }

    fn paths(results: &[SearchResult]) -> Vec<String> {
        results
            .iter()
            .map(|r| r.path.to_string_lossy().into_owned())
            .collect()
    }

    // ── Matching semantics ───────────────────────────────────────────

    #[test]
    fn substring_matches_fragments() {
        let index = idx(&[
            ("a.md", "eigen alone.\n"),
            ("b.md", "an eigenvalue here.\n"),
            ("c.md", "eigen-decomposition there.\n"),
            ("d.md", "nothing relevant.\n"),
        ]);
        assert_eq!(run_q(&index, "eigen").len(), 3);
        assert!(run_q(&index, "zzz").is_empty());
        // A separator-bearing term scans text.
        assert_eq!(
            bodies(&run_q(&index, "eigen-dec")),
            vec!["eigen-decomposition there."]
        );
    }

    #[test]
    fn word_mode_requires_whole_token() {
        let index = idx(&[
            ("a.md", "eigen alone.\n"),
            ("b.md", "an eigenvalue here.\n"),
        ]);
        let results = run_q(&index, "=eigen");
        assert_eq!(bodies(&results), vec!["eigen alone."]);
    }

    #[test]
    fn word_mode_matches_link_target() {
        let index = idx(&[("a.md", "See [[eigen]].\n")]);
        assert_eq!(run_q(&index, "=eigen").len(), 1);
    }

    #[test]
    fn word_matches_accented_text_both_ways() {
        let index = idx(&[("a.md", "O ônibus chegou.\n")]);
        assert_eq!(run_q(&index, "=ônibus").len(), 1);
        assert_eq!(run_q(&index, "=onibus").len(), 1);
        assert_eq!(run_q(&index, "onibus").len(), 1);
        assert!(run_q(&index, "=nibus").is_empty());
    }

    #[test]
    fn word_matches_stem_variants() {
        let index = idx(&[("a.md", "Garden memories.\n")]);
        assert_eq!(run_q(&index, "=memory").len(), 1);
        let off = idx_cfg(&[("a.md", "Garden memories.\n")], &[]);
        assert!(run_q(&off, "=memory").is_empty());
    }

    #[test]
    fn multiple_stemmers() {
        let index = idx_cfg(
            &[("a.md", "Only memories.\n"), ("b.md", "Muitas casas.\n")],
            &["english", "portuguese"],
        );
        assert_eq!(paths(&run_q(&index, "=memory")), vec!["a.md"]);
        assert_eq!(paths(&run_q(&index, "=casa")), vec!["b.md"]);
    }

    #[test]
    fn link_clause_restricts_to_link_targets() {
        let index = idx(&[
            ("a.md", "See [[Memoization]].\n"),
            ("b.md", "prose memoization only.\n"),
            ("Memoization.md", "The note itself, no self link.\n"),
        ]);
        assert_eq!(paths(&run_q(&index, "[[memoization]]")), vec!["a.md"]);
    }

    #[test]
    fn fuzzy_tolerates_typos() {
        let index = idx(&[("a.md", "memoization helps.\n")]);
        assert_eq!(run_q(&index, "~memoizaton").len(), 1);
        assert!(run_q(&index, "~zzzzzzzzz").is_empty());
    }

    #[test]
    fn fuzzy_short_term_prefix_scan() {
        let index = idx(&[("a.md", "foo bar.\n")]);
        assert_eq!(run_q(&index, "~fo").len(), 1, "fo → foo via prefix scan");
    }

    #[test]
    fn fuzzy_matches_link_target_words() {
        let index = idx(&[("a.md", "See [[memoization]].\n")]);
        assert_eq!(run_q(&index, "~memoizaton").len(), 1);
    }

    #[test]
    fn fuzzy_over_non_ascii_term_does_not_panic() {
        let index = idx(&[("a.md", "See [[móveis]] and [[problems with móveis ii]].\n")]);
        assert_eq!(run_q(&index, "~móveis").len(), 1);
        assert!(run_q(&index, "~óé").is_empty());
    }

    #[test]
    fn phrase_requires_contiguous() {
        let index = idx(&[("a.md", "shared memory machine.\n")]);
        assert_eq!(run_q(&index, "\"shared memory\"").len(), 1);
        assert!(run_q(&index, "\"memory shared\"").is_empty());
    }

    #[test]
    fn and_requires_all_terms() {
        let index = idx(&[("a.md", "eigen only.\n\nmemoization only.\n")]);
        assert!(run_q(&index, "eigen memoization").is_empty());
    }

    #[test]
    fn any_mode_unions() {
        let index = idx(&[("a.md", "eigen only.\n\nmemoization only.\n")]);
        assert_eq!(run_any(&index, "eigen memoization").len(), 2);
    }

    #[test]
    fn exclude_filters_after_matching_on_body_only() {
        let index = idx(&[
            ("a.md", "eigen and task.\n"),
            ("b.md", "eigen alone.\n"),
            ("Task Notes.md", "eigen in a task-titled note.\n"),
        ]);
        let results = run_q(&index, "eigen -task");
        assert_eq!(paths(&results), vec!["b.md"]);
        let index = idx(&[("Task Notes.md", "eigen without the word.\n")]);
        assert_eq!(
            run_q(&index, "eigen -task").len(),
            1,
            "title is not excluded"
        );
    }

    #[test]
    fn exclude_link_clause() {
        let index = idx(&[("a.md", "eigen [[Bar]].\n"), ("b.md", "eigen alone.\n")]);
        assert_eq!(paths(&run_q(&index, "eigen -[[Bar]]")), vec!["b.md"]);
    }

    #[test]
    fn empty_query_and_no_match() {
        let index = idx(&[("a.md", "text.\n")]);
        assert!(run_q(&index, "").is_empty());
        assert!(run_q(&index, "absent").is_empty());
    }

    #[test]
    fn wikilink_with_spaces_matches() {
        let index = idx(&[
            ("a.md", "See [[Bar Foo]] here.\n"),
            ("b.md", "bar and foo.\n"),
        ]);
        assert_eq!(paths(&run_q(&index, "[[Bar Foo]]")), vec!["a.md"]);
    }

    #[test]
    fn context_fields_qualify_a_paragraph() {
        let index = idx(&[
            (
                "Memoization.md",
                "# Memoization\n\nCache eigen decompositions here.\n",
            ),
            ("other.md", "eigen elsewhere.\n"),
        ]);
        let results = run_q(&index, "eigen memoization");
        assert_eq!(bodies(&results), vec!["Cache eigen decompositions here."]);
        let r = &results[0];
        assert!(r
            .reasons
            .iter()
            .any(|m| m.clause == "memoization" && m.field == Field::Title));
        assert!(r
            .reasons
            .iter()
            .any(|m| m.clause == "eigen" && m.field == Field::Body));
    }

    #[test]
    fn alias_heading_and_tag_fields_match() {
        let index = idx(&[(
            "Deep Work.md",
            "---\naliases: [focus routine]\ntags: [habits]\n---\n\n## Rollback\n\nTwo hours every morning.\n",
        )]);
        assert_eq!(run_q(&index, "focus routine").len(), 2);
        let r = run_q(&index, "rollback morning");
        assert_eq!(bodies(&r), vec!["Two hours every morning."]);
        assert!(r[0].reasons.iter().any(|m| m.field == Field::Heading));
        assert_eq!(r[0].heading_path, vec!["Rollback"]);
        assert_eq!(run_q(&index, "habits morning").len(), 1);
    }

    // ── Ranking ──────────────────────────────────────────────────────

    #[test]
    fn more_clauses_rank_higher() {
        let index = idx(&[
            ("a.md", "eigen and memoization.\n"),
            ("b.md", "eigen only here.\n"),
        ]);
        let results = run_any(&index, "eigen memoization");
        assert_eq!(results[0].body, "eigen and memoization.");
    }

    #[test]
    fn focused_paragraph_beats_long_list() {
        let list = (0..30)
            .map(|i| {
                if i % 10 == 0 {
                    format!("- item {i} about memory and other things")
                } else {
                    format!("- item {i} about routes, stops, and planners")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let index = idx(&[
            ("list.md", &format!("{list}\n")),
            ("focus.md", "Shared memory is fast.\n"),
        ]);
        assert_eq!(paths(&run_q(&index, "memory"))[0], "focus.md");
    }

    #[test]
    fn rare_terms_outweigh_common() {
        let mut files: Vec<(String, String)> = (0..10)
            .map(|i| (format!("n{i}.md"), format!("the common word number {i}.\n")))
            .collect();
        files.push((
            "rare.md".into(),
            "eigendecomposition factors matrices.\n".into(),
        ));
        files.push(("both.md".into(), "only the article here.\n".into()));
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let index = idx(&refs);
        assert_eq!(
            paths(&run_any(&index, "the eigendecomposition"))[0],
            "rare.md"
        );
    }

    #[test]
    fn title_match_outranks_body_only_match() {
        let index = idx(&[
            ("Eigen.md", "eigen once here.\n"),
            ("Other.md", "eigen once here.\n"),
        ]);
        assert_eq!(paths(&run_q(&index, "eigen"))[0], "Eigen.md");
    }

    #[test]
    fn title_only_matches_do_not_flood() {
        let scratch: String = (0..10)
            .map(|i| format!("Scratch thought number {i} about groceries.\n\n"))
            .collect();
        let index = idx(&[
            ("Short-term-memory.md", &scratch),
            (
                "smp.md",
                "Processors share one main memory with uniform access cost.\n",
            ),
        ]);
        let got = paths(&run_q(&index, "memory"));
        assert_eq!(got.len(), 11);
        assert_eq!(got[0], "smp.md");
    }

    #[test]
    fn deterministic_tiebreak_path_then_line() {
        let index = idx(&[("b.md", "foo.\n"), ("a.md", "foo.\n")]);
        let results = run_q(&index, "foo");
        assert_eq!(paths(&results), vec!["a.md", "b.md"]);
    }

    // ── Filters ──────────────────────────────────────────────────────

    #[test]
    fn path_filter_and_negation() {
        let index = idx(&[
            ("proj/a.md", "cache here.\n"),
            ("archive/b.md", "cache there.\n"),
            ("c.md", "cache too.\n"),
        ]);
        assert_eq!(paths(&run_q(&index, "cache path:proj/")), vec!["proj/a.md"]);
        let neg = paths(&run_q(&index, "cache -path:archive/"));
        assert!(!neg.iter().any(|p| p.starts_with("archive/")));
        assert_eq!(neg.len(), 2);
    }

    #[test]
    fn hierarchical_tag_filter() {
        let index = idx(&[
            ("a.md", "---\ntags: [proj/busao]\n---\n\nRoute one.\n"),
            ("b.md", "---\ntags: [proj/fluxo]\n---\n\nRoute two.\n"),
            ("c.md", "---\ntags: [project]\n---\n\nRoute three.\n"),
            ("d.md", "Route four #idea inline.\n"),
        ]);
        let mut got = paths(&run_q(&index, "route tag:proj"));
        got.sort();
        assert_eq!(got, vec!["a.md", "b.md"]);
        assert_eq!(paths(&run_q(&index, "route tag:#idea")), vec!["d.md"]);
    }

    #[test]
    fn filter_only_query_returns_every_passing_paragraph() {
        let index = idx(&[
            ("proj/a.md", "First #idea.\n\nSecond.\n"),
            ("proj/b.md", "Third #idea.\n"),
            ("c.md", "Fourth #idea.\n"),
        ]);
        let got = paths(&run_q(&index, "path:proj/ tag:idea"));
        assert_eq!(got, vec!["proj/a.md", "proj/b.md"]);
    }

    #[test]
    fn near_only_query_is_empty() {
        let index = idx(&[("a.md", "text.\n")]);
        assert!(run_q(&index, "near:[[a]]").is_empty());
    }

    #[test]
    fn date_filters_need_dates() {
        let index = idx(&[("a.md", "text.\n")]);
        let err = search(
            &index,
            &parse("text after:2026-01-01", false),
            SearchCtx::default(),
        );
        assert!(matches!(err, Err(SearchError::NeedsDates)));
    }

    #[test]
    fn scope_via_graph_preset() {
        let (index, graph) = with_graph(&[
            ("proj/a.md", "cache here.\n"),
            ("know/b.md", "cache there.\n"),
        ]);
        let presets: HashMap<String, String> = [(
            "work".to_string(),
            r#"node where path starts_with "proj/";"#.to_string(),
        )]
        .into();
        let ctx = SearchCtx {
            graph: Some(&graph),
            presets: Some(&presets),
            ..Default::default()
        };
        let got = search(&index, &parse("cache scope:work", false), ctx).unwrap();
        assert_eq!(paths(&got), vec!["proj/a.md"]);
    }

    #[test]
    fn unknown_scope_lists_available() {
        let (index, graph) = with_graph(&[("a.md", "cache.\n")]);
        let ctx = SearchCtx {
            graph: Some(&graph),
            presets: None,
            ..Default::default()
        };
        match search(&index, &parse("cache scope:nope", false), ctx) {
            Err(SearchError::UnknownScope { name, available }) => {
                assert_eq!(name, "nope");
                assert!(available.contains(&"links".to_string()));
            }
            other => panic!("expected UnknownScope, got {other:?}"),
        }
    }

    #[test]
    fn scope_without_graph_errors() {
        let index = idx(&[("a.md", "cache.\n")]);
        let err = search(
            &index,
            &parse("cache scope:links", false),
            SearchCtx::default(),
        );
        assert!(matches!(err, Err(SearchError::NoGraph)));
    }

    /// A git vault where `old.md` was committed on 2026-01-10 and
    /// `new.md` on 2026-10-20, plus an untracked `loose.md`.
    fn dated_vault() -> (assert_fs::TempDir, Vault, SearchIndex) {
        use std::process::Command;
        let tmp = assert_fs::TempDir::new().unwrap();
        let git = |args: &[&str], date: Option<&str>| {
            let mut c = Command::new("git");
            c.current_dir(tmp.path()).args(args);
            if let Some(d) = date {
                c.env("GIT_AUTHOR_DATE", d).env("GIT_COMMITTER_DATE", d);
            }
            let out = c.output().expect("git on PATH");
            assert!(out.status.success(), "git {args:?}: {out:?}");
        };
        git(&["init", "-q", "-b", "main"], None);
        git(&["config", "user.name", "Test"], None);
        git(&["config", "user.email", "test@example.com"], None);
        git(&["config", "commit.gpgsign", "false"], None);
        std::fs::write(tmp.path().join("old.md"), "cache from january.\n").unwrap();
        git(&["add", "old.md"], None);
        git(
            &["commit", "-q", "-m", "old"],
            Some("2026-01-10T12:00:00+00:00"),
        );
        std::fs::write(tmp.path().join("new.md"), "cache from october.\n").unwrap();
        git(&["add", "new.md"], None);
        git(
            &["commit", "-q", "-m", "new"],
            Some("2026-10-20T12:00:00+00:00"),
        );
        std::fs::write(tmp.path().join("loose.md"), "cache never committed.\n").unwrap();
        let scan = crate::scan::scan_vault(tmp.path(), &[]);
        let index = SearchIndex::build(&scan, &[]);
        let vault = Vault {
            path: tmp.path().to_path_buf(),
            config: crate::config::LayeredConfig {
                config: crate::config::Config::default(),
                sources: Vec::new(),
            },
        };
        (tmp, vault, index)
    }

    fn dated(index: &SearchIndex, vault: &Vault, q: &str, sort: Sort) -> DatedSearch {
        let mut cache = BlameCache::default();
        let validity = BlameValidity::snapshot(&vault.path).unwrap();
        search_with_dates(
            index,
            &parse(q, false),
            SearchCtx::default(),
            vault,
            validity.as_ref(),
            &mut cache,
            sort,
        )
        .unwrap()
    }

    #[test]
    fn date_filters_use_blame_dates() {
        let (_tmp, vault, index) = dated_vault();
        let got = dated(&index, &vault, "cache after:2026-10-01", Sort::Relevance);
        assert_eq!(paths(&got.results), vec!["new.md"]);
        assert_eq!(got.undated_dropped, 1, "loose.md has no blame date");
        assert_eq!(got.results[0].date, NaiveDate::from_ymd_opt(2026, 10, 20));
        let got = dated(&index, &vault, "cache before:2026-02-01", Sort::Relevance);
        assert_eq!(paths(&got.results), vec!["old.md"]);
        let got = dated(&index, &vault, "cache -after:2026-10-01", Sort::Relevance);
        assert_eq!(
            paths(&got.results),
            vec!["old.md"],
            "negated keeps older, drops undated"
        );
    }

    #[test]
    fn date_sort_orders_newest_first_undated_last() {
        let (_tmp, vault, index) = dated_vault();
        let got = dated(&index, &vault, "cache", Sort::Date);
        assert_eq!(paths(&got.results), vec!["new.md", "old.md", "loose.md"]);
        assert_eq!(got.undated_dropped, 0);
    }

    #[test]
    fn date_filter_without_git_drops_everything() {
        let index = idx(&[("a.md", "cache here.\n")]);
        let tmp = assert_fs::TempDir::new().unwrap();
        let vault = Vault {
            path: tmp.path().to_path_buf(),
            config: crate::config::LayeredConfig {
                config: crate::config::Config::default(),
                sources: Vec::new(),
            },
        };
        let got = dated(&index, &vault, "cache after:2026-01-01", Sort::Relevance);
        assert!(got.results.is_empty());
        assert_eq!(got.undated_dropped, 1);
    }

    /// One file whose first paragraph was committed in 2023 and whose
    /// second (equally matching) paragraph was rewritten in 2026.
    fn two_era_vault() -> (assert_fs::TempDir, SearchIndex, BlameValidity) {
        use std::process::Command;
        let tmp = assert_fs::TempDir::new().unwrap();
        let git = |args: &[&str], date: Option<&str>| {
            let mut c = Command::new("git");
            c.current_dir(tmp.path()).args(args);
            if let Some(d) = date {
                c.env("GIT_AUTHOR_DATE", d).env("GIT_COMMITTER_DATE", d);
            }
            let out = c.output().expect("git on PATH");
            assert!(out.status.success(), "git {args:?}: {out:?}");
        };
        git(&["init", "-q", "-b", "main"], None);
        git(&["config", "user.name", "Test"], None);
        git(&["config", "user.email", "test@example.com"], None);
        git(&["config", "commit.gpgsign", "false"], None);
        let f = tmp.path().join("note.md");
        std::fs::write(&f, "cache alpha line.\n\nunrelated words here.\n").unwrap();
        git(&["add", "."], None);
        git(
            &["commit", "-q", "-m", "old"],
            Some("2023-01-10T12:00:00+00:00"),
        );
        std::fs::write(&f, "cache alpha line.\n\ncache bravo line.\n").unwrap();
        git(
            &["commit", "-q", "-am", "new"],
            Some("2026-10-20T12:00:00+00:00"),
        );
        let scan = crate::scan::scan_vault(tmp.path(), &[]);
        let index = SearchIndex::build(&scan, &[]);
        let validity = BlameValidity::snapshot(tmp.path()).unwrap().unwrap();
        (tmp, index, validity)
    }

    #[test]
    fn fresh_paragraph_in_an_old_note_ranks_first() {
        let (_tmp, index, validity) = two_era_vault();
        let q = parse("cache", false);
        // Without blame both paragraphs share the file's mtime: a tie.
        let plain = search(&index, &q, SearchCtx::default()).unwrap();
        assert_eq!(plain[0].line_start, 1, "tie breaks on line");
        assert_eq!(plain[0].score, plain[1].score);

        let mut cache = BlameCache::default();
        cache.ensure(Path::new("note.md"), 3, &validity);
        let table = RecencyTable::build(&index, &cache, &validity);
        assert_eq!(table.covered(), 2);
        let ctx = SearchCtx {
            recency: Some(&table),
            ..Default::default()
        };
        let got = search(&index, &q, ctx).unwrap();
        assert_eq!(got[0].line_start, 3, "the 2026 paragraph wins");
        assert!(
            got[0].score / got[1].score <= 1.10 + 1e-9,
            "recency share is bounded"
        );
    }

    #[test]
    fn uncached_file_falls_back_to_mtime() {
        let (_tmp, index, validity) = two_era_vault();
        let q = parse("cache", false);
        let empty = RecencyTable::build(&index, &BlameCache::default(), &validity);
        assert_eq!(empty.covered(), 0);
        let with_empty = search(
            &index,
            &q,
            SearchCtx {
                recency: Some(&empty),
                ..Default::default()
            },
        )
        .unwrap();
        let without = search(&index, &q, SearchCtx::default()).unwrap();
        for (a, b) in with_empty.iter().zip(&without) {
            assert_eq!((a.line_start, a.score), (b.line_start, b.score));
        }
    }

    #[test]
    fn stale_table_is_ignored() {
        let (_tmp, index, validity) = two_era_vault();
        let mut cache = BlameCache::default();
        cache.ensure(Path::new("note.md"), 3, &validity);
        let table = RecencyTable::build(&index, &cache, &validity);
        let other = idx(&[("x.md", "cache one.\n\ncache two.\n\ncache three.\n")]);
        let q = parse("cache", false);
        let with = search(
            &other,
            &q,
            SearchCtx {
                recency: Some(&table),
                ..Default::default()
            },
        )
        .unwrap();
        let without = search(&other, &q, SearchCtx::default()).unwrap();
        assert_eq!(paths(&with), paths(&without));
        assert_eq!(with[0].score, without[0].score);
    }

    // ── Hybrid (semantic) retrieval ──────────────────────────────────

    /// Maps words to concept axes so synonyms embed alike: a
    /// deterministic stand-in for a real model in fusion tests.
    struct ConceptEmbedder;

    impl crate::search::semantic::Embedder for ConceptEmbedder {
        fn id(&self) -> &str {
            "test/concepts"
        }
        fn dim(&self) -> usize {
            4
        }
        fn embed(&self, texts: &[String]) -> Vec<Vec<f32>> {
            const AXES: [&[&str]; 4] = [
                &[
                    "focus",
                    "concentrating",
                    "interruptions",
                    "notifications",
                    "attention",
                ],
                &[
                    "memoization",
                    "remembering",
                    "cache",
                    "computations",
                    "results",
                ],
                &["garden", "tomatoes", "homegrown", "produce"],
                &["the", "a", "and", "of", "here", "note"],
            ];
            texts
                .iter()
                .map(|t| {
                    let mut v = vec![0.0f32; 4];
                    for w in crate::search::text::word_texts(t) {
                        for (i, axis) in AXES.iter().enumerate() {
                            if axis.contains(&w.as_str()) {
                                v[i] += 1.0;
                            }
                        }
                    }
                    v
                })
                .collect()
        }
    }

    fn semantic_of(index: &SearchIndex) -> crate::search::semantic::SemanticIndex {
        let e = ConceptEmbedder;
        let mut cache = crate::search::semantic::cache::EmbeddingCache::new("test/concepts", 4);
        crate::search::semantic::SemanticIndex::build(index, &e, &mut cache).0
    }

    fn hybrid(
        index: &SearchIndex,
        sem: &crate::search::semantic::SemanticIndex,
        q: &str,
    ) -> Vec<SearchResult> {
        let ctx = SearchCtx {
            semantic: Some(SemanticQuery {
                index: sem,
                embedder: &ConceptEmbedder,
                candidates: 10,
                min_similarity: 0.5,
            }),
            ..Default::default()
        };
        search(index, &parse(q, false), ctx).unwrap()
    }

    fn concept_vault() -> SearchIndex {
        idx(&[
            (
                "Deep Work.md",
                "Notifications off, attention on one goal.\n",
            ),
            ("Garden.md", "Tomatoes everywhere this summer.\n"),
            ("archive/Old.md", "Attention notes, archived.\n"),
            ("Memo.md", "Memoization keeps results of pure calls.\n"),
        ])
    }

    #[test]
    fn concept_query_finds_paragraph_without_shared_words() {
        let index = concept_vault();
        let sem = semantic_of(&index);
        assert!(
            run_q(&index, "concentrating interruptions").is_empty(),
            "lexically nothing"
        );
        let got = hybrid(&index, &sem, "concentrating interruptions");
        assert_eq!(got[0].path, PathBuf::from("Deep Work.md"));
        assert!(got[0].matched.is_empty());
        assert_eq!(
            got[0].reasons,
            vec![MatchReason {
                clause: SEMANTIC_LABEL.to_string(),
                field: Field::Semantic
            }]
        );
        assert!(
            !got.iter().any(|r| r.path == *"Garden.md"),
            "below the floor"
        );
    }

    #[test]
    fn filters_and_excludes_bound_semantic_candidates() {
        let index = concept_vault();
        let sem = semantic_of(&index);
        let got = hybrid(&index, &sem, "concentrating -path:archive/");
        assert!(!got.iter().any(|r| r.path.starts_with("archive")));
        let got = hybrid(&index, &sem, "concentrating path:archive/");
        assert_eq!(paths(&got), vec!["archive/Old.md"]);
        let got = hybrid(&index, &sem, "concentrating -attention");
        assert!(
            got.is_empty(),
            "the exclude applies to semantic hits too: {:?}",
            paths(&got)
        );
    }

    #[test]
    fn agreement_between_rankings_ranks_first() {
        // `results` matches Garden best lexically (short paragraph), but its
        // meaning is gardening; Memo is 2nd lexically and 1st by meaning.
        let index = idx(&[
            (
                "Memo.md",
                "Memoization keeps results of pure calls in a cache.\n",
            ),
            ("Garden.md", "results: tomatoes, homegrown produce.\n"),
            (
                "Note.md",
                "results and more results in a longer paragraph with many words.\n",
            ),
        ]);
        let sem = semantic_of(&index);
        let lexical = paths(&run_q(&index, "results"));
        assert_eq!(lexical[0], "Garden.md");
        let got = paths(&hybrid(&index, &sem, "results"));
        assert_eq!(got[0], "Memo.md", "lexical {lexical:?} → hybrid {got:?}");
    }

    #[test]
    fn stale_semantic_index_is_ignored() {
        let index = concept_vault();
        let other = idx(&[("x.md", "attention.\n")]);
        let sem = semantic_of(&other);
        assert_eq!(
            paths(&hybrid(&index, &sem, "attention")),
            paths(&run_q(&index, "attention")),
            "a semantic index for another doc set is lexical-only"
        );
    }

    #[test]
    fn more_like_this() {
        let index = concept_vault();
        let sem = semantic_of(&index);
        let ctx = SearchCtx {
            semantic: Some(SemanticQuery {
                index: &sem,
                embedder: &ConceptEmbedder,
                candidates: 10,
                min_similarity: 0.5,
            }),
            like: Some((Path::new("Deep Work.md"), 1)),
            ..Default::default()
        };
        let got = search(&index, &parse("", false), ctx).unwrap();
        assert_eq!(
            paths(&got),
            vec!["archive/Old.md"],
            "similar, source excluded"
        );
        let got = search(&index, &parse("-path:archive/", false), ctx).unwrap();
        assert!(got.is_empty());

        let no_sem = SearchCtx {
            like: Some((Path::new("Deep Work.md"), 1)),
            ..Default::default()
        };
        assert!(matches!(
            search(&index, &parse("", false), no_sem),
            Err(SearchError::NeedsEmbeddings)
        ));
        let missing = SearchCtx {
            like: Some((Path::new("Deep Work.md"), 99)),
            ..ctx
        };
        assert!(matches!(
            search(&index, &parse("", false), missing),
            Err(SearchError::NoSuchParagraph { .. })
        ));
    }

    #[test]
    fn semantic_text_strips_markup() {
        let q = parse(
            "=word ~fuzz \"two words\" [[Link Target]] plain -gone",
            false,
        );
        let pos: Vec<&Clause> = q.positives().collect();
        assert_eq!(semantic_text(&pos), "word fuzz two words Link Target plain");
    }

    // ── Priors and near ──────────────────────────────────────────────

    #[test]
    fn hub_note_ranks_above_orphan_on_a_tie() {
        let mut files = vec![
            ("hub.md".to_string(), "cache tie text.\n".to_string()),
            ("orphan.md".to_string(), "cache tie text.\n".to_string()),
        ];
        for i in 0..20 {
            files.push((format!("l{i}.md"), "links [[hub]].\n".to_string()));
        }
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let (index, _g) = with_graph(&refs);
        // Same mtime for every file in the temp vault, so only in-degree differs.
        let got = paths(&run_q(&index, "tie"));
        assert_eq!(got, vec!["hub.md", "orphan.md"]);
    }

    #[test]
    fn prior_is_bounded() {
        let mut files = vec![
            ("strong.md".to_string(), "cache cache cache.\n".to_string()),
            (
                "weak.md".to_string(),
                "one cache mention in a much longer paragraph full of other words.\n".to_string(),
            ),
        ];
        for i in 0..20 {
            files.push((format!("l{i}.md"), "links [[weak]].\n".to_string()));
        }
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let (index, _g) = with_graph(&refs);
        let got = run_q(&index, "cache");
        let strong = got.iter().find(|r| r.path == *"strong.md").unwrap();
        let weak = got.iter().find(|r| r.path == *"weak.md").unwrap();
        assert!(weak.score / 1.25 < 0.8 * strong.score / 1.0 + 1e-9 || strong.score > weak.score);
        assert_eq!(got[0].path, PathBuf::from("strong.md"));
    }

    #[test]
    fn near_boost_reorders_without_filtering() {
        let (index, _g) = with_graph(&[
            ("Caching.md", "cache basics [[Linked]].\n"),
            ("Linked.md", "cache details.\n"),
            ("Unrelated.md", "cache cache elsewhere.\n"),
        ]);
        let score = |results: &[SearchResult], p: &str| {
            results.iter().find(|r| r.path == *p).unwrap().score
        };
        let plain = run_q(&index, "cache");
        let near = run_q(&index, "cache near:[[Caching]]");
        assert_eq!(near.len(), 3, "near never filters");
        let ratio = |p| score(&near, p) / score(&plain, p);
        assert!((ratio("Caching.md") - NEAR_BOOST[0]).abs() < 1e-9);
        assert!((ratio("Linked.md") - NEAR_BOOST[1]).abs() < 1e-9);
        assert!((ratio("Unrelated.md") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn unresolved_near_seed_changes_nothing() {
        let (index, _g) = with_graph(&[("a.md", "cache one.\n"), ("b.md", "cache two two.\n")]);
        let plain = run_q(&index, "cache");
        let near = run_q(&index, "cache near:[[Does Not Exist]]");
        assert_eq!(paths(&plain), paths(&near));
        for (a, b) in plain.iter().zip(&near) {
            assert_eq!(a.score, b.score);
        }
    }

    // ── Presentation ─────────────────────────────────────────────────

    fn highlighted(index: &SearchIndex, q: &str) -> Vec<Vec<String>> {
        let query = parse(q, false);
        let mut results = search(index, &query, SearchCtx::default()).unwrap();
        fill_highlights(index, &query, &mut results);
        results
            .iter()
            .map(|r| {
                r.highlights
                    .iter()
                    .map(|h| r.body[h.clone()].to_string())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn highlights_point_into_the_body() {
        let index = idx(&[("a.md", "O Ônibus chegou.\n")]);
        assert_eq!(highlighted(&index, "ônibus"), vec![vec!["Ônibus"]]);
        assert_eq!(highlighted(&index, "=onibus"), vec![vec!["Ônibus"]]);
    }

    #[test]
    fn substring_highlights_the_fragment() {
        let index = idx(&[("a.md", "An eigenvalue and eigen.\n")]);
        assert_eq!(highlighted(&index, "eigen"), vec![vec!["eigen", "eigen"]]);
    }

    #[test]
    fn phrase_fuzzy_and_link_highlights() {
        let index = idx(&[(
            "a.md",
            "Shared  memory? No: shared memory, see [[Memoization]].\n",
        )]);
        assert_eq!(
            highlighted(&index, "\"shared memory\""),
            vec![vec!["shared memory"]]
        );
        assert_eq!(
            highlighted(&index, "~memoizaton"),
            vec![vec!["Memoization"]]
        );
        assert_eq!(
            highlighted(&index, "[[memoization]]"),
            vec![vec!["[[Memoization]]"]]
        );
    }

    #[test]
    fn title_only_match_has_no_body_highlight() {
        let index = idx(&[("Eigen.md", "nothing about it in prose.\n")]);
        assert_eq!(highlighted(&index, "eigen"), vec![Vec::<String>::new()]);
    }

    #[test]
    fn group_orders_by_best_member() {
        let r = |p: &str, l| SearchResult {
            path: PathBuf::from(p),
            line_start: l,
            title: p.trim_end_matches(".md").to_string(),
            ..Default::default()
        };
        let results = vec![r("b.md", 1), r("a.md", 3), r("b.md", 5), r("a.md", 1)];
        let groups = group(&results);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].path, PathBuf::from("b.md"));
        assert_eq!(groups[0].members, vec![0, 2]);
        assert_eq!(groups[1].members, vec![1, 3]);
    }

    // ── perf budget (gated; run with FT_PERF_TESTS=1) ─────────────────

    fn perf_enabled() -> bool {
        std::env::var("FT_PERF_TESTS").as_deref() == Ok("1")
    }

    /// Build a synthetic 5,000-paragraph index in memory (no filesystem
    /// writes): 500 files × 10 paragraphs, with a spread of topics so
    /// substring and fuzzy queries have real work to do; files link to
    /// their neighbours so `near:` has a graph to walk.
    fn synthetic_5k() -> (SearchIndex, Graph) {
        use crate::markdown::{Heading, Paragraph};
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
                    "Paragraph {p} of file {f}: about {topic}, with extra words to pad the text, see [[file-{:04}]]",
                    (f + 1) % 500
                );
                paragraphs.push(Paragraph {
                    line_start: p * 2 + 3,
                    line_end: p * 2 + 3,
                    text,
                });
            }
            let content: String = paragraphs
                .iter()
                .map(|p| format!("{}\n\n", p.text))
                .collect();
            files.push(ParsedFile {
                rel: PathBuf::from(format!("notes/file-{f:04}.md")),
                links: crate::graph::parser::extract_links_and_self_block_refs(&content).0,
                self_block_refs: Vec::new(),
                paragraphs,
                headings: vec![Heading {
                    text: format!("File {f}"),
                    level: 1,
                    line: 1,
                }],
                frontmatter: Some("tags: [proj/synthetic]".to_string()),
                mtime: std::time::UNIX_EPOCH,
                line_count: 25,
            });
        }
        let scan = crate::scan::Scan {
            files,
            ..Default::default()
        };
        let graph = Graph::build(&scan).unwrap();
        let index = SearchIndex::build(&scan, &[]);
        (index, graph)
    }

    #[test]
    fn perf_budget_holds() {
        if !perf_enabled() {
            return;
        }
        let build_start = std::time::Instant::now();
        let (index, graph) = synthetic_5k();
        assert_eq!(index.paragraph_count(), 5000);
        let build_ms = build_start.elapsed().as_secs_f64() * 1000.0;
        assert!(
            build_ms < 1000.0,
            "scan-to-index over 5k paragraphs took {build_ms:.1}ms (budget 1000ms)"
        );

        let ctx = SearchCtx {
            graph: Some(&graph),
            presets: None,
            ..Default::default()
        };
        for query in [
            "eigenvalue",
            "memoization",
            "~memoizaton",
            "=threshold",
            "\"trigram candidate\"",
            "eigenvalue -strategy",
            "pinning path:notes/file-01 near:[[file-0100]]",
        ] {
            let q = parse(query, false);
            let start = std::time::Instant::now();
            let mut results = search(&index, &q, ctx).unwrap();
            results.truncate(50);
            fill_highlights(&index, &q, &mut results);
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

//! `ft notes search` — paragraph search across the vault.
//!
//! Any term (substring by default, `=` word, `~` fuzzy, `"phrase"`,
//! `[[link]]`), filters (`path:`, `tag:`, `after:`/`before:`,
//! `scope:<graph-preset>`), and `near:[[Note]]` boosts; AND by default
//! with `--any` for OR, sorted by relevance or blame date. Everything
//! heavy lives in [`ft_core::search`]; this module is flag parsing, the
//! shared CLI query runner, and rendering.

use std::io::{self, IsTerminal};
use std::ops::Range;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::Args;
use ft_core::blame_cache::{BlameCache, BlameValidity};
use ft_core::scan::Scan;
use ft_core::search::semantic::{
    cache::EmbeddingCache, load_embedder, Embedder, SemanticIndex, SemanticQuery, COMPILED,
};
use ft_core::search::{
    fill_highlights, group, parse_query, search_with_dates, DatedSearch, Field, RecencyTable,
    SearchCtx, SearchError, SearchIndex, SearchQuery, SearchResult, Sort, SEMANTIC_LABEL,
};
use ft_core::vault::Vault;
use serde::Serialize;

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// The query: space-separated terms, quoted as needed (`"a b"`
    /// phrase, `[[Link]]`, `=word`, `~fuzzy`, `-exclude`), filters
    /// (`path:`, `tag:`, `after:`, `before:`, `scope:<preset>`), and
    /// `near:[[Note]]`.
    #[arg(value_name = "QUERY", required_unless_present = "like", num_args = 1..)]
    pub query: Vec<String>,

    /// Any-mode: a paragraph matching ANY text clause qualifies (default:
    /// every clause must match). Filters always apply.
    #[arg(long)]
    pub any: bool,

    /// Sort order: relevance (default) or date (newest edit first).
    #[arg(long, value_enum, default_value = "relevance")]
    pub sort: SortArg,

    /// Cap the number of paragraphs printed (applied before grouping).
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,

    /// One line per paragraph (path, lines, labels, full text) instead
    /// of results grouped by note with snippets.
    #[arg(long, conflicts_with = "json")]
    pub flat: bool,

    /// JSON output instead of the default text rows.
    #[arg(long)]
    pub json: bool,

    /// Add meaning-based (embedding) matches for this query, overriding
    /// `[search.semantic] enabled`. Needs `ft notes embed fetch` once.
    #[arg(long, conflicts_with = "no_semantic")]
    pub semantic: bool,

    /// Lexical matches only, overriding `[search.semantic] enabled`.
    #[arg(long)]
    pub no_semantic: bool,

    /// Rank paragraphs by similarity to the paragraph at `<path>:<line>`
    /// (the query, if any, adds terms and filters). Implies semantic.
    #[arg(long, value_name = "PATH:LINE", conflicts_with = "no_semantic")]
    pub like: Option<String>,
}

/// Per-call semantic overrides.
#[derive(Debug, Default, Clone)]
pub(crate) struct SemanticOpts {
    /// `Some(true)` = `--semantic`, `Some(false)` = `--no-semantic`.
    pub force: Option<bool>,
    /// More-like-this source.
    pub like: Option<(PathBuf, u32)>,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum SortArg {
    Relevance,
    Date,
}

impl From<SortArg> for Sort {
    fn from(s: SortArg) -> Sort {
        match s {
            SortArg::Relevance => Sort::Relevance,
            SortArg::Date => Sort::Date,
        }
    }
}

/// What [`run_query`] produced.
pub(crate) enum QueryOutcome {
    Found(DatedSearch),
    /// A usage error in the query (unknown or invalid `scope:` preset),
    /// reported on stderr with exit 2.
    Usage(String),
}

/// Run `query` the way every CLI surface does: the index of `scan` (the
/// same build the TUI snapshot uses, so both rank identically), the link
/// graph and the user's graph presets when the query has a `scope:`
/// filter, and blame dates when sorting or filtering by date. Prints the
/// undated-paragraph warning when a date filter dropped any.
pub(crate) fn run_query(
    vault: &Vault,
    scan: &Scan,
    query: &SearchQuery,
    sort: Sort,
    semantic: &SemanticOpts,
) -> Result<QueryOutcome> {
    if sort == Sort::Date {
        ft_core::git::discover_repo(&vault.path).ok_or_else(|| {
            anyhow!("the vault is not inside a git repository — `--sort date` needs git history for dates")
        })?;
    }
    let cfg = &vault.config.config;
    let index = SearchIndex::build_with(scan, &cfg.synth.exclude_prefixes, &cfg.search);
    // The graph only serves `scope:`; building it costs more than the
    // whole query otherwise.
    let needs_graph = query
        .filters()
        .any(|c| c.mode == ft_core::search::Mode::Filter(ft_core::search::Filter::Scope));
    let graph = if needs_graph {
        Some(crate::cmd::common::build_graph(scan)?)
    } else {
        None
    };
    // Recency reads whatever blame the cache already holds; a plain
    // relevance search never runs `git blame`.
    let validity = BlameValidity::snapshot(&vault.path).context("reading git state")?;
    let mut cache = if validity.is_some() {
        BlameCache::load(&vault.path).context("loading blame cache")?
    } else {
        BlameCache::default()
    };
    let recency = validity
        .as_ref()
        .map(|v| RecencyTable::build(&index, &cache, v));
    let hybrid = match load_hybrid(vault, &index, semantic)? {
        Ok(h) => h,
        Err(msg) => return Ok(QueryOutcome::Usage(msg)),
    };
    let ctx = SearchCtx {
        graph: graph.as_ref(),
        presets: Some(&cfg.graph.presets),
        recency: recency.as_ref(),
        semantic: hybrid.as_ref().map(|(index, embedder)| SemanticQuery {
            index,
            embedder: embedder.as_ref(),
            candidates: cfg.search.semantic.candidates,
            min_similarity: cfg.search.semantic.min_similarity,
        }),
        like: semantic.like.as_ref().map(|(p, l)| (p.as_path(), *l)),
    };
    match search_with_dates(
        &index,
        query,
        ctx,
        vault,
        validity.as_ref(),
        &mut cache,
        sort,
    ) {
        Ok(mut found) => {
            if found.undated_dropped > 0 {
                eprintln!(
                    "warning: {} matching paragraph(s) have no blame date (untracked, or no git) and were dropped by the date filter",
                    found.undated_dropped
                );
            }
            fill_highlights(&index, query, &mut found.results);
            Ok(QueryOutcome::Found(found))
        }
        Err(
            e @ (SearchError::UnknownScope { .. }
            | SearchError::InvalidScope { .. }
            | SearchError::NeedsEmbeddings
            | SearchError::NoSuchParagraph { .. }),
        ) => Ok(QueryOutcome::Usage(e.to_string())),
        Err(e) => Err(anyhow::Error::new(e).context("searching")),
    }
}

/// The embeddings and model for a hybrid query, when semantic search is
/// on for this call. `Ok(Err(msg))` is a usage error (`--like` without a
/// model). A missing model or a build without the feature degrades to
/// lexical search with one hint on stderr.
#[allow(clippy::type_complexity)]
fn load_hybrid(
    vault: &Vault,
    index: &SearchIndex,
    opts: &SemanticOpts,
) -> Result<std::result::Result<Option<(SemanticIndex, Box<dyn Embedder>)>, String>> {
    let cfg = &vault.config.config.search.semantic;
    let on = opts.like.is_some() || opts.force.unwrap_or(cfg.enabled);
    if !on {
        return Ok(Ok(None));
    }
    if !COMPILED {
        eprintln!(
            "warning: semantic search is not compiled into this ft build — lexical results only"
        );
        return Ok(match opts.like {
            Some(_) => Err("--like needs semantic search, which is not compiled in".to_string()),
            None => Ok(None),
        });
    }
    let Some(embedder) = load_embedder(&cfg.model)? else {
        let hint = format!(
            "the embedding model {} is not available — run `ft notes embed fetch`",
            cfg.model
        );
        if opts.like.is_some() {
            return Ok(Err(hint));
        }
        eprintln!("hint: {hint} (lexical results only)");
        return Ok(Ok(None));
    };
    let mut cache = EmbeddingCache::load(&vault.path, embedder.id(), embedder.dim());
    let (sem, stats) = SemanticIndex::build(index, embedder.as_ref(), &mut cache);
    if stats.embedded > 0 || stats.pruned > 0 {
        cache
            .save(&vault.path)
            .context("saving the embedding cache")?;
    }
    Ok(Ok(Some((sem, embedder))))
}

/// Parse `--like <path>:<line>`.
fn parse_like(spec: &str) -> Result<(PathBuf, u32)> {
    let (path, line) = spec
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("--like expects <path>:<line>, got {spec:?}"))?;
    let line: u32 = line
        .parse()
        .map_err(|_| anyhow!("--like expects <path>:<line>, got {spec:?}"))?;
    Ok((PathBuf::from(path), line))
}

pub fn run(args: SearchArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let query = parse_query(&args.query.join(" "), args.any);
    let sort = Sort::from(args.sort);
    let scan = vault.scan();
    let semantic = SemanticOpts {
        force: match (args.semantic, args.no_semantic) {
            (true, _) => Some(true),
            (_, true) => Some(false),
            _ => None,
        },
        like: args.like.as_deref().map(parse_like).transpose()?,
    };

    let mut results = match run_query(&vault, &scan, &query, sort, &semantic)? {
        QueryOutcome::Found(found) => found.results,
        QueryOutcome::Usage(msg) => {
            eprintln!("{msg}");
            return Ok(ExitCode::from(2));
        }
    };
    if let Some(limit) = args.limit {
        results.truncate(limit);
    }

    let show_date = sort == Sort::Date || query.has_date_filters();
    if args.json {
        render_json(&results, show_date)?;
    } else {
        let use_color = std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal();
        if args.flat {
            render_flat(&results, use_color);
        } else {
            render_grouped(&results, use_color);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn render_flat(results: &[SearchResult], use_color: bool) {
    use owo_colors::OwoColorize;
    for r in results {
        let line = format!(
            "{} L{}-{}  {}  {}",
            r.path.display(),
            r.line_start,
            r.line_end,
            labels_of(r),
            r.body.replace('\n', " ")
        );
        if use_color {
            let (path, rest) = line.split_once(' ').unwrap_or((&line, ""));
            println!("{} {rest}", path.cyan());
        } else {
            println!("{line}");
        }
    }
}

/// The matched clause labels, or `≈` for a meaning-only match.
fn labels_of(r: &SearchResult) -> String {
    if r.matched.is_empty() && r.reasons.iter().any(|m| m.field == Field::Semantic) {
        SEMANTIC_LABEL.to_string()
    } else {
        r.matched.join(" ")
    }
}

/// Snippet window width, in chars.
const SNIPPET_CHARS: usize = 160;

fn render_grouped(results: &[SearchResult], use_color: bool) {
    use owo_colors::OwoColorize;
    for g in group(results) {
        let stem = g
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut header = g.path.display().to_string();
        if g.title != stem {
            header = format!("{header} — {}", g.title);
        }
        if use_color {
            println!("{}", header.cyan());
        } else {
            println!("{header}");
        }
        for &i in &g.members {
            let r = &results[i];
            let mut row = format!("  L{}-{}", r.line_start, r.line_end);
            if !r.heading_path.is_empty() {
                let path = r.heading_path.join(" › ");
                if use_color {
                    row.push_str(&format!("  {}", path.dimmed()));
                } else {
                    row.push_str(&format!("  {path}"));
                }
            }
            let labels = labels_of(r);
            if use_color {
                row.push_str(&format!("  {}  ", labels.dimmed()));
            } else {
                row.push_str(&format!("  {labels}  "));
            }
            row.push_str(&snippet(&r.body, &r.highlights, use_color));
            println!("{row}");
        }
    }
}

/// A one-line window of at most [`SNIPPET_CHARS`] chars around the first
/// highlight, trimmed to word boundaries, with `…` marking cuts and the
/// highlights emphasized when color is on.
fn snippet(body: &str, highlights: &[Range<usize>], use_color: bool) -> String {
    use owo_colors::OwoColorize;
    let flat = body.replace('\n', " ");
    let (start, end) = snippet_window(&flat, highlights.first().map(|h| h.start));

    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    let mut at = start;
    for h in highlights {
        let (hs, he) = (h.start.max(start), h.end.min(end));
        if hs >= he {
            continue;
        }
        out.push_str(&flat[at..hs]);
        if use_color {
            out.push_str(&(&flat[hs..he]).bold().yellow().to_string());
        } else {
            out.push_str(&flat[hs..he]);
        }
        at = he;
    }
    out.push_str(&flat[at..end]);
    if end < flat.len() {
        out.push('…');
    }
    out
}

/// Byte bounds of the snippet window: whole text when short, else a
/// window opening a quarter-width before `focus`, snapped inward to
/// whitespace so no word is cut.
fn snippet_window(flat: &str, focus: Option<usize>) -> (usize, usize) {
    let chars: Vec<(usize, char)> = flat.char_indices().collect();
    if chars.len() <= SNIPPET_CHARS {
        return (0, flat.len());
    }
    let focus_char = focus
        .map(|f| chars.partition_point(|&(b, _)| b < f))
        .unwrap_or(0);
    let mut first = focus_char.saturating_sub(SNIPPET_CHARS / 4);
    first = first.min(chars.len() - SNIPPET_CHARS);
    let mut last = first + SNIPPET_CHARS; // exclusive char index

    if first > 0 {
        if let Some(ws) = chars[first..last].iter().position(|&(_, c)| c == ' ') {
            if first + ws < focus_char {
                first += ws + 1;
            }
        }
    }
    if last < chars.len() {
        if let Some(ws) = chars[first..last].iter().rposition(|&(_, c)| c == ' ') {
            if first + ws > focus_char {
                last = first + ws;
            }
        }
    }
    let start = chars[first].0;
    let end = chars.get(last).map_or(flat.len(), |&(b, _)| b);
    (start, end)
}

#[derive(Serialize)]
struct JsonReason<'a> {
    clause: &'a str,
    field: &'static str,
}

#[derive(Serialize)]
struct JsonRow<'a> {
    path: String,
    line_start: u32,
    line_end: u32,
    body: &'a str,
    matched: &'a [String],
    score: f64,
    title: &'a str,
    heading_path: &'a [String],
    highlights: Vec<[usize; 2]>,
    reasons: Vec<JsonReason<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
}

fn render_json(results: &[SearchResult], show_date: bool) -> Result<()> {
    let rows: Vec<JsonRow> = results
        .iter()
        .map(|r| JsonRow {
            path: r.path.to_string_lossy().into_owned(),
            line_start: r.line_start,
            line_end: r.line_end,
            body: &r.body,
            matched: &r.matched,
            score: r.score,
            title: &r.title,
            heading_path: &r.heading_path,
            highlights: r.highlights.iter().map(|h| [h.start, h.end]).collect(),
            reasons: r
                .reasons
                .iter()
                .map(|m| JsonReason {
                    clause: &m.clause,
                    field: m.field.as_str(),
                })
                .collect(),
            date: if show_date {
                r.date.map(|d| d.to_string())
            } else {
                None
            },
        })
        .collect();
    let s = serde_json::to_string_pretty(&rows).context("serialize search json")?;
    println!("{s}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_whole() {
        assert_eq!(
            snippet("short text", std::slice::from_ref(&(6..10)), false),
            "short text"
        );
    }

    #[test]
    fn long_text_is_windowed_on_word_boundaries() {
        let body = format!("{} needle {}", "lead ".repeat(60), "tail ".repeat(60));
        let at = body.find("needle").unwrap();
        let s = snippet(&body, std::slice::from_ref(&(at..at + 6)), false);
        assert!(s.starts_with('…') && s.ends_with('…'), "{s}");
        assert!(s.contains("needle"));
        assert!(s.chars().count() <= SNIPPET_CHARS + 2);
        let inner = s.trim_matches('…');
        assert!(
            inner.starts_with("lead") && inner.ends_with("tail"),
            "{s:?}"
        );
    }

    #[test]
    fn window_without_highlight_starts_at_the_top() {
        let body = "word ".repeat(100);
        let s = snippet(&body, &[], false);
        assert!(!s.starts_with('…'));
        assert!(s.ends_with('…'));
    }

    #[test]
    fn newlines_flatten_and_highlights_survive() {
        let body = "first line\nsecond needle";
        assert_eq!(
            snippet(body, std::slice::from_ref(&(18..24)), false),
            "first line second needle"
        );
    }
}

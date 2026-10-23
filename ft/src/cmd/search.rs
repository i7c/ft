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
use ft_core::blame_cache::BlameCache;
use ft_core::scan::Scan;
use ft_core::search::{
    fill_highlights, group, parse_query, search_with_dates, DatedSearch, SearchCtx, SearchError,
    SearchIndex, SearchQuery, SearchResult, Sort,
};
use ft_core::vault::Vault;
use serde::Serialize;

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// The query: space-separated terms, quoted as needed (`"a b"`
    /// phrase, `[[Link]]`, `=word`, `~fuzzy`, `-exclude`), filters
    /// (`path:`, `tag:`, `after:`, `before:`, `scope:<preset>`), and
    /// `near:[[Note]]`.
    #[arg(value_name = "QUERY", required = true, num_args = 1..)]
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
    let ctx = SearchCtx {
        graph: graph.as_ref(),
        presets: Some(&cfg.graph.presets),
    };
    let mut cache = if sort == Sort::Date || query.has_date_filters() {
        BlameCache::load(&vault.path).context("loading blame cache")?
    } else {
        BlameCache::default()
    };
    match search_with_dates(&index, query, ctx, vault, &mut cache, sort) {
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
        Err(e @ (SearchError::UnknownScope { .. } | SearchError::InvalidScope { .. })) => {
            Ok(QueryOutcome::Usage(e.to_string()))
        }
        Err(e) => Err(anyhow::Error::new(e).context("searching")),
    }
}

pub fn run(args: SearchArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let query = parse_query(&args.query.join(" "), args.any);
    let sort = Sort::from(args.sort);
    let scan = vault.scan();

    let mut results = match run_query(&vault, &scan, &query, sort)? {
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
            r.matched.join(" "),
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
            let labels = r.matched.join(" ");
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

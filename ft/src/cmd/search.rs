//! `ft notes search` — paragraph search across the vault.
//!
//! The search-driven successor to the deprecated gather feed: any term
//! (substring by default, `=` word, `~` fuzzy, `"phrase"`,
//! `[[link]]`), AND by default with `--any` for OR, sorted by
//! relevance or blame date. Everything heavy lives in
//! [`ft_core::search`]; this module is flag parsing and rendering.

use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::Args;
use ft_core::blame_cache::BlameCache;
use ft_core::search::{parse_query, search, search_with_dates, SearchIndex, Sort};
use serde::Serialize;

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// The query: space-separated terms, quoted as needed
    /// (`"a b"` phrase, `[[Link]]`, `=word`, `~fuzzy`, `-exclude`).
    #[arg(value_name = "QUERY", required = true, num_args = 1..)]
    pub query: Vec<String>,

    /// Any-mode: a paragraph matching ANY clause qualifies (default:
    /// every clause must match).
    #[arg(long)]
    pub any: bool,

    /// Sort order: relevance (default) or date (newest edit first).
    #[arg(long, value_enum, default_value = "relevance")]
    pub sort: SortArg,

    /// Cap the number of results printed.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,

    /// JSON output instead of the default text rows.
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum SortArg {
    Relevance,
    Date,
}

pub fn run(args: SearchArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let query_text = args.query.join(" ");
    let query = parse_query(&query_text, args.any);

    let scan = vault.scan();
    let exclude = vault.config.config.synth.exclude_prefixes.clone();
    let index = SearchIndex::build(&scan, &exclude);
    let sort = match args.sort {
        SortArg::Relevance => Sort::Relevance,
        SortArg::Date => Sort::Date,
    };

    let mut results = match sort {
        Sort::Relevance => search(&index, &query),
        Sort::Date => {
            ft_core::git::discover_repo(&vault.path).ok_or_else(|| {
                anyhow!("the vault is not inside a git repository — `ft notes search --sort date` needs git history for dates")
            })?;
            let mut cache = BlameCache::load(&vault.path).context("loading blame cache")?;
            search_with_dates(&index, &query, &vault, &mut cache).context("searching with dates")?
        }
    };

    if let Some(limit) = args.limit {
        results.truncate(limit);
    }

    if args.json {
        render_json(&results, sort)?;
    } else {
        let use_color = std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal();
        render_text(&results, use_color);
    }
    Ok(ExitCode::SUCCESS)
}

fn render_text(results: &[ft_core::search::SearchResult], use_color: bool) {
    use owo_colors::OwoColorize;
    if results.is_empty() {
        return;
    }
    for r in results {
        let labels = if r.matched.is_empty() {
            String::new()
        } else {
            r.matched.join(" ")
        };
        let body = r.body.replace('\n', " ");
        let line = format!(
            "{} L{}-{}  {}  {}",
            r.path.display(),
            r.line_start,
            r.line_end,
            labels,
            body
        );
        if use_color {
            let (path, rest) = line.split_once(' ').unwrap_or((&line, ""));
            println!("{} {rest}", path.cyan());
        } else {
            println!("{line}");
        }
    }
}

#[derive(Serialize)]
struct JsonRow<'a> {
    path: String,
    line_start: u32,
    line_end: u32,
    body: &'a str,
    matched: &'a [String],
    score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
}

fn render_json(results: &[ft_core::search::SearchResult], sort: Sort) -> Result<()> {
    let rows: Vec<JsonRow> = results
        .iter()
        .map(|r| JsonRow {
            path: r.path.to_string_lossy().into_owned(),
            line_start: r.line_start,
            line_end: r.line_end,
            body: &r.body,
            matched: &r.matched,
            score: r.score,
            date: match sort {
                Sort::Date => r.date.map(|d| d.to_string()),
                Sort::Relevance => None,
            },
        })
        .collect();
    let s = serde_json::to_string_pretty(&rows).context("serialize search json")?;
    println!("{s}");
    Ok(())
}

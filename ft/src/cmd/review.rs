//! `ft review` — paragraph-level frequency of `[[wikilinks]]` newly
//! mentioned in a commit/date window. Drives the "what's been on my
//! mind?" half of the synthesis ritual; the multi-source journal
//! (`ft notes journal --link`) picks up from there.
//!
//! All heavy lifting lives in [`ft_core::link_review`]; this module is
//! flag parsing, window validation, and output rendering.

use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use chrono::Duration;
use clap::Args;
use ft_core::link_review::{compute_link_review, LinkReview, LinkReviewRow, WindowRange};

#[derive(Args, Debug)]
pub struct ReviewArgs {
    /// Duration back from today: `7d`, `24h`, `2w`, `1m`. Mutually
    /// exclusive with `--range`. Defaults to `7d` when neither is set.
    #[arg(long, value_name = "DURATION", conflicts_with = "range")]
    pub since: Option<String>,

    /// Commit range `X..Y` (two git refs). Mutually exclusive with
    /// `--since`.
    #[arg(long, value_name = "X..Y", conflicts_with = "since")]
    pub range: Option<String>,

    /// JSON output instead of the default table.
    #[arg(long)]
    pub json: bool,

    /// Disable colored output (also honored: `NO_COLOR` env var,
    /// non-TTY auto-disable).
    #[arg(long)]
    pub no_color: bool,
}

pub fn run(args: ReviewArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    ft_core::git::discover_repo(&vault.path).ok_or_else(|| {
        anyhow!(
            "the vault is not inside a git repository — `ft review` needs git history to find added links"
        )
    })?;

    let window = resolve_window(&args)?;
    let graph = crate::cmd::common::build_graph(&vault, &vault.scan())?;
    let cfg = vault.config.config.synth.clone();
    let review =
        compute_link_review(&graph, &vault, &window, &cfg).context("computing link review")?;

    if args.json {
        render_json(&review)?;
    } else {
        let use_color =
            !args.no_color && std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal();
        render_table(&review, use_color);
    }
    Ok(ExitCode::SUCCESS)
}

fn resolve_window(args: &ReviewArgs) -> Result<WindowRange> {
    if let Some(s) = args.since.as_deref() {
        let dur = WindowRange::parse_since(s)
            .ok_or_else(|| anyhow!("invalid --since value `{s}` (try e.g. 7d, 24h, 2w, 1m)"))?;
        return Ok(WindowRange::Since(dur));
    }
    if let Some(r) = args.range.as_deref() {
        let (from, to) = r
            .split_once("..")
            .ok_or_else(|| anyhow!("invalid --range value `{r}` (expected `X..Y`)"))?;
        if from.is_empty() || to.is_empty() {
            return Err(anyhow!(
                "invalid --range value `{r}` (both X and Y required)"
            ));
        }
        return Ok(WindowRange::Range {
            from: from.to_string(),
            to: to.to_string(),
        });
    }
    Ok(WindowRange::Since(Duration::days(7)))
}

fn render_table(review: &LinkReview, use_color: bool) {
    if review.rows.is_empty() {
        println!("no new links in window");
        return;
    }
    use owo_colors::OwoColorize;
    for row in &review.rows {
        let line = format_row_plain(row);
        if use_color {
            println!("{}", line.cyan());
        } else {
            println!("{line}");
        }
    }
}

fn format_row_plain(row: &LinkReviewRow) -> String {
    let ghost = if row.is_ghost { "?" } else { "" };
    format!("({}) [[{}]]{}", row.count, row.target, ghost)
}

fn render_json(review: &LinkReview) -> Result<()> {
    #[derive(serde::Serialize)]
    struct Row<'a> {
        count: usize,
        target: &'a str,
        is_ghost: bool,
        source_paths: Vec<String>,
    }
    let rows: Vec<Row> = review
        .rows
        .iter()
        .map(|r| Row {
            count: r.count,
            target: &r.target,
            is_ghost: r.is_ghost,
            source_paths: r
                .source_paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        })
        .collect();
    let s = serde_json::to_string_pretty(&rows).context("serialize review json")?;
    println!("{s}");
    Ok(())
}

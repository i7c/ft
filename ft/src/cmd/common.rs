//! Shared CLI helpers — vault discovery + graph build with the
//! standard `anyhow::Context` strings each `ft *` subcommand uses.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use chrono::NaiveDate;
use ft_core::graph::Graph;
use ft_core::query::{interpolate, SigilCtx};
use ft_core::scan::Scan;
use ft_core::vault::Vault;

/// Discover a vault (`--vault` flag → `$FT_VAULT` → CWD walk-up →
/// user-config `default_vault`) with the standard "could not locate"
/// context attached. Used by every CLI subcommand.
pub fn discover_vault(vault_flag: Option<PathBuf>) -> Result<Vault> {
    Vault::discover(vault_flag).context("could not locate a vault")
}

/// Build the note-link graph from a scan with the standard "graph
/// build failed" context attached.
pub fn build_graph(scan: &Scan) -> Result<Graph> {
    Graph::build(scan).context("could not build graph for vault")
}

/// Build a [`SigilCtx`] for `vault` at `today`, for `@`-sigil
/// interpolation of query strings / presets.
pub fn sigil_ctx<'a>(vault: &'a Vault, today: NaiveDate) -> SigilCtx<'a> {
    SigilCtx {
        today,
        vault_root: &vault.path,
        periodic: &vault.config.config.periodic_notes,
    }
}

/// Interpolate `@`-sigils in `src`, wrapping the error with the query
/// text so the message reads like the adjacent DSL parse-error path.
pub fn interpolate_query(src: &str, vault: &Vault, today: NaiveDate) -> Result<String> {
    interpolate(src, sigil_ctx(vault, today))
        .map_err(|e| anyhow::anyhow!("invalid query `{src}`: {e}"))
}

/// Parse an `A-B` line-range spec into a validated 1-indexed inclusive
/// range. Shared by `ft notes quote` (required) and `ft notes export`
/// (optional): positive integers, `A >= 1`, `A <= B`.
pub fn parse_line_range(spec: &str) -> Result<(u32, u32)> {
    let (a, b) = spec
        .split_once('-')
        .ok_or_else(|| anyhow!("invalid --lines `{spec}` (expected `A-B`)"))?;
    let start: u32 = a
        .trim()
        .parse()
        .map_err(|_| anyhow!("invalid --lines `{spec}` (A must be a positive integer)"))?;
    let end: u32 = b
        .trim()
        .parse()
        .map_err(|_| anyhow!("invalid --lines `{spec}` (B must be a positive integer)"))?;
    if start < 1 {
        return Err(anyhow!(
            "invalid --lines `{spec}` (lines are 1-indexed; A must be >= 1)"
        ));
    }
    if start > end {
        return Err(anyhow!("invalid --lines `{spec}` (A must be <= B)"));
    }
    Ok((start, end))
}

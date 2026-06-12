//! Shared CLI helpers — vault discovery + graph build with the
//! standard `anyhow::Context` strings each `ft *` subcommand uses.

use std::path::PathBuf;

use anyhow::{Context, Result};
use ft_core::graph::Graph;
use ft_core::vault::{Scan, Vault};

/// Discover a vault (`--vault` flag → `$FT_VAULT` → CWD walk-up →
/// user-config `default_vault`) with the standard "could not locate"
/// context attached. Used by every CLI subcommand.
pub fn discover_vault(vault_flag: Option<PathBuf>) -> Result<Vault> {
    Vault::discover(vault_flag).context("could not locate an Obsidian vault")
}

/// Build the note-link graph for `vault` with the standard "graph build
/// failed" context attached.
pub fn build_graph(vault: &Vault, scan: &Scan) -> Result<Graph> {
    Graph::build(vault, scan).context("could not build graph for vault")
}

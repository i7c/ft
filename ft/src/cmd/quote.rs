//! `ft notes quote` — read-only plumbing that emits the canonical
//! protected-section callout for a line range of a vault file.
//!
//! This is the CLI surface for the pinning mechanics scaffold/grow
//! share: per-source clean check against HEAD (`find_dirty_sources`),
//! HEAD short SHA + blake3 content hash (`build_pinned_section`), and
//! the canonical `[!ft-source]` serialization. Nothing is written —
//! the callout goes to stdout for scripts and external tools (notably
//! `ft.nvim` pinning editor selections).

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::Args;
use ft_core::synth::callout::serialize;
use ft_core::synth::scaffold::{build_pinned_section, find_dirty_sources};
use ft_core::synth::slice;
use ft_core::synth::source::SynthSource;

#[derive(Args, Debug)]
pub struct QuoteArgs {
    /// Source file to quote from (vault-relative; absolute paths are
    /// accepted and relativized for the callout header). `.md` is not
    /// appended automatically.
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// 1-indexed inclusive line range `A-B` to quote.
    #[arg(short = 'l', long, value_name = "A-B")]
    pub lines: String,
}

pub fn run_quote(args: QuoteArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let (start, end) = crate::cmd::common::parse_line_range(&args.lines)?;

    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    ft_core::git::discover_repo(&vault.path).ok_or_else(|| {
        anyhow!("vault is not inside a git repository — `ft notes quote` pins to HEAD and needs git history")
    })?;

    let rel = vault.relativize(&args.file);
    let absolute = vault.path.join(&rel);
    let content = std::fs::read_to_string(&absolute)
        .with_context(|| format!("cannot read source file `{}`", rel.display()))?;

    // The pinned section reproduces the working-tree body only when the
    // file matches HEAD — the same per-source prerequisite scaffold
    // enforces. Other dirty files in the tree do not block.
    let repo = ft_core::git::RepoMap::discover(&vault.path)?;
    let dirty = find_dirty_sources(&repo, std::slice::from_ref(&rel))?;
    if !dirty.is_empty() {
        return Err(anyhow!(
            "source file `{}` has uncommitted changes — `ft notes quote` pins to HEAD, \
             so the file must be committed and unmodified (commit or stash first)",
            rel.display()
        ));
    }

    let body = slice::slice_lines(&content, start, end).ok_or_else(|| {
        anyhow!(
            "line range L{}-{} outside file `{}` (file has {} lines)",
            start,
            end,
            rel.display(),
            slice::count_lines(&content)
        )
    })?;

    let short_sha = ft_core::git::head_short_sha(repo.root())?;
    let source = SynthSource {
        source_path: rel,
        line_start: start,
        line_end: end,
        body,
    };
    let section = build_pinned_section(&short_sha, &source);
    println!("{}", serialize(&section));
    Ok(ExitCode::SUCCESS)
}

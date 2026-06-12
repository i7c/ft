//! `ft git sync` — commit, pull, push the vault repo in one shot.
//!
//! Discovery starts at the vault root and walks up looking for a `.git/`
//! entry. The first ancestor that has one is the repo. If no enclosing
//! repo exists, the feature is unavailable (`ExitCode::FAILURE`).
//!
//! Exit codes:
//! - `0` — happy path (clean / synced).
//! - `2` — merge or rebase conflict. The repo is left in its conflicted
//!   state; resolve manually.
//! - `1` — every other error (no repo, no upstream, push rejected,
//!   network failure). Surfaced via the top-level `anyhow` path.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};
use ft_core::git::{discover_repo, status, sync, upstream, PullStrategy, SyncOptions, SyncOutcome};

#[derive(Args)]
pub struct GitArgs {
    #[command(subcommand)]
    pub command: GitCommand,
}

#[derive(Subcommand)]
pub enum GitCommand {
    /// Commit dirty tree, pull upstream, push.
    Sync(SyncArgs),
}

#[derive(Args, Debug)]
pub struct SyncArgs {
    /// Override the auto-generated commit message
    /// (`"ft sync <iso8601-utc>"`).
    #[arg(short = 'm', long, value_name = "MSG")]
    pub message: Option<String>,

    /// Print the plan and exit without touching the repo. Reads
    /// status + upstream only; no writes, no network.
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: GitArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    match args.command {
        GitCommand::Sync(s) => run_sync(s, vault_flag),
    }
}

fn run_sync(args: SyncArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;

    let repo = discover_repo(&vault.path).ok_or_else(|| {
        anyhow!(
            "no git repository found at or above vault root: {}",
            vault.path.display()
        )
    })?;

    let strategy = vault.config.config.git.pull_strategy;

    if args.dry_run {
        return run_dry(&repo, strategy);
    }

    let opts = SyncOptions {
        strategy,
        message: args.message,
    };
    let outcome = sync(&repo, &opts)?;
    render_outcome(outcome)
}

fn run_dry(repo: &std::path::Path, strategy: PullStrategy) -> Result<ExitCode> {
    let up = upstream(repo)?;
    let strategy_label = match strategy {
        PullStrategy::Merge => "merge",
        PullStrategy::Rebase => "rebase",
    };
    match &up {
        Some(u) => println!("upstream: {u} ({strategy_label})"),
        None => println!("upstream: <none configured>"),
    }

    let st = status(repo)?;
    let total = st.modified.len() + st.untracked.len() + st.deleted.len();
    if st.has_conflicts() {
        println!(
            "working tree: {} conflicted file(s) — resolve before syncing",
            st.conflicted.len()
        );
        return Ok(ExitCode::from(2));
    }
    if total == 0 {
        println!("working tree: clean");
        println!("nothing to commit");
    } else {
        println!(
            "working tree: {} change(s) ({} modified, {} untracked, {} deleted)",
            total,
            st.modified.len(),
            st.untracked.len(),
            st.deleted.len()
        );
        println!("would commit {total} file(s)");
    }
    match &up {
        Some(u) => println!("would pull {u}"),
        None => println!("would skip pull (no upstream)"),
    }
    if up.is_some() {
        println!("would push");
    }
    Ok(ExitCode::SUCCESS)
}

fn render_outcome(outcome: SyncOutcome) -> Result<ExitCode> {
    match outcome {
        SyncOutcome::Clean { pushed: false } => {
            println!("already in sync");
            Ok(ExitCode::SUCCESS)
        }
        SyncOutcome::Clean { pushed: true } => {
            println!("pushed local commits");
            Ok(ExitCode::SUCCESS)
        }
        SyncOutcome::Synced {
            committed,
            pulled,
            pushed,
        } => {
            println!("committed {committed} file(s)");
            if pulled {
                println!("pulled");
            }
            if pushed {
                println!("pushed");
            }
            Ok(ExitCode::SUCCESS)
        }
        SyncOutcome::MergeConflict { files } => {
            eprintln!("merge conflict in {} file(s):", files.len());
            for f in &files {
                eprintln!("  {}", f.display());
            }
            eprintln!("resolve, commit, and push manually.");
            Ok(ExitCode::from(2))
        }
        SyncOutcome::RebaseConflict { files } => {
            eprintln!("rebase conflict in {} file(s):", files.len());
            for f in &files {
                eprintln!("  {}", f.display());
            }
            eprintln!("resolve and continue the rebase manually (git rebase --continue).");
            Ok(ExitCode::from(2))
        }
    }
}

//! `ft graph query` — run a graph DSL query against the vault and
//! render the walked subtree.
//!
//! The CLI's mental model is *static traversal*: pass a single DSL
//! expression plus a depth bound, get the full subtree printed at
//! once. Depth defaults to unlimited with cycle-stop semantics — the
//! same default as `tree(1)` — so a typical invocation prints the
//! whole reachable subgraph without configuration.
//!
//! Exit codes:
//! - `0` — happy path.
//! - `2` — DSL parse error or unknown preset (matches the task DSL
//!   convention).
//! - `1` — every other error (vault not found, IO failure, etc.),
//!   surfaced through the top-level anyhow path.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use ft_core::graph::delete::{apply_delete, plan_delete};
use ft_core::graph::preset;
use ft_core::graph::query::{parse_with, CyclePolicy, Profile, WalkOptions};
use ft_core::vault::Vault;

use crate::output::graph::{render, Format};

#[derive(Args)]
pub struct GraphArgs {
    #[command(subcommand)]
    pub command: GraphCommand,
}

#[derive(Subcommand)]
pub enum GraphCommand {
    /// Parse a DSL query, walk the graph, and print the result.
    Query(QueryArgs),
    /// Delete a note file or directory tree from the vault.
    Delete(DeleteArgs),
}

#[derive(Args, Debug)]
pub struct QueryArgs {
    /// DSL source. Mutually exclusive with `--query` / `--from-file` / `--preset`.
    #[arg(value_name = "QUERY", conflicts_with_all = ["query_opt", "from_file", "preset"])]
    pub query: Option<String>,

    /// DSL source as a flag (alternative to the positional form).
    #[arg(
        short = 'q',
        long = "query",
        value_name = "QUERY",
        conflicts_with_all = ["from_file", "preset"]
    )]
    pub query_opt: Option<String>,

    /// Read DSL source from a file. Useful when the query grows past
    /// comfortable shell-quoting length.
    #[arg(long = "from-file", value_name = "PATH", conflicts_with = "preset")]
    pub from_file: Option<PathBuf>,

    /// Resolve a named graph-query preset (built-in or from config)
    /// to its DSL string. User presets shadow built-ins of the same name.
    #[arg(long, value_name = "NAME", conflicts_with_all = ["query", "query_opt", "from_file"])]
    pub preset: Option<String>,

    /// Maximum depth to walk. Default unlimited. `0` returns roots only.
    #[arg(long)]
    pub depth: Option<usize>,

    /// How to handle back-edges that would re-visit an ancestor.
    /// `stop` (default) emits the cycle marker and halts that branch;
    /// `allow` requires `--depth` to bound the traversal.
    #[arg(long, value_enum, default_value_t = CycleArg::Stop)]
    pub cycle_policy: CycleArg,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Tree)]
    pub format: Format,

    /// Parser profile. `default` is the verbose graph syntax;
    /// `tasks` lets you write bare predicates like `priority = high` that
    /// desugar to `node where kind = Task and self.priority = high`.
    #[arg(long, value_enum, default_value_t = ProfileArg::Default)]
    pub profile: ProfileArg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ProfileArg {
    Default,
    Tasks,
}

impl From<ProfileArg> for Profile {
    fn from(v: ProfileArg) -> Self {
        match v {
            ProfileArg::Default => Profile::Default,
            ProfileArg::Tasks => Profile::Tasks,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CycleArg {
    Stop,
    Allow,
}

impl From<CycleArg> for CyclePolicy {
    fn from(v: CycleArg) -> Self {
        match v {
            CycleArg::Stop => CyclePolicy::Stop,
            CycleArg::Allow => CyclePolicy::Allow,
        }
    }
}

pub fn run(args: GraphArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    match args.command {
        GraphCommand::Query(q) => run_query(q, vault_flag),
        GraphCommand::Delete(d) => run_delete(d, vault_flag),
    }
}

fn run_query(args: QueryArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;

    let src = read_query_source(&args, &vault)?;

    let today = ft_core::dates::today();

    let query = match parse_with(&src, args.profile.into(), today) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("{e}");
            return Ok(ExitCode::from(2));
        }
    };

    let graph = crate::cmd::common::build_graph(&vault, &vault.scan())?;

    let opts = WalkOptions {
        max_depth: args.depth,
        cycle_policy: args.cycle_policy.into(),
    };

    if matches!(opts.cycle_policy, CyclePolicy::Allow) && opts.max_depth.is_none() {
        bail!("--cycle-policy allow requires --depth (otherwise a cyclic query loops forever)");
    }

    let tree = query.walk(&graph, &opts);

    let stdout = io::stdout();
    let mut out = stdout.lock();
    render(&mut out, &tree, &graph, args.format)?;
    out.flush().ok();

    Ok(ExitCode::SUCCESS)
}

fn read_query_source(args: &QueryArgs, vault: &Vault) -> Result<String> {
    if let Some(name) = &args.preset {
        return match resolve_preset(name, vault) {
            Some(dsl) => Ok(dsl),
            None => {
                eprintln!("unknown preset: {name}");
                std::process::exit(2);
            }
        };
    }

    match (
        args.query.as_deref(),
        args.query_opt.as_deref(),
        args.from_file.as_deref(),
    ) {
        (Some(s), None, None) | (None, Some(s), None) => Ok(s.to_string()),
        (None, None, Some(p)) => std::fs::read_to_string(p)
            .with_context(|| format!("could not read query from {}", p.display())),
        (None, None, None) => Err(anyhow!(
            "no query supplied — pass a positional QUERY, `--query`, `--from-file PATH`, or `--preset NAME`"
        )),
        _ => Err(anyhow!(
            "QUERY, --query, --from-file, and --preset are mutually exclusive — pass exactly one"
        )),
    }
}

/// `ft graph delete` arguments.
#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// Vault-relative path of the note or directory to delete.
    #[arg(value_name = "PATH")]
    pub path: PathBuf,
}

fn run_delete(args: DeleteArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;

    let abs = vault.path.join(&args.path);
    if !abs.exists() {
        Err(anyhow!("path does not exist: {}", args.path.display()))?;
    }

    let plan = plan_delete(&args.path, &vault.path)
        .with_context(|| format!("cannot delete {}", args.path.display()))?;

    apply_delete(&vault.path, &plan)
        .with_context(|| format!("failed to delete {}", args.path.display()))?;

    let path_display = args.path.display();
    if args
        .path
        .parent()
        .is_some_and(|p| !p.as_os_str().is_empty())
        && args.path.extension().is_some()
    {
        println!("deleted note {path_display}");
    } else {
        println!("deleted {path_display}");
    }

    Ok(ExitCode::SUCCESS)
}

fn resolve_preset(name: &str, vault: &Vault) -> Option<String> {
    if let Some(user) = vault.config.config.graph.presets.get(name) {
        return Some(user.clone());
    }
    preset::builtin(name).map(|s| s.to_string())
}

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use chrono::NaiveDate;
use clap::{Args, Subcommand, ValueEnum};
use ft_core::{
    query::{filter::Filter, sort::default_sort},
    task::{Priority, Status},
    vault::Vault,
};

use crate::output::{self, Format};

#[derive(Args)]
pub struct TasksArgs {
    #[command(subcommand)]
    pub command: TasksCommand,
}

#[derive(Subcommand)]
pub enum TasksCommand {
    /// List tasks across the vault, optionally filtered.
    List(ListArgs),
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum StatusFlag {
    Open,
    Done,
    #[value(name = "in-progress")]
    InProgress,
    Cancelled,
}

impl From<StatusFlag> for Status {
    fn from(s: StatusFlag) -> Self {
        match s {
            StatusFlag::Open => Status::Open,
            StatusFlag::Done => Status::Done,
            StatusFlag::InProgress => Status::InProgress,
            StatusFlag::Cancelled => Status::Cancelled,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum PriorityFlag {
    Highest,
    High,
    Medium,
    Low,
    Lowest,
}

impl From<PriorityFlag> for Priority {
    fn from(p: PriorityFlag) -> Self {
        match p {
            PriorityFlag::Highest => Priority::Highest,
            PriorityFlag::High => Priority::High,
            PriorityFlag::Medium => Priority::Medium,
            PriorityFlag::Low => Priority::Low,
            PriorityFlag::Lowest => Priority::Lowest,
        }
    }
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Filter by status (repeatable).
    #[arg(long, value_enum)]
    pub status: Vec<StatusFlag>,

    /// Filter by priority (repeatable).
    #[arg(long, value_enum)]
    pub priority: Vec<PriorityFlag>,

    /// Filter by tag (repeatable). Leading `#` is optional.
    #[arg(long)]
    pub tag: Vec<String>,

    /// Substring filter on the source file path (repeatable; all must match).
    #[arg(long)]
    pub path: Vec<String>,

    /// Only tasks due strictly before this date (YYYY-MM-DD).
    #[arg(long, value_name = "DATE")]
    pub due_before: Option<NaiveDate>,

    /// Only tasks due strictly after this date (YYYY-MM-DD).
    #[arg(long, value_name = "DATE")]
    pub due_after: Option<NaiveDate>,

    /// Only tasks scheduled strictly before this date (YYYY-MM-DD).
    #[arg(long, value_name = "DATE")]
    pub scheduled_before: Option<NaiveDate>,

    /// Only tasks scheduled strictly after this date (YYYY-MM-DD).
    #[arg(long, value_name = "DATE")]
    pub scheduled_after: Option<NaiveDate>,

    /// Only tasks that have a due date.
    #[arg(long, conflicts_with = "no_due")]
    pub has_due: bool,

    /// Only tasks without a due date.
    #[arg(long)]
    pub no_due: bool,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Table)]
    pub format: Format,

    /// Disable colored output (also honored: `NO_COLOR` env var).
    #[arg(long)]
    pub no_color: bool,
}

pub fn run(args: TasksArgs, vault_flag: Option<PathBuf>) -> Result<()> {
    match args.command {
        TasksCommand::List(list_args) => run_list(list_args, vault_flag),
    }
}

fn run_list(args: ListArgs, vault_flag: Option<PathBuf>) -> Result<()> {
    let vault = Vault::discover(vault_flag).context("could not locate an Obsidian vault")?;
    let scan = vault.scan();

    for err in &scan.errors {
        tracing::warn!("{}", err);
    }

    let filter = build_filter(&args)?;
    let mut matches = filter.apply(&scan.tasks);
    default_sort(&mut matches);

    match args.format {
        Format::Table => {
            let use_color = !args.no_color
                && std::env::var_os("NO_COLOR").is_none()
                && is_terminal::IsTerminal::is_terminal(&std::io::stdout());
            let out = output::table::render(&matches, output::table::TableOpts { use_color });
            println!("{out}");
        }
        Format::Json => output::json::render(&matches)?,
    }

    Ok(())
}

fn build_filter(args: &ListArgs) -> Result<Filter> {
    let has_due = if args.has_due {
        Some(true)
    } else if args.no_due {
        Some(false)
    } else {
        None
    };

    if args.has_due && args.no_due {
        return Err(anyhow!("--has-due and --no-due are mutually exclusive"));
    }

    Ok(Filter {
        statuses: args.status.iter().copied().map(Into::into).collect(),
        priorities: args.priority.iter().copied().map(Into::into).collect(),
        tags: args.tag.clone(),
        paths: args.path.clone(),
        due_before: args.due_before,
        due_after: args.due_after,
        scheduled_before: args.scheduled_before,
        scheduled_after: args.scheduled_after,
        has_due,
    })
}

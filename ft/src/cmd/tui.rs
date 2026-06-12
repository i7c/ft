use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::tui;

#[derive(Args)]
pub struct TuiArgs;

pub fn run(_args: TuiArgs, vault_flag: Option<PathBuf>) -> Result<()> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    tui::run(vault)
}

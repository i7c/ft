use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct VaultArgs;

pub fn run(_args: VaultArgs, vault_flag: Option<PathBuf>) -> Result<()> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;

    println!("Vault: {}", vault.path.display());
    println!();
    println!("Config files (lowest → highest precedence):");
    for (i, src) in vault.config.sources.iter().enumerate() {
        let status = if src.present { "present" } else { "not found" };
        println!(
            "  [{}] {} ({}): {}",
            i + 1,
            src.path.display(),
            src.label,
            status
        );
    }

    println!();
    println!("Merged config:");
    let cfg = &vault.config.config;
    print_opt("default_vault", cfg.default_vault.as_deref());
    print_opt(
        "default_task_location",
        cfg.default_task_location.as_deref(),
    );
    println!("  periodic_notes:");
    for (label, p) in [
        ("daily", &cfg.periodic_notes.daily),
        ("weekly", &cfg.periodic_notes.weekly),
        ("monthly", &cfg.periodic_notes.monthly),
        ("quarterly", &cfg.periodic_notes.quarterly),
        ("yearly", &cfg.periodic_notes.yearly),
    ] {
        match p {
            Some(period) => {
                println!("    [{label}]");
                println!("      path = {:?}", period.path);
                println!("      format = {:?}", period.format);
                print_opt("      template", period.template.as_deref());
            }
            None => println!("    [{label}] = (not configured)"),
        }
    }
    if cfg.ignored_paths.is_empty() {
        println!("  ignored_paths = []");
    } else {
        println!("  ignored_paths = {:?}", cfg.ignored_paths);
    }
    if cfg.presets.is_empty() {
        println!("  presets = {{}}");
    } else {
        println!("  presets:");
        let mut keys: Vec<&String> = cfg.presets.keys().collect();
        keys.sort();
        for k in keys {
            println!("    {} = {:?}", k, cfg.presets[k]);
        }
    }

    Ok(())
}

fn print_opt(key: &str, val: Option<&str>) {
    match val {
        Some(v) => println!("  {} = {:?}", key, v),
        None => println!("  {} = (not set)", key),
    }
}

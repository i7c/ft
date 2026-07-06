//! `ft commands list` — introspect the command registry.
//!
//! Walks every `CommandDef` in the build-time `CommandRegistry` and
//! prints it in one of three formats: a terminal table (default),
//! NDJSON (one JSON object per line), or grouped JSON.

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};

use crate::tui::registry::{self, CommandDef, CommandScope};

#[derive(Args)]
pub struct CommandsArgs {
    #[command(subcommand)]
    pub command: CommandsCommand,
}

#[derive(Subcommand)]
pub enum CommandsCommand {
    /// List every command in the registry
    List(ListArgs),
    /// Generate (or check) the `docs/keybindings.md` markdown reference
    Docs(DocsArgs),
    /// Validate the `[keymap]` section of `config.toml` against the registry
    CheckKeymap(CheckKeymapArgs),
}

#[derive(Args)]
pub struct CheckKeymapArgs {
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    format: OutputFormat,
}

#[derive(Args)]
pub struct DocsArgs {
    /// Path to write to (or check against). Defaults to
    /// `docs/keybindings.md` in the current directory.
    #[arg(long)]
    pub out: Option<std::path::PathBuf>,

    /// Don't write — compare the generator output to the file at
    /// `--out` and exit non-zero if they differ. Used by CI to gate
    /// drift between the registry and the committed docs.
    #[arg(long)]
    pub check: bool,
}

#[derive(Args)]
pub struct ListArgs {
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    format: OutputFormat,

    /// Scope filter (`global`, `tab`, `modal`, or a specific
    /// `tab/<name>` / `modal/<name>`)
    #[arg(long)]
    scope: Option<String>,

    /// Filter by `opens_modal` (true / false). Without the flag, both
    /// kinds are shown.
    #[arg(long)]
    opens_modal: Option<bool>,

    /// Show effective chord-to-command bindings (defaults merged with
    /// user `[keymap]` config) instead of the raw registry view.
    #[arg(long)]
    effective: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum OutputFormat {
    Table,
    Ndjson,
    Json,
}

pub fn run(
    args: CommandsArgs,
    vault_flag: Option<std::path::PathBuf>,
) -> Result<std::process::ExitCode> {
    match args.command {
        CommandsCommand::List(args) => {
            list(args, vault_flag).map(|_| std::process::ExitCode::SUCCESS)
        }
        CommandsCommand::Docs(args) => docs(args).map(|_| std::process::ExitCode::SUCCESS),
        CommandsCommand::CheckKeymap(args) => run_check_keymap(args, vault_flag),
    }
}

fn run_check_keymap(
    args: CheckKeymapArgs,
    vault_flag: Option<std::path::PathBuf>,
) -> Result<std::process::ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let errors = crate::tui::registry::validate_keymap(&vault.config.config);

    if errors.is_empty() {
        match args.format {
            OutputFormat::Table => println!("keymap config ok -- no errors"),
            OutputFormat::Ndjson | OutputFormat::Json => {
                println!("{}", serde_json::json!({ "ok": true, "errors": [] }));
            }
        }
        return Ok(std::process::ExitCode::SUCCESS);
    }

    match args.format {
        OutputFormat::Table => {
            for e in &errors {
                eprintln!("error: {e}");
            }
        }
        OutputFormat::Ndjson => {
            for e in &errors {
                println!("{}", serde_json::json!({ "ok": false, "error": e }));
            }
        }
        OutputFormat::Json => {
            let errs: Vec<serde_json::Value> = errors
                .iter()
                .map(|e| serde_json::json!({ "error": e }))
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "ok": false, "errors": errs }))?
            );
        }
    }
    // Exit 2 on validation errors (matching the lint-tool convention).
    Ok(std::process::ExitCode::from(2))
}

fn docs(args: DocsArgs) -> Result<()> {
    let body = generate_keybindings_md();
    let path = args
        .out
        .unwrap_or_else(|| std::path::PathBuf::from("docs/keybindings.md"));
    if args.check {
        let existing = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("could not read {}: {e}", path.display()))?;
        if existing != body {
            anyhow::bail!(
                "{} is out of date; regenerate with `ft commands docs --out {}`",
                path.display(),
                path.display()
            );
        }
        Ok(())
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("could not create {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, body)
            .map_err(|e| anyhow::anyhow!("could not write {}: {e}", path.display()))?;
        eprintln!("wrote {}", path.display());
        Ok(())
    }
}

/// Walk the command registry, group by scope, and emit a markdown
/// reference. The output is intentionally stable (sorted by name
/// within each scope) so the file is byte-identical across runs as
/// long as the registry hasn't changed.
fn generate_keybindings_md() -> String {
    use std::collections::BTreeMap;
    use std::fmt::Write;

    let reg = registry::build();
    // Group by scope as a stringified key so display order is stable.
    let mut by_scope: BTreeMap<String, Vec<&CommandDef>> = BTreeMap::new();
    for def in reg.iter() {
        by_scope.entry(def.scope.as_str()).or_default().push(def);
    }
    for defs in by_scope.values_mut() {
        defs.sort_by_key(|d| d.name);
    }

    let mut out = String::new();
    writeln!(out, "# Commands & keybindings").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "Generated by `ft commands docs`. Re-run after modifying any \
         `CommandDef` slice. CI's `ft commands docs --check` verifies this \
         file stays in sync."
    )
    .unwrap();
    writeln!(out).unwrap();

    // Emit a canonical order: global first, then tabs alphabetically,
    // then modals alphabetically.
    let mut scopes: Vec<String> = by_scope.keys().cloned().collect();
    scopes.sort_by(|a, b| {
        let rank = |s: &str| {
            if s == "global" {
                0
            } else if s.starts_with("tab/") {
                1
            } else {
                2
            }
        };
        rank(a).cmp(&rank(b)).then(a.cmp(b))
    });

    for scope in &scopes {
        let defs = &by_scope[scope];
        writeln!(out, "## {scope}").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "| Name | Description | Group | Opens modal |").unwrap();
        writeln!(out, "| --- | --- | --- | --- |").unwrap();
        for def in defs {
            writeln!(
                out,
                "| `{}` | {} | {} | {} |",
                def.name,
                def.description,
                def.group,
                if def.opens_modal { "yes" } else { "no" }
            )
            .unwrap();
        }
        writeln!(out).unwrap();
    }

    out
}

fn list(args: ListArgs, vault_flag: Option<std::path::PathBuf>) -> Result<()> {
    if args.effective {
        return list_effective(args, vault_flag);
    }
    let reg = registry::build();
    let mut filtered: Vec<&CommandDef> = reg
        .iter()
        .filter(|def| scope_matches(&def.scope, args.scope.as_deref()))
        .filter(|def| args.opens_modal.is_none_or(|v| def.opens_modal == v))
        .collect();
    // Stable order: by scope, then by name.
    filtered.sort_by_key(|d| (d.scope.as_str(), d.name));

    match args.format {
        OutputFormat::Table => print_table(&filtered),
        OutputFormat::Ndjson => print_ndjson(&filtered)?,
        OutputFormat::Json => print_json(&filtered)?,
    }
    Ok(())
}

fn list_effective(args: ListArgs, vault_flag: Option<std::path::PathBuf>) -> Result<()> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let mut bindings = crate::tui::registry::effective_bindings(&vault.config.config);

    // Apply scope filter.
    if let Some(ref scope_filter) = args.scope {
        bindings.retain(|(scope, _, _)| scope_filter_matches(scope, scope_filter));
    }

    // Stable order: scope, then chord.
    bindings.sort_by(|(sa, ca, _), (sb, cb, _)| sa.cmp(sb).then(ca.cmp(cb)));

    match args.format {
        OutputFormat::Table => {
            use comfy_table::{presets, Cell, Color, ContentArrangement, Table};
            let mut table = Table::new();
            table
                .load_preset(presets::UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec![
                    Cell::new("Scope").fg(Color::Cyan),
                    Cell::new("Chord").fg(Color::Cyan),
                    Cell::new("Command").fg(Color::Cyan),
                ]);
            for (scope, chord, cmd) in &bindings {
                table.add_row(vec![Cell::new(scope), Cell::new(chord), Cell::new(cmd)]);
            }
            println!("{table}");
        }
        OutputFormat::Ndjson => {
            for (scope, chord, cmd) in &bindings {
                println!(
                    "{}",
                    serde_json::json!({ "scope": scope, "chord": chord, "command": cmd })
                );
            }
        }
        OutputFormat::Json => {
            let arr: Vec<serde_json::Value> = bindings
                .iter()
                .map(|(scope, chord, cmd)| {
                    serde_json::json!({ "scope": scope, "chord": chord, "command": cmd })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr)?);
        }
    }
    Ok(())
}

fn scope_filter_matches(scope: &str, filter: &str) -> bool {
    match filter {
        "global" => scope == "global",
        "tab" => scope.starts_with("tab/"),
        "modal" => scope.starts_with("modal/"),
        "widget" => scope.starts_with("widget/"),
        other => scope == other,
    }
}

fn scope_matches(scope: &CommandScope, filter: Option<&str>) -> bool {
    let Some(filter) = filter else { return true };
    match (scope, filter) {
        (CommandScope::Global, "global") => true,
        (CommandScope::Tab(_), "tab") => true,
        (CommandScope::Modal(_), "modal") => true,
        (CommandScope::Widget(_), "widget") => true,
        (CommandScope::Tab(t), other) => other == format!("tab/{t}"),
        (CommandScope::Modal(m), other) => other == format!("modal/{m}"),
        (CommandScope::Widget(w), other) => other == format!("widget/{w}"),
        _ => false,
    }
}

fn print_table(defs: &[&CommandDef]) {
    use comfy_table::{presets, Cell, Color, ContentArrangement, Table};

    let mut table = Table::new();
    table
        .load_preset(presets::UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Name").fg(Color::Cyan),
            Cell::new("Scope").fg(Color::Cyan),
            Cell::new("Opens modal").fg(Color::Cyan),
            Cell::new("Description").fg(Color::Cyan),
        ]);
    for def in defs {
        table.add_row(vec![
            Cell::new(def.name),
            Cell::new(def.scope.as_str()),
            Cell::new(if def.opens_modal { "yes" } else { "no" }),
            Cell::new(def.description),
        ]);
    }
    println!("{table}");
}

fn print_ndjson(defs: &[&CommandDef]) -> Result<()> {
    for def in defs {
        println!("{}", serde_json::to_string(&serialize_def(def))?);
    }
    Ok(())
}

fn print_json(defs: &[&CommandDef]) -> Result<()> {
    let arr: Vec<serde_json::Value> = defs.iter().map(|d| serialize_def(d)).collect();
    println!("{}", serde_json::to_string_pretty(&arr)?);
    Ok(())
}

fn serialize_def(def: &CommandDef) -> serde_json::Value {
    use serde_json::json;
    let args: Vec<serde_json::Value> = def
        .args_schema
        .iter()
        .map(|a| {
            json!({
                "name": a.name,
                "description": a.description,
                "required": a.required,
            })
        })
        .collect();
    json!({
        "name": def.name,
        "description": def.description,
        "scope": def.scope.as_str(),
        "group": def.group,
        "opens_modal": def.opens_modal,
        "is_primary": def.is_primary,
        "args_schema": args,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::registry::CommandScope;

    #[test]
    fn scope_matches_filters() {
        assert!(scope_matches(&CommandScope::Global, None));
        assert!(scope_matches(&CommandScope::Global, Some("global")));
        assert!(!scope_matches(&CommandScope::Global, Some("tab")));
        assert!(scope_matches(&CommandScope::Tab("graph"), Some("tab")));
        assert!(scope_matches(
            &CommandScope::Tab("graph"),
            Some("tab/graph")
        ));
        assert!(!scope_matches(
            &CommandScope::Tab("graph"),
            Some("tab/tasks")
        ));
        assert!(scope_matches(
            &CommandScope::Modal("create"),
            Some("modal/create")
        ));
        assert!(scope_matches(
            &CommandScope::Widget("edit-buffer"),
            Some("widget")
        ));
        assert!(scope_matches(
            &CommandScope::Widget("edit-buffer"),
            Some("widget/edit-buffer")
        ));
        assert!(!scope_matches(
            &CommandScope::Widget("edit-buffer"),
            Some("widget/other")
        ));
    }

    #[test]
    fn registry_build_includes_known_commands() {
        let reg = registry::build();
        // App globals
        assert!(reg.lookup("app.quit").is_some());
        assert!(reg.lookup("app.next-tab").is_some());
        // Tab commands
        assert!(reg.lookup("graph.create-blank").is_some());
        assert!(reg.lookup("tasks.complete").is_some());
        assert!(reg.lookup("notes.open-picker").is_some());
        assert!(reg.lookup("timeblocks.reload").is_some());
        assert!(reg.lookup("gather.open-sources-manager").is_some());
        // Modal commands
        assert!(reg.lookup("create.confirm").is_some());
        assert!(reg.lookup("query-bar.apply").is_some());
        assert!(reg.lookup("periodic-leader.daily").is_some());
    }

    #[test]
    fn serialize_def_shape() {
        let reg = registry::build();
        let def = reg.lookup("app.quit").unwrap();
        let v = serialize_def(def);
        assert_eq!(v["name"], "app.quit");
        assert_eq!(v["scope"], "global");
        assert_eq!(v["opens_modal"], false);
    }

    #[test]
    fn check_keymap_clean_returns_no_errors() {
        // An empty config has no keymap section, so validate_keymap should return empty.
        let config = ft_core::config::Config::default();
        let errors = crate::tui::registry::validate_keymap(&config);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn check_keymap_bad_command_returns_errors() {
        use ft_core::config::{Config, KeymapConfig};
        use std::collections::HashMap;

        let mut scopes: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut global_scope = HashMap::new();
        global_scope.insert("q".to_string(), "app.nonexistent-command".to_string());
        scopes.insert("global".to_string(), global_scope);

        let config = Config {
            keymap: Some(KeymapConfig {
                strict: false,
                unbind: vec![],
                scopes,
            }),
            ..Config::default()
        };
        let errors = crate::tui::registry::validate_keymap(&config);
        assert!(!errors.is_empty(), "expected errors for unknown command");
        assert!(errors[0].contains("nonexistent-command"));
    }

    #[test]
    fn effective_bindings_shows_override_plain_list_does_not() {
        use ft_core::config::{Config, KeymapConfig};
        use std::collections::HashMap;

        // Override 'q' (app.quit) to app.next-tab in the global scope.
        let mut scopes: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut global_scope = HashMap::new();
        global_scope.insert("q".to_string(), "app.next-tab".to_string());
        scopes.insert("global".to_string(), global_scope);

        let config = Config {
            keymap: Some(KeymapConfig {
                strict: false,
                unbind: vec![],
                scopes,
            }),
            ..Config::default()
        };

        // effective_bindings should show the override for 'q' → app.next-tab
        let bindings = crate::tui::registry::effective_bindings(&config);
        let q_global = bindings
            .iter()
            .find(|(scope, chord, _)| scope == "global" && chord == "q");
        assert!(q_global.is_some(), "expected 'q' binding in global scope");
        assert_eq!(q_global.unwrap().2, "app.next-tab");

        // Plain registry does NOT show chords — it shows command defs without
        // user-applied overrides.
        let reg = registry::build();
        let quit_def = reg.lookup("app.quit").unwrap();
        assert_eq!(quit_def.name, "app.quit");
    }
}

//! `ft sources login|logout|list` and the `ft tasks sync` runner.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use ft_core::config::SourceCfg;
use ft_core::sources::{credentials, http::Secret, run, sync};
use ft_core::vault::Vault;

use super::tasks::MoveFormat;

#[derive(Args)]
pub struct SourcesArgs {
    #[command(subcommand)]
    pub command: SourcesCommand,
}

#[derive(Subcommand)]
pub enum SourcesCommand {
    /// Store a source's API token in the OS keychain (verified first).
    /// Reads the token from stdin when it isn't a terminal.
    Login {
        /// Source name (`[sources.<name>]`).
        name: String,
    },
    /// Remove a source's stored token from the OS keychain.
    Logout {
        /// Source name (`[sources.<name>]`).
        name: String,
    },
    /// List configured sources, where their credentials come from, and
    /// the last local sync attempt.
    List {
        #[arg(long, value_enum, default_value_t = MoveFormat::Text)]
        format: MoveFormat,
    },
}

pub fn run(args: SourcesArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = super::common::discover_vault(vault_flag)?;
    match args.command {
        SourcesCommand::Login { name } => login(&vault, &name),
        SourcesCommand::Logout { name } => {
            let cfg = source_cfg(&vault, &name)?;
            credentials::keychain_delete(&name, cfg).map_err(|e| anyhow!(e))?;
            println!("Removed the stored token for {name}");
            Ok(ExitCode::SUCCESS)
        }
        SourcesCommand::List { format } => list(&vault, format),
    }
}

fn source_cfg<'a>(vault: &'a Vault, name: &str) -> Result<&'a SourceCfg> {
    vault.config.config.sources.get(name).ok_or_else(|| {
        let known: Vec<&str> = vault
            .config
            .config
            .sources
            .keys()
            .map(String::as_str)
            .collect();
        anyhow!(
            "no source `{name}` configured (known: {})",
            if known.is_empty() {
                "none".to_string()
            } else {
                known.join(", ")
            }
        )
    })
}

fn login(vault: &Vault, name: &str) -> Result<ExitCode> {
    let cfg = source_cfg(vault, name)?;
    let token = if std::io::stdin().is_terminal() {
        dialoguer::Password::new()
            .with_prompt(format!("API token for {name}"))
            .interact()
            .context("reading token")?
    } else {
        let mut s = String::new();
        std::io::stdin()
            .read_line(&mut s)
            .context("reading token from stdin")?;
        s
    };
    let token = Secret::new(token.trim());
    if token.expose().is_empty() {
        return Err(anyhow!("empty token"));
    }
    let target = cfg.base_url.as_deref().unwrap_or("https://api.github.com");
    let who = run::verify_token(cfg, &token).map_err(|f| match f {
        run::FetchFailure::Unauthorized(code) => anyhow!("token rejected by {target} ({code})"),
        other => anyhow!(
            "could not verify the token against {target}: {}",
            other.message(name)
        ),
    })?;
    credentials::keychain_store(name, cfg, &token).map_err(|e| anyhow!(e))?;
    println!("Logged in to {name} as {who} (token stored in the keychain)");
    Ok(ExitCode::SUCCESS)
}

fn list(vault: &Vault, format: MoveFormat) -> Result<ExitCode> {
    let log = run::AttemptLog::load(&vault.path);
    let rows: Vec<serde_json::Value> = vault
        .config
        .config
        .sources
        .iter()
        .map(|(name, cfg)| {
            let creds = credentials::resolve(name, cfg).map_or("none", |(_, o)| o.as_str());
            let attempt = log.sources.get(name);
            serde_json::json!({
                "name": name,
                "kind": format!("{:?}", cfg.kind).to_lowercase(),
                "base_url": cfg.base_url,
                "credentials": creds,
                "last_attempt": attempt.and_then(|a| a.last_attempt.clone()),
                "last_status": attempt.and_then(|a| a.last_status.clone()),
            })
        })
        .collect();
    match format {
        MoveFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
        MoveFormat::Text if rows.is_empty() => println!("no sources configured"),
        MoveFormat::Text => {
            for r in &rows {
                let s = |k: &str| r[k].as_str().unwrap_or("-").to_string();
                println!(
                    "{}  {}  {}  credentials: {}  last: {} {}",
                    s("name"),
                    s("kind"),
                    s("base_url"),
                    s("credentials"),
                    s("last_attempt"),
                    s("last_status")
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `ft tasks sync`: fetch, plan, and apply (or diff) each selected source.
pub fn run_sync(
    vault: &Vault,
    only: &[String],
    dry_run: bool,
    no_push: bool,
    format: MoveFormat,
) -> Result<ExitCode> {
    let sources = &vault.config.config.sources;
    for name in only {
        source_cfg(vault, name)?;
    }
    if sources.is_empty() {
        return Err(anyhow!(
            "no sources configured — add a [sources.<name>] section to .ft/config.toml"
        ));
    }
    let scan = vault.scan();
    let templates_dir = vault.templates_dir();
    let mut failed = false;
    let mut rows = Vec::new();
    for (name, cfg) in sources {
        if !only.is_empty() && !only.contains(name) {
            continue;
        }
        // A dry run never writes — remotely either.
        let push = !dry_run && !no_push;
        let outcome = run::run_source(name, cfg, &scan, push);
        if !dry_run {
            let status = outcome.as_ref().map_or_else(|f| f.status(), |_| "ok");
            run::AttemptLog::record(&vault.path, name, status);
        }
        let outcome = match outcome {
            Ok(o) => o,
            Err(f) => {
                failed |= !f.is_benign();
                if format == MoveFormat::Text {
                    println!("{name}: {}", f.message(name));
                }
                rows.push(serde_json::json!({
                    "source": name, "status": f.status(), "created": 0, "updated": 0,
                    "pending": 0, "conflicts": 0, "out_of_scope": 0, "message": f.message(name),
                }));
                continue;
            }
        };
        let ctx = sync::SyncCtx {
            vault_root: &vault.path,
            templates_dir: &templates_dir,
            scan: &scan,
            source: name,
            cfg,
            format: vault.task_format(),
            today: ft_core::dates::today(),
        };
        let mut plan = sync::plan_sync(&ctx, &outcome.fetched);
        plan.report.pushed = outcome.push.pushed.clone();
        plan.report.push_failures = outcome.push.failures.clone();
        if push && cfg.push {
            run::AttemptLog::record_push_failures(&vault.path, name, &outcome.push.failures);
        }
        if dry_run && format == MoveFormat::Text {
            for e in plan.edits.iter().filter(|e| e.original != e.new) {
                super::tasks::print_diff(&vault.relativize(&e.path), &e.original, &e.new);
            }
        } else if !dry_run {
            sync::apply_sync_plan(&plan).with_context(|| format!("writing {name} notes"))?;
        }
        let r = &plan.report;
        if format == MoveFormat::Text {
            let mut line = format!(
                "{name}: {} new · {} updated · {} pending push · {} conflicts",
                r.created.len(),
                r.updated.len(),
                r.pending.len(),
                r.conflicts.len()
            );
            if !r.out_of_scope.is_empty() {
                line.push_str(&format!(" · {} out of scope", r.out_of_scope.len()));
            }
            if !r.pushed.is_empty() {
                line.push_str(&format!(" · {} pushed", r.pushed.len()));
            }
            println!("{line}");
            for (key, reason) in &r.push_failures {
                println!("  push failed {key}: {reason}");
            }
            for (key, local, remote) in &r.conflicts {
                println!(
                    "  conflict {key} local={} remote={}",
                    local.as_str(),
                    remote.as_str()
                );
            }
            for w in &r.warnings {
                println!("  warning: {w}");
            }
        }
        rows.push(serde_json::json!({
            "source": name, "status": "ok",
            "created": r.created.len(), "updated": r.updated.len(),
            "pending": r.pending.len(), "conflicts": r.conflicts.len(),
            "out_of_scope": r.out_of_scope.len(),
            "pushed": r.pushed.len(),
            "push_failures": r.push_failures.iter()
                .map(|(k, reason)| serde_json::json!({"key": k, "reason": reason}))
                .collect::<Vec<_>>(),
            "message": r.warnings.join("; "),
        }));
    }
    if format == MoveFormat::Json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    }
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// `ft tasks sync --resolve | --take`: resolve stored conflicts locally.
pub fn run_resolve(vault: &Vault, only: &[String], take: &[String]) -> Result<ExitCode> {
    let scripted = match take.split_first() {
        None => None,
        Some((side, keys)) => {
            let side = match side.as_str() {
                "local" => sync::Take::Local,
                "remote" => sync::Take::Remote,
                other => return Err(anyhow!("--take expects `local` or `remote`, got `{other}`")),
            };
            Some((side, keys.to_vec()))
        }
    };
    let scan = vault.scan();
    let mut conflicts = Vec::new();
    for f in &scan.files {
        let Some(src) = f
            .frontmatter
            .as_deref()
            .and_then(ft_core::frontmatter::ft_source_in)
        else {
            continue;
        };
        if src.conflict.is_none() || (!only.is_empty() && !only.contains(&src.name)) {
            continue;
        }
        let local = scan
            .tasks
            .iter()
            .find(|t| t.source_file == f.rel && t.id.as_deref() == Some(src.key.as_str()))
            .map_or("?", |t| t.status.as_str());
        conflicts.push((
            f.rel.clone(),
            src.key.clone(),
            local,
            src.conflict.clone().unwrap_or_default(),
        ));
    }
    if let Some((_, keys)) = &scripted {
        for k in keys {
            if !conflicts.iter().any(|(_, key, _, _)| key == k) {
                return Err(anyhow!("{k} has no stored conflict"));
            }
        }
    }
    if conflicts.is_empty() {
        println!("no conflicts");
        return Ok(ExitCode::SUCCESS);
    }
    let mut resolved = 0;
    for (rel, key, local, remote) in &conflicts {
        let choice = match &scripted {
            Some((side, keys)) if keys.contains(key) => Some(*side),
            Some(_) => None,
            None => {
                let items = [
                    format!("keep local ({local})"),
                    format!("take remote ({remote})"),
                    "skip".to_string(),
                ];
                let pick = dialoguer::Select::new()
                    .with_prompt(format!("{key}: local={local} remote={remote}"))
                    .items(&items)
                    .default(2)
                    .interact()
                    .context("reading choice")?;
                match pick {
                    0 => Some(sync::Take::Local),
                    1 => Some(sync::Take::Remote),
                    _ => None,
                }
            }
        };
        let Some(take) = choice else { continue };
        let path = vault.path.join(rel);
        let content =
            std::fs::read_to_string(&path).with_context(|| format!("reading {}", rel.display()))?;
        let new =
            sync::resolve_conflict(&content, take, ft_core::dates::today(), vault.task_format())
                .ok_or_else(|| anyhow!("{key}: conflict disappeared — rescan"))?;
        ft_core::fs::write_atomic(&path, &new)?;
        resolved += 1;
        let side = if take == sync::Take::Local {
            "local (pending push)"
        } else {
            "remote"
        };
        println!("{key}: took {side}");
    }
    println!("resolved {resolved} of {} conflict(s)", conflicts.len());
    Ok(ExitCode::SUCCESS)
}

/// `ft tasks promote`.
pub fn run_promote(
    vault: &Vault,
    selector: &str,
    parent: Option<&str>,
    format: MoveFormat,
) -> Result<ExitCode> {
    use ft_core::sources::promote::{self, PromoteError};
    let scan = vault.scan();
    let task = super::tasks::resolve_unique("selector", selector, &scan.tasks)?;
    let target = match promote::check(&scan, task, &vault.config.config.sources, parent) {
        Ok(t) => t,
        Err(e @ PromoteError::ChooseParent { .. }) => {
            let PromoteError::ChooseParent { candidates } = &e else {
                unreachable!()
            };
            let detail = candidates
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "key": c.key,
                        "path": c.path,
                        "line": c.line,
                        "description": c.description,
                    })
                })
                .collect();
            return Err(super::common::ChoiceError {
                message: e.to_string(),
                key: "candidates",
                detail: serde_json::Value::Array(detail),
            }
            .into());
        }
        Err(e) => return Err(anyhow!(e.to_string())),
    };
    let cfg = source_cfg(vault, &target.source)?;
    let new = promote::new_issue(task);
    let (created, push_failure) =
        run::promote(&target.source, cfg, &target.parent, &new, task.status).map_err(
            |f| match f {
                run::FetchFailure::Offline(_) => {
                    anyhow!("{}: offline — promote needs a connection", target.source)
                }
                other => anyhow!("{}: {}", target.source, other.message(&target.source)),
            },
        )?;
    let plan = promote::plan_local(
        &vault.path,
        &vault.templates_dir(),
        &scan,
        &target.source,
        cfg,
        vault.task_format(),
        ft_core::dates::today(),
        task,
        &created,
        &target.parent.key,
    )
    .and_then(|p| promote::apply_guarded(&p.edits).map(|()| p))
    .map_err(|e| anyhow!("created {} but couldn't update the vault: {e}", created.key))?;
    if let Some(reason) = &push_failure {
        eprintln!("warning: couldn't set {}'s status: {reason}", created.key);
    }
    match format {
        MoveFormat::Text => println!(
            "Promoted to {} ({}) → {}",
            created.key,
            created.url,
            plan.note.display()
        ),
        MoveFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "key": created.key,
                "url": created.url,
                "note": plan.note,
                "moved_children": plan.moved_lines,
            }))?
        ),
    }
    Ok(ExitCode::SUCCESS)
}

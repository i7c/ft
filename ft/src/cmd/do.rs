//! `ft do <command> [--arg key=value ...]` — headless command dispatch.
//!
//! Looks up the command in the central `CommandRegistry`, validates
//! args against `CommandDef::args_schema`, and dispatches via a
//! shared headless handler when one exists. Commands tagged
//! `opens_modal = true` are rejected with a clear "this needs the
//! TUI" message and exit code 2.
//!
//! Headless coverage today: `tasks.complete-by-id` (factored in
//! commands-and-keymaps §9.4-9.5). Other registry commands without a
//! headless equivalent — mostly TUI-state-mutating verbs like cursor
//! navigation, view switching, multi-selection toggles — return an
//! explicit "no headless handler" error (exit 3). Add new handlers
//! here as their underlying ft-core operations become atomic enough
//! to call directly.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Result};
use clap::Args;
use ft_core::{
    selector::{self, Selector},
    task::{
        ops::{self, CancelError, CompleteError, CompleteOptions, UpdateError},
        Priority, Task,
    },
};

use crate::tui::registry::{self, CommandDef};

#[derive(Args, Debug)]
pub struct DoArgs {
    /// Command name (`<context>.<verb>`), e.g. `tasks.complete-by-id`.
    pub command: String,

    /// Repeated `--arg key=value` pairs. Validated against the
    /// command's `args_schema`; missing required args produce an
    /// error.
    #[arg(long = "arg", value_name = "KEY=VALUE")]
    pub args: Vec<String>,

    /// Output format for success: `text` (human-readable, default)
    /// or `json` (`{"command":"…","outcome":"ok","details":…}`).
    /// Errors honour the top-level `--json-errors` flag.
    #[arg(long, default_value = "text")]
    pub format: String,
}

/// Exit code mapping:
/// - 0 — success
/// - 2 — usage error (unknown command, missing arg, modal-opening
///   command, unparseable arg)
/// - 3 — no headless handler exists yet for this command (a
///   deferred follow-up; the command is in the registry but the
///   dispatch path hasn't been factored out of the TUI).
pub fn run(args: DoArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let reg = registry::build();
    let Some(def) = reg.lookup(&args.command) else {
        anyhow::bail!("unknown command '{}'; see 'ft commands list'", args.command);
    };
    if def.opens_modal {
        anyhow::bail!(
            "command '{}' opens an interactive flow; use 'ft tui' (v1 limitation)",
            args.command
        );
    }
    let parsed = parse_args(&args.args)?;
    validate_args(def, &parsed)?;

    match args.command.as_str() {
        "tasks.complete-by-id" => {
            let id = arg_value(&parsed, "id").expect("validated above");
            handle_tasks_complete_by_id(id, vault_flag, &args.format)
        }
        "tasks.cancel-by-id" => {
            let id = arg_value(&parsed, "id").expect("validated above");
            let on = arg_value(&parsed, "on").map(String::from);
            handle_tasks_cancel_by_id(id, on, vault_flag, &args.format)
        }
        "tasks.edit-by-id" => {
            let id = arg_value(&parsed, "id").expect("validated above");
            let fields = parsed
                .iter()
                .filter(|(k, _)| k != "id")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            handle_tasks_edit_by_id(id, fields, vault_flag, &args.format)
        }
        _ => {
            // Registered command without a headless handler. Surface
            // an explicit error rather than silently no-op-ing.
            Err(anyhow!(
                "command '{}' has no headless handler yet; this command is registered \
                 for `?` overlay / docs / TUI dispatch but cannot be invoked headlessly",
                args.command
            ))
        }
    }
}

fn arg_value<'a>(parsed: &'a [(String, String)], key: &str) -> Option<&'a str> {
    parsed
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// Headless handler for `tasks.complete-by-id`. Discovers the vault,
/// scans for the task with `id`, and calls
/// [`ft_core::task::ops::complete_task`] (the same path the TUI's
/// `tasks.complete` action takes).
fn handle_tasks_complete_by_id(
    id: &str,
    vault_flag: Option<PathBuf>,
    format: &str,
) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;

    let today = ft_core::dates::today();

    let scan = vault.scan();
    for err in &scan.errors {
        tracing::warn!("{}", err);
    }

    let sel = Selector::Id(id.to_string());
    let matches: Vec<&Task> = selector::resolve(&scan.tasks, &sel);
    let task = match matches.as_slice() {
        [] => anyhow::bail!("no task with id `{id}`"),
        [t] => *t,
        many => anyhow::bail!(
            "selector `id={id}` matched {} tasks (expected exactly one)",
            many.len()
        ),
    };

    let absolute_path = vault.path.join(&task.source_file);
    let outcome = ops::complete_task(
        &absolute_path,
        task.source_line,
        CompleteOptions { on: today },
    )
    .map_err(|e| translate_complete_error(e, &vault.path))?;

    let rel = vault.relativize(&absolute_path);

    if format == "json" {
        let mut obj = serde_json::json!({
            "command": "tasks.complete-by-id",
            "outcome": "ok",
            "details": {
                "file": rel.display().to_string(),
                "line": outcome.completed_line,
                "serialized": outcome.completed_serialized,
            },
        });
        if let Some(next) = outcome.next_instance.as_ref() {
            obj["details"]["next_instance"] = serde_json::json!({
                "line": next.line,
                "serialized": next.serialized,
            });
        }
        println!("{}", serde_json::to_string(&obj)?);
    } else {
        println!(
            "Completed {}:{}\n  {}",
            rel.display(),
            outcome.completed_line,
            outcome.completed_serialized
        );
        if let Some(next) = outcome.next_instance {
            println!(
                "Recurring: next instance at {}:{}\n  {}",
                rel.display(),
                next.line,
                next.serialized
            );
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// Translate a complete-task error into a vault-relative, user-facing
/// message. Mirrors `cmd::tasks::translate_complete_error`; duplicated
/// rather than re-exported to keep the two CLI surfaces decoupled.
fn translate_complete_error(e: CompleteError, vault_root: &std::path::Path) -> anyhow::Error {
    use CompleteError::*;
    match e {
        Read { path, source } => {
            let rel = path.strip_prefix(vault_root).unwrap_or(&path);
            anyhow!("could not read {}: {source}", rel.display())
        }
        LineMissing {
            path,
            line,
            file_lines,
        } => {
            let rel = path.strip_prefix(vault_root).unwrap_or(&path);
            anyhow!(
                "line {line} not found in {} ({file_lines} lines)",
                rel.display()
            )
        }
        NotATask { path, line } => {
            let rel = path.strip_prefix(vault_root).unwrap_or(&path);
            anyhow!("line {line} in {} is not a task", rel.display())
        }
        AlreadyDone { path, line, done } => {
            let rel = path.strip_prefix(vault_root).unwrap_or(&path);
            anyhow!(
                "task at {}:{} is already done (on {done})",
                rel.display(),
                line
            )
        }
        other => anyhow!("{other}"),
    }
}

/// Resolve a single task by id selector, erroring on zero/multi matches.
/// Shared by the `*-by-id` headless handlers.
fn resolve_single_task_by_id<'a>(tasks: &'a [Task], id: &str) -> Result<&'a Task> {
    let sel = Selector::Id(id.to_string());
    match selector::resolve(tasks, &sel).as_slice() {
        [] => anyhow::bail!("no task with id `{id}`"),
        [t] => Ok(*t),
        many => anyhow::bail!(
            "selector `id={id}` matched {} tasks (expected exactly one)",
            many.len()
        ),
    }
}

/// Headless handler for `tasks.cancel-by-id`. Mirrors
/// `handle_tasks_complete_by_id` but calls `ops::cancel_task`.
fn handle_tasks_cancel_by_id(
    id: &str,
    on: Option<String>,
    vault_flag: Option<PathBuf>,
    format: &str,
) -> Result<ExitCode> {
    use ft_core::dates;
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let today = dates::today();
    let on = match on.as_deref() {
        Some(s) => dates::parse(s, today).map_err(|e| anyhow!("--arg on: {e}"))?,
        None => today,
    };
    let scan = vault.scan();
    for err in &scan.errors {
        tracing::warn!("{}", err);
    }
    let task = resolve_single_task_by_id(&scan.tasks, id)?;
    let abs = vault.path.join(&task.source_file);
    let cancelled = ops::cancel_task(&abs, task.source_line, on)
        .map_err(|e| translate_cancel_error(e, &vault.path))?;
    let rel = vault.relativize(&abs);
    if format == "json" {
        println!(
            "{}",
            serde_json::json!({
                "command": "tasks.cancel-by-id",
                "outcome": "ok",
                "details": {
                    "file": rel.display().to_string(),
                    "line": task.source_line,
                    "description": cancelled.description,
                }
            })
        );
    } else {
        println!(
            "Cancelled {}:{}  {}",
            rel.display(),
            task.source_line,
            cancelled.description
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Headless handler for `tasks.edit-by-id`. Applies field overrides
/// (due/scheduled/priority/description) via `ops::update_task_line`.
/// `tags` is parsed space-separated; `none` clears. `due`/`scheduled`
/// accept a date or `none`.
fn handle_tasks_edit_by_id(
    id: &str,
    fields: Vec<(String, String)>,
    vault_flag: Option<PathBuf>,
    format: &str,
) -> Result<ExitCode> {
    use ft_core::dates;
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let today = dates::today();
    let scan = vault.scan();
    for err in &scan.errors {
        tracing::warn!("{}", err);
    }
    let task = resolve_single_task_by_id(&scan.tasks, id)?;
    let abs = vault.path.join(&task.source_file);

    let due = field_opt_date(get(&fields, "due"), today)?;
    let scheduled = field_opt_date(get(&fields, "scheduled"), today)?;
    let priority = match get(&fields, "priority") {
        None => None,
        Some(v) => Some(parse_priority(v)?),
    };
    let description = get(&fields, "description").map(String::from);

    if due.is_none() && scheduled.is_none() && priority.is_none() && description.is_none() {
        anyhow::bail!("no fields given — pass --arg due=/scheduled=/priority=/description=");
    }

    let updated = ops::update_task_line(&abs, task.source_line, |t| {
        if let Some(d) = due {
            t.due = d;
        }
        if let Some(s) = scheduled {
            t.scheduled = s;
        }
        if let Some(p) = priority {
            t.priority = p;
        }
        if let Some(desc) = description.clone() {
            t.description = desc;
        }
    })
    .map_err(|e| translate_update_error(e, &vault.path))?;

    let rel = vault.relativize(&abs);
    if format == "json" {
        println!(
            "{}",
            serde_json::json!({
                "command": "tasks.edit-by-id",
                "outcome": "ok",
                "details": {
                    "file": rel.display().to_string(),
                    "line": task.source_line,
                    "description": updated.description,
                }
            })
        );
    } else {
        println!(
            "Edited {}:{}  {}",
            rel.display(),
            task.source_line,
            updated.description
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn get<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn field_opt_date(
    s: Option<&str>,
    today: chrono::NaiveDate,
) -> Result<Option<Option<chrono::NaiveDate>>> {
    use ft_core::dates;
    match s {
        None => Ok(None),
        Some(v) if v.eq_ignore_ascii_case("none") => Ok(Some(None)),
        Some(v) => Ok(Some(Some(
            dates::parse(v, today).map_err(|e| anyhow!("date: {e}"))?,
        ))),
    }
}

fn parse_priority(s: &str) -> Result<Option<Priority>> {
    if s.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    match s.to_ascii_lowercase().as_str() {
        "highest" => Ok(Some(Priority::Highest)),
        "high" => Ok(Some(Priority::High)),
        "medium" => Ok(Some(Priority::Medium)),
        "low" => Ok(Some(Priority::Low)),
        "lowest" => Ok(Some(Priority::Lowest)),
        other => anyhow::bail!(
            "priority `{other}` not recognized (try none / low / medium / high / highest)"
        ),
    }
}

fn translate_cancel_error(e: CancelError, vault_root: &std::path::Path) -> anyhow::Error {
    use CancelError::*;
    match e {
        AlreadyCancelled {
            path,
            line,
            cancelled,
        } => {
            let rel = path.strip_prefix(vault_root).unwrap_or(&path);
            anyhow!(
                "task at {}:{} is already cancelled (on {cancelled})",
                rel.display(),
                line
            )
        }
        Update(u) => translate_update_error(u, vault_root),
    }
}

fn translate_update_error(e: UpdateError, vault_root: &std::path::Path) -> anyhow::Error {
    use UpdateError::*;
    match e {
        Read { path, source } => {
            let rel = path.strip_prefix(vault_root).unwrap_or(&path);
            anyhow!("could not read {}: {source}", rel.display())
        }
        LineMissing {
            path,
            line,
            file_lines,
        } => {
            let rel = path.strip_prefix(vault_root).unwrap_or(&path);
            anyhow!(
                "line {line} not found in {} ({file_lines} lines)",
                rel.display()
            )
        }
        NotATask { path, line } => {
            let rel = path.strip_prefix(vault_root).unwrap_or(&path);
            anyhow!("line {line} in {} is not a task", rel.display())
        }
        Write { source } => anyhow!("write failed: {source}"),
    }
}

/// Parse a list of `KEY=VALUE` strings into a sorted-by-key list of
/// pairs. Returns an error on the first malformed entry (missing
/// `=`).
fn parse_args(raw: &[String]) -> Result<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::with_capacity(raw.len());
    for entry in raw {
        let Some((k, v)) = entry.split_once('=') else {
            anyhow::bail!("argument '{entry}' is not in KEY=VALUE form (use `--arg name=value`)");
        };
        out.push((k.to_string(), v.to_string()));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Verify every required arg in `def.args_schema` has a value in
/// `parsed`. Unknown args (in parsed but not the schema) are
/// allowed — forward-compatible with `CommandDef` adding optional
/// args later.
fn validate_args(def: &CommandDef, parsed: &[(String, String)]) -> Result<()> {
    for spec in def.args_schema {
        if spec.required && !parsed.iter().any(|(k, _)| k == spec.name) {
            anyhow::bail!(
                "command '{}' is missing required arg '{}' ({})",
                def.name,
                spec.name,
                spec.description
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_returns_empty_for_no_inputs() {
        let v = parse_args(&[]).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn parse_args_sorts_by_key() {
        let v = parse_args(&["z=1".to_string(), "a=2".to_string(), "m=3".to_string()]).unwrap();
        assert_eq!(
            v,
            vec![
                ("a".to_string(), "2".to_string()),
                ("m".to_string(), "3".to_string()),
                ("z".to_string(), "1".to_string()),
            ]
        );
    }

    #[test]
    fn parse_args_rejects_missing_equals() {
        let err = parse_args(&["bare-flag".to_string()]).unwrap_err();
        assert!(err.to_string().contains("KEY=VALUE"));
    }

    #[test]
    fn parse_args_accepts_value_with_equals() {
        // `--arg key=value=with=equals` keeps everything after the
        // first `=` as the value.
        let v = parse_args(&["k=v=w".to_string()]).unwrap();
        assert_eq!(v, vec![("k".to_string(), "v=w".to_string())]);
    }

    #[test]
    fn validate_args_passes_when_required_supplied() {
        let reg = registry::build();
        let def = reg.lookup("app.switch-tab").unwrap();
        // app.switch-tab needs `index`.
        let parsed = vec![("index".to_string(), "0".to_string())];
        validate_args(def, &parsed).unwrap();
    }

    #[test]
    fn validate_args_fails_when_required_missing() {
        let reg = registry::build();
        let def = reg.lookup("app.switch-tab").unwrap();
        let err = validate_args(def, &[]).unwrap_err();
        assert!(err.to_string().contains("missing required arg 'index'"));
    }

    #[test]
    fn run_rejects_unknown_command() {
        let args = DoArgs {
            command: "nonexistent.command".into(),
            args: vec![],
            format: "text".into(),
        };
        let err = run(args, None).unwrap_err();
        assert!(err.to_string().contains("unknown command"));
    }

    #[test]
    fn run_rejects_modal_opening_command() {
        let args = DoArgs {
            command: "graph.create-blank".into(),
            args: vec![],
            format: "text".into(),
        };
        let err = run(args, None).unwrap_err();
        assert!(
            err.to_string().contains("interactive flow"),
            "expected interactive-flow rejection, got: {err}"
        );
    }

    #[test]
    fn run_rejects_command_with_missing_required_arg() {
        let args = DoArgs {
            command: "app.switch-tab".into(),
            args: vec![],
            format: "text".into(),
        };
        let err = run(args, None).unwrap_err();
        assert!(err.to_string().contains("missing required arg"));
    }

    #[test]
    fn run_returns_no_handler_for_unhandled_non_modal_command() {
        // `app.switch-tab` is registered but TUI-state-mutating;
        // headless dispatch deliberately rejects it.
        let args = DoArgs {
            command: "app.switch-tab".into(),
            args: vec!["index=0".to_string()],
            format: "text".into(),
        };
        let err = run(args, None).unwrap_err();
        assert!(
            err.to_string().contains("no headless handler"),
            "expected no-handler rejection, got: {err}"
        );
    }

    /// §9.4-9.5: end-to-end headless completion via id selector.
    /// Mirrors the spec scenario `ft do tasks.complete-by-id --arg id=xyz123`.
    #[test]
    fn run_completes_task_by_id_headlessly() {
        let dir = assert_fs::TempDir::new().unwrap();
        let vault_path = dir.path().join("v");
        std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
        std::fs::write(
            vault_path.join("tasks.md"),
            "- [ ] Buy milk 🆔 abc123 📅 2026-05-10\n",
        )
        .unwrap();

        // FT_TODAY pins the done-date so this test is reproducible.
        std::env::set_var("FT_TODAY", "2026-05-12");

        let args = DoArgs {
            command: "tasks.complete-by-id".into(),
            args: vec!["id=abc123".to_string()],
            format: "text".into(),
        };
        let code = run(args, Some(vault_path.clone())).expect("run should succeed");
        assert_eq!(code, ExitCode::SUCCESS);

        let content = std::fs::read_to_string(vault_path.join("tasks.md")).unwrap();
        assert!(
            content.contains("[x]"),
            "expected the task to be marked done, got: {content}"
        );
        assert!(
            content.contains("✅ 2026-05-12"),
            "expected today's done-date to be recorded, got: {content}"
        );

        std::env::remove_var("FT_TODAY");
    }

    #[test]
    fn run_reports_unknown_id_clearly() {
        let dir = assert_fs::TempDir::new().unwrap();
        let vault_path = dir.path().join("v");
        std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
        std::fs::write(vault_path.join("tasks.md"), "- [ ] Buy milk\n").unwrap();

        let args = DoArgs {
            command: "tasks.complete-by-id".into(),
            args: vec!["id=nope".to_string()],
            format: "text".into(),
        };
        let err = run(args, Some(vault_path)).unwrap_err();
        assert!(
            err.to_string().contains("no task with id"),
            "expected no-match error, got: {err}"
        );
    }

    #[test]
    fn run_cancels_task_by_id_headlessly() {
        let dir = assert_fs::TempDir::new().unwrap();
        let vault_path = dir.path().join("v");
        std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
        std::fs::write(
            vault_path.join("tasks.md"),
            "- [ ] Buy milk 🆔 abc123 📅 2026-05-10\n",
        )
        .unwrap();

        std::env::set_var("FT_TODAY", "2026-05-12");

        let args = DoArgs {
            command: "tasks.cancel-by-id".into(),
            args: vec!["id=abc123".to_string()],
            format: "text".into(),
        };
        let code = run(args, Some(vault_path.clone())).expect("run should succeed");
        assert_eq!(code, ExitCode::SUCCESS);

        let content = std::fs::read_to_string(vault_path.join("tasks.md")).unwrap();
        assert!(
            content.contains("[-]"),
            "expected the task to be marked cancelled, got: {content}"
        );
        assert!(
            content.contains("❌ 2026-05-12"),
            "expected today's cancelled-date to be recorded, got: {content}"
        );

        std::env::remove_var("FT_TODAY");
    }

    #[test]
    fn run_edits_task_due_by_id_headlessly() {
        let dir = assert_fs::TempDir::new().unwrap();
        let vault_path = dir.path().join("v");
        std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
        std::fs::write(vault_path.join("tasks.md"), "- [ ] Buy milk 🆔 abc123\n").unwrap();

        std::env::set_var("FT_TODAY", "2026-05-12");

        let args = DoArgs {
            command: "tasks.edit-by-id".into(),
            args: vec!["id=abc123".to_string(), "due=2026-07-01".to_string()],
            format: "text".into(),
        };
        let code = run(args, Some(vault_path.clone())).expect("run should succeed");
        assert_eq!(code, ExitCode::SUCCESS);

        let content = std::fs::read_to_string(vault_path.join("tasks.md")).unwrap();
        assert!(
            content.contains("📅 2026-07-01"),
            "expected due date set, got: {content}"
        );

        std::env::remove_var("FT_TODAY");
    }

    #[test]
    fn run_edits_task_priority_by_id_headlessly() {
        let dir = assert_fs::TempDir::new().unwrap();
        let vault_path = dir.path().join("v");
        std::fs::create_dir_all(vault_path.join(".obsidian")).unwrap();
        std::fs::write(vault_path.join("tasks.md"), "- [ ] Buy milk 🆔 abc123\n").unwrap();

        let args = DoArgs {
            command: "tasks.edit-by-id".into(),
            args: vec!["id=abc123".to_string(), "priority=high".to_string()],
            format: "text".into(),
        };
        let code = run(args, Some(vault_path.clone())).expect("run should succeed");
        assert_eq!(code, ExitCode::SUCCESS);

        let content = std::fs::read_to_string(vault_path.join("tasks.md")).unwrap();
        assert!(
            content.contains("⏫"),
            "expected high priority emoji, got: {content}"
        );
    }
}

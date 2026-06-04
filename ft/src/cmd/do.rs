//! `ft do <command> [--arg key=value ...]` — headless command dispatch.
//!
//! Looks up the command in the central `CommandRegistry`, validates
//! args against `CommandDef::args_schema`, and dispatches via a
//! shared headless handler when one exists. Commands tagged
//! `opens_modal = true` are rejected with a clear "this needs the
//! TUI" message and exit code 2.
//!
//! V1 limitation: most non-modal commands in the registry are
//! TUI-state-mutating (cursor navigation, view switching, multi-
//! selection toggles) and have no headless equivalent — `ft do` for
//! those returns an explicit "no headless handler" error with exit
//! code 3. Factoring shared handlers out of each tab's
//! `dispatch_command` is a follow-up (commands-and-keymaps §9.4–9.5).

use anyhow::Result;
use clap::Args;

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
pub fn run(args: DoArgs) -> Result<std::process::ExitCode> {
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

    // V1: no shared headless handlers exist yet. Surface an explicit
    // error rather than silently no-op-ing — users get a clear next
    // step (file an issue, or wait for §9.4-9.5).
    anyhow::bail!(
        "command '{}' has no headless handler yet; this is a known v1 gap \
         (commands-and-keymaps §9.4–9.5)",
        args.command
    );
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
        let err = run(args).unwrap_err();
        assert!(err.to_string().contains("unknown command"));
    }

    #[test]
    fn run_rejects_modal_opening_command() {
        let args = DoArgs {
            command: "graph.create-blank".into(),
            args: vec![],
            format: "text".into(),
        };
        let err = run(args).unwrap_err();
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
        let err = run(args).unwrap_err();
        assert!(err.to_string().contains("missing required arg"));
    }

    #[test]
    fn run_returns_no_handler_for_validated_non_modal_command() {
        let args = DoArgs {
            command: "app.switch-tab".into(),
            args: vec!["index=0".to_string()],
            format: "text".into(),
        };
        let err = run(args).unwrap_err();
        assert!(
            err.to_string().contains("no headless handler"),
            "expected the §9.4-9.5 deferral message, got: {err}"
        );
    }
}

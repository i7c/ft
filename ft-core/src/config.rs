use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use figment::{
    providers::{Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Top-level `ft` configuration.
///
/// Unknown keys are rejected with a clear error message so typos are caught
/// immediately rather than silently ignored.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Default vault path (valid in user config only).
    /// Used as last-resort in vault discovery when no other signal is present.
    pub default_vault: Option<String>,
    /// Default file for new tasks, relative to vault root.
    pub default_task_location: Option<String>,
    /// Per-period configuration for periodic notes (daily/weekly/monthly/
    /// quarterly/yearly). Only configured periods are accessible from the
    /// CLI and TUI; unset periods are surfaced as a "not configured" error
    /// at use time.
    #[serde(default)]
    pub periodic_notes: PeriodicNotes,
    /// Glob patterns (relative to vault root) to exclude from scanning.
    #[serde(default)]
    pub ignored_paths: Vec<String>,
    /// Named task queries (presets). Keys are preset names; values are DSL strings.
    #[serde(default)]
    pub presets: HashMap<String, String>,
    /// Note-creation settings.
    #[serde(default)]
    pub notes: Notes,
}

/// Settings for `ft notes create` and the TUI create flows.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Notes {
    /// Folder (vault-relative) holding ft-compatible templates. Defaults
    /// to `templates-ft` when unset. See plan 009 for the rationale.
    pub templates_dir: Option<String>,
}

/// Per-period configuration for periodic notes.
///
/// Each field is `Option` so a user can configure only the periods they
/// use; missing entries surface as "period not configured" errors when a
/// caller asks for them.
///
/// Path and filename patterns use chrono `%`-tokens (see [`crate::periodic`]
/// for the supported set, including the `%q`/`%Q` quarter extensions).
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodicNotes {
    pub daily: Option<PeriodicPeriod>,
    pub weekly: Option<PeriodicPeriod>,
    pub monthly: Option<PeriodicPeriod>,
    pub quarterly: Option<PeriodicPeriod>,
    pub yearly: Option<PeriodicPeriod>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodicPeriod {
    /// Folder pattern, vault-relative. Chrono strftime tokens supported,
    /// plus the `%q`/`%Q` quarter extensions from [`crate::periodic`].
    /// Empty string means "vault root".
    pub path: String,
    /// Filename pattern (without `.md`). Same token surface as `path`.
    pub format: String,
    /// Template name resolved under `[notes].templates_dir` (or an
    /// absolute path). When unset, the new note's body is `# <title>\n\n`.
    pub template: Option<String>,
}

#[derive(Debug)]
pub struct ConfigSource {
    /// Human-readable label: "user" or "vault".
    pub label: String,
    pub path: PathBuf,
    /// Whether the file exists on disk.
    pub present: bool,
}

#[derive(Debug)]
pub struct LayeredConfig {
    pub config: Config,
    /// Sources in order of increasing precedence (last = highest priority).
    pub sources: Vec<ConfigSource>,
}

/// Load configuration by merging user-level and vault-level TOML files.
///
/// Vault config wins over user config. Missing files are silently skipped.
pub fn load(user_config: &Path, vault_config: &Path) -> Result<LayeredConfig> {
    let config = Figment::new()
        .merge(Serialized::defaults(Config::default()))
        .merge(Toml::file(user_config))
        .merge(Toml::file(vault_config))
        .extract::<Config>()
        .map_err(|e| Error::Config {
            path: if vault_config.exists() {
                vault_config.display().to_string()
            } else {
                user_config.display().to_string()
            },
            source: Box::new(e),
        })?;

    Ok(LayeredConfig {
        config,
        sources: vec![
            ConfigSource {
                label: "user".into(),
                path: user_config.to_path_buf(),
                present: user_config.exists(),
            },
            ConfigSource {
                label: "vault".into(),
                path: vault_config.to_path_buf(),
                present: vault_config.exists(),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use assert_fs::TempDir;

    #[test]
    fn defaults_when_no_files() {
        let tmp = TempDir::new().unwrap();
        let lc = load(
            &tmp.path().join("nonexistent-user.toml"),
            &tmp.path().join("nonexistent-vault.toml"),
        )
        .unwrap();
        assert!(lc.config.default_task_location.is_none());
        assert!(lc.config.ignored_paths.is_empty());
        assert!(lc.config.periodic_notes.daily.is_none());
        assert!(lc.config.periodic_notes.weekly.is_none());
        assert!(lc.config.periodic_notes.monthly.is_none());
        assert!(lc.config.periodic_notes.quarterly.is_none());
        assert!(lc.config.periodic_notes.yearly.is_none());
        assert!(!lc.sources[0].present);
        assert!(!lc.sources[1].present);
    }

    #[test]
    fn periodic_notes_daily_only() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[periodic_notes.daily]
path = "journal/%Y"
format = "%Y-%m-%d"
template = "daily"
"#,
        )
        .unwrap();

        let lc = load(user.path(), &tmp.path().join("no-vault.toml")).unwrap();
        let d = lc.config.periodic_notes.daily.as_ref().unwrap();
        assert_eq!(d.path, "journal/%Y");
        assert_eq!(d.format, "%Y-%m-%d");
        assert_eq!(d.template.as_deref(), Some("daily"));
        assert!(lc.config.periodic_notes.weekly.is_none());
    }

    #[test]
    fn periodic_notes_all_five_periods() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[periodic_notes.daily]
path = "journal/%Y"
format = "%Y-%m-%d"

[periodic_notes.weekly]
path = "journal/%Y"
format = "%G-W%V"

[periodic_notes.monthly]
path = "journal/%Y"
format = "%Y-%m"

[periodic_notes.quarterly]
path = "journal/%Y"
format = "%Y-Q%q"

[periodic_notes.yearly]
path = "journal"
format = "%Y"
"#,
            )
            .unwrap();
        let lc = load(&tmp.path().join("no-user.toml"), vault.path()).unwrap();
        assert!(lc.config.periodic_notes.daily.is_some());
        assert!(lc.config.periodic_notes.weekly.is_some());
        assert!(lc.config.periodic_notes.monthly.is_some());
        assert!(lc.config.periodic_notes.quarterly.is_some());
        assert!(lc.config.periodic_notes.yearly.is_some());
        assert_eq!(
            lc.config.periodic_notes.quarterly.as_ref().unwrap().format,
            "%Y-Q%q"
        );
    }

    #[test]
    fn vault_config_wins_over_user_for_periodic_block() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[periodic_notes.daily]
path = "from-user"
format = "%Y-%m-%d"
"#,
        )
        .unwrap();

        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[periodic_notes.daily]
path = "from-vault"
format = "%Y-%m-%d"
"#,
            )
            .unwrap();

        let lc = load(user.path(), vault.path()).unwrap();
        assert_eq!(
            lc.config.periodic_notes.daily.as_ref().unwrap().path,
            "from-vault"
        );
    }

    #[test]
    fn old_daily_notes_block_rejected() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[daily_notes]
source = "explicit"
path = "Journal"
format = "YYYY-MM-DD"
"#,
        )
        .unwrap();
        let r = load(user.path(), &tmp.path().join("no-vault.toml"));
        assert!(
            r.is_err(),
            "old [daily_notes] block should be rejected by deny_unknown_fields"
        );
    }

    #[test]
    fn periodic_notes_typo_rejected() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[periodic_notes.daily]
path = "journal"
format = "%Y-%m-%d"
typo_field = "oops"
"#,
            )
            .unwrap();
        let r = load(&tmp.path().join("no-user.toml"), vault.path());
        assert!(r.is_err());
    }

    #[test]
    fn unknown_key_in_config_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(r#"typo_key = "oops""#).unwrap();

        let result = load(user.path(), &tmp.path().join("no-vault.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn notes_templates_dir_default_is_none() {
        let tmp = TempDir::new().unwrap();
        let lc = load(
            &tmp.path().join("no-user.toml"),
            &tmp.path().join("no-vault.toml"),
        )
        .unwrap();
        assert!(lc.config.notes.templates_dir.is_none());
    }

    #[test]
    fn notes_templates_dir_override() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[notes]
templates_dir = "_templates"
"#,
            )
            .unwrap();
        let lc = load(&tmp.path().join("no-user.toml"), vault.path()).unwrap();
        assert_eq!(lc.config.notes.templates_dir.as_deref(), Some("_templates"));
    }

    #[test]
    fn notes_unknown_key_rejected() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[notes]
typo_field = "oops"
"#,
            )
            .unwrap();
        let r = load(&tmp.path().join("no-user.toml"), vault.path());
        assert!(r.is_err());
    }

    #[test]
    fn presets_loaded_correctly() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[presets]
work = "tag is #work and not done"
"#,
        )
        .unwrap();

        let lc = load(user.path(), &tmp.path().join("no-vault.toml")).unwrap();
        assert_eq!(
            lc.config.presets.get("work").map(|s| s.as_str()),
            Some("tag is #work and not done")
        );
    }
}

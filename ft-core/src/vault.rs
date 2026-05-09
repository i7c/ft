use std::path::{Path, PathBuf};

use tracing::debug;

use crate::{
    config::{self, LayeredConfig},
    error::{Error, Result},
};

#[derive(Debug)]
pub struct Vault {
    pub path: PathBuf,
    pub config: LayeredConfig,
}

impl Vault {
    /// Discover the vault root and load its layered configuration.
    ///
    /// Discovery precedence:
    /// 1. `vault_flag` — from `--vault` CLI flag
    /// 2. `FT_VAULT` environment variable
    /// 3. Walk up from the current working directory looking for `.obsidian/`
    /// 4. `default_vault` key in `~/.config/ft/config.toml`
    ///
    /// If none of the above succeeds, returns [`Error::VaultNotFound`] with
    /// every location that was attempted.
    pub fn discover(vault_flag: Option<PathBuf>) -> Result<Self> {
        let vault_path = find_vault(vault_flag)?;
        debug!(vault = %vault_path.display(), "vault resolved");

        let user_config_path = user_config_dir().join("ft").join("config.toml");
        let vault_config_path = vault_path.join(".ft").join("config.toml");

        let config = config::load(&user_config_path, &vault_config_path)?;

        Ok(Vault {
            path: vault_path,
            config,
        })
    }
}

fn find_vault(vault_flag: Option<PathBuf>) -> Result<PathBuf> {
    let mut tried: Vec<String> = Vec::new();
    // 1. --vault flag
    if let Some(flag_path) = vault_flag {
        let canonical = flag_path
            .canonicalize()
            .unwrap_or_else(|_| flag_path.clone());
        if canonical.join(".obsidian").exists() {
            debug!("vault from --vault flag: {}", canonical.display());
            return Ok(canonical);
        }
        tried.push(format!(
            "  --vault {}: no .obsidian/ found",
            flag_path.display()
        ));
    } else {
        tried.push("  --vault: not provided".into());
    }

    // 2. FT_VAULT env var
    match std::env::var("FT_VAULT") {
        Ok(val) if !val.is_empty() => {
            let p = PathBuf::from(&val);
            let canonical = p.canonicalize().unwrap_or_else(|_| p.clone());
            if canonical.join(".obsidian").exists() {
                debug!("vault from $FT_VAULT: {}", canonical.display());
                return Ok(canonical);
            }
            tried.push(format!("  $FT_VAULT={}: no .obsidian/ found", val));
        }
        _ => {
            tried.push("  $FT_VAULT: not set".into());
        }
    }

    // 3. Walk up from CWD
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Some(found) = walk_up(&cwd) {
        debug!("vault from CWD walk: {}", found.display());
        return Ok(found);
    }
    tried.push(format!(
        "  CWD walk from {}: no ancestor contains .obsidian/",
        cwd.display()
    ));

    // 4. default_vault in user config
    let user_config_path = user_config_dir().join("ft").join("config.toml");
    if let Some(default_vault) = read_default_vault(&user_config_path) {
        let p = PathBuf::from(&default_vault);
        let canonical = p.canonicalize().unwrap_or_else(|_| p.clone());
        if canonical.join(".obsidian").exists() {
            debug!("vault from config default_vault: {}", canonical.display());
            return Ok(canonical);
        }
        tried.push(format!(
            "  {}: default_vault={}: no .obsidian/ found",
            user_config_path.display(),
            default_vault
        ));
    } else {
        tried.push(format!(
            "  {}: default_vault not set",
            user_config_path.display()
        ));
    }

    Err(Error::VaultNotFound { tried })
}

fn walk_up(start: &Path) -> Option<PathBuf> {
    let mut cur = start;
    loop {
        if cur.join(".obsidian").exists() {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
    }
}

fn read_default_vault(config_path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(config_path).ok()?;
    let table: toml::Table = raw.parse().ok()?;
    table
        .get("default_vault")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Returns `~/.config` regardless of platform.
/// On macOS, `dirs::config_dir()` returns `~/Library/Application Support`, but
/// we follow the XDG convention (`~/.config`) for portability and simplicity.
fn user_config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use assert_fs::TempDir;

    fn make_obsidian_dir(dir: &TempDir) {
        dir.child(".obsidian").create_dir_all().unwrap();
    }

    // ── flag ─────────────────────────────────────────────────────────────────

    #[test]
    fn flag_pointing_at_valid_vault() {
        let dir = TempDir::new().unwrap();
        make_obsidian_dir(&dir);
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        assert_eq!(
            vault.path.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn flag_pointing_at_non_vault_errors() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("FT_VAULT");
        let dir = TempDir::new().unwrap();
        // No .obsidian/ here
        let result = Vault::discover(Some(dir.path().to_path_buf()));
        assert!(matches!(result, Err(Error::VaultNotFound { .. })));
    }

    #[test]
    fn error_message_lists_tried_locations() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("FT_VAULT");
        let dir = TempDir::new().unwrap();
        let err = Vault::discover(Some(dir.path().to_path_buf())).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--vault"),
            "error should mention --vault; got: {msg}"
        );
    }

    // ── walk_up ───────────────────────────────────────────────────────────────

    #[test]
    fn walk_up_finds_obsidian_in_parent() {
        let vault_dir = TempDir::new().unwrap();
        make_obsidian_dir(&vault_dir);
        let sub = vault_dir.child("notes/2026");
        sub.create_dir_all().unwrap();

        let found = walk_up(sub.path()).unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            vault_dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn walk_up_returns_none_when_no_obsidian() {
        let dir = TempDir::new().unwrap();
        assert!(walk_up(dir.path()).is_none());
    }

    #[test]
    fn walk_up_finds_self() {
        let dir = TempDir::new().unwrap();
        make_obsidian_dir(&dir);
        let found = walk_up(dir.path()).unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    // ── find_vault (env) ─────────────────────────────────────────────────────
    // These tests use a global shared resource (the environment) and must not
    // run concurrently.  We use a process-level mutex to serialize them.

    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn env_var_valid_vault() {
        let vault_dir = TempDir::new().unwrap();
        make_obsidian_dir(&vault_dir);

        let _guard = ENV_LOCK.lock().unwrap();
        // Ensure --vault flag is not in play (no flag passed = None)
        // We need to make sure CWD doesn't accidentally resolve to a vault.
        std::env::set_var("FT_VAULT", vault_dir.path().to_str().unwrap());

        // Pass a flag that fails so we fall through to env
        let bad_dir = TempDir::new().unwrap();
        let result = find_vault(Some(bad_dir.path().to_path_buf()));
        std::env::remove_var("FT_VAULT");

        // The env var vault should be found
        let found = result.unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            vault_dir.path().canonicalize().unwrap()
        );
    }
}

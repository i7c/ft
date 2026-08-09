use std::path::{Path, PathBuf};

use tracing::debug;

use crate::{
    config::{self, LayeredConfig},
    error::{Error, Result},
    scan::{self, Scan},
    task::{emoji::EmojiFormat, format::TaskFormat},
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
    ///    or `.ft/`
    /// 4. `default_vault` key in `~/.config/ft/config.toml`
    ///
    /// A directory qualifies as a vault root when it contains either an
    /// `.obsidian/` or a `.ft/` marker directory (see `is_vault_root`).
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

    /// Walk the vault, parse every markdown file in parallel, and return all
    /// tasks, per-file graph parse artifacts, the directory list, plus
    /// per-file errors. Respects `.gitignore`, default exclusions
    /// (`.obsidian/`, `.git/`, `attachments/`), and the `ignored_paths`
    /// config.
    ///
    /// This is the single read pass: each file's content is loaded once and
    /// everything downstream needs — tasks, links, paragraphs, headings,
    /// frontmatter block — is extracted here. [`crate::graph::Graph::build`]
    /// consumes the artifacts without re-reading anything.
    pub fn scan(&self) -> Scan {
        scan::scan_vault(&self.path, &self.config.config.ignored_paths)
    }

    /// The task wire format for this vault. Always [`EmojiFormat`] today;
    /// this accessor is the seam where config-driven format detection
    /// will plug in. Callers of `task::ops` should take their format from
    /// here rather than naming `EmojiFormat` directly.
    pub fn task_format(&self) -> &'static dyn TaskFormat {
        &EmojiFormat
    }

    /// Return `path` stripped of the vault root prefix, or `path`
    /// verbatim when it already is vault-relative (or sits outside the
    /// vault). Used for user-facing display of absolute paths. The
    /// incoming `path` is canonicalized first so that a symlinked or
    /// differently-lexical form of the same absolute file still
    /// relativizes against the (canonicalized) vault root — e.g. on
    /// macOS where `/tmp` and `/var` are symlinks to `/private/...`.
    pub fn relativize(&self, path: &Path) -> PathBuf {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        canonical
            .strip_prefix(&self.path)
            .map(Path::to_path_buf)
            .unwrap_or(canonical)
    }

    /// Vault-relative path that holds ft note templates. Defaults to
    /// `templates-ft/` when `[notes] templates_dir` is unset in the
    /// vault config. The folder is **not** required to exist — callers
    /// (TUI template picker, CLI `--template` resolution) tolerate a
    /// missing dir by showing an empty list or erroring with a clear
    /// "template not found" message.
    pub fn templates_dir(&self) -> PathBuf {
        let dir = self
            .config
            .config
            .notes
            .templates_dir
            .as_deref()
            .unwrap_or("templates-ft");
        self.path.join(dir)
    }

    /// Resolve the target file for a new task: an explicit override if
    /// supplied (joined against the vault root when relative), otherwise
    /// today's daily note (from `[periodic_notes.daily]`). Returns an
    /// absolute path.
    ///
    /// Shared by the CLI (`ft tasks create --file`) and the TUI's
    /// quickline `in:PATH` token so both surfaces agree on what "target
    /// file" means for a given vault + day.
    pub fn resolve_target(
        &self,
        today: chrono::NaiveDate,
        file_override: Option<&Path>,
    ) -> Result<PathBuf> {
        if let Some(file) = file_override {
            let p = if file.is_absolute() {
                file.to_path_buf()
            } else {
                self.path.join(file)
            };
            return Ok(p);
        }
        let daily = self
            .config
            .config
            .periodic_notes
            .daily
            .as_ref()
            .ok_or_else(|| {
                Error::Periodic(
                    "no `[periodic_notes.daily]` configured — add it to your config or pass `--file <PATH>`"
                        .to_string(),
                )
            })?;
        crate::periodic::resolve_periodic_path(&self.path, daily, today)
    }

    /// Resolve a write target like [`Self::resolve_target`], but when it is
    /// the default daily note (no `file_override`) and the file does not yet
    /// exist, render the configured daily template first — so a brand-new
    /// day's note matches what `ft notes today` would produce rather than a
    /// bare file holding only the caller's content.
    ///
    /// Explicit `file_override` paths are resolved but never auto-templated:
    /// the caller picked a path, not a periodic note. Use this at every
    /// *write* site (task create, timeblock add); leave [`Self::resolve_target`]
    /// for read-only/display sites that must not create files.
    ///
    /// `date` is the note's date (e.g. tomorrow for a timeblocks pane);
    /// `today`/`now` are the template-rendering context. They are accepted
    /// explicitly so callers with an injected clock (the TUI) and the CLI
    /// (`dates::now_pair()`) agree on "now".
    pub fn ensure_target(
        &self,
        date: chrono::NaiveDate,
        file_override: Option<&Path>,
        today: chrono::NaiveDate,
        now: chrono::NaiveDateTime,
    ) -> Result<PathBuf> {
        if file_override.is_some() {
            return self.resolve_target(date, file_override);
        }
        let daily = self
            .config
            .config
            .periodic_notes
            .daily
            .as_ref()
            .ok_or_else(|| {
                Error::Periodic(
                    "no `[periodic_notes.daily]` configured — add it to your config or pass `--file <PATH>`"
                        .to_string(),
                )
            })?;
        let (path, _created) = crate::periodic::create_or_get_periodic_path(
            &self.path,
            &self.templates_dir(),
            daily,
            date,
            today,
            now,
        )?;
        Ok(path)
    }
}

fn find_vault(vault_flag: Option<PathBuf>) -> Result<PathBuf> {
    let mut tried: Vec<String> = Vec::new();
    // 1. --vault flag
    if let Some(flag_path) = vault_flag {
        let canonical = flag_path
            .canonicalize()
            .unwrap_or_else(|_| flag_path.clone());
        if is_vault_root(&canonical) {
            debug!("vault from --vault flag: {}", canonical.display());
            return Ok(canonical);
        }
        tried.push(format!(
            "  --vault {}: no .obsidian/ or .ft/ found",
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
            if is_vault_root(&canonical) {
                debug!("vault from $FT_VAULT: {}", canonical.display());
                return Ok(canonical);
            }
            tried.push(format!("  $FT_VAULT={}: no .obsidian/ or .ft/ found", val));
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
        "  CWD walk from {}: no ancestor contains .obsidian/ or .ft/",
        cwd.display()
    ));

    // 4. default_vault in user config
    let user_config_path = user_config_dir().join("ft").join("config.toml");
    if let Some(default_vault) = read_default_vault(&user_config_path) {
        let p = PathBuf::from(&default_vault);
        let canonical = p.canonicalize().unwrap_or_else(|_| p.clone());
        if is_vault_root(&canonical) {
            debug!("vault from config default_vault: {}", canonical.display());
            return Ok(canonical);
        }
        tried.push(format!(
            "  {}: default_vault={}: no .obsidian/ or .ft/ found",
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
        if is_vault_root(cur) {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
    }
}

/// A directory is a vault root iff it contains a recognized vault marker
/// directory — either `.obsidian/` (Obsidian vault) or `.ft/` (ft-native
/// standalone vault). The two markers are equivalent: neither is preferred,
/// and a directory with both is one vault. A regular file named `.obsidian`
/// or `.ft` does not qualify; the check uses [`Path::is_dir`] so a stray
/// file can't be mistaken for a marker.
fn is_vault_root(path: &Path) -> bool {
    path.join(".obsidian").is_dir() || path.join(".ft").is_dir()
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
    use chrono::NaiveDate;

    fn make_obsidian_dir(dir: &TempDir) {
        dir.child(".obsidian").create_dir_all().unwrap();
    }

    /// Mark `dir` as an ft-native standalone vault (no `.obsidian/`).
    fn make_ft_dir(dir: &TempDir) {
        dir.child(".ft").create_dir_all().unwrap();
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
    fn flag_pointing_at_ft_only_vault() {
        let dir = TempDir::new().unwrap();
        make_ft_dir(&dir);
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        assert_eq!(
            vault.path.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn flag_pointing_at_dotfile_not_dir_does_not_qualify() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("FT_VAULT");
        let dir = TempDir::new().unwrap();
        // A regular *file* named `.ft` is not a vault marker.
        dir.child(".ft").touch().unwrap();
        let result = Vault::discover(Some(dir.path().to_path_buf()));
        assert!(matches!(result, Err(Error::VaultNotFound { .. })));
    }

    #[test]
    fn flag_pointing_at_non_vault_errors() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("FT_VAULT");
        let dir = TempDir::new().unwrap();
        // No vault marker (`.obsidian/` or `.ft/`) here.
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
        // Each marker-specific line names both recognized markers so the
        // user knows either suffices, and does not assert Obsidian is required.
        assert!(
            msg.contains(".obsidian/ or .ft/"),
            "error should name both markers; got: {msg}"
        );
        assert!(
            !msg.contains("Obsidian vault"),
            "error must not assert Obsidian is required; got: {msg}"
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
    fn discover_walk_up_finds_ft_only_ancestor() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("FT_VAULT");

        let vault_dir = TempDir::new().unwrap();
        make_ft_dir(&vault_dir);
        let sub = vault_dir.child("notes/2026");
        sub.create_dir_all().unwrap();

        // Run find_vault from inside `sub`: the CWD walk-up rung should
        // resolve to the `.ft`-only ancestor. We can't easily set the
        // process CWD portably, so drive walk_up directly — it's the same
        // predicate find_vault uses for rung 3.
        let found = walk_up(sub.path()).unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            vault_dir.path().canonicalize().unwrap()
        );
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

    #[test]
    fn walk_up_finds_ft_marker_in_parent() {
        let vault_dir = TempDir::new().unwrap();
        make_ft_dir(&vault_dir);
        let sub = vault_dir.child("notes/2026");
        sub.create_dir_all().unwrap();

        let found = walk_up(sub.path()).unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            vault_dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn walk_up_finds_ft_marker_self() {
        let dir = TempDir::new().unwrap();
        make_ft_dir(&dir);
        let found = walk_up(dir.path()).unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn walk_up_ignores_dotfile_named_ft() {
        let dir = TempDir::new().unwrap();
        // A file named `.ft` (not a dir) must not count as a marker.
        dir.child(".ft").touch().unwrap();
        assert!(walk_up(dir.path()).is_none());
    }

    // ── find_vault (env) ─────────────────────────────────────────────────────
    // These tests use a global shared resource (the environment) and must not
    // run concurrently.  We use a process-level mutex to serialize them.

    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn ensure_target_renders_daily_template_when_missing() {
        let dir = TempDir::new().unwrap();
        make_obsidian_dir(&dir);
        dir.child(".ft/config.toml")
            .write_str(
                "[periodic_notes.daily]\npath = \"journal\"\nformat = \"%Y-%m-%d\"\ntemplate = \"daily\"\n\n[notes]\ntemplates_dir = \"templates-ft\"\n",
            )
            .unwrap();
        dir.child("templates-ft/daily.md")
            .write_str("# {{ title }}\n\n## Tasks\n")
            .unwrap();
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();

        let date = NaiveDate::from_ymd_opt(2026, 5, 9).unwrap();
        let now = date.and_hms_opt(8, 0, 0).unwrap();
        let path = vault.ensure_target(date, None, date, now).unwrap();

        assert_eq!(
            path,
            dir.path()
                .canonicalize()
                .unwrap()
                .join("journal/2026-05-09.md")
        );
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body, "# 2026-05-09\n\n## Tasks\n");
    }

    #[test]
    fn ensure_target_leaves_existing_daily_note_untouched() {
        let dir = TempDir::new().unwrap();
        make_obsidian_dir(&dir);
        dir.child(".ft/config.toml")
            .write_str("[periodic_notes.daily]\npath = \"journal\"\nformat = \"%Y-%m-%d\"\n")
            .unwrap();
        dir.child("journal/2026-05-09.md")
            .write_str("existing body\n")
            .unwrap();
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();

        let date = NaiveDate::from_ymd_opt(2026, 5, 9).unwrap();
        let now = date.and_hms_opt(8, 0, 0).unwrap();
        vault.ensure_target(date, None, date, now).unwrap();

        let body = std::fs::read_to_string(dir.path().join("journal/2026-05-09.md")).unwrap();
        assert_eq!(body, "existing body\n");
    }

    #[test]
    fn ensure_target_does_not_template_explicit_file() {
        let dir = TempDir::new().unwrap();
        make_obsidian_dir(&dir);
        dir.child(".ft/config.toml")
            .write_str(
                "[periodic_notes.daily]\npath = \"journal\"\nformat = \"%Y-%m-%d\"\ntemplate = \"daily\"\n",
            )
            .unwrap();
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();

        let date = NaiveDate::from_ymd_opt(2026, 5, 9).unwrap();
        let now = date.and_hms_opt(8, 0, 0).unwrap();
        let explicit = Path::new("Inbox.md");
        let path = vault
            .ensure_target(date, Some(explicit), date, now)
            .unwrap();

        assert_eq!(path, dir.path().canonicalize().unwrap().join("Inbox.md"));
        // Explicit paths are resolved but never created/templated here.
        assert!(!path.exists());
    }

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

    #[test]
    fn env_var_ft_only_vault() {
        let vault_dir = TempDir::new().unwrap();
        make_ft_dir(&vault_dir);

        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("FT_VAULT", vault_dir.path().to_str().unwrap());

        // Pass a flag that fails so we fall through to the env rung.
        let bad_dir = TempDir::new().unwrap();
        let result = find_vault(Some(bad_dir.path().to_path_buf()));
        std::env::remove_var("FT_VAULT");

        let found = result.unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            vault_dir.path().canonicalize().unwrap()
        );
    }
}

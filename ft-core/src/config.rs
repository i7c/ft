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
use crate::git::PullStrategy;

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
    /// Note-creation settings.
    #[serde(default)]
    pub notes: Notes,
    /// Editor handoff strategy and popup geometry. See [`Editor`].
    #[serde(default)]
    pub editor: Editor,
    /// Git-sync settings. See [`Git`].
    #[serde(default)]
    pub git: Git,
    /// Timeblocks settings. See [`Timeblocks`].
    #[serde(default)]
    pub timeblocks: Timeblocks,
    /// Graph-tab settings. See [`GraphCfg`].
    #[serde(default)]
    pub graph: GraphCfg,
    /// Quick-capture presets, keyed by preset name. See [`CapturePreset`].
    #[serde(default)]
    pub capture_presets: HashMap<String, CapturePreset>,
    /// Remote task sources (Jira, GitHub), keyed by source name. See
    /// [`SourceCfg`].
    #[serde(default)]
    pub sources: std::collections::BTreeMap<String, SourceCfg>,
    /// User-defined key binding overrides. When `None`, defaults apply.
    /// Vault-level `[keymap]` replaces user-level `[keymap]` whole.
    #[serde(default)]
    pub keymap: Option<KeymapConfig>,
    /// Synthesis settings (search, synth notes). See [`Synth`].
    #[serde(default)]
    pub synth: Synth,
    /// Paragraph-search settings. See [`SearchCfg`].
    #[serde(default)]
    pub search: SearchCfg,
    /// Task-creation settings. See [`Tasks`].
    #[serde(default)]
    pub tasks: Tasks,
    /// TUI settings. See [`Tui`].
    #[serde(default)]
    pub tui: Tui,
    /// Drift-report settings. See [`Drift`].
    #[serde(default)]
    pub drift: Drift,
}

impl Config {
    /// Heading under which timeblock entries live in each daily note.
    /// Defaults to `"Time Blocks"` when [`Timeblocks::heading`] is unset.
    pub fn timeblocks_heading(&self) -> &str {
        self.timeblocks.heading.as_deref().unwrap_or("Time Blocks")
    }
}

/// One entry in `keymap.unbind` — removes a default chord from a scope.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct KeymapUnbindEntry {
    pub scope: String,
    pub chord: String,
}

/// User-defined key binding overrides. Appears as `[keymap]` in `config.toml`.
///
/// `[keymap.<scope>]` sub-tables map chord strings to command names.
/// `keymap.unbind` drops default chords without replacing them.
/// `keymap.strict` turns validation warnings into hard errors at startup.
///
/// Vault-level `[keymap]` replaces user-level `[keymap]` whole (no per-entry
/// merging across config files).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct KeymapConfig {
    /// When `true`, invalid entries abort `ft tui` startup (exit 1).
    /// When `false` (default), they are logged as warnings and skipped.
    #[serde(default)]
    pub strict: bool,
    /// Chords to remove from the default bindings, without replacement.
    #[serde(default)]
    pub unbind: Vec<KeymapUnbindEntry>,
    /// Per-scope override maps: `"global"`, `"tab/<name>"`, `"modal/<name>"`.
    /// Keys are chord strings; values are command names.
    #[serde(flatten)]
    pub scopes: HashMap<String, HashMap<String, String>>,
}

/// `ft timeblocks` configuration. The daily-note path is resolved via the
/// existing [`PeriodicNotes::daily`] block; this struct just controls the
/// section heading the timeblock list is stored under.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Timeblocks {
    /// Heading under which timeblocks live. Defaults to `"Time Blocks"`.
    pub heading: Option<String>,
}

/// TUI graph-tab configuration. Currently just the seed query the tab
/// runs the first time it gains focus; when unset, the tab falls back
/// to a built-in default that shows the vault root.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphCfg {
    /// Optional DSL query that seeds the graph tab on first focus. When
    /// unset, the tab falls back to a built-in default.
    pub default_query: Option<String>,
    /// Named graph-query presets. Keys are preset names; values are graph-DSL
    /// strings. User presets shadow built-ins of the same name.
    #[serde(default)]
    pub presets: HashMap<String, String>,
}

/// Quick-capture preset. Each preset bundles an action (append or create),
/// a template, and optional target resolution fields.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapturePreset {
    /// Whether to append to an existing note or create a new one.
    pub action: CaptureAction,
    /// Template name resolved under `[notes].templates_dir`.
    pub template: String,
    /// Hardcoded target note (vault-relative path) for append presets.
    /// When absent, the target is resolved from tab context.
    #[serde(default)]
    pub note: Option<String>,
    /// Section heading to append under (for append presets). Overrides
    /// any `ft.append.section` frontmatter in the target note.
    #[serde(default)]
    pub section: Option<String>,
    /// Filename pattern for create presets. Supports strftime `%Y`, `%m`,
    /// `%d`, `%q`, `%Q` tokens. `.md` is appended automatically.
    #[serde(default)]
    pub path: Option<String>,
    /// Target folder (vault-relative) for create presets. Defaults to
    /// vault root when absent.
    #[serde(default)]
    pub folder: Option<String>,
}

/// Backend of a remote task source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Jira,
    Github,
}

/// A remote task source (`[sources.<name>]`). Lives in the vault config so
/// every machine syncing the vault shares it; machines without credentials
/// simply skip it.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCfg {
    pub kind: SourceKind,
    /// API base URL. Required for Jira (`https://<site>.atlassian.net`);
    /// optional for GitHub (defaults to `https://api.github.com`).
    #[serde(default)]
    pub base_url: Option<String>,
    /// Atlassian account email (Jira basic-auth user).
    #[serde(default)]
    pub email: Option<String>,
    /// Named JQL / GitHub search queries. Empty means the kind's default
    /// "assigned to me, open" query; see [`SourceCfg::effective_queries`].
    #[serde(default)]
    pub queries: std::collections::BTreeMap<String, String>,
    /// Vault-relative folder for new issue notes. Defaults to the source name.
    #[serde(default)]
    pub folder: Option<String>,
    /// Template (under `[notes].templates_dir`) for the user part of new
    /// issue notes.
    #[serde(default)]
    pub template: Option<String>,
    /// TUI background sync interval; `0` disables interval sync.
    #[serde(default = "default_source_interval")]
    pub interval_minutes: u64,
    /// Also fetch each in-scope issue's parent chain.
    #[serde(default = "default_true")]
    pub fetch_parents: bool,
    /// Most recent comments kept in the note; `0` disables the section.
    #[serde(default = "default_source_comments")]
    pub comments: usize,
    /// Keep the managed `Description` section.
    #[serde(default = "default_true")]
    pub description: bool,
    /// Remote status name → local status (`Open`, `InProgress`, `Done`,
    /// `Cancelled`), overriding the default mapping.
    #[serde(default)]
    pub status_map: std::collections::BTreeMap<String, crate::task::Status>,
    /// Push local status changes (and allow promote). `false` makes the
    /// source read-only.
    #[serde(default = "default_true")]
    pub push: bool,
    /// Jira only: local status → transition name, overriding the
    /// category-based choice. `Cancelled` is only pushed when set here.
    #[serde(default)]
    pub transitions: std::collections::BTreeMap<crate::task::Status, String>,
}

fn default_source_interval() -> u64 {
    10
}

fn default_source_comments() -> usize {
    5
}

fn default_true() -> bool {
    true
}

impl SourceCfg {
    /// The configured queries, or the kind's default when none are set.
    pub fn effective_queries(&self) -> std::collections::BTreeMap<String, String> {
        if !self.queries.is_empty() {
            return self.queries.clone();
        }
        let q = match self.kind {
            SourceKind::Jira => "assignee = currentUser() AND statusCategory != Done",
            SourceKind::Github => "is:issue is:open assignee:@me",
        };
        [("mine".to_string(), q.to_string())].into_iter().collect()
    }

    /// Folder for new issue notes.
    pub fn folder_or<'a>(&'a self, name: &'a str) -> &'a str {
        self.folder.as_deref().unwrap_or(name)
    }

    /// Check the cross-field rules serde can't express.
    pub fn validate(&self, name: &str) -> std::result::Result<(), String> {
        if let Some(url) = &self.base_url {
            let local = url.starts_with("http://localhost") || url.starts_with("http://127.0.0.1");
            if !url.starts_with("https://") && !local {
                return Err(format!(
                    "sources.{name}.base_url must use https:// (got `{url}`)"
                ));
            }
        }
        if self.kind == SourceKind::Github && !self.transitions.is_empty() {
            return Err(format!("sources.{name}.transitions is only valid for jira"));
        }
        if self.kind == SourceKind::Jira {
            if self.base_url.is_none() {
                return Err(format!("sources.{name}: jira sources need `base_url`"));
            }
            if self.email.is_none() {
                return Err(format!("sources.{name}: jira sources need `email`"));
            }
        }
        Ok(())
    }
}

/// Action for a quick-capture preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureAction {
    Append,
    Create,
}

/// `ft git sync` / TUI `g s` configuration. Currently just the pull
/// strategy — conflict handling is identical for both variants and
/// not user-tunable (markers in place, abort before push).
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Git {
    #[serde(default)]
    pub pull_strategy: PullStrategy,
}

/// Task-creation settings. Governs where a new task lands within its
/// target file when the caller gives no explicit position.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tasks {
    /// Heading new tasks land under when the target note has no
    /// `ft.tasks.section` frontmatter and no explicit position is given.
    /// When unset, new tasks append at file end (the historical default).
    pub default_section: Option<String>,
    /// Named task queries (presets). Keys are preset names; values are
    /// task-DSL strings. Serialized as `[tasks.presets]`. User presets
    /// shadow built-ins of the same name.
    #[serde(default)]
    pub presets: HashMap<String, String>,
    /// Bare tag names (no leading `#`) offered by the TUI retag picker.
    /// Empty by default; the picker no-ops (with a toast) when empty.
    #[serde(default)]
    pub retag_tags: Vec<String>,
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

/// First day of the week used to anchor a weekly periodic note.
///
/// Defaults to Monday, matching `ft timeblocks spent` and ISO 8601.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WeekStart {
    #[default]
    Monday,
    Sunday,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
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
    /// First day of the week for the weekly period. Defaults to Monday and
    /// is ignored by the other periods.
    #[serde(default)]
    pub week_start: WeekStart,
}

/// Synthesis configuration. Governs `ft notes search` and the
/// `ft notes synth` flow.
///
/// `folder` is the convenience default location for new synth notes
/// created via `ft synth <name>` without a path-bearing target. It is
/// NOT enforcement: any `.md` anywhere in the vault marked with
/// `ft.synth.enabled: true` in frontmatter is treated as a synth note.
///
/// `exclude_prefixes` filters the paragraph-search index: files whose
/// vault-relative path starts with any listed prefix are skipped.
/// Defaults to empty; users typically add their periodic-notes folder
/// here.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Synth {
    #[serde(default = "default_synth_folder")]
    pub folder: String,
    #[serde(default)]
    pub exclude_prefixes: Vec<String>,
}

/// Paragraph-search configuration (`ft notes search`, the Search tab,
/// `synth scaffold --search`).
///
/// `stemmers` lists the Snowball algorithms whose stems make `=word`
/// clauses match inflected forms (`=memory` finds `memories`). A token
/// matches when it shares a stem with the term under any listed
/// algorithm; `[]` disables stemming. Names are validated at load.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchCfg {
    #[serde(default = "default_stemmers")]
    pub stemmers: Vec<String>,
    /// Meaning-based (embedding) retrieval. See [`SemanticCfg`].
    #[serde(default)]
    pub semantic: SemanticCfg,
}

impl Default for SearchCfg {
    fn default() -> Self {
        SearchCfg {
            stemmers: default_stemmers(),
            semantic: SemanticCfg::default(),
        }
    }
}

/// `[search.semantic]`: hybrid retrieval with Model2Vec embeddings.
///
/// Off by default. `model` is a Hugging Face model id (fetched once with
/// `ft notes embed fetch` into the user cache dir) or a local directory
/// holding `config.json`, `tokenizer.json`, and `model.safetensors`.
/// `candidates` paragraphs with cosine similarity ≥ `min_similarity`
/// join the lexical results, fused by Reciprocal Rank Fusion.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticCfg {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_semantic_model")]
    pub model: String,
    #[serde(default = "default_semantic_candidates")]
    pub candidates: usize,
    #[serde(default = "default_min_similarity")]
    pub min_similarity: f32,
}

impl Default for SemanticCfg {
    fn default() -> Self {
        SemanticCfg {
            enabled: false,
            model: default_semantic_model(),
            candidates: default_semantic_candidates(),
            min_similarity: default_min_similarity(),
        }
    }
}

fn default_semantic_model() -> String {
    "minishlab/potion-base-8M".to_string()
}

fn default_semantic_candidates() -> usize {
    50
}

/// Tuned against the semantic eval (`tests/fixtures/search-eval/semantic.toml`).
fn default_min_similarity() -> f32 {
    0.3
}

fn default_stemmers() -> Vec<String> {
    vec!["english".to_string()]
}

impl SearchCfg {
    /// Reject stemmer names that are not Snowball algorithms and
    /// out-of-range semantic settings.
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.semantic.candidates == 0 {
            return Err("[search.semantic] candidates must be at least 1".to_string());
        }
        if !(-1.0..=1.0).contains(&self.semantic.min_similarity) {
            return Err(format!(
                "[search.semantic] min_similarity must be within -1..1 (got {})",
                self.semantic.min_similarity
            ));
        }
        for name in &self.stemmers {
            if crate::search::text::stemmer_algorithm(name).is_none() {
                return Err(format!(
                    "[search] stemmers: unknown algorithm `{name}` (valid: {})",
                    crate::search::text::STEMMER_NAMES.join(", ")
                ));
            }
        }
        Ok(())
    }
}

/// Drift-report configuration (`ft notes drift`).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Drift {
    /// Glob patterns (`*` matches any sequence including `/`, `?` one
    /// character; case-insensitive) excluding concepts from the drift
    /// report. Matched against ghost target strings and, for notes,
    /// the filename stem and vault-relative path. Typical use:
    /// `exclude = ["*.png", "*.pdf"]` to keep linked attachments out.
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// TUI configuration. The Tasks and Timeblocks tabs are adjacent
/// features (the vault workflow lives in the note-flow tabs), so they
/// are opt-in and default to off.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tui {
    /// Show the Tasks tab in the TUI. Defaults to `false`.
    #[serde(default)]
    pub tasks_tab: bool,
    /// Show the Timeblocks tab in the TUI. Defaults to `false`.
    #[serde(default)]
    pub timeblocks_tab: bool,
}

impl Default for Synth {
    fn default() -> Self {
        Self {
            folder: default_synth_folder(),
            exclude_prefixes: Vec::new(),
        }
    }
}

fn default_synth_folder() -> String {
    "Synthesis/".into()
}

/// Editor handoff configuration — how the TUI launches `$EDITOR` when
/// a tab raises `OpenInEditor`.
///
/// The `tmux-*` strategies require ft to be running inside tmux
/// (`$TMUX` env var set); when ft is invoked outside tmux, any
/// `tmux-*` value collapses to [`EditorStrategy::Suspend`] at use time
/// via [`EditorStrategy::resolve`].
///
/// Default: `tmux-popup`. Outside tmux this resolves to `suspend`, so
/// users who don't use tmux see no behavior change versus pre-plan-011.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Editor {
    #[serde(default)]
    pub strategy: EditorStrategy,
    /// Popup width passed to `tmux display-popup -w` when
    /// `strategy = tmux-popup`. Accepts tmux geometry syntax:
    /// percentages (`"90%"`) or cell counts (`"120"`).
    #[serde(default = "default_popup_width")]
    pub popup_width: String,
    /// Popup height — same syntax as [`Self::popup_width`].
    #[serde(default = "default_popup_height")]
    pub popup_height: String,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            strategy: EditorStrategy::default(),
            popup_width: default_popup_width(),
            popup_height: default_popup_height(),
        }
    }
}

fn default_popup_width() -> String {
    "90%".into()
}
fn default_popup_height() -> String {
    "90%".into()
}

/// How to launch `$EDITOR` from the TUI. See [`Editor`] for the
/// `$TMUX`-fallback rule and the geometry knobs that apply only to
/// `TmuxPopup`.
#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EditorStrategy {
    /// `tmux display-popup -E -- <editor> +N <path>`. The popup's
    /// lifetime is bound to the editor; ESC and other keys forward
    /// through to the editor, the popup closes when the editor exits.
    /// Requires tmux ≥ 3.2.
    #[default]
    TmuxPopup,
    /// `tmux new-window -- <editor> +N <path>`. Editor lands in a new
    /// tmux window; ft stays visible in the original window. ft
    /// blocks via a `tmux wait-for` handshake so the post-edit
    /// refresh runs when the editor closes.
    TmuxWindow,
    /// `tmux split-window -- <editor> +N <path>`. Editor lands in a
    /// split of the current pane. Same `wait-for` handshake as
    /// `TmuxWindow`.
    TmuxSplit,
    /// Current behavior — suspend ft's alt-screen, run the editor
    /// inline, restore the alt-screen on exit. The `tmux-*`
    /// strategies all fall back to this when `$TMUX` is unset.
    Suspend,
}

impl EditorStrategy {
    /// Returns the effective strategy after applying the `$TMUX`
    /// fallback rule. When ft is not running inside tmux, every
    /// `tmux-*` value collapses to `Suspend`. `Suspend` is the
    /// identity.
    ///
    /// Pure modulo the env-var read; tests toggle `$TMUX` to drive
    /// both branches.
    pub fn resolve(self) -> Self {
        let in_tmux = std::env::var_os("TMUX").is_some_and(|v| !v.is_empty());
        match self {
            Self::Suspend => Self::Suspend,
            Self::TmuxPopup | Self::TmuxWindow | Self::TmuxSplit if !in_tmux => Self::Suspend,
            other => other,
        }
    }
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
/// Exception: the `[keymap]` table is replaced whole by the vault config when
/// it is present — no per-entry merging across the two files.
pub fn load(user_config: &Path, vault_config: &Path) -> Result<LayeredConfig> {
    let mut config = Figment::new()
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

    // Figment deep-merges maps, which would combine chord keys from both
    // config files. For keymap the spec requires vault-replaces-user whole:
    // determine which source (if any) defines [keymap] and use it exclusively.
    config.keymap = effective_keymap(user_config, vault_config);

    for (name, src) in &config.sources {
        src.validate(name).map_err(|message| Error::ConfigInvalid {
            path: vault_config.display().to_string(),
            message,
        })?;
    }
    config
        .search
        .validate()
        .map_err(|message| Error::ConfigInvalid {
            path: vault_config.display().to_string(),
            message,
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

/// Parse a single TOML file and return its `keymap` section if present.
fn keymap_from_file(path: &Path) -> Option<KeymapConfig> {
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let value: toml::Value = content.parse().ok()?;
    let km_value = value.get("keymap")?;
    toml::Value::try_into(km_value.clone()).ok()
}

/// Vault replaces user whole: use vault's keymap if it defines one,
/// otherwise fall back to user's.
fn effective_keymap(user_config: &Path, vault_config: &Path) -> Option<KeymapConfig> {
    keymap_from_file(vault_config).or_else(|| keymap_from_file(user_config))
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
    fn periodic_notes_week_start_defaults_to_monday() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[periodic_notes.weekly]
path = "journal/%Y"
format = "%Y-%m-%d"
"#,
        )
        .unwrap();
        let lc = load(user.path(), &tmp.path().join("no-vault.toml")).unwrap();
        let w = lc.config.periodic_notes.weekly.as_ref().unwrap();
        assert_eq!(w.week_start, WeekStart::Monday);
    }

    #[test]
    fn periodic_notes_week_start_sunday_parses() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[periodic_notes.weekly]
path = "journal/%Y"
format = "%Y-%m-%d"
week_start = "sunday"
"#,
        )
        .unwrap();
        let lc = load(user.path(), &tmp.path().join("no-vault.toml")).unwrap();
        let w = lc.config.periodic_notes.weekly.as_ref().unwrap();
        assert_eq!(w.week_start, WeekStart::Sunday);
    }

    #[test]
    fn periodic_notes_week_start_invalid_errors() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[periodic_notes.weekly]
path = "journal/%Y"
format = "%Y-%m-%d"
week_start = "tuesday"
"#,
        )
        .unwrap();
        assert!(load(user.path(), &tmp.path().join("no-vault.toml")).is_err());
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
[tasks.presets]
work = "tag is #work and not done"
"#,
        )
        .unwrap();

        let lc = load(user.path(), &tmp.path().join("no-vault.toml")).unwrap();
        assert_eq!(
            lc.config.tasks.presets.get("work").map(|s| s.as_str()),
            Some("tag is #work and not done")
        );
    }

    #[test]
    fn legacy_top_level_presets_section_is_rejected() {
        // The pre-change location was a top-level `[presets]` section.
        // After the move to `[tasks.presets]`, `Config`'s
        // `deny_unknown_fields` rejects the stray field at load time so
        // the user gets a loud error rather than a silent ignore.
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[presets]
work = "tag is #work and not done"
"#,
        )
        .unwrap();

        let r = load(user.path(), &tmp.path().join("no-vault.toml"));
        assert!(r.is_err(), "legacy top-level [presets] must be rejected");
        let msg = r.unwrap_err().to_string();
        assert!(
            msg.contains("presets"),
            "error should name the stray field `presets`: {msg}"
        );
    }

    #[test]
    fn retag_tags_round_trips() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[tasks]
retag_tags = ["wait", "computer", "physical"]
"#,
        )
        .unwrap();

        let lc = load(user.path(), &tmp.path().join("no-vault.toml")).unwrap();
        assert_eq!(
            lc.config.tasks.retag_tags,
            vec![
                "wait".to_string(),
                "computer".to_string(),
                "physical".to_string()
            ]
        );
    }

    #[test]
    fn retag_tags_defaults_empty() {
        let tmp = TempDir::new().unwrap();
        let lc = load(
            &tmp.path().join("no-user.toml"),
            &tmp.path().join("no-vault.toml"),
        )
        .unwrap();
        assert!(lc.config.tasks.retag_tags.is_empty());
    }

    // ── [editor] block (plan 011 session 1) ──────────────────────────────

    /// Shared mutex so the `$TMUX`-toggling tests don't race each other
    /// (or other tests that read env vars). Local to the editor tests
    /// because `vault.rs` has its own copy for its own env-var tests.
    static EDITOR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn editor_defaults_when_block_absent() {
        let tmp = TempDir::new().unwrap();
        let lc = load(
            &tmp.path().join("no-user.toml"),
            &tmp.path().join("no-vault.toml"),
        )
        .unwrap();
        assert_eq!(lc.config.editor.strategy, EditorStrategy::TmuxPopup);
        assert_eq!(lc.config.editor.popup_width, "90%");
        assert_eq!(lc.config.editor.popup_height, "90%");
    }

    #[test]
    fn editor_strategy_kebab_case_variants_parse() {
        for (s, expected) in [
            ("tmux-popup", EditorStrategy::TmuxPopup),
            ("tmux-window", EditorStrategy::TmuxWindow),
            ("tmux-split", EditorStrategy::TmuxSplit),
            ("suspend", EditorStrategy::Suspend),
        ] {
            let tmp = TempDir::new().unwrap();
            let vault = tmp.child("vault.toml");
            vault
                .write_str(&format!("[editor]\nstrategy = \"{s}\"\n"))
                .unwrap();
            let lc = load(&tmp.path().join("no-user.toml"), vault.path()).unwrap();
            assert_eq!(
                lc.config.editor.strategy, expected,
                "strategy={s:?} should parse to {expected:?}"
            );
        }
    }

    #[test]
    fn editor_unknown_strategy_rejected() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str("[editor]\nstrategy = \"vsplit\"\n")
            .unwrap();
        let r = load(&tmp.path().join("no-user.toml"), vault.path());
        assert!(r.is_err(), "unknown strategy must be rejected");
    }

    #[test]
    fn editor_unknown_field_rejected() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str("[editor]\nstrategy = \"suspend\"\ntypo_field = 1\n")
            .unwrap();
        let r = load(&tmp.path().join("no-user.toml"), vault.path());
        assert!(r.is_err(), "deny_unknown_fields should reject typos");
    }

    #[test]
    fn editor_popup_geometry_overrides_apply() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[editor]
strategy = "tmux-popup"
popup_width = "80"
popup_height = "50%"
"#,
            )
            .unwrap();
        let lc = load(&tmp.path().join("no-user.toml"), vault.path()).unwrap();
        assert_eq!(lc.config.editor.popup_width, "80");
        assert_eq!(lc.config.editor.popup_height, "50%");
    }

    #[test]
    fn editor_strategy_resolve_passes_through_when_in_tmux() {
        let _guard = EDITOR_ENV_LOCK.lock().unwrap();
        std::env::set_var("TMUX", "/tmp/tmux-1000/default,1234,0");
        assert_eq!(
            EditorStrategy::TmuxPopup.resolve(),
            EditorStrategy::TmuxPopup
        );
        assert_eq!(
            EditorStrategy::TmuxWindow.resolve(),
            EditorStrategy::TmuxWindow
        );
        assert_eq!(
            EditorStrategy::TmuxSplit.resolve(),
            EditorStrategy::TmuxSplit
        );
        assert_eq!(EditorStrategy::Suspend.resolve(), EditorStrategy::Suspend);
        std::env::remove_var("TMUX");
    }

    #[test]
    fn editor_strategy_resolve_falls_back_to_suspend_outside_tmux() {
        let _guard = EDITOR_ENV_LOCK.lock().unwrap();
        std::env::remove_var("TMUX");
        assert_eq!(EditorStrategy::TmuxPopup.resolve(), EditorStrategy::Suspend);
        assert_eq!(
            EditorStrategy::TmuxWindow.resolve(),
            EditorStrategy::Suspend
        );
        assert_eq!(EditorStrategy::TmuxSplit.resolve(), EditorStrategy::Suspend);
        assert_eq!(EditorStrategy::Suspend.resolve(), EditorStrategy::Suspend);
    }

    // ── [git] block (plan 012 session 1) ─────────────────────────────────

    #[test]
    fn git_defaults_when_block_absent() {
        let tmp = TempDir::new().unwrap();
        let lc = load(
            &tmp.path().join("no-user.toml"),
            &tmp.path().join("no-vault.toml"),
        )
        .unwrap();
        assert_eq!(lc.config.git.pull_strategy, PullStrategy::Merge);
    }

    #[test]
    fn git_pull_strategy_kebab_case_variants_parse() {
        for (s, expected) in [
            ("merge", PullStrategy::Merge),
            ("rebase", PullStrategy::Rebase),
        ] {
            let tmp = TempDir::new().unwrap();
            let vault = tmp.child("vault.toml");
            vault
                .write_str(&format!("[git]\npull_strategy = \"{s}\"\n"))
                .unwrap();
            let lc = load(&tmp.path().join("no-user.toml"), vault.path()).unwrap();
            assert_eq!(
                lc.config.git.pull_strategy, expected,
                "pull_strategy={s:?} should parse to {expected:?}"
            );
        }
    }

    #[test]
    fn git_unknown_strategy_rejected() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str("[git]\npull_strategy = \"squash\"\n")
            .unwrap();
        let r = load(&tmp.path().join("no-user.toml"), vault.path());
        assert!(r.is_err(), "unknown pull_strategy must be rejected");
    }

    #[test]
    fn git_unknown_field_rejected() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str("[git]\npull_strategy = \"merge\"\ntypo_field = 1\n")
            .unwrap();
        let r = load(&tmp.path().join("no-user.toml"), vault.path());
        assert!(r.is_err(), "deny_unknown_fields should reject typos");
    }

    // ── [editor] block (plan 011 session 1) — continued ──────────────────

    // ── [timeblocks] block (plan 015 session 1) ──────────────────────────

    #[test]
    fn timeblocks_default_heading_is_time_blocks() {
        let tmp = TempDir::new().unwrap();
        let lc = load(
            &tmp.path().join("no-user.toml"),
            &tmp.path().join("no-vault.toml"),
        )
        .unwrap();
        assert!(lc.config.timeblocks.heading.is_none());
        assert_eq!(lc.config.timeblocks_heading(), "Time Blocks");
    }

    #[test]
    fn timeblocks_heading_override_applies() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[timeblocks]
heading = "Day Planner"
"#,
            )
            .unwrap();
        let lc = load(&tmp.path().join("no-user.toml"), vault.path()).unwrap();
        assert_eq!(lc.config.timeblocks_heading(), "Day Planner");
    }

    #[test]
    fn timeblocks_unknown_field_rejected() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[timeblocks]
heading = "Time Blocks"
typo_field = "oops"
"#,
            )
            .unwrap();
        let r = load(&tmp.path().join("no-user.toml"), vault.path());
        assert!(r.is_err(), "deny_unknown_fields should reject typos");
    }

    // ── [graph] block (plan 019 session 3) ──────────────────────────────

    #[test]
    fn graph_default_query_unset_when_block_absent() {
        let tmp = TempDir::new().unwrap();
        let lc = load(
            &tmp.path().join("no-user.toml"),
            &tmp.path().join("no-vault.toml"),
        )
        .unwrap();
        assert!(lc.config.graph.default_query.is_none());
    }

    #[test]
    fn graph_default_query_round_trips() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str("[graph]\ndefault_query = \"node where kind = Directory;\"\n")
            .unwrap();
        let lc = load(&tmp.path().join("no-user.toml"), vault.path()).unwrap();
        assert_eq!(
            lc.config.graph.default_query.as_deref(),
            Some("node where kind = Directory;")
        );
    }

    #[test]
    fn graph_presets_default_empty() {
        let tmp = TempDir::new().unwrap();
        let lc = load(
            &tmp.path().join("no-user.toml"),
            &tmp.path().join("no-vault.toml"),
        )
        .unwrap();
        assert!(lc.config.graph.presets.is_empty());
    }

    #[test]
    fn graph_presets_round_trips() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[graph.presets]
my-backlinks = "node where title includes \"Foo\"; expand where edge.kind = note-link;"
"#,
            )
            .unwrap();
        let lc = load(&tmp.path().join("no-user.toml"), vault.path()).unwrap();
        assert_eq!(
            lc.config
                .graph
                .presets
                .get("my-backlinks")
                .map(|s| s.as_str()),
            Some("node where title includes \"Foo\"; expand where edge.kind = note-link;")
        );
    }

    #[test]
    fn graph_unknown_field_rejected() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str("[graph]\ndefault_query = \"node;\"\ntypo_field = 1\n")
            .unwrap();
        let r = load(&tmp.path().join("no-user.toml"), vault.path());
        assert!(r.is_err(), "deny_unknown_fields should reject typos");
    }

    #[test]
    fn editor_strategy_resolve_treats_empty_tmux_as_unset() {
        // tmux unsets TMUX inside `tmux detach` and a few other paths;
        // an empty value should be treated like "not in tmux".
        let _guard = EDITOR_ENV_LOCK.lock().unwrap();
        std::env::set_var("TMUX", "");
        assert_eq!(EditorStrategy::TmuxPopup.resolve(), EditorStrategy::Suspend);
        std::env::remove_var("TMUX");
    }

    // ── capture_presets ───────────────────────────────────────────────

    #[test]
    fn capture_preset_append_with_hardcoded_note() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[capture_presets.journal]
action = "append"
template = "daily.md"
note = "Journal/daily.md"
"#,
            )
            .unwrap();
        let lc = load(&tmp.path().join("no-user.toml"), vault.path()).unwrap();
        let p = lc.config.capture_presets.get("journal").unwrap();
        assert_eq!(p.action, CaptureAction::Append);
        assert_eq!(p.template, "daily.md");
        assert_eq!(p.note.as_deref(), Some("Journal/daily.md"));
        assert!(p.path.is_none());
    }

    #[test]
    fn capture_preset_create_with_path() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[capture_presets.meeting]
action = "create"
template = "meeting.md"
path = "%Y-%m-%d meeting"
folder = "Meetings"
"#,
            )
            .unwrap();
        let lc = load(&tmp.path().join("no-user.toml"), vault.path()).unwrap();
        let p = lc.config.capture_presets.get("meeting").unwrap();
        assert_eq!(p.action, CaptureAction::Create);
        assert_eq!(p.template, "meeting.md");
        assert_eq!(p.path.as_deref(), Some("%Y-%m-%d meeting"));
        assert_eq!(p.folder.as_deref(), Some("Meetings"));
    }

    #[test]
    fn capture_preset_unknown_key_rejected() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[capture_presets.bad]
action = "append"
template = "t.md"
not_a_key = "x"
"#,
            )
            .unwrap();
        let r = load(&tmp.path().join("no-user.toml"), vault.path());
        assert!(r.is_err(), "deny_unknown_fields should reject unknown key");
    }

    #[test]
    fn capture_preset_invalid_action_rejected() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[capture_presets.bad]
action = "invalid"
template = "t.md"
"#,
            )
            .unwrap();
        let r = load(&tmp.path().join("no-user.toml"), vault.path());
        assert!(r.is_err(), "invalid action variant should be rejected");
    }

    #[test]
    fn capture_preset_missing_template_rejected() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[capture_presets.bad]
action = "append"
"#,
            )
            .unwrap();
        let r = load(&tmp.path().join("no-user.toml"), vault.path());
        assert!(
            r.is_err(),
            "missing required template field should be rejected"
        );
    }

    // ── [keymap] block ─────────────────────────────────────────────────────

    #[test]
    fn keymap_absent_when_no_section() {
        let tmp = TempDir::new().unwrap();
        let lc = load(
            &tmp.path().join("no-user.toml"),
            &tmp.path().join("no-vault.toml"),
        )
        .unwrap();
        assert!(lc.config.keymap.is_none());
    }

    #[test]
    fn keymap_strict_defaults_to_false() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str("[keymap]\n").unwrap();
        let lc = load(user.path(), &tmp.path().join("no-vault.toml")).unwrap();
        let km = lc.config.keymap.as_ref().unwrap();
        assert!(!km.strict);
        assert!(km.unbind.is_empty());
        assert!(km.scopes.is_empty());
    }

    #[test]
    fn keymap_full_table_parsed() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[keymap]
strict = true

[[keymap.unbind]]
scope = "global"
chord = "g"

[keymap.global]
"Ctrl+s" = "app.sync-git"

[keymap."tab/graph"]
"R" = "graph.refresh"
"#,
        )
        .unwrap();
        let lc = load(user.path(), &tmp.path().join("no-vault.toml")).unwrap();
        let km = lc.config.keymap.as_ref().unwrap();
        assert!(km.strict);
        assert_eq!(km.unbind.len(), 1);
        assert_eq!(km.unbind[0].scope, "global");
        assert_eq!(km.unbind[0].chord, "g");
        let global = km.scopes.get("global").unwrap();
        assert_eq!(
            global.get("Ctrl+s").map(|s| s.as_str()),
            Some("app.sync-git")
        );
        let graph = km.scopes.get("tab/graph").unwrap();
        assert_eq!(graph.get("R").map(|s| s.as_str()), Some("graph.refresh"));
    }

    #[test]
    fn keymap_vault_replaces_user_whole() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[keymap.global]
"Ctrl+s" = "app.sync-git"
"#,
        )
        .unwrap();
        let vault = tmp.child("vault.toml");
        vault
            .write_str(
                r#"
[keymap.global]
"Ctrl+f" = "graph.search"
"#,
            )
            .unwrap();
        let lc = load(user.path(), vault.path()).unwrap();
        let km = lc.config.keymap.as_ref().unwrap();
        let global = km.scopes.get("global").unwrap();
        // Vault's entry is present.
        assert!(global.contains_key("Ctrl+f"));
        // User's entry is absent — vault replaced the whole section.
        assert!(!global.contains_key("Ctrl+s"));
    }

    #[test]
    fn keymap_user_used_when_vault_has_none() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[keymap.global]
"Ctrl+s" = "app.sync-git"
"#,
        )
        .unwrap();
        let lc = load(user.path(), &tmp.path().join("no-vault.toml")).unwrap();
        let km = lc.config.keymap.as_ref().unwrap();
        let global = km.scopes.get("global").unwrap();
        assert!(global.contains_key("Ctrl+s"));
    }

    #[test]
    fn synth_defaults() {
        let tmp = TempDir::new().unwrap();
        let lc = load(
            &tmp.path().join("no-user.toml"),
            &tmp.path().join("no-vault.toml"),
        )
        .unwrap();
        assert_eq!(lc.config.synth.folder, "Synthesis/");
        assert!(lc.config.synth.exclude_prefixes.is_empty());
    }

    #[test]
    fn synth_user_overrides() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[synth]
folder = "Notes/Synth/"
exclude_prefixes = ["Periodic/", "Inbox/"]
"#,
        )
        .unwrap();
        let lc = load(user.path(), &tmp.path().join("no-vault.toml")).unwrap();
        assert_eq!(lc.config.synth.folder, "Notes/Synth/");
        assert_eq!(
            lc.config.synth.exclude_prefixes,
            vec!["Periodic/".to_string(), "Inbox/".to_string()]
        );
    }

    #[test]
    fn synth_unknown_key_rejected() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.child("user.toml");
        user.write_str(
            r#"
[synth]
unknown_key = "oops"
"#,
        )
        .unwrap();
        let result = load(user.path(), &tmp.path().join("no-vault.toml"));
        assert!(result.is_err(), "unknown synth key should be rejected");
    }

    // ── sources ─────────────────────────────────────────────────────

    fn load_vault_toml(body: &str) -> Result<LayeredConfig> {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.child("vault.toml");
        vault.write_str(body).unwrap();
        load(&tmp.path().join("no-user.toml"), vault.path())
    }

    #[test]
    fn search_stemmers_default_and_override() {
        let lc = load_vault_toml("").unwrap();
        assert_eq!(lc.config.search.stemmers, vec!["english"]);
        let lc = load_vault_toml("[search]\nstemmers = [\"english\", \"portuguese\"]\n").unwrap();
        assert_eq!(lc.config.search.stemmers, vec!["english", "portuguese"]);
        let lc = load_vault_toml("[search]\nstemmers = []\n").unwrap();
        assert!(lc.config.search.stemmers.is_empty());
    }

    #[test]
    fn semantic_defaults_and_validation() {
        let lc = load_vault_toml("").unwrap();
        let s = &lc.config.search.semantic;
        assert!(!s.enabled);
        assert_eq!(s.model, "minishlab/potion-base-8M");
        assert_eq!(s.candidates, 50);
        let lc =
            load_vault_toml("[search.semantic]\nenabled = true\nmodel = \"/opt/m\"\n").unwrap();
        assert!(lc.config.search.semantic.enabled);
        assert_eq!(lc.config.search.semantic.model, "/opt/m");
        let err = load_vault_toml("[search.semantic]\ncandidates = 0\n").unwrap_err();
        assert!(err.to_string().contains("candidates"), "{err}");
        let err = load_vault_toml("[search.semantic]\nmin_similarity = 2.0\n").unwrap_err();
        assert!(err.to_string().contains("min_similarity"), "{err}");
        assert!(load_vault_toml("[search.semantic]\nbogus = 1\n").is_err());
    }

    #[test]
    fn unknown_stemmer_is_a_load_error() {
        let err = load_vault_toml("[search]\nstemmers = [\"klingon\"]\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("klingon"), "{msg}");
        assert!(msg.contains("portuguese"), "lists valid names: {msg}");
    }

    #[test]
    fn minimal_jira_source_gets_defaults() {
        let lc = load_vault_toml(
            "[sources.jira]\nkind = \"jira\"\nbase_url = \"https://example.atlassian.net\"\nemail = \"me@example.test\"\n",
        )
        .unwrap();
        let src = &lc.config.sources["jira"];
        assert_eq!(src.kind, SourceKind::Jira);
        assert_eq!(src.folder_or("jira"), "jira");
        assert_eq!(src.interval_minutes, 10);
        assert_eq!(src.comments, 5);
        assert!(src.fetch_parents && src.description);
        assert_eq!(
            src.effective_queries()["mine"],
            "assignee = currentUser() AND statusCategory != Done"
        );
    }

    #[test]
    fn github_source_and_status_map() {
        let lc = load_vault_toml(
            "[sources.gh]\nkind = \"github\"\nfolder = \"issues\"\n[sources.gh.queries]\nwork = \"is:open org:acme\"\n[sources.gh.status_map]\n\"closed:duplicate\" = \"Cancelled\"\n",
        )
        .unwrap();
        let src = &lc.config.sources["gh"];
        assert_eq!(src.effective_queries().len(), 1);
        assert_eq!(src.folder_or("gh"), "issues");
        assert_eq!(
            src.status_map["closed:duplicate"],
            crate::task::Status::Cancelled
        );
    }

    #[test]
    fn source_unknown_key_and_bad_values_rejected() {
        assert!(load_vault_toml("[sources.jira]\nkind = \"jira\"\nintervall = 5\n").is_err());
        assert!(load_vault_toml("[sources.x]\nkind = \"gitlab\"\n").is_err());
        assert!(load_vault_toml(
            "[sources.gh]\nkind = \"github\"\n[sources.gh.status_map]\nopen = \"Later\"\n"
        )
        .is_err());
    }

    #[test]
    fn source_cross_field_validation() {
        let err = load_vault_toml("[sources.j]\nkind = \"jira\"\nemail = \"fake-user@example.test\"\nbase_url = \"http://jira.example.com\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("https://"), "{err}");
        let err = load_vault_toml(
            "[sources.j]\nkind = \"jira\"\nbase_url = \"https://x.atlassian.net\"\n",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("need `email`"), "{err}");
        assert!(load_vault_toml(
            "[sources.j]\nkind = \"jira\"\nemail = \"fake-user@example.test\"\nbase_url = \"http://localhost:8080\"\n"
        )
        .is_ok());
    }

    #[test]
    fn push_and_transitions() {
        let lc = load_vault_toml(
            "[sources.jira]\nkind = \"jira\"\nemail = \"fake-user@example.test\"\nbase_url = \"https://x.atlassian.net\"\npush = false\n[sources.jira.transitions]\nCancelled = \"Won't Do\"\nDone = \"Ship it\"\n",
        )
        .unwrap();
        let src = &lc.config.sources["jira"];
        assert!(!src.push);
        assert_eq!(src.transitions[&crate::task::Status::Cancelled], "Won't Do");
        let gh = load_vault_toml(
            "[sources.gh]\nkind = \"github\"\n[sources.gh.transitions]\nDone = \"x\"\n",
        )
        .unwrap_err()
        .to_string();
        assert!(gh.contains("only valid for jira"), "{gh}");
        let lc = load_vault_toml("[sources.gh]\nkind = \"github\"\n").unwrap();
        assert!(lc.config.sources["gh"].push);
    }
}

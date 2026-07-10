//! Quick capture — config-driven one-shot preset execution.
//!
//! Each preset bundles an action (create/append), a template, and optional
//! target resolution fields. From the TUI, pressing `Q` opens a fuzzy
//! picker over configured preset names. Selecting one either executes the
//! preset immediately (no template vars) or transitions into a var-prompt
//! modal before committing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use ft_core::config::{CaptureAction, CapturePreset};
use ft_core::frontmatter::ft_append_section;
use ft_core::fs::write_atomic;
use ft_core::notes::append::append_template as core_append_template;
use ft_core::notes::template::render as render_template;
use ft_core::vault::Vault;

use crate::tui::{
    notes_actions::{
        create::{build_template_context, discover_template_vars},
        queue_toast,
    },
    tab::{AppRequest, TabCtx, ToastStyle},
    widgets::{EditBuffer, PickerItem, PickerSource},
};

// ── Picker source ────────────────────────────────────────────────────

pub struct CapturePresetPickerSource {
    names: Vec<String>,
    matcher: nucleo_matcher::Matcher,
    buf: Vec<char>,
}

impl CapturePresetPickerSource {
    pub fn new(vault: &Vault) -> Self {
        let mut names: Vec<String> = vault
            .config
            .config
            .capture_presets
            .keys()
            .cloned()
            .collect();
        names.sort();
        Self {
            names,
            matcher: nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT),
            buf: Vec::new(),
        }
    }
}

impl PickerSource for CapturePresetPickerSource {
    type Item = String;

    fn query(&mut self, q: &str, limit: usize) -> Vec<PickerItem<String>> {
        let pat = nucleo_matcher::pattern::Pattern::parse(
            q,
            nucleo_matcher::pattern::CaseMatching::Smart,
            nucleo_matcher::pattern::Normalization::Smart,
        );
        let mut ranked: Vec<(u32, usize, Vec<u32>)> = Vec::new();
        for (i, name) in self.names.iter().enumerate() {
            self.buf.clear();
            let haystack = nucleo_matcher::Utf32Str::new(name, &mut self.buf);
            let mut indices = Vec::new();
            if let Some(score) = pat.indices(haystack, &mut self.matcher, &mut indices) {
                ranked.push((score, i, indices));
            }
        }
        ranked.sort_by_key(|b| std::cmp::Reverse(b.0));
        ranked
            .into_iter()
            .take(limit)
            .map(|(_, i, match_indices)| {
                let name = &self.names[i];
                PickerItem {
                    label: name.clone(),
                    match_indices,
                    data: name.clone(),
                }
            })
            .collect()
    }

    fn initial_items(&mut self, limit: usize) -> Vec<PickerItem<String>> {
        self.names
            .iter()
            .take(limit)
            .map(|name| PickerItem {
                label: name.clone(),
                match_indices: Vec::new(),
                data: name.clone(),
            })
            .collect()
    }
}

// ── Var-prompt state ─────────────────────────────────────────────────

/// State for the capture var-prompt flow. When a capture preset's
/// template references `{{ vars.KEY }}`, the flow pauses here to
/// collect values before committing.
pub struct CaptureVarPromptState {
    /// Everything needed to commit once vars are collected.
    pub commit: CaptureCommit,
    /// Vars already collected (KEY → value).
    pub vars_so_far: BTreeMap<String, String>,
    /// Index into `commit.vars_needed` currently being prompted.
    pub next_idx: usize,
    /// Edit buffer for the current var's value.
    pub buf: EditBuffer,
}

/// Resolved payload for a capture preset commit. Carries everything
/// needed to render + write after vars are collected.
pub struct CaptureCommit {
    /// Which action (append or create).
    pub action: CaptureAction,
    /// Template source text.
    pub template_source: String,
    /// Absolute target path. For append: the existing note. For
    /// create: the new file to write.
    pub target_path: PathBuf,
    /// Section override for append presets (from preset config).
    pub section_override: Option<String>,
    /// Vars that need values before rendering. Empty when the
    /// template has no `vars.*` references.
    pub vars_needed: Vec<String>,
}

/// Result of attempting to execute a capture preset.
#[allow(clippy::large_enum_variant)] // single-slot at App level; size doesn't matter
pub enum CaptureResult {
    /// Executed immediately (no vars to prompt for).
    Executed,
    /// Template has vars — the caller must enter var prompting.
    NeedsVars(CaptureVarPromptState),
}

// ── Public entry points ──────────────────────────────────────────────

/// Resolve and try to execute a capture preset.
///
/// If the template has `vars.*` references, returns
/// [`CaptureResult::NeedsVars`] so the caller can prompt.  Otherwise
/// executes immediately and returns [`CaptureResult::Executed`].
///
/// Errors (bad preset, missing template, missing target, etc.) are
/// returned as `Err(String)` — the caller should surface these as a
/// toast.
pub fn try_execute_preset(
    ctx: &TabCtx,
    preset_name: &str,
    target_note_override: Option<PathBuf>,
) -> Result<CaptureResult, String> {
    let preset = ctx
        .vault
        .config
        .config
        .capture_presets
        .get(preset_name)
        .ok_or_else(|| format!("capture preset {preset_name:?} not found"))?
        .clone();

    // Resolve template.
    let tpl_path = resolve_template_for_preset(ctx.vault, &preset.template)?;
    let source =
        std::fs::read_to_string(&tpl_path).map_err(|e| format!("reading template: {e}"))?;

    // Discover vars.
    let vars_needed = discover_template_vars(&source);

    match preset.action {
        CaptureAction::Append => {
            let target_path = resolve_append_target(ctx, &preset, target_note_override)?;

            if !target_path.exists() {
                return Err(format!(
                    "target note does not exist: {}",
                    target_path.display()
                ));
            }

            let commit = CaptureCommit {
                action: CaptureAction::Append,
                template_source: source,
                target_path,
                section_override: preset.section.clone(),
                vars_needed: vars_needed.clone(),
            };

            if vars_needed.is_empty() {
                commit_capture(ctx, &commit, &BTreeMap::new())?;
                Ok(CaptureResult::Executed)
            } else {
                Ok(CaptureResult::NeedsVars(CaptureVarPromptState {
                    commit,
                    vars_so_far: BTreeMap::new(),
                    next_idx: 0,
                    buf: EditBuffer::default(),
                }))
            }
        }
        CaptureAction::Create => {
            let abs_dest = match preset.path.as_deref() {
                Some(pattern) => resolve_create_pattern(&ctx.vault.path, &preset, pattern)?,
                None => {
                    return Err(
                        "create preset without `path` requires interactive filename prompt (not yet wired)"
                            .to_string(),
                    );
                }
            };

            let commit = CaptureCommit {
                action: CaptureAction::Create,
                template_source: source,
                target_path: abs_dest,
                section_override: None,
                vars_needed: vars_needed.clone(),
            };

            if vars_needed.is_empty() {
                commit_capture(ctx, &commit, &BTreeMap::new())?;
                Ok(CaptureResult::Executed)
            } else {
                Ok(CaptureResult::NeedsVars(CaptureVarPromptState {
                    commit,
                    vars_so_far: BTreeMap::new(),
                    next_idx: 0,
                    buf: EditBuffer::default(),
                }))
            }
        }
    }
}

/// Feed a key event to a [`CaptureVarPromptState`]. Returns `true` when
/// the flow has finished (either committed or cancelled). The caller
/// should check `ctx.pending_request` to see if a commit happened.
pub fn handle_capture_var_key(
    state: &mut CaptureVarPromptState,
    k: KeyEvent,
    ctx: &TabCtx,
) -> bool {
    match k.code {
        KeyCode::Esc => return true,
        KeyCode::Enter => {
            let key_name = state
                .commit
                .vars_needed
                .get(state.next_idx)
                .cloned()
                .unwrap_or_default();
            state.vars_so_far.insert(key_name, state.buf.text.clone());
            state.next_idx += 1;
            if state.next_idx >= state.commit.vars_needed.len() {
                // All vars collected — commit.
                if let Err(e) = commit_capture(ctx, &state.commit, &state.vars_so_far) {
                    queue_toast(ctx, &e, ToastStyle::Error);
                }
                return true;
            } else {
                state.buf.text.clear();
                state.buf.cursor = 0;
            }
        }
        _ => {
            let _ = state.buf.handle_event(k);
        }
    }
    false
}

/// Commit a capture preset with collected vars. Handles both append
/// and create actions.
pub fn commit_capture(
    ctx: &TabCtx,
    commit: &CaptureCommit,
    vars: &BTreeMap<String, String>,
) -> Result<(), String> {
    match commit.action {
        CaptureAction::Append => commit_capture_append(ctx, commit, vars),
        CaptureAction::Create => commit_capture_create(ctx, commit, vars),
    }
}

// ── Commit helpers ───────────────────────────────────────────────────

fn commit_capture_append(
    ctx: &TabCtx,
    commit: &CaptureCommit,
    vars: &BTreeMap<String, String>,
) -> Result<(), String> {
    let file_content =
        std::fs::read_to_string(&commit.target_path).map_err(|e| format!("reading target: {e}"))?;

    let title = commit
        .target_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let tctx = build_template_context(title, ctx.today, vars.clone());
    let rendered = render_catch_unwind(&commit.template_source, &tctx)?;

    let section_heading = commit
        .section_override
        .as_deref()
        .map(String::from)
        .or_else(|| ft_append_section(&file_content));

    let (new_content, insert_line) =
        core_append_template(&file_content, &rendered, section_heading.as_deref())
            .map_err(|e| format!("append: {e}"))?;

    write_atomic(&commit.target_path, &new_content).map_err(|e| format!("write: {e}"))?;

    let rel = commit
        .target_path
        .strip_prefix(&ctx.vault.path)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| commit.target_path.clone());
    ctx.recents.record_open(&rel);
    *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
        path: commit.target_path.clone(),
        line: insert_line,
    });
    Ok(())
}

fn commit_capture_create(
    ctx: &TabCtx,
    commit: &CaptureCommit,
    vars: &BTreeMap<String, String>,
) -> Result<(), String> {
    if let Some(parent) = commit.target_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
        }
    }

    let title = commit
        .target_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let tctx = build_template_context(title, ctx.today, vars.clone());
    let content = render_catch_unwind(&commit.template_source, &tctx)?;

    write_atomic(&commit.target_path, &content).map_err(|e| format!("write: {e}"))?;

    let line_count = content.lines().count().max(1);
    let rel = commit
        .target_path
        .strip_prefix(&ctx.vault.path)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| commit.target_path.clone());
    ctx.recents.record_open(&rel);
    *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
        path: commit.target_path.clone(),
        line: line_count,
    });
    Ok(())
}

// ── Resolution helpers ───────────────────────────────────────────────

fn resolve_template_for_preset(vault: &Vault, template_name: &str) -> Result<PathBuf, String> {
    let candidate = vault.templates_dir().join(template_name);
    if candidate.is_file() {
        return Ok(candidate);
    }
    let candidate = vault.templates_dir().join(format!("{template_name}.md"));
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(format!(
        "template {:?} not found in {}",
        template_name,
        vault.templates_dir().display()
    ))
}

fn resolve_append_target(
    ctx: &TabCtx,
    preset: &CapturePreset,
    target_note_override: Option<PathBuf>,
) -> Result<PathBuf, String> {
    match (preset.note.as_deref(), target_note_override) {
        (Some(note), _) => Ok(resolve_note_path(&ctx.vault.path, note, ctx.today)),
        (None, Some(path)) => Ok(path),
        (None, None) => Err("no target note for append preset".to_string()),
    }
}

fn resolve_note_path(vault_root: &Path, note: &str, today: chrono::NaiveDate) -> PathBuf {
    let note = today.format(note).to_string();
    let p = Path::new(&note);
    let with_ext = if p.extension().is_some_and(|e| e == "md") {
        p.to_path_buf()
    } else {
        PathBuf::from(format!("{note}.md"))
    };
    if with_ext.is_absolute() {
        with_ext
    } else {
        vault_root.join(with_ext)
    }
}

fn resolve_create_pattern(
    vault_root: &Path,
    preset: &CapturePreset,
    pattern: &str,
) -> Result<PathBuf, String> {
    let today = ft_core::dates::today();
    let formatted = today.format(pattern).to_string();
    let filename = if formatted.ends_with(".md") {
        formatted
    } else {
        format!("{formatted}.md")
    };
    let folder = preset.folder.as_deref().unwrap_or("");
    let path = if folder.is_empty() {
        vault_root.join(&filename)
    } else {
        vault_root.join(folder).join(&filename)
    };
    Ok(path)
}

/// Render a template with `catch_unwind` to guard against minijinja
/// panics triggered by specific template syntax bugs in some versions.
fn render_catch_unwind(
    source: &str,
    ctx: &ft_core::notes::template::TemplateContext,
) -> Result<String, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        render_template(source, ctx)
    }))
    .map_err(|_| "template render panicked — check template syntax".to_string())?
    .map_err(|e| format!("template render: {e}"))
}

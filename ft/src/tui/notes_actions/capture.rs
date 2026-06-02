//! Quick capture — config-driven one-shot preset execution.
//!
//! Each preset bundles an action (create/append), a template, and optional
//! target resolution fields. From the TUI, pressing `Q` opens a fuzzy
//! picker over configured preset names. Selecting one executes the preset
//! immediately — no template prompt, and (when the target is derivable
//! from the preset config or tab context) no file prompt.

use std::path::{Path, PathBuf};

use ft_core::config::{CaptureAction, CapturePreset};
use ft_core::fs::write_atomic;
use ft_core::notes::append::{append_template as core_append_template, frontmatter_append_section};
use ft_core::notes::template::render as render_template;
use ft_core::vault::Vault;

use crate::tui::{
    notes_actions::create::build_template_context,
    tab::{AppRequest, TabCtx},
    widgets::{PickerItem, PickerSource},
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

// ── Execution ────────────────────────────────────────────────────────

/// Execute a capture preset. For append presets without a hardcoded
/// `note`, `target_note_override` supplies the target (graph tab:
/// selected note's absolute path). When `None`, the caller must resolve
/// the target via a file picker before calling.
pub fn execute_preset(
    ctx: &TabCtx,
    preset_name: &str,
    target_note_override: Option<PathBuf>,
) -> Result<(), String> {
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

    match preset.action {
        CaptureAction::Append => execute_append_preset(ctx, &preset, &source, target_note_override),
        CaptureAction::Create => execute_create_preset(ctx, &preset, &source),
    }
}

fn execute_append_preset(
    ctx: &TabCtx,
    preset: &CapturePreset,
    template_source: &str,
    target_note_override: Option<PathBuf>,
) -> Result<(), String> {
    let target_path = match (preset.note.as_deref(), target_note_override) {
        (Some(note), _) => resolve_note_path(&ctx.vault.path, note, ctx.today),
        (None, Some(path)) => path,
        (None, None) => {
            return Err("no target note for append preset".to_string());
        }
    };

    if !target_path.exists() {
        return Err(format!(
            "target note does not exist: {}",
            target_path.display()
        ));
    }

    let file_content =
        std::fs::read_to_string(&target_path).map_err(|e| format!("reading target: {e}"))?;

    let title = target_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let tctx = build_template_context(title, ctx.today, Default::default());
    let rendered = render_catch_unwind(template_source, &tctx)?;

    let section_heading = preset
        .section
        .as_deref()
        .map(String::from)
        .or_else(|| frontmatter_append_section(&file_content));

    let (new_content, insert_line) =
        core_append_template(&file_content, &rendered, section_heading.as_deref())
            .map_err(|e| format!("append: {e}"))?;

    write_atomic(&target_path, &new_content).map_err(|e| format!("write: {e}"))?;

    let rel = target_path
        .strip_prefix(&ctx.vault.path)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| target_path.clone());
    ctx.recents.record_open(&rel);
    *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
        path: target_path,
        line: insert_line,
    });
    Ok(())
}

fn execute_create_preset(
    ctx: &TabCtx,
    preset: &CapturePreset,
    template_source: &str,
) -> Result<(), String> {
    let abs_dest = match preset.path.as_deref() {
        Some(pattern) => resolve_create_pattern(&ctx.vault.path, preset, pattern)?,
        None => {
            return Err(
                "create preset without `path` requires interactive filename prompt (not yet wired)"
                    .to_string(),
            );
        }
    };

    if let Some(parent) = abs_dest.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
        }
    }

    let title = abs_dest
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let tctx = build_template_context(title, ctx.today, Default::default());
    let content = render_catch_unwind(template_source, &tctx)?;

    write_atomic(&abs_dest, &content).map_err(|e| format!("write: {e}"))?;

    // Open at last line of the new file.
    let line_count = content.lines().count().max(1);

    let rel = abs_dest
        .strip_prefix(&ctx.vault.path)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| abs_dest.clone());
    ctx.recents.record_open(&rel);
    *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
        path: abs_dest,
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
    let today = std::env::var("FT_TODAY")
        .ok()
        .and_then(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .unwrap_or_else(|| chrono::Local::now().date_naive());
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

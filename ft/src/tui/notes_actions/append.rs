//! Tab-agnostic "append with template" flow.
//!
//! The flow has up to four visible steps:
//! 1. **TemplatePicking** — fuzzy pick a template under the configured
//!    templates dir (same as the create flow's step 1).
//! 2. **FilePicking** — (notes tab only) pick the target note from the
//!    vault. Skipped when the tab already knows the target (graph tab).
//! 3. **VarPrompt** — if the template references `{{ vars.KEY }}`,
//!    prompt for each one in sequence before committing.
//! 4. **Commit** — render the template, read `ft.append.section` from the
//!    target note's frontmatter (unless a section override is supplied),
//!    append, write atomically, open editor at the insertion line.
//!
//! Unlike the create flow, there is no folder picker or filename prompt —
//! the target note already exists.
//!
//! [`handle_key`] is the public entry point; tabs feed every key event
//! through it while the append flow is active.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};
use ft_core::frontmatter::ft_append_section;
use ft_core::fs::write_atomic;
use ft_core::notes::append::append_template as core_append_template;
use ft_core::notes::template::render as render_template;

use crate::tui::{
    notes_actions::{
        create::{self, discover_template_vars, TemplatePick},
        queue_toast,
    },
    tab::{AppRequest, TabCtx, ToastStyle},
    widgets::{
        edit_keymap::EditOutcome, EditBuffer, FuzzyPicker, PathListPickerSource, PickerOutcome,
        VaultFilePickerSource,
    },
};

// ── State ────────────────────────────────────────────────────────────

/// The append flow's state machine. Owned by whichever tab is currently
/// running the flow.
pub enum AppendState {
    /// Step 1: pick a template. If `target_path` is `Some`, the flow
    /// commits directly after template selection (graph tab path).
    /// If `None`, the flow transitions to `FilePicking` (notes tab path).
    TemplatePicking {
        picker: FuzzyPicker<PathListPickerSource>,
        target_path: Option<PathBuf>,
        section_override: Option<String>,
    },
    /// Step 2 (notes tab only): pick the target file after the template
    /// was chosen. `template` is carried forward.
    FilePicking {
        template: TemplatePick,
        section_override: Option<String>,
        picker: FuzzyPicker<VaultFilePickerSource>,
    },
    /// Step 3: prompt for template vars (`{{ vars.KEY }}` references),
    /// one at a time. Commits when all vars have been collected.
    VarPrompt {
        template: TemplatePick,
        target_path: PathBuf,
        section_override: Option<String>,
        vars_so_far: BTreeMap<String, String>,
        /// Index into `template.vars_needed` currently being prompted.
        next_idx: usize,
        buf: EditBuffer,
    },
}

/// Result of feeding a key event to the append flow.
pub enum AppendStep {
    Stay,
    NotHandled,
    Transition(Box<AppendState>),
    Finished,
}

impl AppendState {
    /// Build a `TemplatePicking` state with a known target (graph tab).
    pub fn begin_with_target(
        ctx: &TabCtx,
        target_path: PathBuf,
        section_override: Option<String>,
    ) -> Self {
        let templates = create::enumerate_templates(ctx.vault);
        AppendState::TemplatePicking {
            picker: FuzzyPicker::new(PathListPickerSource::new(templates)),
            target_path: Some(target_path),
            section_override,
        }
    }

    /// Build a `TemplatePicking` state without a target (notes tab).
    /// After the template is chosen, the flow transitions to `FilePicking`.
    pub fn begin_no_target(ctx: &TabCtx, section_override: Option<String>) -> Self {
        let templates = create::enumerate_templates(ctx.vault);
        AppendState::TemplatePicking {
            picker: FuzzyPicker::new(PathListPickerSource::new(templates)),
            target_path: None,
            section_override,
        }
    }
}

// ── Dispatcher ───────────────────────────────────────────────────────

/// Feed a key event to the active `AppendState`.
pub fn handle_key(state: &mut AppendState, k: KeyEvent, ctx: &TabCtx) -> AppendStep {
    match state {
        AppendState::TemplatePicking {
            picker,
            target_path,
            section_override,
        } => handle_template_picker_key(k, picker, target_path, section_override, ctx),
        AppendState::FilePicking {
            template,
            section_override,
            picker,
        } => handle_file_picker_key(k, template, section_override, picker, ctx),
        AppendState::VarPrompt {
            template,
            target_path,
            section_override,
            vars_so_far,
            next_idx,
            buf,
        } => handle_var_key(
            k,
            template,
            target_path,
            section_override,
            vars_so_far,
            next_idx,
            buf,
            ctx,
        ),
    }
}

/// Transition helper: if the template has vars that need prompting, go to
/// VarPrompt; otherwise commit immediately.
fn prompt_vars_or_commit(
    ctx: &TabCtx,
    template: TemplatePick,
    target_path: PathBuf,
    section_override: Option<String>,
) -> AppendStep {
    if template.vars_needed.is_empty() {
        commit_append(
            ctx,
            &template,
            &target_path,
            section_override.as_deref(),
            &BTreeMap::new(),
        );
        AppendStep::Finished
    } else {
        AppendStep::Transition(Box::new(AppendState::VarPrompt {
            template,
            target_path,
            section_override,
            vars_so_far: BTreeMap::new(),
            next_idx: 0,
            buf: EditBuffer::default(),
        }))
    }
}

fn handle_template_picker_key(
    k: KeyEvent,
    picker: &mut FuzzyPicker<PathListPickerSource>,
    target_path: &mut Option<PathBuf>,
    section_override: &mut Option<String>,
    ctx: &TabCtx,
) -> AppendStep {
    match picker.handle_key(k) {
        PickerOutcome::Selected(rel) => {
            let abs = ctx.vault.templates_dir().join(&rel);
            let source = match std::fs::read_to_string(&abs) {
                Ok(s) => s,
                Err(e) => {
                    queue_toast(
                        ctx,
                        &format!("could not read template: {e}"),
                        ToastStyle::Error,
                    );
                    return AppendStep::Finished;
                }
            };
            let vars_needed = discover_template_vars(&source);
            let template = TemplatePick {
                rel,
                source,
                vars_needed,
            };

            if let Some(tgt) = target_path.take() {
                // Graph tab: prompt vars or commit immediately.
                prompt_vars_or_commit(ctx, template, tgt, section_override.take())
            } else {
                // Notes tab: transition to file picker.
                AppendStep::Transition(Box::new(AppendState::FilePicking {
                    template,
                    section_override: section_override.take(),
                    picker: FuzzyPicker::new(VaultFilePickerSource::new(
                        Arc::clone(ctx.vault),
                        Arc::clone(ctx.recents),
                    )),
                }))
            }
        }
        PickerOutcome::Cancelled => AppendStep::Finished,
        PickerOutcome::StillOpen => AppendStep::Stay,
        PickerOutcome::NotHandled => AppendStep::NotHandled,
    }
}

fn handle_file_picker_key(
    k: KeyEvent,
    template: &mut TemplatePick,
    section_override: &mut Option<String>,
    picker: &mut FuzzyPicker<VaultFilePickerSource>,
    ctx: &TabCtx,
) -> AppendStep {
    match picker.handle_key(k) {
        PickerOutcome::Selected(hit) => {
            let abs = ctx.vault.path.join(&hit.path);
            // Take ownership of template and section_override; the state
            // will be replaced with the transition.
            let tpl = TemplatePick {
                rel: std::mem::take(&mut template.rel),
                source: std::mem::take(&mut template.source),
                vars_needed: std::mem::take(&mut template.vars_needed),
            };
            prompt_vars_or_commit(ctx, tpl, abs, section_override.take())
        }
        PickerOutcome::Cancelled => AppendStep::Transition(Box::new(AppendState::begin_no_target(
            ctx,
            section_override.take(),
        ))),
        PickerOutcome::StillOpen => AppendStep::Stay,
        PickerOutcome::NotHandled => AppendStep::NotHandled,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_var_key(
    k: KeyEvent,
    template: &mut TemplatePick,
    target_path: &Path,
    section_override: &mut Option<String>,
    vars_so_far: &mut BTreeMap<String, String>,
    next_idx: &mut usize,
    buf: &mut EditBuffer,
    ctx: &TabCtx,
) -> AppendStep {
    match k.code {
        KeyCode::Esc => AppendStep::Finished,
        KeyCode::Enter => {
            let key_name = template
                .vars_needed
                .get(*next_idx)
                .cloned()
                .unwrap_or_default();
            vars_so_far.insert(key_name, buf.text.clone());
            *next_idx += 1;
            if *next_idx >= template.vars_needed.len() {
                // All vars collected — commit.
                commit_append(
                    ctx,
                    template,
                    target_path,
                    section_override.as_deref(),
                    vars_so_far,
                );
                AppendStep::Finished
            } else {
                buf.text.clear();
                buf.cursor = 0;
                AppendStep::Stay
            }
        }
        // Delegate all text edits + cursor moves to the buffer's
        // EDIT_KEYMAP (Ctrl+A/E, Alt+B/F/D, Ctrl+W, Ctrl+Y, etc.).
        _ => match buf.handle_event(k) {
            EditOutcome::Consumed => AppendStep::Stay,
            EditOutcome::NotHandled => AppendStep::NotHandled,
        },
    }
}

// ── Commit ─────────────────────────────────────────────────────────────

/// Render the template, determine the section target, append, write
/// atomically, and queue an `OpenInEditor` request at the insertion line.
fn commit_append(
    ctx: &TabCtx,
    template: &TemplatePick,
    target_path: &Path,
    section_override: Option<&str>,
    vars: &BTreeMap<String, String>,
) {
    // Read the target file.
    let file_content = match std::fs::read_to_string(target_path) {
        Ok(s) => s,
        Err(e) => {
            queue_toast(
                ctx,
                &format!("could not read target: {e}"),
                ToastStyle::Error,
            );
            return;
        }
    };

    // Derive title from the target file's stem.
    let title = target_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Render the template.
    let tctx = create::build_template_context(title, ctx.today, vars.clone());
    let rendered = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        render_template(&template.source, &tctx)
    })) {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            queue_toast(
                ctx,
                &format!("template render failed: {e}"),
                ToastStyle::Error,
            );
            return;
        }
        Err(_panic) => {
            queue_toast(
                ctx,
                "template render panicked — check template syntax",
                ToastStyle::Error,
            );
            return;
        }
    };

    // Determine section heading: explicit override > frontmatter > None.
    let section_heading = section_override
        .map(String::from)
        .or_else(|| ft_append_section(&file_content));

    // Append.
    let (new_content, insert_line) =
        match core_append_template(&file_content, &rendered, section_heading.as_deref()) {
            Ok(v) => v,
            Err(e) => {
                queue_toast(ctx, &format!("append failed: {e}"), ToastStyle::Error);
                return;
            }
        };

    // Write atomically.
    if let Err(e) = write_atomic(target_path, &new_content) {
        queue_toast(ctx, &format!("write failed: {e}"), ToastStyle::Error);
        return;
    }

    // Record open and queue editor.
    let rel = target_path
        .strip_prefix(&ctx.vault.path)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| target_path.to_path_buf());
    ctx.recents.record_open(&rel);
    *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
        path: target_path.to_path_buf(),
        line: insert_line,
    });
}

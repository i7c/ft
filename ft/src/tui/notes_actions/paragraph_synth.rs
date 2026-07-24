//! Paragraph-synth flow: copy paragraphs from a source note into a
//! target note as protected `[!ft-source]` callouts.
//!
//! This is the source-driven sibling of the gather/recent send-to-synth
//! flows: instead of accepting pre-computed feed paragraphs, the user
//! picks a note, multi-selects its paragraphs (with optional shrink-only
//! range adjust), and commits them via the existing
//! [`plan_synth_scaffold`] + [`apply_synth_scaffold`] path. The source
//! is never mutated (copy, not move); pins go to HEAD.
//!
//! The flow's outer shape mirrors [`section_move`]: a step state
//! machine ([`ParagraphSynthState`]) driven by free-function key
//! handlers returning a [`SynthStep`] outcome. The host modal wraps
//! the handlers (see `ft/src/tui/modal.rs`'s `ParagraphSynth` variant).
//!
//! Entry points:
//! - Graph tab (`y` on a Note node): [`begin_for_source`] seeds directly
//!   into [`ParagraphSynthState::ParagraphMultiSelect`] (no source
//!   picker).
//! - Notes tab (`y`): opens [`ParagraphSynthState::SourcePicking`], a
//!   fuzzy note picker; selecting advances to paragraph multi-select.
//!
//! Both entry points guard on a clean git working tree first (pins go
//! to HEAD, so a dirty tree would produce unverifiable pins).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ft_core::markdown::{extract_paragraphs, Paragraph};
use ft_core::synth::scaffold::{apply_synth_scaffold, plan_synth_scaffold};
use ft_core::synth::source::SynthSource;

use crate::tui::{
    notes_actions::{create::enumerate_vault_folders, queue_toast},
    tab::{AppRequest, TabCtx, ToastStyle},
    widgets::{
        EditBuffer, FuzzyPicker, PathListPickerSource, PickerOutcome, VaultFilePickerSource,
    },
};

// ── State ────────────────────────────────────────────────────────────

/// Per-pick shrink adjustment. Defaults to identity (full paragraph).
/// The effective range is `(line_start + top_trim) ..= (line_end - bot_trim)`,
/// always leaving ≥ 1 line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Adjust {
    pub top_trim: u32,
    pub bot_trim: u32,
}

impl Adjust {
    /// Effective `(line_start, line_end)` for a paragraph with this
    /// adjustment. Clamped so the range always contains ≥ 1 line.
    pub fn effective(&self, p: &Paragraph) -> (u32, u32) {
        let len = p.line_end.saturating_sub(p.line_start) + 1;
        let top = self.top_trim.min(len.saturating_sub(1));
        let bot = self.bot_trim.min(len.saturating_sub(1) - top);
        (p.line_start + top, p.line_end - bot)
    }

    /// Shrink the top by one line (clamped to the 1-line floor).
    fn trim_top(&mut self, p: &Paragraph) {
        let len = p.line_end.saturating_sub(p.line_start) + 1;
        let max = len.saturating_sub(1);
        self.top_trim = (self.top_trim + 1).min(max);
    }

    /// Shrink the bottom by one line (clamped to the 1-line floor).
    fn trim_bot(&mut self, p: &Paragraph) {
        let len = p.line_end.saturating_sub(p.line_start) + 1;
        let max = len.saturating_sub(1);
        self.bot_trim = (self.bot_trim + 1).min(max);
    }
}

/// State machine for the paragraph-synth flow. Variants line up with
/// the steps: source pick (Notes-tab entry) → paragraph multi-select →
/// target pick → (commit, which closes the modal).
pub enum ParagraphSynthState {
    /// Step 1 (Notes-tab entry only): fuzzy pick the source note.
    SourcePicking {
        picker: FuzzyPicker<VaultFilePickerSource>,
    },
    /// Step 2: multi-select paragraphs of `source_rel`, with optional
    /// shrink adjust on the focused pick. `adjust` is keyed by
    /// paragraph index; absent ⇒ identity (full paragraph).
    ParagraphMultiSelect {
        source_rel: PathBuf,
        source_content: String,
        paragraphs: Vec<Paragraph>,
        selected: BTreeSet<usize>,
        adjust: BTreeMap<usize, Adjust>,
        focus: usize,
    },
    /// Step 3a: pick an existing target note. `error` carries the
    /// source-equals-target rejection. The step-2 state is carried so
    /// `Esc` restores multi-select with selections + adjustments intact.
    TargetPicking {
        source_rel: PathBuf,
        source_content: String,
        paragraphs: Vec<Paragraph>,
        selected: BTreeSet<usize>,
        adjust: BTreeMap<usize, Adjust>,
        focus: usize,
        picker: FuzzyPicker<VaultFilePickerSource>,
        error: Option<String>,
    },
    /// Step 3b (`S`): create a new target. Folder → title → blank stub.
    NewTargetFolder {
        source_rel: PathBuf,
        source_content: String,
        paragraphs: Vec<Paragraph>,
        selected: BTreeSet<usize>,
        adjust: BTreeMap<usize, Adjust>,
        focus: usize,
        picker: FuzzyPicker<PathListPickerSource>,
    },
    NewTargetTitle {
        source_rel: PathBuf,
        source_content: String,
        paragraphs: Vec<Paragraph>,
        selected: BTreeSet<usize>,
        adjust: BTreeMap<usize, Adjust>,
        focus: usize,
        folder: PathBuf,
        buf: EditBuffer,
        error: Option<String>,
    },
}

/// Step outcome, mirroring [`crate::tui::notes_actions::section_move::MoveStep`].
#[allow(clippy::large_enum_variant)]
pub enum SynthStep {
    /// Consumed, no state change.
    Stay,
    /// Replace the current state with `next`.
    Transition(ParagraphSynthState),
    /// The flow ended; the host drops the modal slot.
    Finished,
    /// The key was not relevant; the host may try its own bindings.
    NotHandled,
}

// ── Clean-tree guard ─────────────────────────────────────────────────

/// True when the git working tree rooted at the vault is clean (no
/// modified / untracked / deleted / conflicted paths). Pins go to HEAD,
/// so a dirty tree would produce unverifiable pins — callers refuse to
/// open the flow when this returns false (with a toast).
fn working_tree_clean(vault_root: &Path) -> bool {
    match ft_core::git::RepoMap::discover(vault_root) {
        Ok(repo) => match ft_core::git::status(repo.root()) {
            Ok(s) => s.is_clean(),
            // Can't determine status → treat as dirty (refuse).
            Err(_) => false,
        },
        Err(_) => false,
    }
}

fn refuse_dirty_tree(ctx: &TabCtx) -> bool {
    if working_tree_clean(&ctx.vault.path) {
        false
    } else {
        queue_toast(
            ctx,
            "synth needs a clean working tree (commit or stash first)",
            ToastStyle::Error,
        );
        true
    }
}

// ── Entry points ─────────────────────────────────────────────────────

/// Begin the flow from a known source note (Graph-tab entry): skip the
/// source picker and open directly at paragraph multi-select. Returns
/// `None` — after queuing an error toast — when the tree is dirty, the
/// source can't be read, or it has no paragraphs.
pub fn begin_for_source(ctx: &TabCtx, source_rel: PathBuf) -> Option<ParagraphSynthState> {
    if refuse_dirty_tree(ctx) {
        return None;
    }
    let abs = ctx.vault.path.join(&source_rel);
    let content = match std::fs::read_to_string(&abs) {
        Ok(s) => s,
        Err(e) => {
            queue_toast(
                ctx,
                &format!("could not read source: {e}"),
                ToastStyle::Error,
            );
            return None;
        }
    };
    let paragraphs = extract_paragraphs(&content);
    if paragraphs.is_empty() {
        queue_toast(ctx, "source has no paragraphs to pin", ToastStyle::Error);
        return None;
    }
    Some(ParagraphSynthState::ParagraphMultiSelect {
        source_rel,
        source_content: content,
        paragraphs,
        selected: BTreeSet::new(),
        adjust: BTreeMap::new(),
        focus: 0,
    })
}

/// Begin the flow at the source-note picker (Notes-tab entry). Returns
/// `None` when the tree is dirty (toast already queued).
pub fn begin_with_picker(ctx: &TabCtx) -> Option<ParagraphSynthState> {
    if refuse_dirty_tree(ctx) {
        return None;
    }
    Some(ParagraphSynthState::SourcePicking {
        picker: FuzzyPicker::new(VaultFilePickerSource::new(
            Arc::clone(ctx.vault),
            Arc::clone(ctx.recents),
        )),
    })
}

// ── Step transitions ─────────────────────────────────────────────────

/// Advance from the source picker to paragraph multi-select on a pick.
fn advance_to_multiselect(ctx: &TabCtx, source_rel: PathBuf) -> SynthStep {
    let abs = ctx.vault.path.join(&source_rel);
    let content = match std::fs::read_to_string(&abs) {
        Ok(s) => s,
        Err(e) => {
            queue_toast(
                ctx,
                &format!("could not read source: {e}"),
                ToastStyle::Error,
            );
            return SynthStep::Finished;
        }
    };
    let paragraphs = extract_paragraphs(&content);
    if paragraphs.is_empty() {
        queue_toast(ctx, "source has no paragraphs to pin", ToastStyle::Error);
        return SynthStep::Finished;
    }
    SynthStep::Transition(ParagraphSynthState::ParagraphMultiSelect {
        source_rel,
        source_content: content,
        paragraphs,
        selected: BTreeSet::new(),
        adjust: BTreeMap::new(),
        focus: 0,
    })
}

/// Carry the step-2 state fields into a step-3 transition, taking the
/// `ParagraphMultiSelect` fields by move to avoid cloning.
#[allow(clippy::too_many_arguments)]
fn carry_to_target(
    source_rel: PathBuf,
    source_content: String,
    paragraphs: Vec<Paragraph>,
    selected: BTreeSet<usize>,
    adjust: BTreeMap<usize, Adjust>,
    focus: usize,
    picker: FuzzyPicker<VaultFilePickerSource>,
    error: Option<String>,
) -> ParagraphSynthState {
    ParagraphSynthState::TargetPicking {
        source_rel,
        source_content,
        paragraphs,
        selected,
        adjust,
        focus,
        picker,
        error,
    }
}

// ── Effective source building ─────────────────────────────────────────

/// Build the `SynthSource` values for each selected paragraph, applying
/// its adjustment (if any). The body is re-sliced from the source
/// content at the effective line range (1-indexed inclusive).
fn build_sources(
    source_rel: &Path,
    source_content: &str,
    paragraphs: &[Paragraph],
    selected: &BTreeSet<usize>,
    adjust: &BTreeMap<usize, Adjust>,
) -> Vec<SynthSource> {
    let lines: Vec<&str> = source_content.lines().collect();
    let mut out = Vec::with_capacity(selected.len());
    for &i in selected {
        let Some(p) = paragraphs.get(i) else {
            continue;
        };
        let adj = adjust.get(&i).copied().unwrap_or_default();
        let (start, end) = adj.effective(p);
        // 1-indexed inclusive → 0-indexed slice. Clamp to the lines we have.
        let s0 = (start as usize).saturating_sub(1).min(lines.len());
        let e0 = (end as usize).min(lines.len());
        let body = lines[s0..e0].join("\n");
        out.push(SynthSource {
            source_path: source_rel.to_path_buf(),
            line_start: start,
            line_end: end,
            body,
        });
    }
    out
}

// ── Commit ────────────────────────────────────────────────────────────

/// Plan + apply + editor handoff. `target_rel` is vault-relative. On
/// append to an existing note, the target is marked synth first. On a
/// dirty-source error (a source dirtied mid-flow), the step-2 state is
/// returned so the user can retry after committing/stashing.
#[allow(clippy::too_many_arguments)]
fn commit(
    ctx: &TabCtx,
    source_rel: PathBuf,
    source_content: String,
    paragraphs: Vec<Paragraph>,
    selected: BTreeSet<usize>,
    adjust: BTreeMap<usize, Adjust>,
    focus: usize,
    target_rel: PathBuf,
) -> SynthStep {
    let sources = build_sources(
        &source_rel,
        &source_content,
        &paragraphs,
        &selected,
        &adjust,
    );
    if sources.is_empty() {
        queue_toast(ctx, "synth: no paragraphs selected", ToastStyle::Error);
        return SynthStep::Stay;
    }

    let target_abs = ctx.vault.path.join(&target_rel);
    let exists = target_abs.exists();
    if exists {
        // Mark an existing non-synth target before appending.
        if let Err(e) = crate::tui::tabs::gather::mark_note_as_synth(&target_abs) {
            queue_toast(
                ctx,
                &format!("could not add ft.synth marker: {e}"),
                ToastStyle::Error,
            );
            return SynthStep::Stay;
        }
    }

    let plan = match plan_synth_scaffold(ctx.vault, &target_rel, &sources) {
        Ok(p) => p,
        Err(e) => {
            queue_toast(ctx, &format!("synth plan failed: {e}"), ToastStyle::Error);
            // A dirty-source error means retry from paragraph select.
            return SynthStep::Transition(ParagraphSynthState::ParagraphMultiSelect {
                source_rel,
                source_content,
                paragraphs,
                selected,
                adjust,
                focus,
            });
        }
    };
    let dedup_skipped = plan.dedup_skipped;
    let section_count = plan.sections.len();
    let written = match apply_synth_scaffold(ctx.vault, &plan) {
        Ok(p) => p,
        Err(e) => {
            queue_toast(ctx, &format!("synth write failed: {e}"), ToastStyle::Error);
            return SynthStep::Stay;
        }
    };
    let msg = if dedup_skipped > 0 {
        format!(
            "{} {} synth section(s) to {} ({} already pinned, skipped)",
            if plan.create { "created" } else { "appended" },
            section_count,
            target_rel.display(),
            dedup_skipped,
        )
    } else {
        format!(
            "{} {} synth section(s) to {}",
            if plan.create { "created" } else { "appended" },
            section_count,
            target_rel.display(),
        )
    };
    queue_toast(ctx, &msg, ToastStyle::Success);
    *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
        path: written,
        line: 1,
    });
    let _ = focus; // carried only for the error-return path above
    SynthStep::Finished
}

// ── Key handlers ──────────────────────────────────────────────────────

/// Top-level key handler. Dispatches to the per-step handler.
pub fn handle_key(state: &mut ParagraphSynthState, k: KeyEvent, ctx: &TabCtx) -> SynthStep {
    match state {
        ParagraphSynthState::SourcePicking { picker } => handle_source_picker_key(k, picker, ctx),
        ParagraphSynthState::ParagraphMultiSelect {
            source_rel,
            source_content,
            paragraphs,
            selected,
            adjust,
            focus,
            ..
        } => handle_multiselect_key(
            k,
            ctx,
            source_rel,
            source_content,
            paragraphs,
            selected,
            adjust,
            focus,
        ),
        ParagraphSynthState::TargetPicking {
            source_rel,
            source_content,
            paragraphs,
            selected,
            adjust,
            focus,
            picker,
            error,
        } => handle_target_picker_key(
            k,
            ctx,
            source_rel,
            source_content,
            paragraphs,
            selected,
            adjust,
            focus,
            picker,
            error,
        ),
        ParagraphSynthState::NewTargetFolder {
            source_rel,
            source_content,
            paragraphs,
            selected,
            adjust,
            focus,
            picker,
        } => handle_new_target_folder_key(
            k,
            ctx,
            source_rel,
            source_content,
            paragraphs,
            selected,
            adjust,
            focus,
            picker,
        ),
        ParagraphSynthState::NewTargetTitle {
            source_rel,
            source_content,
            paragraphs,
            selected,
            adjust,
            focus,
            folder,
            buf,
            error,
        } => handle_new_target_title_key(
            k,
            ctx,
            source_rel,
            source_content,
            paragraphs,
            selected,
            adjust,
            focus,
            folder,
            buf,
            error,
        ),
    }
}

fn handle_source_picker_key(
    k: KeyEvent,
    picker: &mut FuzzyPicker<VaultFilePickerSource>,
    ctx: &TabCtx,
) -> SynthStep {
    match picker.handle_key(k) {
        PickerOutcome::Selected(hit) => advance_to_multiselect(ctx, hit.path),
        PickerOutcome::Cancelled | PickerOutcome::NotHandled => SynthStep::Finished,
        PickerOutcome::StillOpen => SynthStep::Stay,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_multiselect_key(
    k: KeyEvent,
    ctx: &TabCtx,
    source_rel: &mut PathBuf,
    source_content: &mut String,
    paragraphs: &mut Vec<Paragraph>,
    selected: &mut BTreeSet<usize>,
    adjust: &mut BTreeMap<usize, Adjust>,
    focus: &mut usize,
) -> SynthStep {
    let n = paragraphs.len();
    match (k.code, k.modifiers) {
        (KeyCode::Esc, _) => {
            // Graph-tab entry has no prior step → close. Notes-tab entry
            // returns to the source picker.
            SynthStep::Transition(ParagraphSynthState::SourcePicking {
                picker: FuzzyPicker::new(VaultFilePickerSource::new(
                    Arc::clone(ctx.vault),
                    Arc::clone(ctx.recents),
                )),
            })
        }
        (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
            if *focus > 0 {
                *focus -= 1;
            } else if n > 0 {
                *focus = n - 1;
            }
            SynthStep::Stay
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
            if n > 0 {
                *focus = (*focus + 1) % n;
            }
            SynthStep::Stay
        }
        (KeyCode::Char(' '), _) => {
            if n > 0 && !selected.insert(*focus) {
                selected.remove(focus);
            }
            SynthStep::Stay
        }
        // Shrink the focused paragraph's top.
        (KeyCode::Char('['), _) => {
            if let Some(p) = paragraphs.get(*focus) {
                adjust.entry(*focus).or_default().trim_top(p);
            }
            SynthStep::Stay
        }
        // Shrink the focused paragraph's bottom.
        (KeyCode::Char(']'), _) => {
            if let Some(p) = paragraphs.get(*focus) {
                adjust.entry(*focus).or_default().trim_bot(p);
            }
            SynthStep::Stay
        }
        // Reset the focused paragraph to full range.
        (KeyCode::Char('r'), KeyModifiers::NONE) => {
            adjust.remove(focus);
            SynthStep::Stay
        }
        (KeyCode::Char('s'), KeyModifiers::NONE) => advance_to_target_existing(
            ctx,
            source_rel,
            source_content,
            paragraphs,
            selected,
            adjust,
            focus,
        ),
        (KeyCode::Char('S'), _) => {
            let folders = enumerate_vault_folders(ctx.vault);
            SynthStep::Transition(ParagraphSynthState::NewTargetFolder {
                source_rel: std::mem::take(source_rel),
                source_content: std::mem::take(source_content),
                paragraphs: std::mem::take(paragraphs),
                selected: std::mem::take(selected),
                adjust: std::mem::take(adjust),
                focus: *focus,
                picker: FuzzyPicker::new(PathListPickerSource::new(folders)),
            })
        }
        (KeyCode::Enter, _) => {
            if selected.is_empty() {
                queue_toast(ctx, "select at least one paragraph", ToastStyle::Error);
                return SynthStep::Stay;
            }
            // Enter defaults to existing-note target pick (same as `s`).
            advance_to_target_existing(
                ctx,
                source_rel,
                source_content,
                paragraphs,
                selected,
                adjust,
                focus,
            )
        }
        _ => SynthStep::NotHandled,
    }
}

#[allow(clippy::too_many_arguments)]
fn advance_to_target_existing(
    ctx: &TabCtx,
    source_rel: &mut PathBuf,
    source_content: &mut String,
    paragraphs: &mut Vec<Paragraph>,
    selected: &mut BTreeSet<usize>,
    adjust: &mut BTreeMap<usize, Adjust>,
    focus: &mut usize,
) -> SynthStep {
    SynthStep::Transition(carry_to_target(
        std::mem::take(source_rel),
        std::mem::take(source_content),
        std::mem::take(paragraphs),
        std::mem::take(selected),
        std::mem::take(adjust),
        *focus,
        FuzzyPicker::new(VaultFilePickerSource::new(
            Arc::clone(ctx.vault),
            Arc::clone(ctx.recents),
        )),
        None,
    ))
}

#[allow(clippy::too_many_arguments)]
fn handle_target_picker_key(
    k: KeyEvent,
    ctx: &TabCtx,
    source_rel: &mut PathBuf,
    source_content: &mut String,
    paragraphs: &mut Vec<Paragraph>,
    selected: &mut BTreeSet<usize>,
    adjust: &mut BTreeMap<usize, Adjust>,
    focus: &mut usize,
    picker: &mut FuzzyPicker<VaultFilePickerSource>,
    error: &mut Option<String>,
) -> SynthStep {
    match picker.handle_key(k) {
        PickerOutcome::Selected(hit) => {
            if hit.path == *source_rel {
                *error = Some("source cannot also be the synth target".into());
                return SynthStep::Stay;
            }
            *error = None;
            commit(
                ctx,
                std::mem::take(source_rel),
                std::mem::take(source_content),
                std::mem::take(paragraphs),
                std::mem::take(selected),
                std::mem::take(adjust),
                *focus,
                hit.path,
            )
        }
        PickerOutcome::Cancelled => {
            let source_rel_taken = std::mem::take(source_rel);
            SynthStep::Transition(ParagraphSynthState::ParagraphMultiSelect {
                source_rel: source_rel_taken,
                source_content: std::mem::take(source_content),
                paragraphs: std::mem::take(paragraphs),
                selected: std::mem::take(selected),
                adjust: std::mem::take(adjust),
                focus: *focus,
            })
        }
        PickerOutcome::StillOpen => {
            if error.is_some() {
                *error = None;
            }
            SynthStep::Stay
        }
        PickerOutcome::NotHandled => SynthStep::NotHandled,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_new_target_folder_key(
    k: KeyEvent,
    _ctx: &TabCtx,
    source_rel: &mut PathBuf,
    source_content: &mut String,
    paragraphs: &mut Vec<Paragraph>,
    selected: &mut BTreeSet<usize>,
    adjust: &mut BTreeMap<usize, Adjust>,
    focus: &mut usize,
    picker: &mut FuzzyPicker<PathListPickerSource>,
) -> SynthStep {
    match picker.handle_key(k) {
        PickerOutcome::Selected(folder) => {
            // `folder` is a vault-relative folder path. Move to the title prompt.
            SynthStep::Transition(ParagraphSynthState::NewTargetTitle {
                source_rel: std::mem::take(source_rel),
                source_content: std::mem::take(source_content),
                paragraphs: std::mem::take(paragraphs),
                selected: std::mem::take(selected),
                adjust: std::mem::take(adjust),
                focus: *focus,
                folder,
                buf: EditBuffer::default(),
                error: None,
            })
        }
        PickerOutcome::Cancelled => {
            let source_rel_taken = std::mem::take(source_rel);
            SynthStep::Transition(ParagraphSynthState::ParagraphMultiSelect {
                source_rel: source_rel_taken,
                source_content: std::mem::take(source_content),
                paragraphs: std::mem::take(paragraphs),
                selected: std::mem::take(selected),
                adjust: std::mem::take(adjust),
                focus: *focus,
            })
        }
        PickerOutcome::StillOpen => SynthStep::Stay,
        PickerOutcome::NotHandled => SynthStep::NotHandled,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_new_target_title_key(
    k: KeyEvent,
    ctx: &TabCtx,
    source_rel: &mut PathBuf,
    source_content: &mut String,
    paragraphs: &mut Vec<Paragraph>,
    selected: &mut BTreeSet<usize>,
    adjust: &mut BTreeMap<usize, Adjust>,
    focus: &mut usize,
    folder: &mut PathBuf,
    buf: &mut EditBuffer,
    error: &mut Option<String>,
) -> SynthStep {
    match (k.code, k.modifiers) {
        (KeyCode::Esc, _) => {
            // Back to folder pick, preserving step-2 state.
            let folders = enumerate_vault_folders(ctx.vault);
            SynthStep::Transition(ParagraphSynthState::NewTargetFolder {
                source_rel: std::mem::take(source_rel),
                source_content: std::mem::take(source_content),
                paragraphs: std::mem::take(paragraphs),
                selected: std::mem::take(selected),
                adjust: std::mem::take(adjust),
                focus: *focus,
                picker: FuzzyPicker::new(PathListPickerSource::new(folders)),
            })
        }
        (KeyCode::Enter, _) => {
            let title = buf.text.trim().to_string();
            if title.is_empty() {
                *error = Some("title cannot be empty".into());
                return SynthStep::Stay;
            }
            let mut name = title;
            if !name.ends_with(".md") {
                name.push_str(".md");
            }
            let target_rel = if folder.as_os_str().is_empty() || folder == Path::new(".") {
                PathBuf::from(&name)
            } else {
                folder.join(&name)
            };
            *error = None;
            commit(
                ctx,
                std::mem::take(source_rel),
                std::mem::take(source_content),
                std::mem::take(paragraphs),
                std::mem::take(selected),
                std::mem::take(adjust),
                *focus,
                target_rel,
            )
        }
        _ => {
            use crate::tui::widgets::edit_keymap::EditOutcome;
            match buf.handle_event(k) {
                EditOutcome::Consumed => {
                    if error.is_some() {
                        *error = None;
                    }
                    SynthStep::Stay
                }
                EditOutcome::NotHandled => SynthStep::NotHandled,
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn p(start: u32, end: u32, text: &str) -> Paragraph {
        Paragraph {
            line_start: start,
            line_end: end,
            text: text.into(),
        }
    }

    #[test]
    fn effective_range_is_identity_by_default() {
        let adj = Adjust::default();
        assert_eq!(adj.effective(&p(12, 18, "x")), (12, 18));
    }

    #[test]
    fn trim_top_shrinks_from_top() {
        let mut adj = Adjust::default();
        let para = p(12, 18, "x");
        adj.trim_top(&para);
        assert_eq!(adj.effective(&para), (13, 18));
        adj.trim_top(&para);
        assert_eq!(adj.effective(&para), (14, 18));
    }

    #[test]
    fn trim_bot_shrinks_from_bottom() {
        let mut adj = Adjust::default();
        let para = p(12, 18, "x");
        adj.trim_bot(&para);
        assert_eq!(adj.effective(&para), (12, 17));
        adj.trim_bot(&para);
        assert_eq!(adj.effective(&para), (12, 16));
    }

    #[test]
    fn trim_top_and_bot_compose() {
        let mut adj = Adjust::default();
        let para = p(12, 18, "x"); // 7 lines
        adj.trim_top(&para);
        adj.trim_bot(&para);
        assert_eq!(adj.effective(&para), (13, 17));
    }

    #[test]
    fn floor_of_one_line_enforced_on_single_line() {
        let mut adj = Adjust::default();
        let para = p(5, 5, "x"); // 1 line
        adj.trim_top(&para);
        adj.trim_bot(&para);
        assert_eq!(adj.effective(&para), (5, 5));
    }

    #[test]
    fn floor_enforced_when_trimming_to_exhaustion() {
        let mut adj = Adjust::default();
        let para = p(12, 14, "x"); // 3 lines
        adj.trim_top(&para); // → 13..14 (2 left)
        adj.trim_top(&para); // → 14..14 (1 left) — top_trim clamped at 2
        adj.trim_top(&para); // no-op, clamped
        assert_eq!(adj.effective(&para), (14, 14));
        // bot can't shrink the last line either.
        adj.trim_bot(&para);
        assert_eq!(adj.effective(&para), (14, 14));
    }

    #[test]
    fn build_sources_slices_effective_range_from_content() {
        let content = "alpha\nbeta\ngamma\ndelta\nepsilon\n";
        // paragraphs: one spanning L1-5.
        let paragraphs = vec![p(1, 5, "alpha\nbeta\ngamma\ndelta\nepsilon")];
        let mut selected = BTreeSet::new();
        selected.insert(0);
        let mut adjust: BTreeMap<usize, Adjust> = BTreeMap::new();
        // Shrink to L2-4 (trim top 1, bottom 1).
        adjust.entry(0).or_default().trim_top(&paragraphs[0]);
        adjust.entry(0).or_default().trim_bot(&paragraphs[0]);
        let sources = build_sources(
            Path::new("notes/source.md"),
            content,
            &paragraphs,
            &selected,
            &adjust,
        );
        assert_eq!(sources.len(), 1);
        assert_eq!((sources[0].line_start, sources[0].line_end), (2, 4));
        assert_eq!(sources[0].body, "beta\ngamma\ndelta");
    }

    #[test]
    fn build_sources_uses_full_range_when_unadjusted() {
        let content = "alpha\nbeta\n\ngamma\n";
        let paragraphs = vec![p(1, 2, "alpha\nbeta"), p(4, 4, "gamma")];
        let selected: BTreeSet<usize> = [0, 1].into_iter().collect();
        let sources = build_sources(
            Path::new("notes/source.md"),
            content,
            &paragraphs,
            &selected,
            &BTreeMap::new(),
        );
        assert_eq!(sources.len(), 2);
        assert_eq!((sources[0].line_start, sources[0].line_end), (1, 2));
        assert_eq!(sources[0].body, "alpha\nbeta");
        assert_eq!((sources[1].line_start, sources[1].line_end), (4, 4));
        assert_eq!(sources[1].body, "gamma");
    }

    #[test]
    fn working_tree_clean_false_without_git() {
        // A temp dir with no git repo → not clean (refuse).
        let tmp =
            std::env::temp_dir().join(format!("ft-paragraph-synth-no-git-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        assert!(!working_tree_clean(&tmp));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

//! Tab-agnostic section-move flow.
//!
//! Lifted from `tabs/notes/mod.rs` so the Graph tab can drive the same
//! 4-step flow (source pick → heading multi-select → target pick → compose
//! → commit) without duplicating it. The flow's *outer* shape is owned by
//! the calling tab (Notes wraps it in `NotesState::MoveSection`; Graph
//! wraps it in its own move outer), but every step's data, key handler
//! and on-commit pipeline lives here.
//!
//! Each step handler returns a [`MoveStep`] describing how the caller's
//! own state should advance — same pattern as `notes_actions::create`:
//!
//! * `Stay` — consumed, no state change.
//! * `Transition(next)` — replace the current `SectionMoveState` with `next`.
//! * `Finished` — the flow ended; the caller drops the slot to `Idle`.
//! * `NotHandled` — the key was not relevant; the caller may try its own
//!   bindings.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ft_core::markdown::{extract_headings, Heading};
use ft_core::notes::template::render as render_template;
use ft_core::notes::{
    extract_sections, move_sections, shift_section_level, write_pair, Placement, Section,
    SectionPick,
};
use ft_core::search::Hit;

use crate::tui::{
    notes_actions::{
        create::{
            build_template_context, discover_template_vars, enumerate_templates,
            enumerate_vault_folders, CollisionChoice, TemplatePick,
        },
        queue_toast,
    },
    tab::{TabCtx, ToastStyle},
    widgets::{
        edit_keymap::EditOutcome, EditBuffer, FuzzyPicker, PathListPickerSource, PickerOutcome,
        VaultFilePickerSource,
    },
};

// ── State ────────────────────────────────────────────────────────────

/// Step-3 state for the section-move flow, bundled so the
/// [`NewTargetState`] sub-flow can thread it through without copying
/// every field name into five enum variants.
#[derive(Debug)]
pub struct MoveCarry {
    pub source_rel: PathBuf,
    pub source_abs: PathBuf,
    pub source_content: String,
    pub headings: Vec<Heading>,
    pub selected: BTreeSet<usize>,
    pub focus: usize,
    pub clipboard: Vec<ClipboardItem>,
}

/// State machine for the section-move flow. Variants line up 1:1 with
/// the four steps documented in the plan.
pub enum SectionMoveState {
    /// Step 1/4 — pick the source note (or a heading inside one — we
    /// only use the file part of the hit).
    SourcePicking {
        picker: FuzzyPicker<VaultFilePickerSource>,
    },
    /// Step 2/4 — choose which sections to move. `selected` carries the
    /// **explicit** picks by 1-indexed source line number; descendants
    /// are computed on the fly so deselecting a parent restores the
    /// children's idle state without bookkeeping.
    HeadingMultiSelect {
        source_rel: PathBuf,
        source_abs: PathBuf,
        source_content: String,
        headings: Vec<Heading>,
        selected: BTreeSet<usize>,
        focus: usize,
    },
    /// Step 3/4 — pick the target note. The picker's same-file pick is
    /// rejected inline (`error` is shown in the popup footer) and the
    /// state stays put. `headings`/`selected`/`focus` are carried so
    /// `Esc` can rebuild the multi-select with the user's prior choices.
    /// `clipboard` is the extracted-section payload that will feed the
    /// compose view.
    TargetPicking {
        source_rel: PathBuf,
        source_abs: PathBuf,
        source_content: String,
        headings: Vec<Heading>,
        selected: BTreeSet<usize>,
        focus: usize,
        clipboard: Vec<ClipboardItem>,
        picker: FuzzyPicker<VaultFilePickerSource>,
        error: Option<String>,
    },
    /// Plan 009 session 5 — sub-flow for creating the move's target
    /// from a template (or blank) before reaching [`Self::Composing`].
    /// `Ctrl+N` from [`Self::TargetPicking`] enters this sub-flow; on
    /// successful filename + (optional) var prompts, lands in
    /// `Composing` with `target_is_new: true` and the rendered content
    /// held in memory. Cancel paths return to `TargetPicking` with the
    /// step-3 state preserved — no file is ever written until commit.
    NewTargetCreating(NewTargetState),
    /// Step 4/4 — compose the move. The layout interleaves the target's
    /// own headings (as `Anchor` rows, immutable) with the clipboard's
    /// pending picks (as `Pending` rows, reorderable + level-shiftable).
    /// `Enter` commits via [`commit_move`]; `Esc` returns to step 3 with
    /// the layout intact. The step-3 state (`source_*`, `headings`,
    /// `selected`) is carried so the Esc round-trip preserves prior picks.
    Composing {
        source_rel: PathBuf,
        source_abs: PathBuf,
        source_content: String,
        headings: Vec<Heading>,
        selected: BTreeSet<usize>,
        target_rel: PathBuf,
        target_abs: PathBuf,
        target_headings: Vec<Heading>,
        /// Target content used by [`commit_move`] when `target_is_new`
        /// is true (we never wrote the file, so this is the canonical
        /// copy). When `target_is_new` is false this still holds the
        /// content read at compose-entry, but commit re-reads from
        /// disk to pick up any external edits.
        target_content: String,
        /// True when the target was synthesised by the new-target
        /// sub-flow (template render or blank stub) and hasn't been
        /// written yet. Commit writes via `write_pair`; cancel paths
        /// leave the filesystem untouched.
        target_is_new: bool,
        clipboard: Vec<ClipboardItem>,
        layout: Vec<ComposeRow>,
        focus: usize,
        /// Transient sub-mode: while `Some`, the user is typing into a
        /// rename buffer attached to the focused Pending row. `None` is
        /// the normal compose-keymap mode.
        editing: Option<RenameBuffer>,
    },
}

/// Inline rename buffer attached to a Pending row in the compose layout.
/// Owns its own `EditBuffer` so the compose state's level/order is
/// untouched until the user commits with `Enter`.
#[derive(Debug, Clone)]
pub struct RenameBuffer {
    /// Index of the Pending row in `Composing.layout` being renamed.
    pub row_idx: usize,
    /// Single-line text input. Pre-filled with the row's current
    /// effective text on open; commits to `ComposeRow::Pending.rename`
    /// on `Enter`, discards on `Esc`.
    pub buf: EditBuffer,
}

/// One row in the compose layout. Anchor rows are the target's pre-existing
/// headings (read-only, shown for context); Pending rows are the picks
/// awaiting commit (movable, level-shiftable).
#[derive(Debug, Clone)]
pub enum ComposeRow {
    Anchor {
        line: usize,
        level: u8,
        text: String,
    },
    Pending {
        clip_idx: usize,
        level: u8,
        /// `Some(s)` overrides the source heading text at commit time
        /// (threaded into `SectionPick.new_text`). `None` keeps the
        /// source text. Independent of `level` — both can change.
        rename: Option<String>,
    },
}

/// Section-move new-target sub-flow (plan 009 session 5). Mirrors
/// [`CreateState`] but threads a [`MoveCarry`] through every variant so
/// `Esc` paths can return to [`SectionMoveState::TargetPicking`] with
/// the user's prior clipboard intact.
pub enum NewTargetState {
    TemplatePicking {
        carry: MoveCarry,
        /// Picker source includes a synthetic `(no template / blank)`
        /// row at the top whose `data` is an empty `PathBuf` — picked
        /// it skips template rendering and uses a `# <title>\n` stub.
        picker: FuzzyPicker<PathListPickerSource>,
    },
    FolderPicking {
        carry: MoveCarry,
        template: Option<TemplatePick>,
        picker: FuzzyPicker<PathListPickerSource>,
    },
    FilenamePrompt {
        carry: MoveCarry,
        template: Option<TemplatePick>,
        folder: PathBuf,
        buf: EditBuffer,
        error: Option<String>,
    },
    VarPrompt {
        carry: MoveCarry,
        template: TemplatePick,
        folder: PathBuf,
        filename: String,
        vars_so_far: BTreeMap<String, String>,
        next_idx: usize,
        buf: EditBuffer,
    },
    CollisionPrompt {
        carry: MoveCarry,
        template: Option<TemplatePick>,
        folder: PathBuf,
        filename: String,
        vars: BTreeMap<String, String>,
        target_abs: PathBuf,
        target_rel: PathBuf,
        focus: CollisionChoice,
    },
}

/// One section pending insertion into the target. Built at the
/// step-2 → step-3 transition from the in-memory source content. The
/// `body` is post-extraction (heading line included, body trimmed at
/// the next equal-or-higher heading) and is used only for the compose
/// preview — the commit re-extracts from a fresh disk read so the body
/// shown isn't the body written.
#[derive(Debug, Clone)]
pub struct ClipboardItem {
    /// 1-indexed source-file line of the heading. The freshness check
    /// at commit time looks the heading up by line number.
    pub source_line: usize,
    /// Original heading text. The freshness check rejects the commit if
    /// the line still has a heading but the text or level changed.
    pub source_text: String,
    /// Original heading level (1-6). The compose layout's `Pending`
    /// rows start at this level; `←/→` shift moves them per row.
    pub level: u8,
    /// The extracted section body — heading line included, trimmed at
    /// the next equal-or-higher heading. Used by the compose preview;
    /// not used by the commit path.
    #[allow(dead_code)]
    pub body: String,
}

// ── Step outcome ─────────────────────────────────────────────────────

/// Result of feeding a key event to the section-move flow. The caller
/// (tab) maps these to its own outer-state transitions.
///
/// The `Transition` variant carries the full new `SectionMoveState`
/// (~368 bytes). Boxing would force a heap allocation on every step
/// transition with no real win — the step is moved into the tab's
/// state slot on the next line — so we keep it unboxed.
#[allow(clippy::large_enum_variant)]
pub enum MoveStep {
    /// Key consumed; no state change.
    Stay,
    /// Key not recognized; caller may try its own bindings.
    NotHandled,
    /// Replace the current `SectionMoveState` with `next`.
    Transition(SectionMoveState),
    /// The flow has ended (committed, cancelled, or fatal error). The
    /// caller should drop its move-state slot back to its idle state.
    Finished,
}

// ── Helpers / handlers ───────────────────────────────────────────────

/// Public entry point: feed one key event to the active
/// `SectionMoveState`. The caller maps the returned [`MoveStep`] onto
/// its own outer-state transitions.
pub fn handle_key(state: &mut SectionMoveState, k: KeyEvent, ctx: &TabCtx) -> MoveStep {
    match state {
        SectionMoveState::SourcePicking { picker } => handle_source_picker_key(k, picker, ctx),
        SectionMoveState::HeadingMultiSelect {
            source_rel,
            source_abs,
            source_content,
            headings,
            selected,
            focus,
        } => handle_multiselect_key(
            k,
            source_rel,
            source_abs,
            source_content,
            headings,
            selected,
            focus,
            ctx,
        ),
        SectionMoveState::TargetPicking {
            source_rel,
            source_abs,
            source_content,
            headings,
            selected,
            focus,
            clipboard,
            picker,
            error,
        } => handle_target_picker_key(
            k,
            source_rel,
            source_abs,
            source_content,
            headings,
            selected,
            focus,
            clipboard,
            picker,
            error,
            ctx,
        ),
        SectionMoveState::NewTargetCreating(nts) => handle_new_target_key(k, nts, ctx),
        SectionMoveState::Composing {
            source_rel,
            source_abs,
            source_content,
            headings,
            selected,
            target_rel,
            target_abs,
            target_headings,
            target_content,
            target_is_new,
            clipboard,
            layout,
            focus,
            editing,
        } => handle_compose_key(
            k,
            source_rel,
            source_abs,
            source_content,
            headings,
            selected,
            target_rel,
            target_abs,
            target_headings,
            target_content,
            target_is_new,
            clipboard,
            layout,
            focus,
            editing,
            ctx,
        ),
    }
}

/// Entry point used by tabs: build a `SourcePicking` state with a fresh
/// vault picker.
pub fn begin_with_picker(ctx: &TabCtx) -> SectionMoveState {
    SectionMoveState::SourcePicking {
        picker: FuzzyPicker::new(VaultFilePickerSource::new(
            Arc::clone(ctx.vault),
            Arc::clone(ctx.recents),
        )),
    }
}

fn handle_source_picker_key(
    k: KeyEvent,
    picker: &mut FuzzyPicker<VaultFilePickerSource>,
    ctx: &TabCtx,
) -> MoveStep {
    match picker.handle_key(k) {
        PickerOutcome::Selected(hit) => advance_to_multiselect(ctx, hit),
        PickerOutcome::Cancelled => MoveStep::Finished,
        PickerOutcome::StillOpen => MoveStep::Stay,
        PickerOutcome::NotHandled => MoveStep::NotHandled,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_multiselect_key(
    k: KeyEvent,
    source_rel: &mut PathBuf,
    source_abs: &mut PathBuf,
    source_content: &mut String,
    headings: &mut Vec<Heading>,
    selected: &mut BTreeSet<usize>,
    focus: &mut usize,
    ctx: &TabCtx,
) -> MoveStep {
    match (k.code, k.modifiers) {
        (KeyCode::Esc, _) => MoveStep::Transition(SectionMoveState::SourcePicking {
            picker: FuzzyPicker::new(VaultFilePickerSource::new(
                Arc::clone(ctx.vault),
                Arc::clone(ctx.recents),
            )),
        }),
        (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
            if *focus > 0 {
                *focus -= 1;
            } else {
                *focus = headings.len().saturating_sub(1);
            }
            MoveStep::Stay
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
            if headings.is_empty() {
                return MoveStep::Stay;
            }
            *focus = (*focus + 1) % headings.len();
            MoveStep::Stay
        }
        (KeyCode::Char(' '), _) => {
            toggle_selection(headings, selected, *focus);
            MoveStep::Stay
        }
        (KeyCode::Enter, _) => {
            if selected.is_empty() {
                queue_toast(ctx, "select at least one heading", ToastStyle::Error);
                return MoveStep::Stay;
            }
            let clipboard = build_clipboard(source_content, headings, selected);
            if clipboard.is_empty() {
                queue_toast(ctx, "no sections extracted", ToastStyle::Error);
                return MoveStep::Stay;
            }
            MoveStep::Transition(SectionMoveState::TargetPicking {
                source_rel: std::mem::take(source_rel),
                source_abs: std::mem::take(source_abs),
                source_content: std::mem::take(source_content),
                headings: std::mem::take(headings),
                selected: std::mem::take(selected),
                focus: *focus,
                clipboard,
                picker: FuzzyPicker::new(VaultFilePickerSource::new(
                    Arc::clone(ctx.vault),
                    Arc::clone(ctx.recents),
                )),
                error: None,
            })
        }
        _ => MoveStep::NotHandled,
    }
}

#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
fn handle_target_picker_key(
    k: KeyEvent,
    source_rel: &mut PathBuf,
    source_abs: &mut PathBuf,
    source_content: &mut String,
    headings: &mut Vec<Heading>,
    selected: &mut BTreeSet<usize>,
    focus: &mut usize,
    clipboard: &mut Vec<ClipboardItem>,
    picker: &mut FuzzyPicker<VaultFilePickerSource>,
    error: &mut Option<String>,
    ctx: &TabCtx,
) -> MoveStep {
    // Ctrl+N: enter the new-target sub-flow. Intercepted before the
    // picker so `n` alone still filters normally.
    if k.code == KeyCode::Char('n') && k.modifiers.contains(KeyModifiers::CONTROL) {
        let carry = MoveCarry {
            source_rel: std::mem::take(source_rel),
            source_abs: std::mem::take(source_abs),
            source_content: std::mem::take(source_content),
            headings: std::mem::take(headings),
            selected: std::mem::take(selected),
            focus: *focus,
            clipboard: std::mem::take(clipboard),
        };
        return MoveStep::Transition(SectionMoveState::NewTargetCreating(
            begin_new_target_template_picking(ctx, carry),
        ));
    }
    match picker.handle_key(k) {
        PickerOutcome::Selected(hit) => {
            if hit.path == *source_rel {
                *error = Some("same-file move is out of scope — pick a different target".into());
                MoveStep::Stay
            } else {
                advance_to_composing(
                    ctx,
                    std::mem::take(source_rel),
                    std::mem::take(source_abs),
                    std::mem::take(source_content),
                    std::mem::take(headings),
                    std::mem::take(selected),
                    std::mem::take(clipboard),
                    hit,
                )
            }
        }
        PickerOutcome::Cancelled => MoveStep::Transition(SectionMoveState::HeadingMultiSelect {
            source_rel: std::mem::take(source_rel),
            source_abs: std::mem::take(source_abs),
            source_content: std::mem::take(source_content),
            headings: std::mem::take(headings),
            selected: std::mem::take(selected),
            focus: *focus,
        }),
        PickerOutcome::StillOpen => {
            // Any text-edit / nav keystroke clears a stale "same file" error.
            if error.is_some() {
                *error = None;
            }
            MoveStep::Stay
        }
        PickerOutcome::NotHandled => MoveStep::NotHandled,
    }
}

pub fn advance_to_multiselect(ctx: &TabCtx, hit: Hit) -> MoveStep {
    match begin_for_source(ctx, hit.path) {
        Some(state) => MoveStep::Transition(state),
        // `begin_for_source` already queued the toast on failure.
        None => MoveStep::Finished,
    }
}

/// Begin the section-move flow seeded to a known source note, skipping the
/// source picker and opening directly at heading multi-select. Returns
/// `None` — after queuing an error toast — when the source can't be read
/// or has no headings, so the caller simply declines to open the modal.
///
/// This is the shared primitive behind [`advance_to_multiselect`] (the
/// source-picker path) and the History tab's row-seeded move.
pub fn begin_for_source(ctx: &TabCtx, source_rel: PathBuf) -> Option<SectionMoveState> {
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
    let headings = extract_headings(&content);
    if headings.is_empty() {
        queue_toast(ctx, "source has no headings to move", ToastStyle::Error);
        return None;
    }
    Some(SectionMoveState::HeadingMultiSelect {
        source_rel,
        source_abs: abs,
        source_content: content,
        headings,
        selected: BTreeSet::new(),
        focus: 0,
    })
}

/// Build the step-4 (compose) state from the step-3 transition. Reads
/// the target file, extracts its headings, and seeds the layout with
/// the target's anchors followed by the clipboard's pending picks at
/// their original level. On IO error (target unreadable) drops a toast
/// and snaps the user back to idle.
#[allow(clippy::too_many_arguments)]
fn advance_to_composing(
    ctx: &TabCtx,
    source_rel: PathBuf,
    source_abs: PathBuf,
    source_content: String,
    headings: Vec<Heading>,
    selected: BTreeSet<usize>,
    clipboard: Vec<ClipboardItem>,
    target_hit: Hit,
) -> MoveStep {
    let target_abs = ctx.vault.path.join(&target_hit.path);
    let target_content = match std::fs::read_to_string(&target_abs) {
        Ok(s) => s,
        Err(e) => {
            queue_toast(
                ctx,
                &format!("could not read target: {e}"),
                ToastStyle::Error,
            );
            return MoveStep::Finished;
        }
    };
    let target_headings = extract_headings(&target_content);
    let mut layout: Vec<ComposeRow> = target_headings
        .iter()
        .map(|h| ComposeRow::Anchor {
            line: h.line,
            level: h.level,
            text: h.text.clone(),
        })
        .collect();
    for (idx, item) in clipboard.iter().enumerate() {
        layout.push(ComposeRow::Pending {
            clip_idx: idx,
            level: item.level,
            rename: None,
        });
    }
    // Focus the first pending row so the user lands on something
    // movable. With no anchors the first row is already pending; with
    // some anchors it sits at `target_headings.len()`.
    let focus = target_headings.len().min(layout.len().saturating_sub(1));
    MoveStep::Transition(SectionMoveState::Composing {
        source_rel,
        source_abs,
        source_content,
        headings,
        selected,
        target_rel: target_hit.path,
        target_abs,
        target_headings,
        target_content,
        target_is_new: false,
        clipboard,
        layout,
        focus,
        editing: None,
    })
}

/// Step-4 key dispatcher. Up/Down move focus across the whole layout
/// (Anchor rows are read-only context but still focusable so the user
/// can see where they're inserting); Shift+Up/Down reorders the focused
/// Pending row (Anchor rows can't move); Left/Right shifts the focused
/// Pending row's heading level with cascade-overflow blocking; Enter
/// commits via [`commit_move`]; Esc returns to TargetPicking with the
/// step-3 state carried forward.
#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
fn handle_compose_key(
    k: KeyEvent,
    source_rel: &mut PathBuf,
    source_abs: &mut PathBuf,
    source_content: &mut String,
    headings: &mut Vec<Heading>,
    selected: &mut BTreeSet<usize>,
    target_rel: &mut PathBuf,
    target_abs: &mut PathBuf,
    _target_headings: &mut Vec<Heading>,
    target_content: &mut String,
    target_is_new: &mut bool,
    clipboard: &mut Vec<ClipboardItem>,
    layout: &mut Vec<ComposeRow>,
    focus: &mut usize,
    editing: &mut Option<RenameBuffer>,
    ctx: &TabCtx,
) -> MoveStep {
    // Rename buffer is a sub-mode of Composing: when open it consumes
    // every compose key so `r`/`Shift+↑`/`←` etc. don't fire under it.
    if editing.is_some() {
        return handle_rename_buffer_key(k, layout, editing, ctx);
    }
    let shift = k.modifiers.contains(KeyModifiers::SHIFT);
    match (k.code, shift) {
        (KeyCode::Esc, _) => MoveStep::Transition(SectionMoveState::TargetPicking {
            source_rel: std::mem::take(source_rel),
            source_abs: std::mem::take(source_abs),
            source_content: std::mem::take(source_content),
            headings: std::mem::take(headings),
            selected: std::mem::take(selected),
            focus: 0,
            clipboard: std::mem::take(clipboard),
            picker: FuzzyPicker::new(VaultFilePickerSource::new(
                Arc::clone(ctx.vault),
                Arc::clone(ctx.recents),
            )),
            error: None,
        }),
        (KeyCode::Up, false) | (KeyCode::Char('k'), false) => {
            if *focus > 0 {
                *focus -= 1;
            } else {
                *focus = layout.len().saturating_sub(1);
            }
            MoveStep::Stay
        }
        (KeyCode::Down, false) | (KeyCode::Char('j'), false) => {
            if !layout.is_empty() {
                *focus = (*focus + 1) % layout.len();
            }
            MoveStep::Stay
        }
        (KeyCode::Up, true) | (KeyCode::Char('K'), _) => {
            if reorder_pending(layout, *focus, -1) {
                *focus -= 1;
            }
            MoveStep::Stay
        }
        (KeyCode::Down, true) | (KeyCode::Char('J'), _) => {
            if reorder_pending(layout, *focus, 1) {
                *focus += 1;
            }
            MoveStep::Stay
        }
        (KeyCode::Left, _) | (KeyCode::Char('h'), false) => {
            shift_focused_level(layout, clipboard, *focus, -1, ctx);
            MoveStep::Stay
        }
        (KeyCode::Right, _) | (KeyCode::Char('l'), false) => {
            shift_focused_level(layout, clipboard, *focus, 1, ctx);
            MoveStep::Stay
        }
        (KeyCode::Char('r'), false) => {
            open_rename_buffer(layout, clipboard, *focus, editing);
            MoveStep::Stay
        }
        (KeyCode::Enter, _) => {
            commit_move(
                ctx,
                source_rel,
                source_abs,
                target_rel,
                target_abs,
                target_content,
                *target_is_new,
                clipboard,
                layout,
            );
            MoveStep::Finished
        }
        _ => MoveStep::NotHandled,
    }
}

/// Open the inline rename buffer on the focused Pending row, pre-filled
/// with that row's current effective text (override if set, otherwise
/// the source heading). No-op on Anchor rows.
fn open_rename_buffer(
    layout: &[ComposeRow],
    clipboard: &[ClipboardItem],
    focus: usize,
    editing: &mut Option<RenameBuffer>,
) {
    let Some(row) = layout.get(focus) else {
        return;
    };
    let ComposeRow::Pending {
        clip_idx, rename, ..
    } = row
    else {
        return;
    };
    let initial = rename
        .as_deref()
        .unwrap_or_else(|| clipboard[*clip_idx].source_text.as_str());
    *editing = Some(RenameBuffer {
        row_idx: focus,
        buf: EditBuffer::from(initial),
    });
}

/// Handle a single key while the rename buffer is open. Printable chars
/// go into the buffer; `Enter` validates + commits into the row;
/// `Esc` discards; everything else is consumed without effect (so
/// `r`/`Shift+↑`/`←` etc. don't leak through to compose-level handlers).
fn handle_rename_buffer_key(
    k: KeyEvent,
    layout: &mut [ComposeRow],
    editing: &mut Option<RenameBuffer>,
    ctx: &TabCtx,
) -> MoveStep {
    let Some(rb) = editing.as_mut() else {
        return MoveStep::NotHandled;
    };
    match k.code {
        KeyCode::Esc => {
            *editing = None;
            MoveStep::Stay
        }
        KeyCode::Enter => {
            let trimmed = rb.buf.text.trim();
            if trimmed.is_empty() {
                queue_toast(ctx, "rename cannot be empty", ToastStyle::Error);
                return MoveStep::Stay;
            }
            if trimmed.contains('\n') || trimmed.contains('\r') {
                queue_toast(ctx, "rename cannot contain newlines", ToastStyle::Error);
                return MoveStep::Stay;
            }
            let new_text = trimmed.to_string();
            let row_idx = rb.row_idx;
            if let Some(ComposeRow::Pending { rename, .. }) = layout.get_mut(row_idx) {
                *rename = Some(new_text);
            }
            *editing = None;
            MoveStep::Stay
        }
        // All edits + cursor moves go through the buffer's keymap.
        // Unrecognised chords are swallowed (return Stay, not
        // NotHandled) so compose-level keys (`r`, `Shift+↑`, …) don't
        // fire under the rename buffer.
        _ => {
            let _ = rb.buf.handle_event(k);
            MoveStep::Stay
        }
    }
}

/// Move the row at `focus` by `delta` (-1 or +1) within `layout`,
/// constrained to swaps with other `Pending` rows or `Anchor` rows in
/// either direction. Returns true iff a swap happened. Anchor rows at
/// `focus` are immutable (returns false).
fn reorder_pending(layout: &mut [ComposeRow], focus: usize, delta: i32) -> bool {
    if focus >= layout.len() {
        return false;
    }
    if !matches!(layout[focus], ComposeRow::Pending { .. }) {
        return false;
    }
    let target = focus as i32 + delta;
    if target < 0 || target as usize >= layout.len() {
        return false;
    }
    layout.swap(focus, target as usize);
    true
}

/// Shift the focused row's level by `delta`. Anchor rows ignore the
/// keystroke (with a toast); Pending rows clamp at level 1 and bail
/// with an error toast if the cascade would push any nested heading
/// past level 6.
fn shift_focused_level(
    layout: &mut [ComposeRow],
    clipboard: &[ClipboardItem],
    focus: usize,
    delta: i32,
    ctx: &TabCtx,
) {
    let Some(row) = layout.get_mut(focus) else {
        return;
    };
    let (clip_idx, cur_level) = match row {
        ComposeRow::Pending {
            clip_idx, level, ..
        } => (*clip_idx, *level),
        ComposeRow::Anchor { .. } => {
            return;
        }
    };
    let next = cur_level as i32 + delta;
    if next < 1 {
        return;
    }
    if next > 6 {
        queue_toast(ctx, "heading level 6 is the max", ToastStyle::Error);
        return;
    }
    let new_level = next as u8;
    // Dry-run the cascade against the cached body so the user finds out
    // *before* commit that an inner heading would overflow. We rebuild
    // a Section from the clipboard's cached body — its line numbers are
    // garbage but `shift_section_level` only walks levels.
    let item = &clipboard[clip_idx];
    let probe_section = Section {
        heading: Heading {
            line: 1,
            level: item.level,
            text: item.source_text.clone(),
        },
        body: item.body.clone(),
    };
    if let Err(e) = shift_section_level(&probe_section, new_level) {
        queue_toast(
            ctx,
            &format!("cascade would overflow: {e}"),
            ToastStyle::Error,
        );
        return;
    }
    if let ComposeRow::Pending { level, .. } = row {
        *level = new_level;
    }
}

/// Commit the move: re-read source, freshness-check every pick, build
/// `picks` + `plan` from the current layout, call `move_sections` +
/// `write_pair`. Emits a success toast on the happy path, an error
/// toast on any failure. Returning to Idle is the caller's job.
///
/// When `target_is_new` is true, `target_content` is the canonical
/// (in-memory) target — we don't re-read from disk because the file
/// doesn't exist yet. `write_pair` writes it for the first time.
#[allow(clippy::too_many_arguments)]
fn commit_move(
    ctx: &TabCtx,
    source_rel: &Path,
    source_abs: &Path,
    target_rel: &Path,
    target_abs: &Path,
    target_content: &str,
    target_is_new: bool,
    clipboard: &[ClipboardItem],
    layout: &[ComposeRow],
) {
    let fresh_source = match std::fs::read_to_string(source_abs) {
        Ok(s) => s,
        Err(e) => {
            queue_toast(
                ctx,
                &format!("could not re-read source: {e}"),
                ToastStyle::Error,
            );
            return;
        }
    };
    let fresh_headings = extract_headings(&fresh_source);
    for item in clipboard {
        let still_matches = fresh_headings.iter().any(|h| {
            h.line == item.source_line && h.level == item.level && h.text == item.source_text
        });
        if !still_matches {
            queue_toast(ctx, "source changed on disk — aborted", ToastStyle::Error);
            return;
        }
    }

    // For new targets, the on-disk file doesn't exist — use the
    // in-memory rendered content. For existing targets, re-read so we
    // pick up any external edits since compose-entry.
    let fresh_target: String = if target_is_new {
        target_content.to_string()
    } else {
        match std::fs::read_to_string(target_abs) {
            Ok(s) => s,
            Err(e) => {
                queue_toast(
                    ctx,
                    &format!("could not re-read target: {e}"),
                    ToastStyle::Error,
                );
                return;
            }
        }
    };

    let (picks, plan) = build_picks_and_plan(layout, clipboard);
    if picks.is_empty() {
        queue_toast(ctx, "no sections to move", ToastStyle::Error);
        return;
    }
    let (new_source, new_target) = match move_sections(&fresh_source, &picks, &fresh_target, &plan)
    {
        Ok(pair) => pair,
        Err(e) => {
            queue_toast(ctx, &format!("move failed: {e}"), ToastStyle::Error);
            return;
        }
    };
    if let Err(e) = write_pair(target_abs, &new_target, source_abs, &new_source) {
        queue_toast(ctx, &format!("write failed: {e}"), ToastStyle::Error);
        return;
    }
    queue_toast(
        ctx,
        &format!(
            "Moved {} section(s): {} → {}",
            picks.len(),
            source_rel.display(),
            target_rel.display(),
        ),
        ToastStyle::Success,
    );
    // A move rewrites two files; the shared graph snapshot is now stale.
    ctx.request_graph_refresh();
}

/// Walk the layout in order, emitting one `SectionPick` per `Pending`
/// row (in layout order — that's what the user sees) and one
/// `Placement` pointing at the most recently passed `Anchor` (`None`
/// before any anchors, i.e. top of the target). The picks vector is
/// indexed positionally so `plan[i].pick_idx == i`.
fn build_picks_and_plan(
    layout: &[ComposeRow],
    clipboard: &[ClipboardItem],
) -> (Vec<SectionPick>, Vec<Placement>) {
    let mut picks: Vec<SectionPick> = Vec::new();
    let mut plan: Vec<Placement> = Vec::new();
    let mut after_line: Option<usize> = None;
    for row in layout {
        match row {
            ComposeRow::Anchor { line, .. } => {
                after_line = Some(*line);
            }
            ComposeRow::Pending {
                clip_idx,
                level,
                rename,
            } => {
                let item = &clipboard[*clip_idx];
                let pick_idx = picks.len();
                picks.push(SectionPick {
                    source_line: item.source_line,
                    new_level: *level,
                    new_text: rename.clone(),
                });
                plan.push(Placement {
                    pick_idx,
                    after_line,
                });
            }
        }
    }
    (picks, plan)
}

/// Toggle the explicit selection state of `headings[focus]`. Implicit
/// (ancestor-selected) targets are left alone — the rule the plan
/// spells out is "descendants can't be toggled while the parent is
/// selected". When the user newly selects a parent that has explicit
/// children, those children are demoted to implicit (so the eventual
/// pick list stays disjoint and `validate_disjoint` is happy).
fn toggle_selection(headings: &[Heading], selected: &mut BTreeSet<usize>, focus: usize) {
    if focus >= headings.len() {
        return;
    }
    let line = headings[focus].line;
    if is_implicitly_selected(headings, focus, selected) {
        return;
    }
    if selected.contains(&line) {
        selected.remove(&line);
        return;
    }
    // Newly selecting: clear any explicit descendants — they'll be
    // implicit from now on.
    let descendants = descendant_lines(headings, focus);
    for d in descendants {
        selected.remove(&d);
    }
    selected.insert(line);
}

/// True if any ancestor of `headings[i]` is in `selected`. Walks back
/// up the implicit tree by tracking the smallest level we've yet to
/// pierce — when a heading's level drops below `cur_level`, it's our
/// next ancestor.
pub(crate) fn is_implicitly_selected(
    headings: &[Heading],
    i: usize,
    selected: &BTreeSet<usize>,
) -> bool {
    if i >= headings.len() {
        return false;
    }
    let mut cur_level = headings[i].level;
    for h in headings[..i].iter().rev() {
        if h.level < cur_level {
            if selected.contains(&h.line) {
                return true;
            }
            cur_level = h.level;
            if cur_level == 1 {
                break;
            }
        }
    }
    false
}

/// 1-indexed source-file line numbers for every descendant of
/// `headings[i]`. Used when newly selecting a parent so explicit
/// children get demoted to implicit.
fn descendant_lines(headings: &[Heading], i: usize) -> Vec<usize> {
    if i >= headings.len() {
        return Vec::new();
    }
    let level = headings[i].level;
    let mut out = Vec::new();
    for h in headings[i + 1..].iter() {
        if h.level <= level {
            break;
        }
        out.push(h.line);
    }
    out
}

/// Pull the picked sections out of `source_content`, returning a
/// clipboard entry per explicit pick (in document order). Uses
/// `extract_sections` so the body bounds match what `move_sections`
/// will compute at commit time.
fn build_clipboard(
    source_content: &str,
    headings: &[Heading],
    selected: &BTreeSet<usize>,
) -> Vec<ClipboardItem> {
    let sections = extract_sections(source_content);
    let mut items: Vec<ClipboardItem> = headings
        .iter()
        .filter(|h| selected.contains(&h.line))
        .filter_map(|h| {
            sections
                .iter()
                .find(|s| s.heading.line == h.line)
                .map(|s| ClipboardItem {
                    source_line: h.line,
                    source_text: h.text.clone(),
                    level: h.level,
                    body: s.body.clone(),
                })
        })
        .collect();
    items.sort_by_key(|c| c.source_line);
    items
}

// ── Section-move new-target sub-flow (plan 009 session 5) ────────────────────

/// Build the [`NewTargetState::TemplatePicking`] entry state. The picker
/// is seeded with a synthetic `(no template / blank)` row followed by
/// the configured templates dir contents. Empty templates dir is fine:
/// only the blank option appears.
fn begin_new_target_template_picking(ctx: &TabCtx, carry: MoveCarry) -> NewTargetState {
    let templates = enumerate_templates(ctx.vault);
    let mut items: Vec<(String, PathBuf)> = Vec::with_capacity(templates.len() + 1);
    items.push(("(no template / blank)".to_string(), PathBuf::new()));
    for t in templates {
        let label = t.display().to_string();
        items.push((label, t));
    }
    NewTargetState::TemplatePicking {
        carry,
        picker: FuzzyPicker::new(PathListPickerSource::with_labels(items)),
    }
}

fn begin_new_target_folder_picking(
    ctx: &TabCtx,
    carry: MoveCarry,
    template: Option<TemplatePick>,
) -> NewTargetState {
    let folders = enumerate_vault_folders(ctx.vault);
    NewTargetState::FolderPicking {
        carry,
        template,
        picker: FuzzyPicker::new(PathListPickerSource::new(folders)),
    }
}

/// Top-level dispatcher for the new-target sub-flow. Mirrors
/// [`NotesTab::handle_create_key`] shape but lives outside the impl so
/// it can take `&mut NewTargetState` without re-borrowing self.
fn handle_new_target_key(k: KeyEvent, nts: &mut NewTargetState, ctx: &TabCtx) -> MoveStep {
    match nts {
        NewTargetState::TemplatePicking { carry, picker } => {
            handle_new_target_template_picker_key(k, carry, picker, ctx)
        }
        NewTargetState::FolderPicking {
            carry,
            template,
            picker,
        } => handle_new_target_folder_picker_key(k, carry, template, picker, ctx),
        NewTargetState::FilenamePrompt {
            carry,
            template,
            folder,
            buf,
            error,
        } => handle_new_target_filename_key(k, carry, template, folder, buf, error, ctx),
        NewTargetState::VarPrompt {
            carry,
            template,
            folder,
            filename,
            vars_so_far,
            next_idx,
            buf,
        } => handle_new_target_var_key(
            k,
            carry,
            template,
            folder,
            filename,
            vars_so_far,
            next_idx,
            buf,
            ctx,
        ),
        NewTargetState::CollisionPrompt {
            carry,
            template,
            folder,
            filename,
            vars,
            target_abs,
            target_rel,
            focus,
        } => handle_new_target_collision_key(
            k, carry, template, folder, filename, vars, target_abs, target_rel, focus, ctx,
        ),
    }
}

/// Rebuild [`SectionMoveState::TargetPicking`] from `carry`. Used by
/// every cancel path in the new-target sub-flow.
fn back_to_target_picking(ctx: &TabCtx, carry: MoveCarry) -> MoveStep {
    MoveStep::Transition(SectionMoveState::TargetPicking {
        source_rel: carry.source_rel,
        source_abs: carry.source_abs,
        source_content: carry.source_content,
        headings: carry.headings,
        selected: carry.selected,
        focus: carry.focus,
        clipboard: carry.clipboard,
        picker: FuzzyPicker::new(VaultFilePickerSource::new(
            Arc::clone(ctx.vault),
            Arc::clone(ctx.recents),
        )),
        error: None,
    })
}

fn handle_new_target_template_picker_key(
    k: KeyEvent,
    carry: &mut MoveCarry,
    picker: &mut FuzzyPicker<PathListPickerSource>,
    ctx: &TabCtx,
) -> MoveStep {
    match picker.handle_key(k) {
        PickerOutcome::Selected(rel) => {
            let template: Option<TemplatePick> = if rel.as_os_str().is_empty() {
                None
            } else {
                let abs = ctx.vault.templates_dir().join(&rel);
                let source = match std::fs::read_to_string(&abs) {
                    Ok(s) => s,
                    Err(e) => {
                        queue_toast(
                            ctx,
                            &format!("could not read template: {e}"),
                            ToastStyle::Error,
                        );
                        return back_to_target_picking(ctx, take_carry(carry));
                    }
                };
                let vars_needed = discover_template_vars(&source);
                Some(TemplatePick {
                    rel,
                    source,
                    vars_needed,
                })
            };
            MoveStep::Transition(SectionMoveState::NewTargetCreating(
                begin_new_target_folder_picking(ctx, take_carry(carry), template),
            ))
        }
        PickerOutcome::Cancelled => back_to_target_picking(ctx, take_carry(carry)),
        PickerOutcome::StillOpen => MoveStep::Stay,
        PickerOutcome::NotHandled => MoveStep::NotHandled,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_new_target_folder_picker_key(
    k: KeyEvent,
    carry: &mut MoveCarry,
    template: &mut Option<TemplatePick>,
    picker: &mut FuzzyPicker<PathListPickerSource>,
    ctx: &TabCtx,
) -> MoveStep {
    match picker.handle_key(k) {
        PickerOutcome::Selected(folder) => {
            let folder = if folder == Path::new(".") {
                PathBuf::new()
            } else {
                folder
            };
            MoveStep::Transition(SectionMoveState::NewTargetCreating(
                NewTargetState::FilenamePrompt {
                    carry: take_carry(carry),
                    template: template.take(),
                    folder,
                    buf: EditBuffer::default(),
                    error: None,
                },
            ))
        }
        // Esc → back to the template picker (always present in the
        // sub-flow, since this is the new-target path).
        PickerOutcome::Cancelled => MoveStep::Transition(SectionMoveState::NewTargetCreating(
            begin_new_target_template_picking(ctx, take_carry(carry)),
        )),
        PickerOutcome::StillOpen => MoveStep::Stay,
        PickerOutcome::NotHandled => MoveStep::NotHandled,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_new_target_filename_key(
    k: KeyEvent,
    carry: &mut MoveCarry,
    template: &mut Option<TemplatePick>,
    folder: &mut PathBuf,
    buf: &mut EditBuffer,
    error: &mut Option<String>,
    ctx: &TabCtx,
) -> MoveStep {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match (k.code, ctrl) {
        (KeyCode::Esc, _) => MoveStep::Transition(SectionMoveState::NewTargetCreating(
            begin_new_target_folder_picking(ctx, take_carry(carry), template.take()),
        )),
        (KeyCode::Enter, _) => {
            let trimmed = buf.text.trim();
            if trimmed.is_empty() {
                *error = Some("filename is required".into());
                return MoveStep::Stay;
            }
            if trimmed.contains('/') || trimmed.contains('\\') {
                *error = Some("filename can't contain path separators".into());
                return MoveStep::Stay;
            }
            let filename = if trimmed.ends_with(".md") {
                trimmed.to_string()
            } else {
                format!("{trimmed}.md")
            };
            let abs_path = ctx.vault.path.join(folder.as_path()).join(&filename);
            let rel_path = if folder.as_os_str().is_empty() {
                PathBuf::from(&filename)
            } else {
                folder.join(&filename)
            };

            // Same-file collision: the user's source can't also be the
            // new target.
            if rel_path == carry.source_rel {
                *error = Some("can't create a new target equal to the source file".into());
                return MoveStep::Stay;
            }

            if abs_path.exists() {
                return MoveStep::Transition(SectionMoveState::NewTargetCreating(
                    NewTargetState::CollisionPrompt {
                        carry: take_carry(carry),
                        template: template.take(),
                        folder: std::mem::take(folder),
                        filename,
                        vars: BTreeMap::new(),
                        target_abs: abs_path,
                        target_rel: rel_path,
                        focus: CollisionChoice::Overwrite,
                    },
                ));
            }

            // No collision. Advance to var prompts (if any) or commit
            // straight into Composing.
            match template.take() {
                Some(tpl) if !tpl.vars_needed.is_empty() => MoveStep::Transition(
                    SectionMoveState::NewTargetCreating(NewTargetState::VarPrompt {
                        carry: take_carry(carry),
                        template: tpl,
                        folder: std::mem::take(folder),
                        filename,
                        vars_so_far: BTreeMap::new(),
                        next_idx: 0,
                        buf: EditBuffer::default(),
                    }),
                ),
                tpl => advance_new_target_to_composing(
                    ctx,
                    take_carry(carry),
                    tpl.as_ref(),
                    std::mem::take(folder),
                    filename,
                    &BTreeMap::new(),
                    abs_path,
                    rel_path,
                    /*overwrite_existing=*/ false,
                ),
            }
        }
        _ => match buf.handle_event(k) {
            EditOutcome::Consumed => {
                *error = None;
                MoveStep::Stay
            }
            EditOutcome::NotHandled => MoveStep::NotHandled,
        },
    }
}

#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
fn handle_new_target_var_key(
    k: KeyEvent,
    carry: &mut MoveCarry,
    template: &mut TemplatePick,
    folder: &mut PathBuf,
    filename: &mut String,
    vars_so_far: &mut BTreeMap<String, String>,
    next_idx: &mut usize,
    buf: &mut EditBuffer,
    ctx: &TabCtx,
) -> MoveStep {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match (k.code, ctrl) {
        // Esc here cancels the entire new-target sub-flow, returning
        // to TargetPicking with the clipboard preserved. Going "back"
        // step-by-step is more granular than the user wants here.
        (KeyCode::Esc, _) => back_to_target_picking(ctx, take_carry(carry)),
        (KeyCode::Enter, _) => {
            let key_name = template
                .vars_needed
                .get(*next_idx)
                .cloned()
                .unwrap_or_default();
            vars_so_far.insert(key_name, buf.text.clone());
            *next_idx += 1;
            if *next_idx >= template.vars_needed.len() {
                let abs_path = ctx.vault.path.join(folder.as_path()).join(&*filename);
                let rel_path = if folder.as_os_str().is_empty() {
                    PathBuf::from(&*filename)
                } else {
                    folder.join(&*filename)
                };
                advance_new_target_to_composing(
                    ctx,
                    take_carry(carry),
                    Some(template),
                    std::mem::take(folder),
                    std::mem::take(filename),
                    vars_so_far,
                    abs_path,
                    rel_path,
                    /*overwrite_existing=*/ false,
                )
            } else {
                buf.text.clear();
                buf.cursor = 0;
                MoveStep::Stay
            }
        }
        _ => match buf.handle_event(k) {
            EditOutcome::Consumed => MoveStep::Stay,
            EditOutcome::NotHandled => MoveStep::NotHandled,
        },
    }
}

#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
fn handle_new_target_collision_key(
    k: KeyEvent,
    carry: &mut MoveCarry,
    template: &mut Option<TemplatePick>,
    folder: &mut PathBuf,
    filename: &mut String,
    vars: &mut BTreeMap<String, String>,
    target_abs: &mut PathBuf,
    target_rel: &mut PathBuf,
    focus: &mut CollisionChoice,
    ctx: &TabCtx,
) -> MoveStep {
    let go = |choice: CollisionChoice,
              carry: &mut MoveCarry,
              template: &mut Option<TemplatePick>,
              folder: &mut PathBuf,
              filename: &mut String,
              vars: &mut BTreeMap<String, String>,
              target_abs: &mut PathBuf,
              target_rel: &mut PathBuf|
     -> MoveStep {
        match choice {
            CollisionChoice::Overwrite => {
                advance_new_target_to_composing(
                    ctx,
                    take_carry(carry),
                    template.as_ref(),
                    std::mem::take(folder),
                    std::mem::take(filename),
                    vars,
                    std::mem::take(target_abs),
                    std::mem::take(target_rel),
                    /*overwrite_existing=*/ true,
                )
            }
            CollisionChoice::UseExisting => {
                // Read the existing file and treat it as the target —
                // no rendering, no overwrite. This matches the
                // standalone create flow's "use existing" semantics.
                let existing = match std::fs::read_to_string(&*target_abs) {
                    Ok(s) => s,
                    Err(e) => {
                        queue_toast(
                            ctx,
                            &format!("could not read existing target: {e}"),
                            ToastStyle::Error,
                        );
                        return back_to_target_picking(ctx, take_carry(carry));
                    }
                };
                compose_with_existing_target(
                    take_carry(carry),
                    std::mem::take(target_rel),
                    std::mem::take(target_abs),
                    existing,
                )
            }
            CollisionChoice::Cancel => MoveStep::Transition(SectionMoveState::NewTargetCreating(
                NewTargetState::FilenamePrompt {
                    carry: take_carry(carry),
                    template: template.take(),
                    folder: std::mem::take(folder),
                    buf: EditBuffer::from(filename),
                    error: None,
                },
            )),
        }
    };

    match (k.code, k.modifiers) {
        (KeyCode::Char('o'), KeyModifiers::NONE) | (KeyCode::Char('O'), _) => go(
            CollisionChoice::Overwrite,
            carry,
            template,
            folder,
            filename,
            vars,
            target_abs,
            target_rel,
        ),
        (KeyCode::Char('u'), KeyModifiers::NONE) | (KeyCode::Char('U'), _) => go(
            CollisionChoice::UseExisting,
            carry,
            template,
            folder,
            filename,
            vars,
            target_abs,
            target_rel,
        ),
        (KeyCode::Char('c'), KeyModifiers::NONE) | (KeyCode::Char('C'), _) | (KeyCode::Esc, _) => {
            go(
                CollisionChoice::Cancel,
                carry,
                template,
                folder,
                filename,
                vars,
                target_abs,
                target_rel,
            )
        }
        (KeyCode::Enter, _) => go(
            *focus, carry, template, folder, filename, vars, target_abs, target_rel,
        ),
        (KeyCode::Left, _) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
            *focus = focus.prev();
            MoveStep::Stay
        }
        (KeyCode::Right, _) | (KeyCode::Char('l'), KeyModifiers::NONE) => {
            *focus = focus.next();
            MoveStep::Stay
        }
        _ => MoveStep::NotHandled,
    }
}

/// Render the template (or build a `# <title>\n` stub for blank), then
/// build the `Composing` state with `target_is_new: true` and the
/// rendered content in memory. The file is **not** written here —
/// `commit_move` does that on `Enter`. `overwrite_existing` is kept for
/// future use (e.g. surface in toast wording); it has no behavioural
/// effect since the on-disk file is replaced wholesale by `write_pair`.
#[allow(clippy::too_many_arguments)]
fn advance_new_target_to_composing(
    ctx: &TabCtx,
    carry: MoveCarry,
    template: Option<&TemplatePick>,
    _folder: PathBuf,
    filename: String,
    vars: &BTreeMap<String, String>,
    target_abs: PathBuf,
    target_rel: PathBuf,
    _overwrite_existing: bool,
) -> MoveStep {
    let title = std::path::Path::new(&filename)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let target_content = match template {
        None => format!("# {title}\n"),
        Some(tpl) => {
            let tctx = build_template_context(title.clone(), ctx.today, vars.clone());
            match render_template(&tpl.source, &tctx) {
                Ok(s) => s,
                Err(e) => {
                    queue_toast(
                        ctx,
                        &format!("template render failed: {e}"),
                        ToastStyle::Error,
                    );
                    return back_to_target_picking(ctx, carry);
                }
            }
        }
    };
    let target_headings = extract_headings(&target_content);

    let mut layout: Vec<ComposeRow> = target_headings
        .iter()
        .map(|h| ComposeRow::Anchor {
            line: h.line,
            level: h.level,
            text: h.text.clone(),
        })
        .collect();
    for (idx, item) in carry.clipboard.iter().enumerate() {
        layout.push(ComposeRow::Pending {
            clip_idx: idx,
            level: item.level,
            rename: None,
        });
    }
    let focus = target_headings.len().min(layout.len().saturating_sub(1));

    MoveStep::Transition(SectionMoveState::Composing {
        source_rel: carry.source_rel,
        source_abs: carry.source_abs,
        source_content: carry.source_content,
        headings: carry.headings,
        selected: carry.selected,
        target_rel,
        target_abs,
        target_headings,
        target_content,
        target_is_new: true,
        clipboard: carry.clipboard,
        layout,
        focus,
        editing: None,
    })
}

/// Compose against an existing file (the user chose "Use existing" in
/// the collision prompt). Behavior matches the regular target-picker
/// path: we use the on-disk content as-is and treat the file as
/// non-new (`target_is_new: false`).
pub fn compose_with_existing_target(
    carry: MoveCarry,
    target_rel: PathBuf,
    target_abs: PathBuf,
    target_content: String,
) -> MoveStep {
    let target_headings = extract_headings(&target_content);
    let mut layout: Vec<ComposeRow> = target_headings
        .iter()
        .map(|h| ComposeRow::Anchor {
            line: h.line,
            level: h.level,
            text: h.text.clone(),
        })
        .collect();
    for (idx, item) in carry.clipboard.iter().enumerate() {
        layout.push(ComposeRow::Pending {
            clip_idx: idx,
            level: item.level,
            rename: None,
        });
    }
    let focus = target_headings.len().min(layout.len().saturating_sub(1));
    MoveStep::Transition(SectionMoveState::Composing {
        source_rel: carry.source_rel,
        source_abs: carry.source_abs,
        source_content: carry.source_content,
        headings: carry.headings,
        selected: carry.selected,
        target_rel,
        target_abs,
        target_headings,
        target_content,
        target_is_new: false,
        clipboard: carry.clipboard,
        layout,
        focus,
        editing: None,
    })
}

/// Default-replace `*carry` (which sits behind a `&mut`) and return the
/// owned value. Used by every transition out of the sub-flow because
/// pattern-matched fields can't be moved directly.
fn take_carry(carry: &mut MoveCarry) -> MoveCarry {
    std::mem::replace(
        carry,
        MoveCarry {
            source_rel: PathBuf::new(),
            source_abs: PathBuf::new(),
            source_content: String::new(),
            headings: Vec::new(),
            selected: BTreeSet::new(),
            focus: 0,
            clipboard: Vec::new(),
        },
    )
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn discover(tmp: &assert_fs::TempDir) -> Arc<ft_core::vault::Vault> {
        Arc::new(ft_core::vault::Vault::discover(Some(tmp.path().to_path_buf())).unwrap())
    }

    /// `begin_for_source` on a note with headings opens the multi-select
    /// step seeded to that note — no source-picker round-trip.
    #[test]
    fn begin_for_source_opens_multiselect_scoped_to_note() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("Note.md")
            .write_str("# Note\n\n## Alpha\n\nBody a.\n\n## Beta\n\nBody b.\n")
            .unwrap();
        let vault = discover(&tmp);
        let recents = Arc::new(ft_core::recents::RecentsLog::with_log_path(
            vault.path.clone(),
            vault.path.join("recents.jsonl"),
        ));
        let last_refresh = std::cell::Cell::new(None);
        let pending = std::cell::RefCell::new(None);
        let graph_refresh = std::cell::Cell::new(false);
        let ctx = crate::tui::tab::TabCtx {
            vault: &vault,
            recents: &recents,
            today: chrono::NaiveDate::from_ymd_opt(2026, 7, 4).unwrap(),
            last_refresh: &last_refresh,
            pending_request: &pending,
            active_modal_name: None,
            host_popup_open: false,
            snapshot: None,
            graph_refresh: &graph_refresh,
        };

        let state =
            begin_for_source(&ctx, PathBuf::from("Note.md")).expect("note has headings → Some");
        match state {
            SectionMoveState::HeadingMultiSelect {
                source_rel,
                headings,
                selected,
                focus,
                ..
            } => {
                assert_eq!(source_rel, PathBuf::from("Note.md"));
                let texts: Vec<&str> = headings.iter().map(|h| h.text.as_str()).collect();
                assert_eq!(texts, vec!["Note", "Alpha", "Beta"]);
                assert!(selected.is_empty());
                assert_eq!(focus, 0);
            }
            _ => panic!("expected HeadingMultiSelect, seeded to the note"),
        }
    }

    /// A note with no headings yields `None` (nothing to move).
    #[test]
    fn begin_for_source_none_when_no_headings() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("Flat.md")
            .write_str("Just body, no headings.\n")
            .unwrap();
        let vault = discover(&tmp);
        let recents = Arc::new(ft_core::recents::RecentsLog::with_log_path(
            vault.path.clone(),
            vault.path.join("recents.jsonl"),
        ));
        let last_refresh = std::cell::Cell::new(None);
        let pending = std::cell::RefCell::new(None);
        let graph_refresh = std::cell::Cell::new(false);
        let ctx = crate::tui::tab::TabCtx {
            vault: &vault,
            recents: &recents,
            today: chrono::NaiveDate::from_ymd_opt(2026, 7, 4).unwrap(),
            last_refresh: &last_refresh,
            pending_request: &pending,
            active_modal_name: None,
            host_popup_open: false,
            snapshot: None,
            graph_refresh: &graph_refresh,
        };
        assert!(begin_for_source(&ctx, PathBuf::from("Flat.md")).is_none());
    }
}

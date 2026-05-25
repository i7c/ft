//! Notes tab — Obsidian-flavoured editing surface.
//!
//! Session 3 (plan 003) wired the tab into the App and added the open
//! flow. Session 4 added steps 1-3 of the section-move flow (source pick →
//! heading multi-select → target pick). Session 5 lands the compose view
//! and commit: an interleaved layout of target anchors + pending picks,
//! per-row level shift, drag-to-reorder, and a final freshness-checked
//! `move_sections` + `write_pair` commit.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ft_core::markdown::{extract_headings, Heading};
use ft_core::notes::template::render as render_template;
use ft_core::notes::{
    extract_sections, move_sections, shift_section_level, write_pair, Placement, Section,
    SectionPick,
};
use ft_core::periodic::Period;
use ft_core::search::Hit;
use ratatui::{layout::Rect, Frame};

use crate::tui::{
    event::Event,
    notes_actions::{
        create::{
            self, begin_folder_picking, begin_template_picking, build_template_context,
            discover_template_vars, enumerate_templates, enumerate_vault_folders, CollisionChoice,
            CreateState, CreateStep, TemplatePick,
        },
        periodic::run_periodic_open,
    },
    tab::{AppRequest, EventOutcome, Tab, TabCtx, ToastStyle},
    widgets::{
        EditBuffer, FuzzyPicker, PathListPickerSource, PickerOutcome, VaultFilePickerSource,
    },
};

pub(crate) mod view;

/// Top-level state for the Notes tab. Each variant owns the data the
/// corresponding view needs — no shared mutable scratch.
pub enum NotesState {
    /// Default landing surface. Shows the keymap-style help panel; `o`
    /// opens the file picker, `m` enters the section-move flow, `c`/`C`
    /// enters the create flow.
    Idle,
    /// File / heading picker open for the "open in editor / Obsidian"
    /// flow. `Enter` → editor at line 1, `Ctrl+O` → Obsidian URL, `Esc`
    /// → back to idle.
    OpenPicking {
        picker: FuzzyPicker<VaultFilePickerSource>,
    },
    /// Section-move flow (sessions 4 + 5). See [`SectionMoveState`].
    MoveSection(SectionMoveState),
    /// Create-flow (plan 009 session 4). See [`CreateState`].
    Creating(CreateState),
    /// Plan 010 session 3 — transient modal entered by pressing `p` from
    /// idle. A second key (`d|w|m|q|y`) fires the periodic-open flow for
    /// the corresponding period and drops back to idle. Any other key
    /// (including `Esc`) cancels back to idle with no toast.
    PeriodicLeader,
}

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

pub struct NotesTab {
    state: NotesState,
    /// Whether the tab-local help overlay is showing. Toggled by `?` while
    /// idle; the overlay shadows the help-panel body until dismissed.
    show_help: bool,
}

impl NotesTab {
    pub fn new() -> Self {
        Self {
            state: NotesState::Idle,
            show_help: false,
        }
    }

    fn new_vault_picker(ctx: &TabCtx) -> FuzzyPicker<VaultFilePickerSource> {
        FuzzyPicker::new(VaultFilePickerSource::new(
            Arc::clone(ctx.vault),
            Arc::clone(ctx.recents),
        ))
    }

    fn handle_idle_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        if self.show_help {
            return match k.code {
                KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => {
                    self.show_help = false;
                    EventOutcome::Consumed
                }
                _ => EventOutcome::Consumed,
            };
        }
        match (k.code, k.modifiers) {
            (KeyCode::Char('?'), _) => {
                self.show_help = true;
                EventOutcome::Consumed
            }
            (KeyCode::Char('o'), KeyModifiers::NONE) => {
                self.state = NotesState::OpenPicking {
                    picker: Self::new_vault_picker(ctx),
                };
                EventOutcome::Consumed
            }
            (KeyCode::Char('m'), KeyModifiers::NONE) => {
                self.state = NotesState::MoveSection(SectionMoveState::SourcePicking {
                    picker: Self::new_vault_picker(ctx),
                });
                EventOutcome::Consumed
            }
            (KeyCode::Char('c'), KeyModifiers::NONE) => {
                self.state = NotesState::Creating(begin_folder_picking(ctx, None));
                EventOutcome::Consumed
            }
            (KeyCode::Char('C'), _) | (KeyCode::Char('c'), KeyModifiers::SHIFT) => {
                self.state = NotesState::Creating(begin_template_picking(ctx, None));
                EventOutcome::Consumed
            }
            // `t` is a one-shot synonym for `p` then `d` — opens today's
            // daily note directly without entering the leader modal.
            (KeyCode::Char('t'), KeyModifiers::NONE) => {
                run_periodic_open(ctx, Period::Daily);
                EventOutcome::Consumed
            }
            // `p` enters the leader; the next key chooses a period.
            (KeyCode::Char('p'), KeyModifiers::NONE) => {
                self.state = NotesState::PeriodicLeader;
                EventOutcome::Consumed
            }
            _ => EventOutcome::NotHandled,
        }
    }

    fn handle_periodic_leader_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        // Period letters fire the open flow; everything else (including
        // `Esc`, `p` re-entry, and unknown letters) cancels back to idle
        // silently. The state transition happens before the open flow so
        // a toast from `run_periodic_open` lands cleanly under idle.
        let period = match (k.code, k.modifiers) {
            (KeyCode::Char('d'), KeyModifiers::NONE) => Some(Period::Daily),
            (KeyCode::Char('w'), KeyModifiers::NONE) => Some(Period::Weekly),
            (KeyCode::Char('m'), KeyModifiers::NONE) => Some(Period::Monthly),
            (KeyCode::Char('q'), KeyModifiers::NONE) => Some(Period::Quarterly),
            (KeyCode::Char('y'), KeyModifiers::NONE) => Some(Period::Yearly),
            _ => None,
        };
        self.state = NotesState::Idle;
        if let Some(p) = period {
            run_periodic_open(ctx, p);
        }
        EventOutcome::Consumed
    }

    fn handle_open_picker_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        let NotesState::OpenPicking { picker } = &mut self.state else {
            return EventOutcome::NotHandled;
        };
        // `Ctrl+O` is our own binding; intercept before handing to picker.
        if k.code == KeyCode::Char('o') && k.modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(item) = picker.selected_item() {
                let hit = item.data.clone();
                request_open_in_obsidian(ctx, &hit);
                self.state = NotesState::Idle;
            }
            return EventOutcome::Consumed;
        }
        match picker.handle_key(k) {
            PickerOutcome::Selected(hit) => {
                request_open_in_editor(ctx, &hit);
                self.state = NotesState::Idle;
                EventOutcome::Consumed
            }
            PickerOutcome::Cancelled => {
                self.state = NotesState::Idle;
                EventOutcome::Consumed
            }
            PickerOutcome::StillOpen => EventOutcome::Consumed,
            PickerOutcome::NotHandled => EventOutcome::NotHandled,
        }
    }

    fn handle_create_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        let NotesState::Creating(cs) = &mut self.state else {
            return EventOutcome::NotHandled;
        };
        match create::handle_key(cs, k, ctx) {
            CreateStep::Stay => EventOutcome::Consumed,
            CreateStep::NotHandled => EventOutcome::NotHandled,
            CreateStep::Transition(next) => {
                self.state = NotesState::Creating(next);
                EventOutcome::Consumed
            }
            CreateStep::Finished => {
                self.state = NotesState::Idle;
                EventOutcome::Consumed
            }
        }
    }

    fn handle_move_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        let NotesState::MoveSection(ms) = &mut self.state else {
            return EventOutcome::NotHandled;
        };
        let next = match ms {
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
        };
        match next {
            MoveAction::Stay => EventOutcome::Consumed,
            MoveAction::NotHandled => EventOutcome::NotHandled,
            MoveAction::Set(next) => {
                self.state = *next;
                EventOutcome::Consumed
            }
        }
    }
}

impl Tab for NotesTab {
    fn title(&self) -> &str {
        "Notes"
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };
        let outcome = match &self.state {
            NotesState::Idle => self.handle_idle_key(k, ctx),
            NotesState::OpenPicking { .. } => self.handle_open_picker_key(k, ctx),
            NotesState::MoveSection(_) => self.handle_move_key(k, ctx),
            NotesState::Creating(_) => self.handle_create_key(k, ctx),
            NotesState::PeriodicLeader => self.handle_periodic_leader_key(k, ctx),
        };
        Ok(outcome)
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        view::render(frame, area, ctx, &mut self.state, self.show_help);
    }
}

/// Outcome of a step-handler: either keep the current state, replace
/// it, or pass on the keypress. Lets the handlers run with `&mut` on
/// individual fields without re-borrowing `self.state`.
enum MoveAction {
    Stay,
    NotHandled,
    Set(Box<NotesState>),
}

fn handle_source_picker_key(
    k: KeyEvent,
    picker: &mut FuzzyPicker<VaultFilePickerSource>,
    ctx: &TabCtx,
) -> MoveAction {
    match picker.handle_key(k) {
        PickerOutcome::Selected(hit) => MoveAction::Set(Box::new(advance_to_multiselect(ctx, hit))),
        PickerOutcome::Cancelled => MoveAction::Set(Box::new(NotesState::Idle)),
        PickerOutcome::StillOpen => MoveAction::Stay,
        PickerOutcome::NotHandled => MoveAction::NotHandled,
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
) -> MoveAction {
    match (k.code, k.modifiers) {
        (KeyCode::Esc, _) => MoveAction::Set(Box::new(NotesState::MoveSection(
            SectionMoveState::SourcePicking {
                picker: NotesTab::new_vault_picker(ctx),
            },
        ))),
        (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
            if *focus > 0 {
                *focus -= 1;
            } else {
                *focus = headings.len().saturating_sub(1);
            }
            MoveAction::Stay
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
            if headings.is_empty() {
                return MoveAction::Stay;
            }
            *focus = (*focus + 1) % headings.len();
            MoveAction::Stay
        }
        (KeyCode::Char(' '), _) => {
            toggle_selection(headings, selected, *focus);
            MoveAction::Stay
        }
        (KeyCode::Enter, _) => {
            if selected.is_empty() {
                queue_toast(ctx, "select at least one heading", ToastStyle::Error);
                return MoveAction::Stay;
            }
            let clipboard = build_clipboard(source_content, headings, selected);
            if clipboard.is_empty() {
                queue_toast(ctx, "no sections extracted", ToastStyle::Error);
                return MoveAction::Stay;
            }
            MoveAction::Set(Box::new(NotesState::MoveSection(
                SectionMoveState::TargetPicking {
                    source_rel: std::mem::take(source_rel),
                    source_abs: std::mem::take(source_abs),
                    source_content: std::mem::take(source_content),
                    headings: std::mem::take(headings),
                    selected: std::mem::take(selected),
                    focus: *focus,
                    clipboard,
                    picker: NotesTab::new_vault_picker(ctx),
                    error: None,
                },
            )))
        }
        _ => MoveAction::NotHandled,
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
) -> MoveAction {
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
        return MoveAction::Set(Box::new(NotesState::MoveSection(
            SectionMoveState::NewTargetCreating(begin_new_target_template_picking(ctx, carry)),
        )));
    }
    match picker.handle_key(k) {
        PickerOutcome::Selected(hit) => {
            if hit.path == *source_rel {
                *error = Some("same-file move is out of scope — pick a different target".into());
                MoveAction::Stay
            } else {
                MoveAction::Set(Box::new(advance_to_composing(
                    ctx,
                    std::mem::take(source_rel),
                    std::mem::take(source_abs),
                    std::mem::take(source_content),
                    std::mem::take(headings),
                    std::mem::take(selected),
                    std::mem::take(clipboard),
                    hit,
                )))
            }
        }
        PickerOutcome::Cancelled => MoveAction::Set(Box::new(NotesState::MoveSection(
            SectionMoveState::HeadingMultiSelect {
                source_rel: std::mem::take(source_rel),
                source_abs: std::mem::take(source_abs),
                source_content: std::mem::take(source_content),
                headings: std::mem::take(headings),
                selected: std::mem::take(selected),
                focus: *focus,
            },
        ))),
        PickerOutcome::StillOpen => {
            // Any text-edit / nav keystroke clears a stale "same file" error.
            if error.is_some() {
                *error = None;
            }
            MoveAction::Stay
        }
        PickerOutcome::NotHandled => MoveAction::NotHandled,
    }
}

fn advance_to_multiselect(ctx: &TabCtx, hit: Hit) -> NotesState {
    let abs = ctx.vault.path.join(&hit.path);
    let content = match std::fs::read_to_string(&abs) {
        Ok(s) => s,
        Err(e) => {
            queue_toast(
                ctx,
                &format!("could not read source: {e}"),
                ToastStyle::Error,
            );
            return NotesState::Idle;
        }
    };
    let headings = extract_headings(&content);
    if headings.is_empty() {
        queue_toast(ctx, "source has no headings to move", ToastStyle::Error);
        return NotesState::Idle;
    }
    NotesState::MoveSection(SectionMoveState::HeadingMultiSelect {
        source_rel: hit.path,
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
) -> NotesState {
    let target_abs = ctx.vault.path.join(&target_hit.path);
    let target_content = match std::fs::read_to_string(&target_abs) {
        Ok(s) => s,
        Err(e) => {
            queue_toast(
                ctx,
                &format!("could not read target: {e}"),
                ToastStyle::Error,
            );
            return NotesState::Idle;
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
    NotesState::MoveSection(SectionMoveState::Composing {
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
) -> MoveAction {
    // Rename buffer is a sub-mode of Composing: when open it consumes
    // every compose key so `r`/`Shift+↑`/`←` etc. don't fire under it.
    if editing.is_some() {
        return handle_rename_buffer_key(k, layout, editing, ctx);
    }
    let shift = k.modifiers.contains(KeyModifiers::SHIFT);
    match (k.code, shift) {
        (KeyCode::Esc, _) => MoveAction::Set(Box::new(NotesState::MoveSection(
            SectionMoveState::TargetPicking {
                source_rel: std::mem::take(source_rel),
                source_abs: std::mem::take(source_abs),
                source_content: std::mem::take(source_content),
                headings: std::mem::take(headings),
                selected: std::mem::take(selected),
                focus: 0,
                clipboard: std::mem::take(clipboard),
                picker: NotesTab::new_vault_picker(ctx),
                error: None,
            },
        ))),
        (KeyCode::Up, false) | (KeyCode::Char('k'), false) => {
            if *focus > 0 {
                *focus -= 1;
            } else {
                *focus = layout.len().saturating_sub(1);
            }
            MoveAction::Stay
        }
        (KeyCode::Down, false) | (KeyCode::Char('j'), false) => {
            if !layout.is_empty() {
                *focus = (*focus + 1) % layout.len();
            }
            MoveAction::Stay
        }
        (KeyCode::Up, true) | (KeyCode::Char('K'), _) => {
            if reorder_pending(layout, *focus, -1) {
                *focus -= 1;
            }
            MoveAction::Stay
        }
        (KeyCode::Down, true) | (KeyCode::Char('J'), _) => {
            if reorder_pending(layout, *focus, 1) {
                *focus += 1;
            }
            MoveAction::Stay
        }
        (KeyCode::Left, _) | (KeyCode::Char('h'), false) => {
            shift_focused_level(layout, clipboard, *focus, -1, ctx);
            MoveAction::Stay
        }
        (KeyCode::Right, _) | (KeyCode::Char('l'), false) => {
            shift_focused_level(layout, clipboard, *focus, 1, ctx);
            MoveAction::Stay
        }
        (KeyCode::Char('r'), false) => {
            open_rename_buffer(layout, clipboard, *focus, editing);
            MoveAction::Stay
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
            MoveAction::Set(Box::new(NotesState::Idle))
        }
        _ => MoveAction::NotHandled,
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
) -> MoveAction {
    let Some(rb) = editing.as_mut() else {
        return MoveAction::NotHandled;
    };
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match (k.code, ctrl) {
        (KeyCode::Esc, _) => {
            *editing = None;
            MoveAction::Stay
        }
        (KeyCode::Enter, _) => {
            let trimmed = rb.buf.text.trim();
            if trimmed.is_empty() {
                queue_toast(ctx, "rename cannot be empty", ToastStyle::Error);
                return MoveAction::Stay;
            }
            if trimmed.contains('\n') || trimmed.contains('\r') {
                queue_toast(ctx, "rename cannot contain newlines", ToastStyle::Error);
                return MoveAction::Stay;
            }
            let new_text = trimmed.to_string();
            let row_idx = rb.row_idx;
            if let Some(ComposeRow::Pending { rename, .. }) = layout.get_mut(row_idx) {
                *rename = Some(new_text);
            }
            *editing = None;
            MoveAction::Stay
        }
        (KeyCode::Char('w'), true) => {
            rb.buf.delete_word_backward();
            MoveAction::Stay
        }
        (KeyCode::Char(c), false) => {
            rb.buf.insert(c);
            MoveAction::Stay
        }
        (KeyCode::Backspace, _) => {
            rb.buf.backspace();
            MoveAction::Stay
        }
        (KeyCode::Delete, _) => {
            rb.buf.delete();
            MoveAction::Stay
        }
        (KeyCode::Left, _) => {
            rb.buf.left();
            MoveAction::Stay
        }
        (KeyCode::Right, _) => {
            rb.buf.right();
            MoveAction::Stay
        }
        (KeyCode::Home, _) => {
            rb.buf.home();
            MoveAction::Stay
        }
        (KeyCode::End, _) => {
            rb.buf.end();
            MoveAction::Stay
        }
        // Swallow everything else so compose-level keys (`r`, `Shift+↑`,
        // navigation, Enter-modifiers) can't fire under the buffer.
        _ => MoveAction::Stay,
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

fn queue_toast(ctx: &TabCtx, text: &str, style: ToastStyle) {
    *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast {
        text: text.to_string(),
        style,
    });
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

fn request_open_in_editor(ctx: &TabCtx, hit: &Hit) {
    let abs = ctx.vault.path.join(&hit.path);
    let line = hit.heading.as_ref().map(|h| h.line).unwrap_or(1);
    // Record the open before raising the AppRequest so a subsequent
    // picker invocation surfaces this note at the top of recents.
    // `record_open` is best-effort and never errors.
    ctx.recents.record_open(&hit.path);
    *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor { path: abs, line });
}

fn request_open_in_obsidian(ctx: &TabCtx, hit: &Hit) {
    let vault_name = ctx
        .vault
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "vault".to_string());
    let url = ft_core::notes::obsidian_url(&vault_name, &hit.path, hit.heading.as_ref());
    ctx.recents.record_open(&hit.path);
    *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInObsidian { url });
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
fn handle_new_target_key(k: KeyEvent, nts: &mut NewTargetState, ctx: &TabCtx) -> MoveAction {
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
fn back_to_target_picking(ctx: &TabCtx, carry: MoveCarry) -> NotesState {
    NotesState::MoveSection(SectionMoveState::TargetPicking {
        source_rel: carry.source_rel,
        source_abs: carry.source_abs,
        source_content: carry.source_content,
        headings: carry.headings,
        selected: carry.selected,
        focus: carry.focus,
        clipboard: carry.clipboard,
        picker: NotesTab::new_vault_picker(ctx),
        error: None,
    })
}

fn handle_new_target_template_picker_key(
    k: KeyEvent,
    carry: &mut MoveCarry,
    picker: &mut FuzzyPicker<PathListPickerSource>,
    ctx: &TabCtx,
) -> MoveAction {
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
                        return MoveAction::Set(Box::new(back_to_target_picking(
                            ctx,
                            take_carry(carry),
                        )));
                    }
                };
                let vars_needed = discover_template_vars(&source);
                Some(TemplatePick {
                    rel,
                    source,
                    vars_needed,
                })
            };
            MoveAction::Set(Box::new(NotesState::MoveSection(
                SectionMoveState::NewTargetCreating(begin_new_target_folder_picking(
                    ctx,
                    take_carry(carry),
                    template,
                )),
            )))
        }
        PickerOutcome::Cancelled => {
            MoveAction::Set(Box::new(back_to_target_picking(ctx, take_carry(carry))))
        }
        PickerOutcome::StillOpen => MoveAction::Stay,
        PickerOutcome::NotHandled => MoveAction::NotHandled,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_new_target_folder_picker_key(
    k: KeyEvent,
    carry: &mut MoveCarry,
    template: &mut Option<TemplatePick>,
    picker: &mut FuzzyPicker<PathListPickerSource>,
    ctx: &TabCtx,
) -> MoveAction {
    match picker.handle_key(k) {
        PickerOutcome::Selected(folder) => {
            let folder = if folder == Path::new(".") {
                PathBuf::new()
            } else {
                folder
            };
            MoveAction::Set(Box::new(NotesState::MoveSection(
                SectionMoveState::NewTargetCreating(NewTargetState::FilenamePrompt {
                    carry: take_carry(carry),
                    template: template.take(),
                    folder,
                    buf: EditBuffer::default(),
                    error: None,
                }),
            )))
        }
        // Esc → back to the template picker (always present in the
        // sub-flow, since this is the new-target path).
        PickerOutcome::Cancelled => MoveAction::Set(Box::new(NotesState::MoveSection(
            SectionMoveState::NewTargetCreating(begin_new_target_template_picking(
                ctx,
                take_carry(carry),
            )),
        ))),
        PickerOutcome::StillOpen => MoveAction::Stay,
        PickerOutcome::NotHandled => MoveAction::NotHandled,
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
) -> MoveAction {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match (k.code, ctrl) {
        (KeyCode::Esc, _) => MoveAction::Set(Box::new(NotesState::MoveSection(
            SectionMoveState::NewTargetCreating(begin_new_target_folder_picking(
                ctx,
                take_carry(carry),
                template.take(),
            )),
        ))),
        (KeyCode::Enter, _) => {
            let trimmed = buf.text.trim();
            if trimmed.is_empty() {
                *error = Some("filename is required".into());
                return MoveAction::Stay;
            }
            if trimmed.contains('/') || trimmed.contains('\\') {
                *error = Some("filename can't contain path separators".into());
                return MoveAction::Stay;
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
                return MoveAction::Stay;
            }

            if abs_path.exists() {
                return MoveAction::Set(Box::new(NotesState::MoveSection(
                    SectionMoveState::NewTargetCreating(NewTargetState::CollisionPrompt {
                        carry: take_carry(carry),
                        template: template.take(),
                        folder: std::mem::take(folder),
                        filename,
                        vars: BTreeMap::new(),
                        target_abs: abs_path,
                        target_rel: rel_path,
                        focus: CollisionChoice::Overwrite,
                    }),
                )));
            }

            // No collision. Advance to var prompts (if any) or commit
            // straight into Composing.
            match template.take() {
                Some(tpl) if !tpl.vars_needed.is_empty() => {
                    MoveAction::Set(Box::new(NotesState::MoveSection(
                        SectionMoveState::NewTargetCreating(NewTargetState::VarPrompt {
                            carry: take_carry(carry),
                            template: tpl,
                            folder: std::mem::take(folder),
                            filename,
                            vars_so_far: BTreeMap::new(),
                            next_idx: 0,
                            buf: EditBuffer::default(),
                        }),
                    )))
                }
                tpl => MoveAction::Set(Box::new(advance_new_target_to_composing(
                    ctx,
                    take_carry(carry),
                    tpl.as_ref(),
                    std::mem::take(folder),
                    filename,
                    &BTreeMap::new(),
                    abs_path,
                    rel_path,
                    /*overwrite_existing=*/ false,
                ))),
            }
        }
        (KeyCode::Char('w'), true) => {
            buf.delete_word_backward();
            *error = None;
            MoveAction::Stay
        }
        (KeyCode::Char(c), false) => {
            buf.insert(c);
            *error = None;
            MoveAction::Stay
        }
        (KeyCode::Backspace, _) => {
            buf.backspace();
            *error = None;
            MoveAction::Stay
        }
        (KeyCode::Delete, _) => {
            buf.delete();
            *error = None;
            MoveAction::Stay
        }
        (KeyCode::Left, _) => {
            buf.left();
            MoveAction::Stay
        }
        (KeyCode::Right, _) => {
            buf.right();
            MoveAction::Stay
        }
        (KeyCode::Home, _) => {
            buf.home();
            MoveAction::Stay
        }
        (KeyCode::End, _) => {
            buf.end();
            MoveAction::Stay
        }
        _ => MoveAction::NotHandled,
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
) -> MoveAction {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match (k.code, ctrl) {
        // Esc here cancels the entire new-target sub-flow, returning
        // to TargetPicking with the clipboard preserved. Going "back"
        // step-by-step is more granular than the user wants here.
        (KeyCode::Esc, _) => {
            MoveAction::Set(Box::new(back_to_target_picking(ctx, take_carry(carry))))
        }
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
                MoveAction::Set(Box::new(advance_new_target_to_composing(
                    ctx,
                    take_carry(carry),
                    Some(template),
                    std::mem::take(folder),
                    std::mem::take(filename),
                    vars_so_far,
                    abs_path,
                    rel_path,
                    /*overwrite_existing=*/ false,
                )))
            } else {
                buf.text.clear();
                buf.cursor = 0;
                MoveAction::Stay
            }
        }
        (KeyCode::Char('w'), true) => {
            buf.delete_word_backward();
            MoveAction::Stay
        }
        (KeyCode::Char(c), false) => {
            buf.insert(c);
            MoveAction::Stay
        }
        (KeyCode::Backspace, _) => {
            buf.backspace();
            MoveAction::Stay
        }
        (KeyCode::Delete, _) => {
            buf.delete();
            MoveAction::Stay
        }
        (KeyCode::Left, _) => {
            buf.left();
            MoveAction::Stay
        }
        (KeyCode::Right, _) => {
            buf.right();
            MoveAction::Stay
        }
        (KeyCode::Home, _) => {
            buf.home();
            MoveAction::Stay
        }
        (KeyCode::End, _) => {
            buf.end();
            MoveAction::Stay
        }
        _ => MoveAction::NotHandled,
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
) -> MoveAction {
    let go = |choice: CollisionChoice,
              carry: &mut MoveCarry,
              template: &mut Option<TemplatePick>,
              folder: &mut PathBuf,
              filename: &mut String,
              vars: &mut BTreeMap<String, String>,
              target_abs: &mut PathBuf,
              target_rel: &mut PathBuf|
     -> MoveAction {
        match choice {
            CollisionChoice::Overwrite => {
                MoveAction::Set(Box::new(advance_new_target_to_composing(
                    ctx,
                    take_carry(carry),
                    template.as_ref(),
                    std::mem::take(folder),
                    std::mem::take(filename),
                    vars,
                    std::mem::take(target_abs),
                    std::mem::take(target_rel),
                    /*overwrite_existing=*/ true,
                )))
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
                        return MoveAction::Set(Box::new(back_to_target_picking(
                            ctx,
                            take_carry(carry),
                        )));
                    }
                };
                MoveAction::Set(Box::new(compose_with_existing_target(
                    take_carry(carry),
                    std::mem::take(target_rel),
                    std::mem::take(target_abs),
                    existing,
                )))
            }
            CollisionChoice::Cancel => MoveAction::Set(Box::new(NotesState::MoveSection(
                SectionMoveState::NewTargetCreating(NewTargetState::FilenamePrompt {
                    carry: take_carry(carry),
                    template: template.take(),
                    folder: std::mem::take(folder),
                    buf: EditBuffer::from(filename),
                    error: None,
                }),
            ))),
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
            MoveAction::Stay
        }
        (KeyCode::Right, _) | (KeyCode::Char('l'), KeyModifiers::NONE) => {
            *focus = focus.next();
            MoveAction::Stay
        }
        _ => MoveAction::NotHandled,
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
) -> NotesState {
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

    NotesState::MoveSection(SectionMoveState::Composing {
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
fn compose_with_existing_target(
    carry: MoveCarry,
    target_rel: PathBuf,
    target_abs: PathBuf,
    target_content: String,
) -> NotesState {
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
    NotesState::MoveSection(SectionMoveState::Composing {
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

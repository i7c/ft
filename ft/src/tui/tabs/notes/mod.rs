//! Notes tab — Obsidian-flavoured editing surface.
//!
//! Session 3 (plan 003) wired the tab into the App and added the open
//! flow. Session 4 added steps 1-3 of the section-move flow (source pick →
//! heading multi-select → target pick). Session 5 lands the compose view
//! and commit: an interleaved layout of target anchors + pending picks,
//! per-row level shift, drag-to-reorder, and a final freshness-checked
//! `move_sections` + `write_pair` commit.

use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ft_core::periodic::Period;
use ft_core::search::Hit;
use ratatui::{layout::Rect, Frame};

use crate::tui::{
    event::Event,
    notes_actions::{
        create::{self, begin_folder_picking, begin_template_picking, CreateState, CreateStep},
        periodic::run_periodic_open,
        section_move::{self, MoveStep, SectionMoveState},
    },
    tab::{AppRequest, EventOutcome, Tab, TabCtx},
    widgets::{FuzzyPicker, PickerOutcome, VaultFilePickerSource},
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
                self.state = NotesState::MoveSection(section_move::begin_with_picker(ctx));
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
        match section_move::handle_key(ms, k, ctx) {
            MoveStep::Stay => EventOutcome::Consumed,
            MoveStep::NotHandled => EventOutcome::NotHandled,
            MoveStep::Transition(next) => {
                self.state = NotesState::MoveSection(next);
                EventOutcome::Consumed
            }
            MoveStep::Finished => {
                self.state = NotesState::Idle;
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

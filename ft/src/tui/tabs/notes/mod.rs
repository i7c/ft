//! Notes tab — Obsidian-flavoured editing surface.
//!
//! Session 3 (plan 003) wired the tab into the App and added the open
//! flow. Session 4 added steps 1-3 of the section-move flow (source pick →
//! heading multi-select → target pick). Session 5 lands the compose view
//! and commit: an interleaved layout of target anchors + pending picks,
//! per-row level shift, drag-to-reorder, and a final freshness-checked
//! `move_sections` + `write_pair` commit.

use std::sync::Arc;
use std::sync::LazyLock;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ft_core::periodic::Period;
use ft_core::search::Hit;
use ratatui::{layout::Rect, Frame};

use crate::tui::{
    command::{Command, CommandDef, CommandOutcome, CommandScope},
    event::Event,
    help::HelpSection,
    keymap::{KeyChord, KeyMap},
    notes_actions::{
        append::{self, AppendState, AppendStep},
        capture::{self, CapturePresetPickerSource, CaptureVarPromptState},
        create::{self, begin_folder_picking, begin_template_picking, CreateState, CreateStep},
        periodic::run_periodic_open,
        section_move::{self, MoveStep, SectionMoveState},
    },
    tab::{AppRequest, EventOutcome, Tab, TabCtx},
    widgets::{FuzzyPicker, PickerOutcome, VaultFilePickerSource},
};

// ── Commands ─────────────────────────────────────────────────────────

/// Every Idle-state action the Notes tab exposes. Sub-state handlers
/// (OpenPicking, MoveSection, Creating, Appending, CapturePicking,
/// CaptureVarPrompt, PeriodicLeader) capture raw keys and bypass the
/// keymap — same pattern as JournalTab's picker overlay.
pub(crate) static NOTES_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "notes.open-picker",
        description: "Open the fuzzy file / heading picker to open a note",
        scope: CommandScope::Tab("notes"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "notes.move-section",
        description: "Enter the move-section flow",
        scope: CommandScope::Tab("notes"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "notes.create-blank",
        description: "Create a new note (blank)",
        scope: CommandScope::Tab("notes"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "notes.append",
        description: "Append a template to a note",
        scope: CommandScope::Tab("notes"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "notes.quick-capture",
        description: "Quick capture (run a preset)",
        scope: CommandScope::Tab("notes"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "notes.create-from-template",
        description: "Create a new note from a template",
        scope: CommandScope::Tab("notes"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "notes.today",
        description: "Open today's daily note",
        scope: CommandScope::Tab("notes"),
        group: "Periodic notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "notes.periodic-leader",
        description: "Enter the periodic-note leader (then d/w/m/q/y)",
        scope: CommandScope::Tab("notes"),
        group: "Periodic notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
];

/// Default keymap for the Notes tab's Idle state. Sub-states are
/// handled by `handle_*_key` methods, bypassing the keymap.
pub(crate) static NOTES_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("o", "notes.open-picker")
        .bind("m", "notes.move-section")
        .bind("c", "notes.create-blank")
        .bind("a", "notes.append")
        .bind("Q", "notes.quick-capture")
        .bind("C", "notes.create-from-template")
        .bind("t", "notes.today")
        .bind("p", "notes.periodic-leader")
});

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
    /// Append-with-template flow. See [`AppendState`].
    Appending(AppendState),
    /// Quick capture preset picker.
    CapturePicking {
        picker: FuzzyPicker<CapturePresetPickerSource>,
    },
    /// Quick capture var prompt (template has `vars.*` references).
    CaptureVarPrompt(CaptureVarPromptState),
    /// Plan 010 session 3 — transient modal entered by pressing `p` from
    /// idle. A second key (`d|w|m|q|y`) fires the periodic-open flow for
    /// the corresponding period and drops back to idle. Any other key
    /// (including `Esc`) cancels back to idle with no toast.
    PeriodicLeader,
}

pub struct NotesTab {
    state: NotesState,
    keymap: crate::tui::keymap::KeyMap,
}

impl NotesTab {
    pub fn new() -> Self {
        Self {
            state: NotesState::Idle,
            keymap: NOTES_KEYMAP.clone(),
        }
    }

    pub fn with_keymap_overlay(mut self, overlay: &crate::tui::keymap::KeymapOverlay) -> Self {
        self.keymap = NOTES_KEYMAP.with_overlay(overlay);
        self
    }

    fn new_vault_picker(ctx: &TabCtx) -> FuzzyPicker<VaultFilePickerSource> {
        FuzzyPicker::new(VaultFilePickerSource::new(
            Arc::clone(ctx.vault),
            Arc::clone(ctx.recents),
        ))
    }

    fn handle_idle_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        // The Idle keymap is the only one declared in `NOTES_KEYMAP` —
        // sub-state handlers (open-picker, create, append, capture,
        // periodic leader) capture raw keys and bypass this path.
        let chord = KeyChord::from_key_event(k);
        let Some(cmd) = self.keymap.lookup(chord).cloned() else {
            return EventOutcome::NotHandled;
        };
        self.dispatch_idle_command(&cmd, ctx)
    }

    /// Apply one Idle-state command. Split off from `Tab::dispatch_command`
    /// so callers with `&TabCtx` (not `&mut TabCtx`) can invoke it —
    /// `handle_idle_key` receives `&TabCtx` from `handle_event` to keep
    /// the per-state borrow shape stable.
    fn dispatch_idle_command(&mut self, cmd: &Command, ctx: &TabCtx) -> EventOutcome {
        match cmd.name {
            "notes.open-picker" => {
                self.state = NotesState::OpenPicking {
                    picker: Self::new_vault_picker(ctx),
                };
                EventOutcome::Consumed
            }
            "notes.move-section" => {
                self.state = NotesState::MoveSection(section_move::begin_with_picker(ctx));
                EventOutcome::Consumed
            }
            "notes.create-blank" => {
                self.state = NotesState::Creating(begin_folder_picking(ctx, None));
                EventOutcome::Consumed
            }
            "notes.append" => {
                self.state = NotesState::Appending(AppendState::begin_no_target(ctx, None));
                EventOutcome::Consumed
            }
            "notes.quick-capture" => {
                let src = CapturePresetPickerSource::new(ctx.vault);
                self.state = NotesState::CapturePicking {
                    picker: FuzzyPicker::new(src),
                };
                EventOutcome::Consumed
            }
            "notes.create-from-template" => {
                self.state = NotesState::Creating(begin_template_picking(ctx, None));
                EventOutcome::Consumed
            }
            "notes.today" => {
                // One-shot synonym for `p` then `d` — opens today's
                // daily note directly without entering the leader.
                run_periodic_open(ctx, Period::Daily);
                EventOutcome::Consumed
            }
            "notes.periodic-leader" => {
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

    fn handle_append_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        let NotesState::Appending(as_) = &mut self.state else {
            return EventOutcome::NotHandled;
        };
        match append::handle_key(as_, k, ctx) {
            AppendStep::Stay => EventOutcome::Consumed,
            AppendStep::NotHandled => EventOutcome::NotHandled,
            AppendStep::Transition(next) => {
                self.state = NotesState::Appending(*next);
                EventOutcome::Consumed
            }
            AppendStep::Finished => {
                self.state = NotesState::Idle;
                EventOutcome::Consumed
            }
        }
    }

    fn handle_capture_picker_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        let NotesState::CapturePicking { picker } = &mut self.state else {
            return EventOutcome::NotHandled;
        };
        match picker.handle_key(k) {
            PickerOutcome::Selected(name) => {
                match capture::try_execute_preset(ctx, &name, None) {
                    Ok(capture::CaptureResult::Executed) => {
                        self.state = NotesState::Idle;
                    }
                    Ok(capture::CaptureResult::NeedsVars(vs)) => {
                        self.state = NotesState::CaptureVarPrompt(vs);
                    }
                    Err(e) => {
                        self.state = NotesState::Idle;
                        crate::tui::notes_actions::queue_toast(
                            ctx,
                            &e,
                            crate::tui::tab::ToastStyle::Error,
                        );
                    }
                }
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

    fn handle_capture_var_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        let NotesState::CaptureVarPrompt(vs) = &mut self.state else {
            return EventOutcome::NotHandled;
        };
        if capture::handle_capture_var_key(vs, k, ctx) {
            self.state = NotesState::Idle;
        }
        EventOutcome::Consumed
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

    fn commands(&self) -> &'static [CommandDef] {
        NOTES_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &self.keymap
    }

    fn dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome {
        // The keymap exposes only Idle-state commands; sub-state
        // handlers are reached via `handle_event` and not via the
        // command registry. `ft do` callers that try to invoke
        // `notes.open-picker` etc. get the `opens_modal=true` rejection
        // upstream (the picker captures the keyboard headlessly).
        match self.dispatch_idle_command(cmd, &*ctx) {
            EventOutcome::Consumed => CommandOutcome::Handled,
            _ => CommandOutcome::NotHandled,
        }
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
            NotesState::Appending(_) => self.handle_append_key(k, ctx),
            NotesState::CapturePicking { .. } => self.handle_capture_picker_key(k, ctx),
            NotesState::CaptureVarPrompt(_) => self.handle_capture_var_key(k, ctx),
            NotesState::PeriodicLeader => self.handle_periodic_leader_key(k, ctx),
        };
        Ok(outcome)
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        view::render(frame, area, ctx, &mut self.state);
    }

    fn help_sections(&self) -> Vec<HelpSection> {
        vec![
            HelpSection::new(
                "Notes",
                &[
                    ("o", "open file / heading picker"),
                    ("m", "move section(s) to another file"),
                    ("c", "create note (blank)"),
                    ("Shift+C", "create note from template"),
                    ("a", "append template to a note"),
                    ("Q", "quick capture (run a preset)"),
                ],
            ),
            HelpSection::new(
                "Periodic notes",
                &[
                    ("t", "open today's daily note"),
                    ("p", "leader → d/w/m/q/y for daily…yearly"),
                ],
            ),
            HelpSection::new(
                "In any picker / form",
                &[
                    ("↑ / ↓", "select prev / next (also Ctrl+J / Ctrl+K)"),
                    ("Enter", "select / advance"),
                    ("Esc", "back / cancel"),
                    ("Ctrl+W / Ctrl+⌫", "delete previous word"),
                    ("Ctrl+N", "create new target (target picker)"),
                ],
            ),
            HelpSection::new(
                "Move flow — compose",
                &[
                    ("↑ / ↓ · j / k", "focus row"),
                    ("Shift+↑ / Shift+↓", "reorder pending row (also K / J)"),
                    ("← / → · h / l", "decrease / increase heading level"),
                    ("r", "rename focused pending row"),
                    ("Enter", "commit move"),
                ],
            ),
        ]
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

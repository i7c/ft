//! Modal infrastructure for the TUI.
//!
//! A *modal* is any overlay that captures the keyboard ahead of the
//! active tab — pickers, multi-step flows (create / append / move),
//! confirmation dialogs, the query input bar. Before this module, every
//! tab held its own `Option<...>` slot per modal kind and a long
//! dispatch chain prioritised them by `is_some()` order. That worked at
//! small modal counts and broke down at ~15 (see `tabs/graph.rs`).
//!
//! The pattern here is:
//!
//! - [`Modal`] — a uniform interface every modal implements: dispatch a
//!   key, render an overlay, report help, identify itself by name.
//! - [`ActiveModal`] — a closed enum of every modal variant the App may
//!   hold. One `Option<ActiveModal>` slot replaces every per-tab modal
//!   field.
//! - [`ModalOutcome`] — the four-way result of dispatching a key:
//!   consumed, closed (drop the slot), open-sibling (swap the slot for
//!   a new modal), not-handled (fall through to the tab).
//!
//! ## Section-1 scope (this file)
//!
//! This file defines the trait, the outcome, and the enum. The `Modal`
//! impls for variants whose handler logic already lives as free
//! functions in `notes_actions/` are real — they wrap the existing
//! `handle_key` calls. The impls for variants whose state lives inline
//! on `GraphTab` are stubs that compile but do not yet dispatch — they
//! will be fleshed out in Section 4 when `GraphTab` surgery lifts the
//! match arms out of `tabs/graph.rs`. Nothing in the TUI yet routes
//! events through this module; Section 2 introduces the App-level slot
//! and dispatch.

#![allow(dead_code)] // wired up in Section 2; nothing calls into here yet

use crossterm::event::KeyCode;
use ft_core::periodic::Period;
use ratatui::layout::Rect;
use ratatui::Frame;

use crate::tui::command::{Command, CommandDef, CommandOutcome};
use crate::tui::event::Event;
use crate::tui::help::HelpSection;
use crate::tui::keymap::KeyMap;
use crate::tui::modal_commands as mc;
use crate::tui::notes_actions::append::{handle_key as append_handle_key, AppendState, AppendStep};
use crate::tui::notes_actions::capture::{handle_capture_var_key, CaptureVarPromptState};
use crate::tui::notes_actions::create::{handle_key as create_handle_key, CreateState, CreateStep};
use crate::tui::notes_actions::section_move::{
    handle_key as section_move_handle_key, MoveStep, SectionMoveState,
};
use crate::tui::tab::{empty_keymap, AppRequest, TabCtx};
use crate::tui::tabs::graph::{
    CapturePickerModal, GraphMoveOuter, GraphRenameState, PresetPickerModal, RelatedModal,
    SearchPickerModal,
};
use crate::tui::tabs::notes::view::{
    render_append_overlay, render_capture_var_prompt, render_create_overlay, render_move_overlay,
    render_periodic_leader,
};

// ── Trait ────────────────────────────────────────────────────────────

/// One overlay that captures the keyboard ahead of the active tab.
///
/// Implementors handle exactly one event at a time, draw themselves
/// inside an area the App chose, expose a help section for the `?`
/// overlay, and report a stable name (used by the status-bar modal
/// indicator and by tests).
pub trait Modal {
    /// Dispatch one event. Most modals only care about [`Event::Key`];
    /// other variants typically return [`ModalOutcome::NotHandled`].
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome;

    /// Render the modal's overlay over `area`. The App calls this after
    /// the active tab has drawn so the modal lands on top.
    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx);

    /// Hand-curated `?` overlay rows. **Deprecated** by §6 of
    /// commands-and-keymaps — the `?` overlay reads
    /// `Modal::keymap()` + the central `CommandRegistry` now.
    /// Existing overrides are kept until the next cleanup pass.
    #[allow(dead_code)]
    fn keymap_help(&self) -> HelpSection {
        HelpSection {
            title: self.name().to_string(),
            entries: Vec::new(),
        }
    }

    /// Stable identifier for the status-bar indicator and tests. Each
    /// implementation returns a short kebab-case string.
    fn name(&self) -> &'static str;

    /// Static slice of every command this modal owns. The default
    /// (`&[]`) lets pre-conversion modals coexist with the registry
    /// without claiming commands they can't execute.
    #[allow(dead_code)] // wired in §5 (per-modal CommandDef)
    fn commands(&self) -> &'static [CommandDef] {
        &[]
    }

    /// Modal-scoped key bindings — looked up before the tab's keymap
    /// and the App-global keymap. Default is the shared empty map.
    #[allow(dead_code)] // wired in §5 (per-modal keymap)
    fn keymap(&self) -> &KeyMap {
        empty_keymap()
    }

    /// Dispatch a resolved command on this modal. Returns
    /// [`CommandOutcome::NotHandled`] when the command isn't owned
    /// here. Default returns `NotHandled`.
    #[allow(dead_code)] // wired in §5 (per-modal dispatch)
    fn dispatch_command(&mut self, _cmd: &Command, _ctx: &TabCtx) -> CommandOutcome {
        CommandOutcome::NotHandled
    }
}

// ── Outcome ──────────────────────────────────────────────────────────

/// What a modal's `handle_event` returns to the dispatch layer.
pub enum ModalOutcome {
    /// Modal handled the key, modal remains active.
    Consumed,
    /// Modal closed itself (e.g. Esc, Enter-commit). The App should
    /// clear the active-modal slot.
    Closed,
    /// Modal closed itself and asks the App to open a sibling modal in
    /// its place (e.g. section-move advancing from source-picker to
    /// heading-multiselect). `Box` for indirection since
    /// [`ActiveModal`] is large.
    OpenSibling(Box<ActiveModal>),
    /// Modal didn't recognise the key; the dispatch layer falls through
    /// to the active tab.
    NotHandled,
}

// ── Active modal enum ────────────────────────────────────────────────

/// The set of modal variants the App may hold at a given time. Each
/// variant wraps the state type that owns the modal's data; the variant
/// itself is the discriminator for dispatch.
///
/// Some variants reference types defined in `tabs/graph.rs`. Those
/// types were made `pub` in this change but their fields remain
/// private to their defining module — the wrappers here only need to
/// name them, not introspect them.
#[allow(clippy::large_enum_variant)] // single-slot at App level; size doesn't matter
pub enum ActiveModal {
    /// Multi-step "create a new note" flow.
    Create(CreateState),
    /// Multi-step "append a template into a note" flow.
    Append(AppendState),
    /// Fuzzy picker over quick-capture presets.
    CapturePicker(CapturePickerModal),
    /// Per-variable prompt for capture-preset templates that reference
    /// `vars.KEY`.
    CaptureVar(CaptureVarPromptState),
    /// Multi-step "move section(s) from one note to another" flow.
    SectionMove(SectionMoveState),
    /// Graph-tab outer wrapper for the section-move flow (tree-driven
    /// source/target picking before/after the shared flow).
    MoveOuter(GraphMoveOuter),
    /// Inline rename-in-place modal for notes / directories selected in
    /// the graph tab tree.
    Rename(GraphRenameState),
    /// Fuzzy picker over saved graph queries (user + built-in presets).
    PresetPicker(PresetPickerModal),
    /// Modal for editing a note's `## Related` section by toggling
    /// co-occurrence-scored candidates.
    Related(RelatedModal),
    /// In-tree fuzzy search over the active graph view's reachable
    /// subgraph.
    Search(SearchPickerModal),
    /// Leader chord for periodic-note open (`p` then `d`/`w`/`m`/…).
    PeriodicLeader,
    /// The active view's query-input bar owns the keyboard. The
    /// payload identifies which view (multi-view tab strip).
    QueryBar { view_id: usize },
}

impl Modal for ActiveModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        match self {
            ActiveModal::Create(s) => s.handle_event(ev, ctx),
            ActiveModal::Append(s) => s.handle_event(ev, ctx),
            ActiveModal::CapturePicker(s) => s.handle_event(ev, ctx),
            ActiveModal::CaptureVar(s) => s.handle_event(ev, ctx),
            ActiveModal::SectionMove(s) => s.handle_event(ev, ctx),
            ActiveModal::MoveOuter(s) => s.handle_event(ev, ctx),
            ActiveModal::Rename(s) => s.handle_event(ev, ctx),
            ActiveModal::PresetPicker(s) => s.handle_event(ev, ctx),
            ActiveModal::Related(s) => s.handle_event(ev, ctx),
            ActiveModal::Search(s) => s.handle_event(ev, ctx),
            ActiveModal::PeriodicLeader => PeriodicLeader.handle_event(ev, ctx),
            ActiveModal::QueryBar { view_id } => {
                QueryBar { view_id: *view_id }.handle_event(ev, ctx)
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        match self {
            ActiveModal::Create(s) => s.render(frame, area, ctx),
            ActiveModal::Append(s) => s.render(frame, area, ctx),
            ActiveModal::CapturePicker(s) => s.render(frame, area, ctx),
            ActiveModal::CaptureVar(s) => s.render(frame, area, ctx),
            ActiveModal::SectionMove(s) => s.render(frame, area, ctx),
            ActiveModal::MoveOuter(s) => s.render(frame, area, ctx),
            ActiveModal::Rename(s) => s.render(frame, area, ctx),
            ActiveModal::PresetPicker(s) => s.render(frame, area, ctx),
            ActiveModal::Related(s) => s.render(frame, area, ctx),
            ActiveModal::Search(s) => s.render(frame, area, ctx),
            ActiveModal::PeriodicLeader => PeriodicLeader.render(frame, area, ctx),
            ActiveModal::QueryBar { view_id } => {
                QueryBar { view_id: *view_id }.render(frame, area, ctx)
            }
        }
    }

    fn keymap_help(&self) -> HelpSection {
        match self {
            ActiveModal::Create(s) => s.keymap_help(),
            ActiveModal::Append(s) => s.keymap_help(),
            ActiveModal::CapturePicker(s) => s.keymap_help(),
            ActiveModal::CaptureVar(s) => s.keymap_help(),
            ActiveModal::SectionMove(s) => s.keymap_help(),
            ActiveModal::MoveOuter(s) => s.keymap_help(),
            ActiveModal::Rename(s) => s.keymap_help(),
            ActiveModal::PresetPicker(s) => s.keymap_help(),
            ActiveModal::Related(s) => s.keymap_help(),
            ActiveModal::Search(s) => s.keymap_help(),
            ActiveModal::PeriodicLeader => PeriodicLeader.keymap_help(),
            ActiveModal::QueryBar { view_id } => QueryBar { view_id: *view_id }.keymap_help(),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            ActiveModal::Create(_) => "create",
            ActiveModal::Append(_) => "append",
            ActiveModal::CapturePicker(_) => "capture-picker",
            ActiveModal::CaptureVar(_) => "capture-var",
            ActiveModal::SectionMove(_) => "section-move",
            ActiveModal::MoveOuter(_) => "move",
            ActiveModal::Rename(_) => "rename",
            ActiveModal::PresetPicker(_) => "preset-picker",
            ActiveModal::Related(_) => "related",
            ActiveModal::Search(_) => "search",
            ActiveModal::PeriodicLeader => "periodic-leader",
            ActiveModal::QueryBar { .. } => "query-bar",
        }
    }

    fn commands(&self) -> &'static [CommandDef] {
        match self {
            ActiveModal::Create(s) => s.commands(),
            ActiveModal::Append(s) => s.commands(),
            ActiveModal::CapturePicker(s) => s.commands(),
            ActiveModal::CaptureVar(s) => s.commands(),
            ActiveModal::SectionMove(s) => s.commands(),
            ActiveModal::MoveOuter(s) => s.commands(),
            ActiveModal::Rename(s) => s.commands(),
            ActiveModal::PresetPicker(s) => s.commands(),
            ActiveModal::Related(s) => s.commands(),
            ActiveModal::Search(s) => s.commands(),
            ActiveModal::PeriodicLeader => PeriodicLeader.commands(),
            ActiveModal::QueryBar { view_id } => QueryBar { view_id: *view_id }.commands(),
        }
    }

    fn keymap(&self) -> &KeyMap {
        match self {
            ActiveModal::Create(s) => s.keymap(),
            ActiveModal::Append(s) => s.keymap(),
            ActiveModal::CapturePicker(s) => s.keymap(),
            ActiveModal::CaptureVar(s) => s.keymap(),
            ActiveModal::SectionMove(s) => s.keymap(),
            ActiveModal::MoveOuter(s) => s.keymap(),
            ActiveModal::Rename(s) => s.keymap(),
            ActiveModal::PresetPicker(s) => s.keymap(),
            ActiveModal::Related(s) => s.keymap(),
            ActiveModal::Search(s) => s.keymap(),
            ActiveModal::PeriodicLeader => empty_keymap(),
            ActiveModal::QueryBar { .. } => empty_keymap(),
        }
    }

    fn dispatch_command(&mut self, cmd: &Command, ctx: &TabCtx) -> CommandOutcome {
        match self {
            ActiveModal::Create(s) => s.dispatch_command(cmd, ctx),
            ActiveModal::Append(s) => s.dispatch_command(cmd, ctx),
            ActiveModal::CapturePicker(s) => s.dispatch_command(cmd, ctx),
            ActiveModal::CaptureVar(s) => s.dispatch_command(cmd, ctx),
            ActiveModal::SectionMove(s) => s.dispatch_command(cmd, ctx),
            ActiveModal::MoveOuter(s) => s.dispatch_command(cmd, ctx),
            ActiveModal::Rename(s) => s.dispatch_command(cmd, ctx),
            ActiveModal::PresetPicker(s) => s.dispatch_command(cmd, ctx),
            ActiveModal::Related(s) => s.dispatch_command(cmd, ctx),
            ActiveModal::Search(s) => s.dispatch_command(cmd, ctx),
            ActiveModal::PeriodicLeader => CommandOutcome::NotHandled,
            ActiveModal::QueryBar { .. } => CommandOutcome::NotHandled,
        }
    }
}

// ── Modal impls — flows with free-function handlers ──────────────────

impl Modal for CreateState {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match create_handle_key(self, k, ctx) {
            CreateStep::Stay => ModalOutcome::Consumed,
            CreateStep::Transition(next) => {
                *self = next;
                ModalOutcome::Consumed
            }
            CreateStep::Finished => ModalOutcome::Closed,
            CreateStep::NotHandled => ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        render_create_overlay(frame, area, self);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Create note",
            &[
                ("Type", "filter / edit"),
                ("↑ / ↓", "navigate"),
                ("Enter", "confirm step"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "create"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::CREATE_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::CREATE_KEYMAP
    }
}

impl Modal for AppendState {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match append_handle_key(self, k, ctx) {
            AppendStep::Stay => ModalOutcome::Consumed,
            AppendStep::Transition(next) => {
                *self = *next;
                ModalOutcome::Consumed
            }
            AppendStep::Finished => ModalOutcome::Closed,
            AppendStep::NotHandled => ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        render_append_overlay(frame, area, self);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Append template",
            &[
                ("Type", "filter / edit"),
                ("↑ / ↓", "navigate"),
                ("Enter", "confirm step"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "append"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::APPEND_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::APPEND_KEYMAP
    }
}

impl Modal for SectionMoveState {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match section_move_handle_key(self, k, ctx) {
            MoveStep::Stay => ModalOutcome::Consumed,
            MoveStep::Transition(next) => {
                *self = next;
                ModalOutcome::Consumed
            }
            MoveStep::Finished => ModalOutcome::Closed,
            MoveStep::NotHandled => ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        render_move_overlay(frame, area, self);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Move section",
            &[
                ("Space", "toggle"),
                ("↑ / ↓", "navigate"),
                ("Enter", "confirm step"),
                ("Esc", "cancel / back"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "section-move"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::SECTION_MOVE_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::SECTION_MOVE_KEYMAP
    }
}

impl Modal for CaptureVarPromptState {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        // `handle_capture_var_key` returns `true` when the flow has
        // ended (either committed via Enter on the last var, or
        // cancelled via Esc). Map to Closed / Consumed accordingly.
        if handle_capture_var_key(self, k, ctx) {
            ModalOutcome::Closed
        } else {
            ModalOutcome::Consumed
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        render_capture_var_prompt(frame, area, self);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Capture var prompt",
            &[("Enter", "next / commit"), ("Esc", "cancel")],
        )
    }

    fn name(&self) -> &'static str {
        "capture-var"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::CAPTURE_VAR_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::CAPTURE_VAR_KEYMAP
    }
}

// Picker variants don't share a blanket Modal impl. Each picker
// (SearchPickerModal, PresetPickerModal, CapturePickerModal) is a
// newtype in `tabs/graph.rs` so it can post a tab-specific
// `AppRequest` on `PickerOutcome::Selected` with the typed payload
// the host expects (e.g. `GraphJumpToNodes`, `GraphApplyPreset`).

/// Unit modal: the periodic-note leader is "awaiting the next
/// keystroke" — `d`/`w`/`m`/`q`/`y` open the matching period; any other
/// key cancels. Mirrors the pre-migration semantics in `GraphTab`
/// (any key closes the modal; period letters also fire the open).
struct PeriodicLeader;

impl Modal for PeriodicLeader {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        let period = match k.code {
            KeyCode::Char('d') => Some(Period::Daily),
            KeyCode::Char('w') => Some(Period::Weekly),
            KeyCode::Char('m') => Some(Period::Monthly),
            KeyCode::Char('q') => Some(Period::Quarterly),
            KeyCode::Char('y') => Some(Period::Yearly),
            _ => None,
        };
        if let Some(p) = period {
            *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphNavigatePeriodic(p));
        }
        // Any key (period letter, Esc, or anything else) closes the
        // leader modal — matches the pre-migration "any key clears"
        // behaviour in `GraphTab::handle_periodic_leader_key`.
        ModalOutcome::Closed
    }
    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        render_periodic_leader(frame, area);
    }
    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Periodic note",
            &[
                ("d", "daily"),
                ("w", "weekly"),
                ("m", "monthly"),
                ("q", "quarterly"),
                ("y", "yearly"),
                ("Esc", "cancel"),
            ],
        )
    }
    fn name(&self) -> &'static str {
        "periodic-leader"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::PERIODIC_LEADER_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::PERIODIC_LEADER_KEYMAP
    }
}

/// Marker modal: the active view's query bar owns the keyboard. The
/// actual buffer state stays on the view; this modal just owns the
/// keyboard and forwards each editing keystroke back to the host tab
/// via `AppRequest::GraphQueryBarKey`. Render is a no-op — the host
/// tab renders the prompt cell and cursor (the host checks
/// `ctx.active_modal_name == Some("query-bar")` to apply input-mode
/// styling).
struct QueryBar {
    view_id: usize,
}

impl Modal for QueryBar {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match (k.code, k.modifiers) {
            (KeyCode::Esc, _) => ModalOutcome::Closed,
            (KeyCode::Enter, _) => {
                *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphApplyQueryBar {
                    view_id: self.view_id,
                });
                ModalOutcome::Closed
            }
            // Forward editing keys to the host. `Char | Backspace |
            // Delete | Left | Right | Home | End` mirror the
            // pre-migration `handle_input_event` set; other keys fall
            // through.
            (
                KeyCode::Char(_)
                | KeyCode::Backspace
                | KeyCode::Delete
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End,
                _,
            ) => {
                *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphQueryBarKey {
                    view_id: self.view_id,
                    key: k,
                });
                ModalOutcome::Consumed
            }
            _ => ModalOutcome::Consumed,
        }
    }
    fn render(&mut self, _frame: &mut Frame, _area: Rect, _ctx: &TabCtx) {}
    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Query bar",
            &[
                ("Type", "edit query"),
                ("Enter", "apply"),
                ("Esc", "cancel"),
            ],
        )
    }
    fn name(&self) -> &'static str {
        "query-bar"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::QUERY_BAR_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::QUERY_BAR_KEYMAP
    }
}

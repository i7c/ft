use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Local, NaiveDate};
use ft_core::recents::RecentsLog;
use ft_core::vault::Vault;
use ratatui::{layout::Rect, Frame};

use crate::tui::command::{Command, CommandDef, CommandOutcome};
use crate::tui::event::Event;
use crate::tui::help::HelpSection;
use crate::tui::keymap::KeyMap;
use crate::tui::modal::ActiveModal;

/// Empty static keymap, used as the default return for `Tab::keymap()` and
/// `Modal::keymap()` before a tab/modal adopts the command/keymap layer.
/// A single shared instance avoids reallocating a fresh `Vec` per dispatch.
static EMPTY_KEYMAP: KeyMap = KeyMap::empty();

/// Borrow the shared empty keymap. Same address every call — safe to compare
/// or share across calls.
pub fn empty_keymap() -> &'static KeyMap {
    &EMPTY_KEYMAP
}

/// Side-effect a tab/view can request from the App. Lets the App orchestrate
/// surface-level concerns (suspending the alt-screen for `$EDITOR`, pushing
/// a status-bar toast) without each tab reaching for terminal state.
///
/// `Clone` is dropped because [`ActiveModal`] (in
/// [`AppRequest::OpenModal`]) wraps state types like [`FuzzyPicker`] that
/// aren't `Clone`. `Debug` is implemented manually so the
/// `Option<AppRequest>` assertions in TUI tests keep working — the
/// `OpenModal` variant prints its inner modal name and elides the rest.
pub enum AppRequest {
    OpenInEditor {
        path: PathBuf,
        line: usize,
    },
    /// Launch the OS handler for an `obsidian://...` URL. Unlike
    /// [`OpenInEditor`], the app does NOT suspend the alt-screen — Obsidian
    /// raises its own window, so the TUI keeps drawing underneath.
    OpenInObsidian {
        url: String,
    },
    /// Show a transient status-bar message — replaces the
    /// `refreshed HH:MM:SS` cell for ~3 seconds.
    Toast {
        text: String,
        style: ToastStyle,
    },
    /// Run `ft_core::git::sync` against the vault's enclosing repo.
    /// V1 always sends `None` from the TUI (the chord is one-shot,
    /// the message is auto-generated); the field is kept for parity
    /// with the CLI surface and future per-tab overrides.
    SyncGit {
        message: Option<String>,
    },
    /// Switch the active tab to the Journal tab and queue the given
    /// vault-relative note path on it so the journal auto-loads. Raised
    /// by the graph tab's `Shift+J` keybinding.
    JournalForNote {
        path: PathBuf,
    },
    /// Install a new active modal. The App writes the variant into its
    /// `active_modal` slot and, on the next event, dispatches keys to
    /// the modal ahead of the active tab. Tabs use this to launch
    /// flows (create, append, capture, picker, etc.) without owning
    /// per-tab modal state.
    OpenModal(Box<ActiveModal>),
    /// Like [`OpenModal`] but also pushes a status-bar toast in the
    /// same App-side step. Used by retry-after-validation-failure
    /// paths (e.g. the move-section host hooks) that need to re-open
    /// a modal AND surface "why" to the user — `pending_request` is
    /// a single slot, so combining the two avoids one overwriting
    /// the other.
    OpenModalWithToast {
        modal: Box<ActiveModal>,
        toast_text: String,
        toast_style: ToastStyle,
    },
    /// Routed back to the Graph tab: jump the cursor to a node by path
    /// (auto-expanding ancestors). Raised by the search-picker modal
    /// on Enter; the App finds the Graph tab and calls
    /// [`Tab::graph_jump_to_nodes`].
    GraphJumpToNodes(Vec<ft_core::graph::NoteId>),
    /// Routed back to the Graph tab: apply a preset DSL string to the
    /// currently-active view's query. Raised by the preset-picker
    /// modal on Enter; the App calls [`Tab::graph_apply_preset`].
    GraphApplyPreset(String),
    /// Routed back to the Graph tab: open the `QueryBar` modal on the
    /// active view. Raised by the preset-picker modal when the user
    /// cancels a "new view with presets" flow (`Ctrl+N`) so the
    /// freshly-pushed blank view drops into edit mode.
    GraphFocusQueryBar,
    /// Routed back to the Graph tab: commit a rename for the given
    /// node. Raised by the rename modal on Enter. The host runs
    /// `plan_rename` / `plan_multi_rename` against its in-memory
    /// graph, applies the plan, and refreshes the graph on success
    /// (or queues a toast on failure).
    GraphCommitRename {
        note_id: ft_core::graph::NoteId,
        is_directory: bool,
        source_rel: PathBuf,
        new_name: String,
    },
    /// Routed back to the Graph tab: apply a `## Related` section
    /// update to the target note. Raised by the Related modal on
    /// Enter; the host reads the file, runs
    /// `ft_core::related::plan_related_update` + `apply_related_update`,
    /// and toasts the outcome.
    GraphConfirmRelated {
        target_path: PathBuf,
        selected_titles: Vec<String>,
    },
    /// Routed back to the Graph tab: forward a single key event to
    /// the view's query buffer (printable chars, Backspace, Delete,
    /// arrows, Home/End). Raised by the `QueryBar` modal on each
    /// editing keystroke.
    GraphQueryBarKey {
        view_id: usize,
        key: crossterm::event::KeyEvent,
    },
    /// Routed back to the Graph tab: parse and apply the active
    /// view's query buffer (Enter from the `QueryBar` modal).
    GraphApplyQueryBar {
        view_id: usize,
    },
    /// Routed back to the Graph tab: confirm the currently-selected
    /// tree row as the move-section source. Raised by the
    /// `MoveOuter::SourceFromTree` modal on `m`. The host calls
    /// `selected_note_hit` + `advance_to_multiselect` and, on success,
    /// posts a follow-up `OpenModal(MoveOuter(Inner(...)))`. On
    /// non-Note selection it toasts and re-opens
    /// `MoveOuter(SourceFromTree)` so the user can navigate and retry.
    GraphMoveConfirmSourceFromTree,
    /// Routed back to the Graph tab: confirm the currently-selected
    /// tree row as the move-section target. Raised by the
    /// `MoveOuter::TargetFromTree` modal on `m`. Carries the carry
    /// state through the round-trip so the modal can be reopened on
    /// a recoverable error (same-file, non-Note selection).
    GraphMoveConfirmTargetFromTree {
        carry: Box<crate::tui::notes_actions::section_move::MoveCarry>,
    },
    /// Routed back to the Graph tab: confirm the currently-selected
    /// directory row as the Flow A move target. Raised by the
    /// `MoveOuter::MoveTargetFromTree` modal on `m`/Enter. The host
    /// plans + applies a multi-rename and refreshes the graph; on a
    /// validation failure it re-opens `MoveTargetFromTree` so the
    /// user can navigate to a different row and retry.
    GraphMoveConfirmMoveTarget {
        selected: HashSet<ft_core::graph::NoteId>,
    },
    /// Routed back to the Graph tab: execute a Flow A multi-move
    /// against an explicit target directory (chosen from the fuzzy
    /// picker rather than the tree). Raised by the
    /// `MoveOuter::MoveTargetPicker` modal on `PickerOutcome::Selected`.
    GraphMoveExecuteMultiMove {
        selected: HashSet<ft_core::graph::NoteId>,
        dir_path: PathBuf,
    },
    /// Routed back to the Graph tab: navigate within the active
    /// view's tree to the periodic note for the given period.
    /// Raised by the `PeriodicLeader` modal on period-letter keypress.
    /// The App calls [`Tab::graph_navigate_periodic`].
    GraphNavigatePeriodic(ft_core::periodic::Period),
}

impl std::fmt::Debug for AppRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::tui::modal::Modal;
        match self {
            AppRequest::OpenInEditor { path, line } => f
                .debug_struct("OpenInEditor")
                .field("path", path)
                .field("line", line)
                .finish(),
            AppRequest::OpenInObsidian { url } => {
                f.debug_struct("OpenInObsidian").field("url", url).finish()
            }
            AppRequest::Toast { text, style } => f
                .debug_struct("Toast")
                .field("text", text)
                .field("style", style)
                .finish(),
            AppRequest::SyncGit { message } => {
                f.debug_struct("SyncGit").field("message", message).finish()
            }
            AppRequest::JournalForNote { path } => f
                .debug_struct("JournalForNote")
                .field("path", path)
                .finish(),
            AppRequest::OpenModal(modal) => {
                f.debug_tuple("OpenModal").field(&modal.name()).finish()
            }
            AppRequest::OpenModalWithToast {
                modal,
                toast_text,
                toast_style,
            } => f
                .debug_struct("OpenModalWithToast")
                .field("modal", &modal.name())
                .field("toast_text", toast_text)
                .field("toast_style", toast_style)
                .finish(),
            AppRequest::GraphJumpToNodes(path) => f
                .debug_tuple("GraphJumpToNodes")
                .field(&path.len())
                .finish(),
            AppRequest::GraphApplyPreset(dsl) => {
                f.debug_tuple("GraphApplyPreset").field(dsl).finish()
            }
            AppRequest::GraphFocusQueryBar => f.write_str("GraphFocusQueryBar"),
            AppRequest::GraphCommitRename {
                note_id,
                is_directory,
                source_rel,
                new_name,
            } => f
                .debug_struct("GraphCommitRename")
                .field("note_id", note_id)
                .field("is_directory", is_directory)
                .field("source_rel", source_rel)
                .field("new_name", new_name)
                .finish(),
            AppRequest::GraphConfirmRelated {
                target_path,
                selected_titles,
            } => f
                .debug_struct("GraphConfirmRelated")
                .field("target_path", target_path)
                .field("selected_titles", selected_titles)
                .finish(),
            AppRequest::GraphQueryBarKey { view_id, key } => f
                .debug_struct("GraphQueryBarKey")
                .field("view_id", view_id)
                .field("key", key)
                .finish(),
            AppRequest::GraphApplyQueryBar { view_id } => f
                .debug_struct("GraphApplyQueryBar")
                .field("view_id", view_id)
                .finish(),
            AppRequest::GraphMoveConfirmSourceFromTree => {
                f.write_str("GraphMoveConfirmSourceFromTree")
            }
            AppRequest::GraphMoveConfirmTargetFromTree { carry } => f
                .debug_struct("GraphMoveConfirmTargetFromTree")
                .field("source_rel", &carry.source_rel)
                .finish(),
            AppRequest::GraphMoveConfirmMoveTarget { selected } => f
                .debug_struct("GraphMoveConfirmMoveTarget")
                .field("selected_count", &selected.len())
                .finish(),
            AppRequest::GraphMoveExecuteMultiMove { selected, dir_path } => f
                .debug_struct("GraphMoveExecuteMultiMove")
                .field("selected_count", &selected.len())
                .field("dir_path", dir_path)
                .finish(),
            AppRequest::GraphNavigatePeriodic(period) => f
                .debug_tuple("GraphNavigatePeriodic")
                .field(period)
                .finish(),
        }
    }
}

/// Visual styling for a [`Toast`]. Green for success (create, save),
/// red for errors (IO failures, validation fallout), cyan for
/// informational notices (background-job heads-ups). The middle of
/// the status bar runs all toasts through one renderer, so adding a
/// new shade later is a single match arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastStyle {
    Success,
    Error,
    Info,
}

/// What the App should do after a tab handles an event. `Consumed` and `Quit`
/// are part of the contract but unused in session 1; sessions 2+ surface them
/// (e.g. a tab swallowing `q` while editing a query, or a tab signalling exit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventOutcome {
    Consumed,
    NotHandled,
    /// Tab signals it wants to switch the active tab. Constructed by no
    /// current tab — kept so a future tab (e.g. a launcher screen) can
    /// request a switch without reaching for App state directly.
    #[allow(dead_code)]
    SwitchTab(usize),
    /// Tab signals the app should exit. Currently unused — `q`/`Ctrl+C` are
    /// handled by the global keymap — but kept so a future tab (e.g. a modal
    /// confirm-quit dialog) can request exit without reaching for app state.
    #[allow(dead_code)]
    Quit,
}

/// Shared context passed to tabs on every event/render.
///
/// `today` is the date used to resolve DSL keywords (`today` / `tomorrow`)
/// and to bucket overdue vs upcoming tasks; it is fixed for the lifetime of
/// the App so a long-running session has stable bucketing. The clock for
/// the live sidebar display is separate (see `tabs::tasks::ClockFn`).
///
/// `last_refresh` is wrapped in a `Cell` so views can update it through
/// the shared `&TabCtx` they receive in `render` and `handle_event` —
/// the App reads it back when drawing the status bar.
pub struct TabCtx<'a> {
    /// `&Arc<Vault>` (rather than `&Vault`) so a tab can `Arc::clone(ctx.vault)`
    /// to hand a vault handle to a widget whose lifetime outlives the
    /// borrow of `App` — e.g. the fuzzy picker tucked inside a popup.
    /// Existing `ctx.vault.scan()` / `ctx.vault.path` callers keep working
    /// through `Arc`'s auto-deref to `&Vault`.
    pub vault: &'a Arc<Vault>,
    /// Per-vault "recently opened notes" log (plan 008). Shared across
    /// the four picker sites so an open recorded by one shows up in the
    /// others, and shared with the open-chokepoint sites so opens get
    /// recorded as the user navigates.
    pub recents: &'a Arc<RecentsLog>,
    pub today: NaiveDate,
    pub last_refresh: &'a Cell<Option<DateTime<Local>>>,
    /// Pending side-effect for the App to handle after `handle_event` returns.
    /// `RefCell` rather than `Cell` because [`AppRequest`] isn't `Copy`.
    pub pending_request: &'a RefCell<Option<AppRequest>>,
    /// Name of the App's currently-active modal, if any (e.g.
    /// `Some("query-bar")` when `ActiveModal::QueryBar` is up). Used
    /// by tabs to render input cues without owning a parallel state
    /// flag (extract-modal-driver §5). Populated by the App when
    /// building the context for `handle_event` and `render`.
    pub active_modal_name: Option<&'static str>,
}

/// A top-level tab in the TUI. New tabs slot in by adding a `Box<dyn Tab>` to
/// the App's tab list — no surgery on the core loop.
pub trait Tab {
    fn title(&self) -> &str;

    fn on_focus(&mut self, _ctx: &mut TabCtx) -> Result<()> {
        Ok(())
    }

    fn on_blur(&mut self, _ctx: &mut TabCtx) -> Result<()> {
        Ok(())
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome>;

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx);

    fn refresh(&mut self, _ctx: &mut TabCtx) -> Result<()> {
        Ok(())
    }

    /// Hand-curated `?` overlay sections. **Deprecated** by §6 of
    /// commands-and-keymaps — the `?` overlay is generated from
    /// `Tab::keymap()` + the central `CommandRegistry` now. Each
    /// tab's existing override is no-op-from-the-renderer's-POV and
    /// can be deleted at the next cleanup pass.
    #[allow(dead_code)]
    fn help_sections(&self) -> Vec<HelpSection> {
        Vec::new()
    }

    /// Hook for the App's startup-action mechanism (see
    /// [`crate::tui::InitialAction`]). The graph tab uses this to
    /// queue opening the Related updater modal for a specific note
    /// when launched via `ft notes update-related`. Default is a
    /// no-op: tabs that don't host the modal ignore the request.
    fn queue_related_modal(&mut self, _note_path: &std::path::Path) {}

    /// Hook for the cross-tab Journal jump (see
    /// [`AppRequest::JournalForNote`]). The Journal tab overrides
    /// this to store a vault-relative path; the path is consumed and
    /// turned into a load on the tab's next `on_focus`. Default is a
    /// no-op: other tabs ignore the request.
    fn queue_journal_for(&mut self, _note_path: &std::path::Path) {}

    /// Hook for the in-tree search picker (see
    /// [`AppRequest::GraphJumpToNodes`]). The Graph tab overrides
    /// this to call its own `jump_to_path` helper, materialising the
    /// ancestor chain and landing the cursor on the leaf. Other tabs
    /// ignore the request.
    fn graph_jump_to_nodes(&mut self, _path: Vec<ft_core::graph::NoteId>) {}

    /// Hook for the preset picker (see [`AppRequest::GraphApplyPreset`]).
    /// The Graph tab overrides this to apply the preset DSL string to
    /// the active view. Other tabs ignore.
    fn graph_apply_preset(&mut self, _dsl: String) {}

    /// Hook for the preset picker's cancel-from-new-view path (see
    /// [`AppRequest::GraphFocusQueryBar`]). The Graph tab overrides
    /// this to open the `QueryBar` modal on the freshly-pushed view
    /// (extract-modal-driver §5).
    fn graph_focus_query_bar(&mut self, _ctx: &TabCtx) {}

    /// Hook for the rename modal commit (see
    /// [`AppRequest::GraphCommitRename`]). The Graph tab overrides
    /// this to plan + apply a rename via its in-memory graph and
    /// refresh on success. Other tabs ignore.
    fn graph_commit_rename(
        &mut self,
        _ctx: &TabCtx,
        _note_id: ft_core::graph::NoteId,
        _is_directory: bool,
        _source_rel: PathBuf,
        _new_name: String,
    ) {
    }

    /// Hook for the Related modal commit (see
    /// [`AppRequest::GraphConfirmRelated`]). The Graph tab overrides
    /// this to apply the related-section update on disk and toast.
    fn graph_confirm_related(
        &mut self,
        _ctx: &TabCtx,
        _target_path: PathBuf,
        _selected_titles: Vec<String>,
    ) {
    }

    /// Hook for the QueryBar modal's per-key forwarding (see
    /// [`AppRequest::GraphQueryBarKey`]). The Graph tab overrides
    /// this to mutate the view's query buffer (insert / backspace /
    /// arrows / etc.).
    fn graph_query_bar_key(&mut self, _view_id: usize, _key: crossterm::event::KeyEvent) {}

    /// Hook for the QueryBar modal's commit (see
    /// [`AppRequest::GraphApplyQueryBar`]). The Graph tab overrides
    /// this to parse and apply the view's current query buffer.
    fn graph_apply_query_bar(&mut self, _view_id: usize) {}

    /// Hook for the move-section `SourceFromTree` confirm (see
    /// [`AppRequest::GraphMoveConfirmSourceFromTree`]). The Graph tab
    /// overrides this to advance the flow to the shared
    /// heading-multi-select step (or toast + re-open the source modal
    /// if the selection isn't a valid Note row).
    fn graph_move_confirm_source_from_tree(&mut self, _ctx: &TabCtx) {}

    /// Hook for the move-section `TargetFromTree` confirm (see
    /// [`AppRequest::GraphMoveConfirmTargetFromTree`]). The Graph tab
    /// overrides this to validate the selection against the carry's
    /// source and either compose the existing-target flow or re-open
    /// `TargetFromTree` with the carry intact for retry.
    fn graph_move_confirm_target_from_tree(
        &mut self,
        _ctx: &TabCtx,
        _carry: crate::tui::notes_actions::section_move::MoveCarry,
    ) {
    }

    /// Hook for the move-section Flow A target-dir confirm (see
    /// [`AppRequest::GraphMoveConfirmMoveTarget`]). The Graph tab
    /// overrides this to plan + apply the multi-rename and refresh,
    /// or re-open `MoveTargetFromTree` with `selected` preserved.
    fn graph_move_confirm_move_target(
        &mut self,
        _ctx: &TabCtx,
        _selected: HashSet<ft_core::graph::NoteId>,
    ) {
    }

    /// Hook for the Flow A fuzzy-picker variant (see
    /// [`AppRequest::GraphMoveExecuteMultiMove`]). The Graph tab
    /// overrides this to execute the multi-rename to an explicit
    /// `dir_path` chosen via the picker (no tree-row lookup).
    fn graph_move_execute_multi_move(
        &mut self,
        _ctx: &TabCtx,
        _selected: HashSet<ft_core::graph::NoteId>,
        _dir_path: PathBuf,
    ) {
    }

    /// Hook for the periodic-note navigation flow (see
    /// [`AppRequest::GraphNavigatePeriodic`]). The Graph tab
    /// overrides this to resolve the periodic note path, find the
    /// shortest BFS path from the active query's roots, and jump
    /// the tree cursor to it. Other tabs ignore.
    fn graph_navigate_periodic(&mut self, _ctx: &TabCtx, _period: ft_core::periodic::Period) {}

    /// Test-only probe: does the currently-selected row represent a
    /// real Note? Default is `false`; the graph tab overrides this
    /// so cross-tab tests can assert their precondition without
    /// reaching into private state.
    #[cfg(test)]
    fn selected_is_note_for_test(&self) -> bool {
        false
    }

    /// Static slice of every command this tab owns, used by the
    /// `CommandRegistry` to compose the build-time union and by
    /// `?` / `ft commands list` / `ft do` to look up metadata.
    /// Default is empty so tabs can adopt the pattern incrementally.
    #[allow(dead_code)] // wired in §§4–8 (per-tab CommandDef + registry build)
    fn commands(&self) -> &'static [CommandDef] {
        &[]
    }

    /// Tab-scoped key bindings — looked up before the App's global
    /// keymap. Default is the shared empty map so tabs adopt the
    /// pattern only when they have bindings to declare.
    #[allow(dead_code)] // wired in §§4–6 (per-tab keymaps + ? overlay)
    fn keymap(&self) -> &KeyMap {
        empty_keymap()
    }

    /// Dispatch a resolved command on this tab. Returns
    /// [`CommandOutcome::NotHandled`] if the command isn't owned by
    /// this scope (the caller falls through to the next scope). The
    /// default returns `NotHandled` so unimplemented tabs don't claim
    /// commands they can't execute.
    #[allow(dead_code)] // wired in §4 (per-tab dispatch)
    fn dispatch_command(&mut self, _cmd: &Command, _ctx: &mut TabCtx) -> CommandOutcome {
        CommandOutcome::NotHandled
    }
}

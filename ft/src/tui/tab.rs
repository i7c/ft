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

/// What the Journal tab should build a feed for. A `Note` carries a
/// vault-relative path to a real backing file; a `Ghost` carries the
/// raw unresolved-link target string (e.g. `"Phantom"`), since a ghost
/// has no path and is keyed only by that string within a `Graph`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalTarget {
    Note(PathBuf),
    Ghost(String),
}

/// Cross-tab request for the Journal tab to enter multi-target mode.
/// Built by the Review tab when the user presses Enter on a selection
/// of links; consumed by the Journal tab's `queue_journal_for_multi`
/// hook and turned into a multi-source journal load on the next
/// `on_focus`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiTargetRequest {
    pub targets: Vec<JournalTarget>,
    /// Window range that produced these targets, if any. Enables the
    /// Journal tab's `--in-window` toggle when present.
    pub window: Option<JournalWindow>,
}

/// Serializable mirror of `ft_core::link_review::WindowRange` for the
/// cross-tab handoff. Kept here (rather than importing the core enum
/// into `tab.rs` directly) to avoid pulling link-review types into the
/// Tab trait surface; the Journal tab converts back to the core type
/// when running the in-window filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalWindow {
    Since(chrono::Duration),
    Range { from: String, to: String },
}

impl JournalWindow {
    pub fn to_core(&self) -> ft_core::link_review::WindowRange {
        match self {
            JournalWindow::Since(d) => ft_core::link_review::WindowRange::Since(*d),
            JournalWindow::Range { from, to } => ft_core::link_review::WindowRange::Range {
                from: from.clone(),
                to: to.clone(),
            },
        }
    }
}

impl JournalTarget {
    /// Single-line user-facing label: the vault-relative path for a
    /// note, the raw target string (suffixed with `(ghost)`) for a
    /// ghost. Used in the Journal tab's header and error messages.
    pub fn label(&self) -> String {
        match self {
            JournalTarget::Note(p) => p.display().to_string(),
            JournalTarget::Ghost(raw) => format!("{raw} (ghost)"),
        }
    }
}

/// Whether an external "add these targets" hand-off should append to
/// the Journal tab's current source set or replace it. Carried on
/// [`AppRequest::JournalAddSources`] as the initial focus of the
/// Append-or-Replace prompt; the user can still flip the choice in the
/// prompt before committing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOrReplaceMode {
    Append,
    Replace,
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
    /// Run `ft_core::git::commit` (lightweight sync: stage + commit
    /// only, no pull/push) against the vault's enclosing repo. Raised
    /// by the `g c` leader chord. Same `message` parity as `SyncGit`.
    CommitGit {
        message: Option<String>,
    },
    /// Switch the active tab to the Journal tab and queue the given
    /// target on it so the journal auto-loads. Raised by the graph
    /// tab's `Shift+J` keybinding; accepts both real notes and ghost
    /// (unresolved-link) targets, since the journal feed is defined
    /// for either.
    JournalFor {
        target: JournalTarget,
    },
    /// Switch to the Journal tab and queue a multi-target request.
    /// Raised by the Review tab on Enter; the Journal tab consumes
    /// the request on its next `on_focus` and builds the multi-source
    /// journal across all `targets`.
    JournalForMulti {
        request: MultiTargetRequest,
    },
    /// Switch to the Journal tab and queue an "add these targets"
    /// request. Raised by the graph tab's `Shift+A` keybinding (and
    /// any future cross-tab adders). The Journal tab consumes the
    /// request on its next `on_focus` and raises the Append-or-Replace
    /// prompt — the user decides whether to union with the current
    /// source set or replace it.
    JournalAddSources {
        targets: Vec<JournalTarget>,
        default_mode: AppendOrReplaceMode,
    },
    /// Commit a new source set to the Journal tab. Raised by the
    /// Sources Manager modal on Enter and by the Append-or-Replace
    /// prompt on commit; the App routes it back to the Journal tab
    /// which replaces `sources` (and the optional `window`) with the
    /// provided values and rebuilds the journal feed.
    JournalCommitSources {
        sources: Vec<JournalTarget>,
        window: Option<JournalWindow>,
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
    /// Routed back to the Graph tab: execute a confirmed delete of
    /// the given target path. Raised by the `ConfirmDelete` modal
    /// on Yes. The host calls `plan_delete` + `apply_delete`,
    /// refreshes the graph, and toasts the outcome.
    GraphConfirmDelete {
        target: PathBuf,
        is_directory: bool,
    },
    /// Routed back to the Graph tab: create a subdirectory under
    /// the given parent path with the given name. Raised by the
    /// `CreateSubdir` modal on Enter. The host creates the
    /// directory, refreshes the graph, and toasts.
    GraphCreateSubdir {
        parent: PathBuf,
        name: String,
    },
    /// Apply validated popup fields to a task at `(path, line)` via
    /// `ops::update_task_line` (graph-task-edit-modal §3).
    GraphTaskEdit {
        path: PathBuf,
        line: usize,
        fields: crate::tui::tabs::tasks::edit_popup::PopupFields,
    },
    /// Create a new task from the Graph-tab `TaskCreate` popup via
    /// `ops::create_task` (graph-task-edit-modal §4). `target` is the raw
    /// target-field text (`Path` or `Path#heading`); `subtask_parent`, when
    /// set, nests the new task under that `(file, line)`.
    GraphTaskCommitCreate {
        fields: crate::tui::tabs::tasks::edit_popup::PopupFields,
        target: String,
        subtask_parent: Option<(PathBuf, usize)>,
    },
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
            AppRequest::CommitGit { message } => f
                .debug_struct("CommitGit")
                .field("message", message)
                .finish(),
            AppRequest::JournalFor { target } => f
                .debug_struct("JournalFor")
                .field("target", target)
                .finish(),
            AppRequest::JournalForMulti { request } => f
                .debug_struct("JournalForMulti")
                .field("targets_count", &request.targets.len())
                .field("has_window", &request.window.is_some())
                .finish(),
            AppRequest::JournalAddSources {
                targets,
                default_mode,
            } => f
                .debug_struct("JournalAddSources")
                .field("targets_count", &targets.len())
                .field("default_mode", default_mode)
                .finish(),
            AppRequest::JournalCommitSources { sources, window } => f
                .debug_struct("JournalCommitSources")
                .field("sources_count", &sources.len())
                .field("has_window", &window.is_some())
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
            AppRequest::GraphConfirmDelete {
                target,
                is_directory,
            } => f
                .debug_struct("GraphConfirmDelete")
                .field("target", target)
                .field("is_directory", is_directory)
                .finish(),
            AppRequest::GraphCreateSubdir { parent, name } => f
                .debug_struct("GraphCreateSubdir")
                .field("parent", parent)
                .field("name", name)
                .finish(),
            AppRequest::GraphTaskEdit { path, line, .. } => f
                .debug_struct("GraphTaskEdit")
                .field("path", path)
                .field("line", line)
                .finish_non_exhaustive(),
            AppRequest::GraphTaskCommitCreate {
                target,
                subtask_parent,
                ..
            } => f
                .debug_struct("GraphTaskCommitCreate")
                .field("target", target)
                .field("subtask_parent", subtask_parent)
                .finish_non_exhaustive(),
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
/// the App so a long-running session has stable bucketing.
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
    /// Whether the active tab's currently-focused `EditBuffer` has an
    /// open completion popup. Filled by the App via
    /// [`Tab::host_popup_open`]; consulted by the modal that owns the
    /// buffer (currently only `QueryBar`) to decide whether `Esc`
    /// should dismiss the popup (forward to the buffer) or close the
    /// modal (handle directly). `false` everywhere else.
    pub host_popup_open: bool,
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

    /// Whether the tab's currently-focused [`EditBuffer`] has an open
    /// completion popup. The default returns `false`; tabs that mount
    /// an `EditBuffer` inside a modal forwarder (currently only
    /// `GraphTab` with its query bar) override this so the App can
    /// pre-fill [`TabCtx::host_popup_open`] before dispatching a key
    /// event to the active modal.
    ///
    /// [`EditBuffer`]: crate::tui::widgets::EditBuffer
    fn host_popup_open(&self) -> bool {
        false
    }

    /// Test-only: attach a completion provider to the tab's
    /// currently-focused [`EditBuffer`] (the one a modal forwarder
    /// routes keys into). Default is a no-op; only [`GraphTab`]
    /// overrides this for the query bar buffer.
    ///
    /// [`EditBuffer`]: crate::tui::widgets::EditBuffer
    /// [`GraphTab`]: crate::tui::tabs::graph::GraphTab
    #[cfg(test)]
    fn set_focused_buffer_completion_for_test(
        &mut self,
        _provider: Box<dyn crate::tui::widgets::CompletionProvider>,
    ) {
    }

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
    /// queue opening the Related panel modal for a specific note
    /// when launched via `ft notes update-related`. Default is a
    /// no-op: tabs that don't host the modal ignore the request.
    fn queue_related_modal(&mut self, _note_path: &std::path::Path) {}

    /// Hook for the cross-tab Journal jump (see
    /// [`AppRequest::JournalFor`]). The Journal tab overrides this to
    /// store the target; it's consumed and turned into a load on the
    /// tab's next `on_focus`. Default is a no-op: other tabs ignore
    /// the request.
    fn queue_journal_for(&mut self, _target: &JournalTarget) {}

    /// Hook for the cross-tab multi-target Journal handoff (see
    /// [`AppRequest::JournalForMulti`]). The Journal tab overrides this
    /// to store the request; it's consumed on next `on_focus` and turned
    /// into a multi-source journal load. Default is a no-op.
    fn queue_journal_for_multi(&mut self, _request: &MultiTargetRequest) {}

    /// Hook for the cross-tab "add sources" handoff (see
    /// [`AppRequest::JournalAddSources`]). The Journal tab overrides
    /// this to store the request; it's consumed on next `on_focus` and
    /// turned into an Append-or-Replace prompt. Default is a no-op.
    fn queue_journal_add_sources(
        &mut self,
        _targets: Vec<JournalTarget>,
        _default_mode: AppendOrReplaceMode,
    ) {
    }

    /// Hook for the Sources Manager / Append-or-Replace commit (see
    /// [`AppRequest::JournalCommitSources`]). The Journal tab overrides
    /// this to replace its `sources` slot and rebuild the feed
    /// synchronously. Takes a `&mut TabCtx` so the override can run
    /// `rebuild_journal` without bouncing through `on_focus`. Default
    /// is a no-op.
    fn queue_journal_commit_sources(
        &mut self,
        _ctx: &mut TabCtx,
        _sources: Vec<JournalTarget>,
        _window: Option<JournalWindow>,
    ) {
    }

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

    /// Hook for the confirmation-delete flow (see
    /// [`AppRequest::GraphConfirmDelete`]). The Graph tab overrides
    /// this to plan + apply deletion, refresh the graph, and toast
    /// the outcome.
    fn graph_confirm_delete(&mut self, _ctx: &TabCtx, _target: PathBuf, _is_directory: bool) {}

    /// Hook for the create-subdirectory flow (see
    /// [`AppRequest::GraphCreateSubdir`]). The Graph tab overrides
    /// this to create the directory, refresh the graph, and toast.
    fn graph_create_subdir(&mut self, _ctx: &TabCtx, _parent: PathBuf, _name: String) {}

    /// Hook for the task-edit popup commit (see [`AppRequest::GraphTaskEdit`]).
    /// The Graph tab overrides this to apply the fields via
    /// `ops::update_task_line`, refresh the graph, and restore the cursor.
    fn graph_task_edit(
        &mut self,
        _ctx: &TabCtx,
        _path: PathBuf,
        _line: usize,
        _fields: crate::tui::tabs::tasks::edit_popup::PopupFields,
    ) {
    }

    /// Hook for the task-create popup commit (see
    /// [`AppRequest::GraphTaskCommitCreate`]). The Graph tab overrides this
    /// to resolve the target file/position, write via `ops::create_task`,
    /// refresh the graph, and land the cursor on the new task.
    fn graph_task_commit_create(
        &mut self,
        _ctx: &TabCtx,
        _fields: crate::tui::tabs::tasks::edit_popup::PopupFields,
        _target: String,
        _subtask_parent: Option<(PathBuf, usize)>,
    ) {
    }

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

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
/// Every cross-tab / modal-raised request that the Graph tab (and only the
/// Graph tab) services. Carried as the single payload of
/// [`AppRequest::Graph`] and dispatched through the single
/// [`Tab::handle_graph_request`] hook — replaces what used to be one
/// dedicated `AppRequest::Graph*` variant + one dedicated `Tab::graph_*`
/// method per action (tui-tab-request-routing).
#[derive(Debug)]
pub enum GraphRequest {
    /// Jump the cursor to a node by path (auto-expanding ancestors).
    /// Raised by the search-picker modal on Enter.
    JumpToNodes(Vec<ft_core::graph::NoteId>),
    /// Apply a preset DSL string to the currently-active view's query.
    /// Raised by the preset-picker modal on Enter.
    ApplyPreset(String),
    /// Open the `QueryBar` modal on the active view. Raised by the
    /// preset-picker modal when the user cancels a "new view with
    /// presets" flow (`Ctrl+N`) so the freshly-pushed blank view drops
    /// into edit mode.
    FocusQueryBar,
    /// Commit a rename for the given node. Raised by the rename modal
    /// on Enter.
    CommitRename {
        note_id: ft_core::graph::NoteId,
        is_directory: bool,
        source_rel: PathBuf,
        new_name: String,
    },
    /// Apply a `## Related` section update to the target note. Raised
    /// by the Related modal on Enter.
    ConfirmRelated {
        target_path: PathBuf,
        selected_titles: Vec<String>,
    },
    /// Forward a single key event to the view's query buffer (printable
    /// chars, Backspace, Delete, arrows, Home/End). Raised by the
    /// `QueryBar` modal on each editing keystroke.
    QueryBarKey {
        view_id: usize,
        key: crossterm::event::KeyEvent,
    },
    /// Parse and apply the active view's query buffer (Enter from the
    /// `QueryBar` modal).
    ApplyQueryBar { view_id: usize },
    /// Confirm the currently-selected tree row as the move-section
    /// source. Raised by the `MoveOuter::SourceFromTree` modal on `m`.
    MoveConfirmSourceFromTree,
    /// Confirm the currently-selected tree row as the move-section
    /// target. Raised by the `MoveOuter::TargetFromTree` modal on `m`.
    MoveConfirmTargetFromTree {
        carry: Box<crate::tui::notes_actions::section_move::MoveCarry>,
    },
    /// Confirm the currently-selected directory row as the Flow A move
    /// target. Raised by the `MoveOuter::MoveTargetFromTree` modal on
    /// `m`/Enter.
    MoveConfirmMoveTarget {
        selected: HashSet<ft_core::graph::NoteId>,
    },
    /// Execute a Flow A multi-move against an explicit target directory
    /// chosen via the fuzzy picker. Raised by the
    /// `MoveOuter::MoveTargetPicker` modal on `PickerOutcome::Selected`.
    MoveExecuteMultiMove {
        selected: HashSet<ft_core::graph::NoteId>,
        dir_path: PathBuf,
    },
    /// Navigate within the active view's tree to the periodic note for
    /// the given period. Raised by the `PeriodicLeader` modal on
    /// period-letter keypress.
    NavigatePeriodic(ft_core::periodic::Period),
    /// Execute a confirmed delete of the given target path. Raised by
    /// the `ConfirmDelete` modal on Yes.
    ConfirmDelete { target: PathBuf, is_directory: bool },
    /// Create a subdirectory under the given parent path with the given
    /// name. Raised by the `CreateSubdir` modal on Enter.
    CreateSubdir { parent: PathBuf, name: String },
    /// Apply validated popup fields to a task at `(path, line)` via
    /// `ops::update_task_line` (graph-task-edit-modal §3).
    TaskEdit {
        path: PathBuf,
        line: usize,
        fields: crate::tui::tabs::tasks::edit_popup::PopupFields,
    },
    /// Create a new task from the Graph-tab `TaskCreate` popup via
    /// `ops::create_task` (graph-task-edit-modal §4). `target` is the raw
    /// target-field text (`Path` or `Path#heading`); `subtask_parent`,
    /// when set, nests the new task under that `(file, line)`.
    TaskCommitCreate {
        fields: crate::tui::tabs::tasks::edit_popup::PopupFields,
        target: String,
        subtask_parent: Option<(PathBuf, usize)>,
    },
}

/// Every cross-tab / modal-raised request that the Tasks tab (and only the
/// Tasks tab) services. A parallel of [`GraphRequest`]: carried as the
/// single payload of [`AppRequest::Tasks`] and dispatched through the
/// single [`Tab::handle_tasks_request`] hook. Today only the preset-picker
/// modal raises a request (on Enter), but the channel is generic so future
/// Tasks-targeted modal commits reuse it.
#[derive(Debug)]
pub enum TasksRequest {
    /// Replace the active SearchView's query text with a preset DSL string
    /// and recompute matches. Raised by the task-preset-picker modal on
    /// Enter. Parsed under `Profile::Tasks` (the same profile the inline
    /// query bar uses).
    ApplyPreset(String),
    /// Retag the Tasks SearchView's selected task. Raised by the
    /// task-retag-picker modal on Enter. `tag` is the bare name (no
    /// leading `#`) picked from `config.tasks.retag_tags`; the view's
    /// `apply_retag` writes it via `ops::retag_task`, swapping out any
    /// prior tag from the same configured list.
    RetagSelected(String),
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
    /// Switch to the Search tab and set its query (in the given
    /// any-mode), re-running the search. Raised by the Pulse tab's
    /// Enter handoff and the graph tab's `J` / `Ctrl+J` handoffs: the
    /// selected links/notes become `[[…]]` clauses.
    SearchWithQuery {
        query: String,
        any: bool,
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
    /// Routed to the Graph tab: the App looks the tab up by
    /// `TabKind::Graph` and calls `Tab::handle_graph_request` with the
    /// carried [`GraphRequest`] (tui-tab-request-routing). Replaces what
    /// used to be sixteen dedicated `AppRequest::Graph*` variants, one
    /// per action.
    Graph(GraphRequest),
    /// Routed to the Tasks tab: the App looks the tab up by
    /// `TabKind::Tasks` and calls `Tab::handle_tasks_request` with the
    /// carried [`TasksRequest`]. A parallel of [`AppRequest::Graph`] for
    /// Tasks-targeted, modal-raised requests (today: the preset picker).
    Tasks(TasksRequest),
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
            AppRequest::SearchWithQuery { query, any } => f
                .debug_struct("SearchWithQuery")
                .field("query", query)
                .field("any", any)
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
            AppRequest::Graph(req) => f.debug_tuple("Graph").field(req).finish(),
            AppRequest::Tasks(req) => f.debug_tuple("Tasks").field(req).finish(),
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
    /// The App-owned graph snapshot, if one has been installed yet (see
    /// `crate::tui::snapshot`). `None` until the first background build
    /// completes after startup — tabs render a loading state. Cheap
    /// `Arc` clone per ctx; tabs may stash it across events.
    pub snapshot: Option<std::sync::Arc<crate::tui::snapshot::GraphSnapshot>>,
    /// Set via [`TabCtx::request_graph_refresh`] after a vault mutation.
    /// The App consumes it at its drain points and kicks one background
    /// rebuild. A `Cell` flag rather than an [`AppRequest`] because
    /// `pending_request` is a single slot and mutation handlers usually
    /// occupy it with a toast in the same event.
    pub graph_refresh: &'a Cell<bool>,
}

impl TabCtx<'_> {
    /// Ask the App to rebuild the shared graph snapshot after this
    /// event completes. Coalesces freely — any number of calls in one
    /// event cost one rebuild request.
    pub fn request_graph_refresh(&self) {
        self.graph_refresh.set(true);
    }
}

/// Typed identity of a tab, used by the App to route cross-tab
/// [`AppRequest`]s to their target (e.g. `Graph*` actions to the Graph
/// tab, `Journal*` queues to the Journal tab) without matching on the
/// display string from [`Tab::title`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKind {
    Graph,
    Tasks,
    Notes,
    Timeblocks,
    Recent,
    Pulse,
    Search,
}

/// A top-level tab in the TUI. New tabs slot in by adding a `Box<dyn Tab>` to
/// the App's tab list — no surgery on the core loop.
pub trait Tab {
    fn title(&self) -> &str;

    /// Typed identity for request routing. Unlike [`Self::title`] (display
    /// text, free to change), this is the stable routing key.
    fn kind(&self) -> TabKind;

    fn on_focus(&mut self, _ctx: &mut TabCtx) -> Result<()> {
        Ok(())
    }

    fn on_blur(&mut self, _ctx: &mut TabCtx) -> Result<()> {
        Ok(())
    }

    /// Called on the **active** tab when a new graph snapshot installs
    /// (`BgEvent::GraphReady`). Re-derive view state from
    /// `ctx.snapshot` and resolve any pending cursor anchor here.
    /// Background tabs are not called — they catch up by comparing
    /// `ctx.snapshot` generation in `on_focus`. Default: no-op.
    fn on_graph_ready(&mut self, _ctx: &mut TabCtx) {}

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

    /// Hook for the cross-tab Search handoff (see
    /// [`AppRequest::SearchWithQuery`]). The Search tab overrides this
    /// to install the query + any-mode and re-run; other tabs ignore it.
    fn queue_search_query(&mut self, _query: String, _any: bool) {}

    /// Single hook for every cross-tab / modal-raised request targeting
    /// the Graph tab (see [`GraphRequest`] and [`AppRequest::Graph`]).
    /// The App looks the tab up by `TabKind::Graph` and calls this once
    /// per request — replaces what used to be sixteen dedicated
    /// `graph_*` methods, one per action. Default is a no-op; only
    /// `GraphTab` overrides it.
    fn handle_graph_request(&mut self, _req: GraphRequest, _ctx: &mut TabCtx) {}

    /// Single hook for every cross-tab / modal-raised request targeting
    /// the Tasks tab (see [`TasksRequest`] and [`AppRequest::Tasks`]).
    /// A parallel of [`handle_graph_request`](Self::handle_graph_request):
    /// the App looks the tab up by `TabKind::Tasks` and calls this once
    /// per request. Default is a no-op; only `TasksTab` overrides it.
    fn handle_tasks_request(&mut self, _req: TasksRequest, _ctx: &mut TabCtx) {}

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

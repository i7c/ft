//! Graph tab — infinite-tree viewer for the note-link graph.
//!
//! State is split between the [`GraphTab`] (graph + view list + global
//! input flag) and per-view [`ExpandedView`] (query text/cursor/parse
//! error, parsed query, the set of expanded root-anchored paths, the
//! flat tree derived from the graph and that path set, selection,
//! scroll). The split is what lets the tree survive a graph rebuild —
//! the view spec (`expanded_paths` + `selected_path`) is independent
//! of the rebuilt [`Graph`], so [`Tab::refresh`] can re-derive a fresh
//! tree that respects deleted/added nodes while preserving the user's
//! exploration state.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use ft_core::graph::delete::{apply_delete, plan_delete};
use ft_core::graph::preset;
use ft_core::graph::query::{parse as parse_query, GraphQuery};
use ft_core::graph::rename::{
    apply_rename_plan, collect_directory_notes, plan_multi_rename, plan_rename,
};
use ft_core::graph::{Graph, NodeKey, NodeKind, NoteId};

use std::sync::Arc;

use ft_core::periodic::Period;
use ft_core::search::Hit;
use ft_core::task::ops::{self, CompleteOptions, CreateInput, CreateOptions, Position};
use ft_core::task::{Priority, Status};

use crate::tui::{
    command::{Command, CommandDef, CommandOutcome, CommandScope},
    event::Event,
    help::HelpSection,
    keymap::{KeyChord, KeyMap},
    modal::{
        ActiveModal, ConfirmChoice, ConfirmDeleteState, CreateSubdirState, Modal, ModalOutcome,
    },
    modal_commands as mc,
    notes_actions::{
        append::AppendState,
        capture::{self, CapturePresetPickerSource},
        create, queue_toast,
        section_move::{
            self, advance_to_multiselect, compose_with_existing_target, MoveCarry, MoveStep,
            SectionMoveState,
        },
    },
    palette,
    tab::{AppRequest, EventOutcome, GraphRequest, Tab, TabCtx, TabKind, ToastStyle},
    tabs::notes::view as notes_view,
    tabs::tasks::edit_popup::EditPopup,
    widgets::{
        render_inline_input, render_scroll_list, CursorMode, EditBuffer, FuzzyPicker, InlineInput,
        PickerOutcome, ScrollListOpts, VaultFilePickerSource,
    },
};

// ── Preset picker source ──────────────────────────────────────────────

mod commands;
mod dispatch;
mod modals;
mod mutations;
mod render;
mod tasks;
mod view;

#[cfg(test)]
mod tests;

use commands::{BUILTIN_DEFAULT_QUERY, VIEW_LABEL_QUERY_WIDTH};
pub(crate) use commands::{GRAPH_COMMANDS, GRAPH_KEYMAP};
pub use modals::{
    CapturePickerModal, GraphMoveOuter, GraphRenameState, GraphSearchPickerSource,
    PresetPickerModal, PresetPickerSource, RelatedModal, SearchPickerModal, TaskCreateState,
    TaskEditState, TaskLeader,
};
pub use view::ExpandedView;
use view::{leaf_display, node_kind_color};

pub struct GraphTab {
    /// The App-owned snapshot this tab last derived its views from
    /// (openspec: shared-graph-snapshot). Adopted from `ctx.snapshot`
    /// whenever the generation differs; never built here.
    snapshot: Option<std::sync::Arc<crate::tui::snapshot::GraphSnapshot>>,
    views: Vec<ExpandedView>,
    active: usize,
    /// Cursor target waiting for the next snapshot adoption: a task's
    /// `(vault-relative path, 1-indexed line)`. Set by mutation flows
    /// whose result only exists in the *next* graph (create task, edit
    /// task); resolved by `adopt_snapshot` via `restore_task_cursor`.
    pending_task_anchor: Option<(PathBuf, usize)>,
    /// Whether view 0 has been seeded with the default query on first
    /// snapshot adoption.
    seeded: bool,
    /// Vault-relative path of a note whose Related modal should open
    /// on the next focus once the graph is built. Set by
    /// [`crate::tui::App`] when the TUI was launched via
    /// `ft notes update-related`.
    queued_related_path: Option<PathBuf>,
    /// Effective keymap: static defaults overlaid with user config.
    keymap: crate::tui::keymap::KeyMap,
}

impl GraphTab {
    pub fn new() -> Self {
        Self {
            snapshot: None,
            views: vec![ExpandedView::default()],
            active: 0,
            pending_task_anchor: None,
            seeded: false,
            queued_related_path: None,
            keymap: GRAPH_KEYMAP.clone(),
        }
    }

    /// The graph from the last-adopted App snapshot, if any.
    fn graph(&self) -> Option<&Graph> {
        Self::graph_of(&self.snapshot)
    }

    /// Field-precise form of [`Self::graph`]: borrows only the
    /// `snapshot` field, so call sites can hold `&mut self.views` at
    /// the same time (the method form borrows all of `self`).
    fn graph_of(
        snapshot: &Option<std::sync::Arc<crate::tui::snapshot::GraphSnapshot>>,
    ) -> Option<&Graph> {
        snapshot.as_ref().map(|s| &s.graph)
    }

    /// Adopt `ctx.snapshot` when its generation differs from the one
    /// this tab last derived views from: re-derive every view's tree
    /// (expansion/selection survive via `NodeKey`) and resolve any
    /// pending task-cursor anchor. Cheap no-op when the generation is
    /// unchanged — safe to call from `on_focus`, `on_graph_ready`, and
    /// the top of `handle_event`.
    fn adopt_snapshot(&mut self, ctx: &TabCtx) {
        let Some(snap) = ctx.snapshot.as_ref() else {
            return;
        };
        if self.snapshot.as_ref().map(|s| s.generation) == Some(snap.generation) {
            return;
        }
        self.snapshot = Some(std::sync::Arc::clone(snap));
        if !self.seeded {
            self.seeded = true;
            // First snapshot: seed the FIRST view only — additional
            // views (created later via Ctrl+N) start empty by design.
            // Skip if a query is already present (test paths construct
            // the tab with state pre-populated).
            let v0 = &mut self.views[0];
            if v0.query_buf.text.trim().is_empty() {
                let seed = ctx
                    .vault
                    .config
                    .config
                    .graph
                    .default_query
                    .clone()
                    .unwrap_or_else(|| BUILTIN_DEFAULT_QUERY.to_string());
                v0.set_query_text(seed);
                // Parse + materialize; `restore_all_views` below only
                // re-derives views that already hold a parsed query.
                v0.apply_query(Some(&snap.graph), ft_core::dates::today());
            }
        }
        self.restore_all_views();
        if let Some(anchor) = self.pending_task_anchor.take() {
            self.restore_task_cursor(&anchor);
        }
    }

    /// Test-only: install a pre-built graph as this tab's snapshot,
    /// bypassing the App lifecycle. Unit tests for tree/view logic use
    /// this; integration flows go through the App pump instead.
    #[cfg(test)]
    fn set_graph_for_test(&mut self, graph: Graph) {
        self.snapshot = Some(std::sync::Arc::new(crate::tui::snapshot::GraphSnapshot {
            generation: 1,
            scan: ft_core::vault::Scan::default(),
            graph,
            citations: ft_core::synth::citations::CitationIndex::default(),
        }));
        self.seeded = true;
    }

    /// Return a new `GraphTab` with the given keymap overlay applied.
    pub fn with_keymap_overlay(mut self, overlay: &crate::tui::keymap::KeymapOverlay) -> Self {
        self.keymap = GRAPH_KEYMAP.with_overlay(overlay);
        self
    }

    /// Return the `NoteId` of the currently-selected Note row, or
    /// `None` for non-Note rows (directories, ghosts, paragraphs).
    fn selected_note_id(&self) -> Option<NoteId> {
        let graph = Self::graph_of(&self.snapshot)?;
        let v = self.active_view();
        let row = v.tree.rows().get(v.selected)?;
        matches!(graph.node(row.note_id), NodeKind::Note(_)).then_some(row.note_id)
    }

    /// Handle a key while the Related panel modal is open.
    /// Compute scores and build the Related panel modal for the
    /// note at `note_path`. Returns `None` when the path doesn't
    /// resolve to a real note. Caller is responsible for posting
    /// `AppRequest::OpenModal(Related(...))`.
    fn build_related_modal_for_path(&self, note_path: &Path, ctx: &TabCtx) -> Option<RelatedModal> {
        let graph = Self::graph_of(&self.snapshot)?;
        let note_id = graph.note_by_path(note_path)?;
        self.build_related_modal_for_id(note_id, ctx)
    }

    /// Build the Related panel modal for a known `NoteId`. Toasts on
    /// errors (non-note row, scoring failure). Ghost rows are
    /// rejected here — a ghost has no file to write, so the panel is
    /// Note-only; ghost reading is via `ft notes related`.
    fn build_related_modal_for_id(&self, note_id: NoteId, ctx: &TabCtx) -> Option<RelatedModal> {
        let graph = Self::graph_of(&self.snapshot)?;
        let NodeKind::Note(note_data) = graph.node(note_id) else {
            queue_toast(
                ctx,
                "select a Note row — ghosts are read via `ft notes related`",
                ToastStyle::Error,
            );
            return None;
        };
        let target_path = note_data.path.clone();
        let target_title = note_data.title.clone();
        let scores = match ft_core::related::score_related(graph, note_id, ctx.vault) {
            Ok(s) => s,
            Err(e) => {
                queue_toast(ctx, &format!("scoring failed: {e}"), ToastStyle::Error);
                return None;
            }
        };
        let (already, candidates): (Vec<_>, Vec<_>) =
            scores.into_iter().partition(|s| s.already_in_related);
        Some(RelatedModal {
            target_path,
            target_title,
            already,
            candidates,
            checked: HashSet::new(),
            cursor: 0,
        })
    }

    /// Resolve the currently-selected row to a `Hit` that the shared
    /// section-move flow can consume. Returns `None` for non-Note rows
    /// (directories, ghosts, empty selection).
    fn selected_note_hit(&self) -> Option<Hit> {
        let graph = Self::graph_of(&self.snapshot)?;
        let v = self.active_view();
        let row = v.tree.rows().get(v.selected)?;
        let NodeKind::Note(n) = graph.node(row.note_id) else {
            return None;
        };
        Some(Hit {
            path: n.path.clone(),
            heading: None,
            file_score: 0,
            heading_score: None,
            total_score: 0,
        })
    }

    fn active_view(&self) -> &ExpandedView {
        &self.views[self.active]
    }

    fn active_view_mut(&mut self) -> &mut ExpandedView {
        &mut self.views[self.active]
    }

    /// Open a new view. If graph presets exist (user or built-in), opens
    /// the preset picker first; on selection, pre-fills the query. On
    /// dismiss, creates a blank view.
    /// `Ctrl+N` path: push a blank view, then open the preset picker
    /// with `for_active_view = false`. If no presets exist, just push
    /// the blank view and drop into input mode (no picker to open).
    fn add_view_with_presets(&mut self, ctx: &TabCtx) {
        let src = PresetPickerSource::new(ctx.vault);
        if src.items.is_empty() {
            self.add_view();
            return;
        }
        self.views.push(ExpandedView::default());
        self.active = self.views.len() - 1;
        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
            ActiveModal::PresetPicker(PresetPickerModal::new(src, false)),
        )));
    }

    /// `Ctrl+P` path: open the preset picker bound to the *current*
    /// active view. On selection the active view's query is replaced
    /// in-place; on dismiss nothing changes.
    fn open_preset_picker_for_active_view(&mut self, ctx: &TabCtx) {
        let src = PresetPickerSource::new(ctx.vault);
        if src.items.is_empty() {
            return;
        }
        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
            ActiveModal::PresetPicker(PresetPickerModal::new(src, true)),
        )));
    }

    /// Open a new blank view. Used when no presets exist (or by test
    /// code). The caller is responsible for posting
    /// `OpenModal(QueryBar)` if they want the new view to drop into
    /// input mode (the production `Ctrl+N` path does this; test code
    /// often doesn't).
    fn add_view(&mut self) {
        self.views.push(ExpandedView::default());
        self.active = self.views.len() - 1;
    }

    /// Apply a preset DSL string to the currently-active view. Called
    /// from the `GraphRequest::ApplyPreset` arm of `handle_graph_request`
    /// when the preset-picker modal commits.
    fn apply_preset_to_active_view(&mut self, dsl: &str) {
        let graph = Self::graph_of(&self.snapshot);
        let v = &mut self.views[self.active];
        v.set_query_text(dsl);
        v.apply_query(graph, ft_core::dates::today());
    }

    /// Land the cursor on the node at the end of `path`, auto-expanding
    /// every ancestor along the way. Writes the path components into
    /// `expanded_paths` and stores the full path in `selected_path` so the
    /// jump survives a subsequent graph refresh, then re-runs
    /// `restore_expansion` to materialize the tree.
    pub fn jump_to_path(&mut self, path: Vec<NoteId>) {
        if path.is_empty() {
            return;
        }
        let Some(graph) = Self::graph_of(&self.snapshot) else {
            return;
        };
        let key_path: Vec<NodeKey> = path.iter().map(|id| graph.stable_key(*id)).collect();
        let v = &mut self.views[self.active];
        if key_path.len() > 1 {
            v.add_expansion_path(key_path[..key_path.len() - 1].to_vec());
        }
        v.selected_path = Some(key_path);
        v.restore_expansion(graph, ft_core::dates::today());
        // Approximate visible-rows budget; render's scroll_to_selection
        // corrects against the real area on the next draw.
        v.scroll_to_selection(20);
    }

    /// BFS from the active query's roots to `target`, returning the
    /// shortest path (root-to-target inclusive) on first hit. Returns
    /// `None` if `target` is not reachable. Reuses the visited-set
    /// pattern from [`collect_search_candidates`] but stops early.
    fn find_node_path(&self, target: NoteId) -> Option<Vec<NoteId>> {
        use std::collections::VecDeque;

        let graph = Self::graph_of(&self.snapshot)?;
        let v = self.active_view();
        let query = v.query.as_ref()?;

        let roots = query.select(graph);
        let mut visited: HashSet<NoteId> = HashSet::with_capacity(roots.len());
        let mut queue: VecDeque<(NoteId, Vec<NoteId>)> = VecDeque::new();

        for r in &roots {
            if visited.insert(*r) {
                if *r == target {
                    return Some(vec![*r]);
                }
                queue.push_back((*r, vec![*r]));
            }
        }

        while let Some((id, path)) = queue.pop_front() {
            if let Some(children) = query.expand(graph, id) {
                for child in children {
                    if visited.insert(child) {
                        let mut child_path = path.clone();
                        child_path.push(child);
                        if child == target {
                            return Some(child_path);
                        }
                        queue.push_back((child, child_path));
                    }
                }
            }
        }
        None
    }

    /// Navigate to the periodic note for `period` within the active
    /// view's tree. Resolves the expected path (no file creation),
    /// looks up the NoteId, runs BFS from the query roots, and either
    /// jumps the cursor via [`jump_to_path`] or queues a toast when
    /// the note is unreachable.
    fn navigate_periodic(&mut self, ctx: &TabCtx, period: Period) {
        let Some(graph) = Self::graph_of(&self.snapshot) else {
            return;
        };
        let pn = &ctx.vault.config.config.periodic_notes;
        let cfg = match period {
            Period::Daily => pn.daily.as_ref(),
            Period::Weekly => pn.weekly.as_ref(),
            Period::Monthly => pn.monthly.as_ref(),
            Period::Quarterly => pn.quarterly.as_ref(),
            Period::Yearly => pn.yearly.as_ref(),
        };
        let Some(cfg) = cfg else {
            queue_toast(
                ctx,
                &format!("{} not configured", period.as_str()),
                ToastStyle::Error,
            );
            return;
        };

        let abs_path =
            match ft_core::periodic::resolve_periodic_path(&ctx.vault.path, cfg, ctx.today) {
                Ok(p) => p,
                Err(e) => {
                    queue_toast(ctx, &format!("{e}"), ToastStyle::Error);
                    return;
                }
            };

        let rel = match abs_path.strip_prefix(&ctx.vault.path) {
            Ok(p) => p.to_path_buf(),
            Err(_) => {
                queue_toast(ctx, "periodic note is outside the vault", ToastStyle::Error);
                return;
            }
        };

        let Some(note_id) = graph.note_by_path(&rel) else {
            queue_toast(
                ctx,
                &format!(
                    "{} note is not in the current graph results",
                    period.as_str()
                ),
                ToastStyle::Info,
            );
            return;
        };

        match self.find_node_path(note_id) {
            Some(path) => self.jump_to_path(path),
            None => {
                queue_toast(
                    ctx,
                    &format!(
                        "{} note is not in the current graph results",
                        period.as_str()
                    ),
                    ToastStyle::Info,
                );
            }
        }
    }

    /// Rewrite the active view's query to root on the currently-selected
    /// node. Only works for Note and Directory nodes (which have paths).
    /// Ghost and Task nodes are no-ops.
    fn rewrite_query_for_root(&mut self) {
        // Gather all needed data first, then mutate the view.
        let Some(graph) = Self::graph_of(&self.snapshot) else {
            return;
        };
        let v = &self.views[self.active];
        let Some(row) = v.tree.rows().get(v.selected) else {
            return;
        };
        let note_id = row.note_id;
        let (kind_str, path_str) = match graph.node(note_id) {
            NodeKind::Note(n) => ("Note", n.path.to_string_lossy().into_owned()),
            NodeKind::Directory(d) => ("Directory", d.path.to_string_lossy().into_owned()),
            _ => return, // Ghost, Task — no path attribute
        };

        // Escape double-quote and backslash in the path.
        let escaped_path: String = path_str
            .chars()
            .flat_map(|c| match c {
                '\\' => vec!['\\', '\\'],
                '"' => vec!['\\', '"'],
                other => vec![other],
            })
            .collect();

        // Preserve the expand block from the current parsed query.
        let query = v.query.clone();
        let expand_part = match query.as_ref() {
            Some(q) => {
                let full = format!("{q}");
                full.find("; expand")
                    .map(|idx| full[idx..].to_string())
                    .unwrap_or_else(|| ";".to_string())
            }
            None => ";".to_string(),
        };
        // Drop immutable references before mutating.
        let _ = v;

        let new_query =
            format!("node where kind in {{{kind_str}}} and path = \"{escaped_path}\"{expand_part}");

        let v = &mut self.views[self.active];
        v.set_query_text(new_query);
        v.apply_query(Some(graph), ft_core::dates::today());
    }

    /// Close the active view. If it's the last view, replace it with a
    /// fresh empty view so we never have zero views (avoids a special
    /// "no views" rendering path).
    fn close_view(&mut self) {
        if self.views.len() == 1 {
            self.views[0] = ExpandedView::default();
            return;
        }
        self.views.remove(self.active);
        if self.active >= self.views.len() {
            self.active = self.views.len() - 1;
        }
    }

    fn next_view(&mut self) {
        if self.views.len() <= 1 {
            return;
        }
        self.active = (self.active + 1) % self.views.len();
    }

    fn prev_view(&mut self) {
        if self.views.len() <= 1 {
            return;
        }
        self.active = (self.active + self.views.len() - 1) % self.views.len();
    }

    fn switch_view(&mut self, idx: usize) {
        if idx < self.views.len() {
            self.active = idx;
        }
    }

    /// Re-derive every view's tree from the current graph (used on
    /// `refresh()` and after the first `on_focus` populates the graph
    /// for views that already had a parsed query). Drops only the
    /// `multi_selected` keys whose underlying nodes have actually
    /// disappeared; existing marks survive a graph rebuild.
    fn restore_all_views(&mut self) {
        let Some(g) = Self::graph_of(&self.snapshot) else {
            return;
        };
        for v in self.views.iter_mut() {
            v.multi_selected.retain(|k| g.id_for_key(k).is_some());
            v.restore_expansion(g, ft_core::dates::today());
        }
    }

    fn request_open_selected_in_editor(&self, ctx: &TabCtx) {
        let Some(graph) = Self::graph_of(&self.snapshot) else {
            return;
        };
        let v = self.active_view();
        let Some(row) = v.tree.rows().get(v.selected) else {
            return;
        };
        match graph.node(row.note_id) {
            NodeKind::Note(n) => {
                let abs = ctx.vault.path.join(&n.path);
                ctx.recents.record_open(&n.path);
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::OpenInEditor { path: abs, line: 1 });
            }
            // graph-task-interaction §7.4: open the task's owning note at
            // the task's line, so the user lands on the task in context.
            NodeKind::Task(t) => {
                let abs = ctx.vault.path.join(&t.source_file);
                ctx.recents.record_open(&t.source_file);
                *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
                    path: abs,
                    line: t.source_line,
                });
            }
            _ => {}
        }
    }

    fn request_open_selected_in_obsidian(&self, ctx: &TabCtx) {
        let Some(graph) = Self::graph_of(&self.snapshot) else {
            return;
        };
        let v = self.active_view();
        let Some(row) = v.tree.rows().get(v.selected) else {
            return;
        };
        if let NodeKind::Note(n) = graph.node(row.note_id) {
            let vault_name = ctx
                .vault
                .path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "vault".to_string());
            let url = ft_core::notes::obsidian_url(&vault_name, &n.path, None);
            ctx.recents.record_open(&n.path);
            *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInObsidian { url });
        }
    }
}

impl Tab for GraphTab {
    fn title(&self) -> &str {
        "Graph"
    }

    fn kind(&self) -> TabKind {
        TabKind::Graph
    }

    fn host_popup_open(&self) -> bool {
        // The graph query bar is the only EditBuffer this tab mounts
        // behind a modal forwarder; check the active view's buffer.
        self.views
            .get(self.active)
            .map(|v| v.query_buf.popup_is_open())
            .unwrap_or(false)
    }

    fn on_graph_ready(&mut self, ctx: &mut TabCtx) {
        self.adopt_snapshot(ctx);
    }

    #[cfg(test)]
    fn set_focused_buffer_completion_for_test(
        &mut self,
        provider: Box<dyn crate::tui::widgets::CompletionProvider>,
    ) {
        if let Some(v) = self.views.get_mut(self.active) {
            v.query_buf.set_completion(provider);
        }
    }

    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()> {
        // Seeding of view 0 with the default query happens inside
        // `adopt_snapshot` on first adoption — the first snapshot may
        // land via `on_graph_ready` before any focus, or via this call.
        self.adopt_snapshot(ctx);
        // If a queued Related modal was requested before the graph
        // existed (e.g. `ft notes update-related <note>`), open it now.
        if let Some(path) = self.queued_related_path.take() {
            if let Some(modal) = self.build_related_modal_for_path(&path, ctx) {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::OpenModal(Box::new(ActiveModal::Related(modal))));
            }
        }
        Ok(())
    }

    fn queue_related_modal(&mut self, note_path: &Path) {
        self.queued_related_path = Some(note_path.to_path_buf());
    }

    /// Single hook for every cross-tab / modal-raised request targeting
    /// this tab (tui-tab-request-routing). Each arm calls the same
    /// private helper the removed dedicated `graph_*` method called —
    /// no behavior change, only the dispatch surface collapsed.
    fn handle_graph_request(&mut self, req: GraphRequest, ctx: &mut TabCtx) {
        match req {
            GraphRequest::JumpToNodes(path) => self.jump_to_path(path),
            GraphRequest::ApplyPreset(dsl) => self.apply_preset_to_active_view(&dsl),
            GraphRequest::FocusQueryBar => {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::OpenModal(Box::new(ActiveModal::QueryBar {
                        view_id: self.active,
                    })));
            }
            GraphRequest::CommitRename {
                note_id,
                is_directory,
                source_rel,
                new_name,
            } => self.commit_rename(ctx, note_id, is_directory, source_rel, &new_name),
            GraphRequest::ConfirmRelated {
                target_path,
                selected_titles,
            } => self.confirm_related(ctx, target_path, selected_titles),
            GraphRequest::QueryBarKey { view_id, key } => {
                // Per-key forwarding from the `QueryBar` modal into the view's
                // `EditBuffer`. Ignores keys for non-active views so a
                // `view_id` racing a view-close becomes a no-op rather than a
                // panic.
                //
                // The buffer's `handle_event` runs the chord through
                // `EDIT_KEYMAP` (readline bindings: Ctrl+A/E/K/W, Alt+B/F/D,
                // …) and falls back to printable-char insert. Anything the
                // buffer didn't recognise is dropped here — the QueryBar modal
                // already consumed it at the modal layer so it never reaches
                // tab- or global-level bindings.
                if view_id >= self.views.len() {
                    return;
                }
                self.active = view_id;
                let _ = self.active_view_mut().query_buf.handle_event(key);
            }
            GraphRequest::ApplyQueryBar { view_id } => {
                if view_id >= self.views.len() {
                    return;
                }
                self.active = view_id;
                let graph = Self::graph_of(&self.snapshot);
                self.views[self.active].apply_query(graph, ft_core::dates::today());
            }
            GraphRequest::MoveConfirmSourceFromTree => self.confirm_source_from_tree(ctx),
            GraphRequest::MoveConfirmTargetFromTree { carry } => {
                self.confirm_target_from_tree(ctx, *carry)
            }
            GraphRequest::MoveConfirmMoveTarget { selected } => {
                self.confirm_move_target(ctx, selected)
            }
            GraphRequest::MoveExecuteMultiMove { selected, dir_path } => {
                self.execute_multi_move(ctx, &selected, &dir_path)
            }
            GraphRequest::NavigatePeriodic(period) => self.navigate_periodic(ctx, period),
            GraphRequest::ConfirmDelete {
                target,
                is_directory,
            } => self.confirm_delete(ctx, target, is_directory),
            GraphRequest::CreateSubdir { parent, name } => self.create_subdir(ctx, parent, name),
            GraphRequest::TaskEdit { path, line, fields } => {
                self.task_edit(ctx, path, line, fields)
            }
            GraphRequest::TaskCommitCreate {
                fields,
                target,
                subtask_parent,
            } => self.task_commit_create(ctx, fields, target, subtask_parent),
        }
    }

    fn commands(&self) -> &'static [CommandDef] {
        GRAPH_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &self.keymap
    }

    /// Delegates to `dispatch::dispatch_command_impl` — a single
    /// `impl Tab for GraphTab` block can't span files, but the
    /// grouped handler methods it calls into can live anywhere in the
    /// `tabs::graph` module tree (tui-tab-request-routing).
    fn dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome {
        self.dispatch_command_impl(cmd, ctx)
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        // Pick up a newer snapshot before acting on the key so line
        // numbers and NoteIds come from the freshest installed build.
        self.adopt_snapshot(ctx);

        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };

        // App-global Tab cycling & plain-digit tab-switch must beat the
        // tab keymap so the user can switch tabs from anywhere. Modified
        // digits (Alt+N) are view jumps and ARE in the keymap.
        if matches!(k.code, KeyCode::Tab | KeyCode::BackTab)
            || (matches!(k.code, KeyCode::Char(c) if c.is_ascii_digit())
                && k.modifiers == KeyModifiers::NONE)
        {
            return Ok(EventOutcome::NotHandled);
        }

        // Graph-missing or empty-tree gate: most keys would no-op or
        // toast because they need a selected row. Keep the gate, but
        // still let view-management and the query-bar through so the
        // user can recover from an empty result (e.g. Ctrl+P to pick a
        // different preset, Ctrl+W to close the view).
        let graph_missing = Self::graph_of(&self.snapshot).is_none();
        let chord = KeyChord::from_key_event(k);
        let cmd = self.keymap.lookup(chord).cloned();
        if graph_missing || self.active_view().tree.is_empty() {
            let allowed = cmd.as_ref().is_some_and(|c| empty_tree_allows(c.name));
            if !allowed {
                return Ok(EventOutcome::NotHandled);
            }
        }

        // Tab keymap → dispatch_command.
        let Some(cmd) = cmd else {
            return Ok(EventOutcome::NotHandled);
        };
        Ok(match self.dispatch_command(&cmd, ctx) {
            CommandOutcome::Handled => EventOutcome::Consumed,
            CommandOutcome::NotHandled => EventOutcome::NotHandled,
        })
    }

    /// Delegates to `render::render_impl` (see `dispatch_command` above
    /// for why this is a delegate rather than a direct trait-method
    /// body).
    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        self.render_impl(frame, area, ctx);
    }

    fn refresh(&mut self, ctx: &mut TabCtx) -> Result<()> {
        // Adopt anything newer already installed, then ask for a fresh
        // build (post-editor / post-sync content may have changed).
        self.adopt_snapshot(ctx);
        ctx.request_graph_refresh();
        Ok(())
    }

    fn help_sections(&self) -> Vec<HelpSection> {
        vec![
            HelpSection::new(
                "Navigation",
                &[
                    ("↑ / ↓ · j / k", "select prev / next row"),
                    ("Enter / l", "expand / collapse node"),
                    ("h", "collapse · jump to parent"),
                    ("g / G", "first / last row"),
                    ("Ctrl+D / Ctrl+U", "half-page down / up"),
                    ("z", "root view on selected node"),
                    ("f", "search & jump to node in current view"),
                    ("r", "refresh graph from disk"),
                ],
            ),
            HelpSection::new(
                "Query",
                &[
                    ("/", "edit query (this view)"),
                    ("Enter", "apply query"),
                    ("Esc", "cancel query edit"),
                    ("Ctrl+P", "load preset into this view"),
                ],
            ),
            HelpSection::new(
                "Files",
                &[
                    ("o", "open selected note in $EDITOR"),
                    ("Ctrl+O", "open selected note in Obsidian"),
                    ("c", "create blank note in current folder"),
                    ("Shift+C", "create note from template"),
                    ("A", "append template to selected note"),
                    ("Q", "quick capture (run a preset)"),
                ],
            ),
            HelpSection::new(
                "Move section",
                &[
                    ("m", "start move (then m = use selected, t = picker)"),
                    ("Esc", "cancel move flow"),
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
                "Views",
                &[
                    ("Ctrl+N", "new view (pick preset or blank)"),
                    ("Ctrl+W", "close active view"),
                    ("Ctrl+PageDown / PageUp", "next / previous view"),
                    ("Alt+1..9", "jump to view N"),
                ],
            ),
            HelpSection::new(
                "Related",
                &[
                    ("Shift+R", "open Related panel for the selected note"),
                    ("Space", "toggle candidate (in modal)"),
                    ("Enter", "append checked concepts (in modal)"),
                    ("Esc / q", "close modal without writing"),
                ],
            ),
            HelpSection::new(
                "Cross-tab",
                &[
                    ("Shift+J", "open Journal tab for the selected note"),
                    ("Ctrl+J", "append selected (or cursor) to Journal sources"),
                ],
            ),
            HelpSection::new(
                "Tasks (on a Task row)",
                &[
                    ("x", "complete task"),
                    ("Shift+X", "cancel task"),
                    ("] / [", "due date +1 / -1 day"),
                    ("} / {", "scheduled date +1 / -1 day"),
                    ("= / -", "cycle priority up / down"),
                    ("Shift+T", "set due date to today"),
                    ("e", "edit task (full form)"),
                    ("o", "open source note at the task line"),
                ],
            ),
            HelpSection::new(
                "Tasks (any row)",
                &[
                    ("a", "leader → c = new task · s = new subtask"),
                    ("v", "view the focused note's task subtree"),
                ],
            ),
        ]
    }

    #[cfg(test)]
    fn selected_is_note_for_test(&self) -> bool {
        self.selected_note_id().is_some()
    }
}

// ── ExpandedView ──────────────────────────────────────────────────────

fn starts_with<T: PartialEq>(haystack: &[T], needle: &[T]) -> bool {
    haystack.len() >= needle.len() && haystack[..needle.len()] == *needle
}

/// Walk [`EdgeKind::Contains`] edges from `dir_id` via BFS to collect
/// all reachable notes with their current vault-relative paths.
/// Build a rectangle centred in `area` taking `percent_x` / `percent_y`
/// of the available space (same helper used by the Notes tab for its
/// modal popups).
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// ── TreeState ─────────────────────────────────────────────────────────

/// Commands that remain usable when the active view's tree is empty
/// (or before the graph has been built). Everything else needs a
/// selected row or query result and is gated off until the user
/// recovers the view via one of these.
fn empty_tree_allows(name: &str) -> bool {
    matches!(
        name,
        "graph.add-view"
            | "graph.preset-pick"
            | "graph.close-view"
            | "graph.next-view"
            | "graph.prev-view"
            | "graph.switch-view"
            | "graph.query-bar"
            | "graph.refresh"
    )
}

/// Parse a `TaskData` date string (YYYY-MM-DD) into a `NaiveDate`.
/// Used by [`leaf_display`] to render relative due/scheduled labels.
fn parse_task_date(s: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// Parse a priority DSL string ("Highest"/"High"/… ) into a `Priority`.
/// Mirrors the `as_str` spelling the graph exposes.
fn parse_priority(s: &str) -> Option<Priority> {
    match s {
        "Highest" => Some(Priority::Highest),
        "High" => Some(Priority::High),
        "Medium" => Some(Priority::Medium),
        "Low" => Some(Priority::Low),
        "Lowest" => Some(Priority::Lowest),
        _ => None,
    }
}

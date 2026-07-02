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
    tab::{AppRequest, EventOutcome, Tab, TabCtx, TabKind, ToastStyle},
    tabs::notes::view as notes_view,
    tabs::tasks::edit_popup::EditPopup,
    widgets::{
        render_inline_input, render_scroll_list, CursorMode, EditBuffer, FuzzyPicker, InlineInput,
        PickerOutcome, ScrollListOpts, VaultFilePickerSource,
    },
};

// ── Preset picker source ──────────────────────────────────────────────

mod commands;
mod modals;
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

    /// Apply the modal's selected concepts to the target note via
    /// the `ft-core::related` plan/apply pair. Called by
    /// `Tab::graph_confirm_related` when the Related modal commits.
    fn confirm_related(
        &mut self,
        ctx: &TabCtx,
        target_path: PathBuf,
        selected_titles: Vec<String>,
    ) {
        if selected_titles.is_empty() {
            return;
        }
        let abs = ctx.vault.path.join(&target_path);
        let content = match std::fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(e) => {
                queue_toast(
                    ctx,
                    &format!("read {}: {e}", target_path.display()),
                    ToastStyle::Error,
                );
                return;
            }
        };
        let plan = ft_core::related::plan_related_update(&content, &selected_titles);
        if let Err(e) = ft_core::related::apply_related_update(&plan, &abs) {
            queue_toast(
                ctx,
                &format!("write {}: {e}", target_path.display()),
                ToastStyle::Error,
            );
            return;
        }
        queue_toast(
            ctx,
            &format!("added {} concept(s) to Related", plan.appended.len()),
            ToastStyle::Info,
        );
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

    fn open_source_picker(&self, ctx: &TabCtx) -> FuzzyPicker<VaultFilePickerSource> {
        FuzzyPicker::new(VaultFilePickerSource::new(
            Arc::clone(ctx.vault),
            Arc::clone(ctx.recents),
        ))
    }

    /// Confirm the currently-selected node as move source.
    ///
    /// Called by [`Tab::graph_move_confirm_source_from_tree`] after the
    /// [`GraphMoveOuter::SourceFromTree`] modal posts
    /// [`AppRequest::GraphMoveConfirmSourceFromTree`] on `m`. Validates the
    /// selection, calls [`advance_to_multiselect`], and either advances the
    /// flow by posting `OpenModal(MoveOuter(Inner(...)))` or — on toast paths
    /// (non-Note row, IO error, no headings) — re-opens `SourceFromTree` so
    /// the user can navigate and retry.
    fn confirm_source_from_tree(&mut self, ctx: &TabCtx) {
        let Some(hit) = self.selected_note_hit() else {
            // Toast + reopen in one shot (single-slot `pending_request`).
            *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModalWithToast {
                modal: Box::new(ActiveModal::MoveOuter(GraphMoveOuter::SourceFromTree)),
                toast_text: "select a note row to use as source".into(),
                toast_style: ToastStyle::Error,
            });
            return;
        };
        match advance_to_multiselect(ctx, hit) {
            MoveStep::Transition(inner) => {
                *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
                    ActiveModal::MoveOuter(GraphMoveOuter::Inner(inner)),
                )));
            }
            MoveStep::Finished => {
                // advance_to_multiselect already queued its own toast
                // via the side-effect queue; reopen the source modal so
                // the user can pick a different note. (The toast it
                // queued went into `pending_request` before we got
                // here — but our OpenModal overwrites it. Surface a
                // generic retry message instead so the user still gets
                // feedback.)
                *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModalWithToast {
                    modal: Box::new(ActiveModal::MoveOuter(GraphMoveOuter::SourceFromTree)),
                    toast_text: "source has no movable headings".into(),
                    toast_style: ToastStyle::Error,
                });
            }
            // advance_to_multiselect only ever yields Transition / Finished.
            MoveStep::Stay | MoveStep::NotHandled => {}
        }
    }

    /// Confirm the currently-selected node as move target.
    ///
    /// Called by [`Tab::graph_move_confirm_target_from_tree`] after the
    /// [`GraphMoveOuter::TargetFromTree`] modal posts
    /// [`AppRequest::GraphMoveConfirmTargetFromTree`] on `m`. The modal
    /// hands the [`MoveCarry`] through the round-trip so this method can
    /// re-open `TargetFromTree` (carry intact) on a recoverable error
    /// (non-Note selection, same-file pick).
    fn confirm_target_from_tree(&mut self, ctx: &TabCtx, carry: MoveCarry) {
        let Some(hit) = self.selected_note_hit() else {
            *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModalWithToast {
                modal: Box::new(ActiveModal::MoveOuter(GraphMoveOuter::TargetFromTree {
                    carry,
                })),
                toast_text: "select a note row to use as target".into(),
                toast_style: ToastStyle::Error,
            });
            return;
        };
        if hit.path == carry.source_rel {
            *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModalWithToast {
                modal: Box::new(ActiveModal::MoveOuter(GraphMoveOuter::TargetFromTree {
                    carry,
                })),
                toast_text: "same-file move is out of scope — pick a different target".into(),
                toast_style: ToastStyle::Error,
            });
            return;
        }
        let target_abs = ctx.vault.path.join(&hit.path);
        let target_content = match std::fs::read_to_string(&target_abs) {
            Ok(s) => s,
            Err(e) => {
                queue_toast(
                    ctx,
                    &format!("could not read target: {e}"),
                    ToastStyle::Error,
                );
                // Match the pre-migration behaviour: IO failure drops the
                // user back to idle (the carry is consumed; not restored).
                return;
            }
        };
        match compose_with_existing_target(carry, hit.path, target_abs, target_content) {
            MoveStep::Transition(inner) => {
                *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
                    ActiveModal::MoveOuter(GraphMoveOuter::Inner(inner)),
                )));
            }
            // Other variants don't surface from this helper today.
            MoveStep::Stay | MoveStep::NotHandled | MoveStep::Finished => {}
        }
    }

    /// Confirm the currently-selected row as the move target for Flow A.
    ///
    /// Called by [`Tab::graph_move_confirm_move_target`] after the
    /// [`GraphMoveOuter::MoveTargetFromTree`] modal posts
    /// [`AppRequest::GraphMoveConfirmMoveTarget`] on `m`/Enter. On a
    /// recoverable failure (no row / non-Directory) re-opens
    /// `MoveTargetFromTree` with `selected` intact so the user can navigate
    /// to a different row.
    fn confirm_move_target(&mut self, ctx: &TabCtx, selected: HashSet<NoteId>) {
        let Some(graph) = Self::graph_of(&self.snapshot) else {
            return;
        };
        let v = self.active_view();
        let Some(row) = v.tree.rows().get(v.selected) else {
            *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModalWithToast {
                modal: Box::new(ActiveModal::MoveOuter(GraphMoveOuter::MoveTargetFromTree {
                    selected,
                })),
                toast_text: "select a directory as target".into(),
                toast_style: ToastStyle::Error,
            });
            return;
        };
        let dir_path = match graph.node(row.note_id) {
            NodeKind::Directory(d) => d.path.clone(),
            _ => {
                *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModalWithToast {
                    modal: Box::new(ActiveModal::MoveOuter(GraphMoveOuter::MoveTargetFromTree {
                        selected,
                    })),
                    toast_text: "select a directory as target".into(),
                    toast_style: ToastStyle::Error,
                });
                return;
            }
        };
        self.execute_multi_move(ctx, &selected, &dir_path);
    }

    /// Execute a multi-note move: plan and apply renames for each
    /// selected note to `target_dir/`, then refresh. Directory
    /// selections are expanded to their contained notes via BFS.
    fn execute_multi_move(&mut self, ctx: &TabCtx, selected: &HashSet<NoteId>, target_dir: &Path) {
        let Some(graph) = Self::graph_of(&self.snapshot) else {
            return;
        };
        let vault_root = &ctx.vault.path;

        let mut moves: Vec<(NoteId, PathBuf)> = Vec::new();
        let mut seen: HashSet<NoteId> = HashSet::new();
        let mut skipped = 0usize;
        let mut dir_count = 0usize;
        for &id in selected {
            match graph.node(id) {
                NodeKind::Note(n) => {
                    if !seen.insert(id) {
                        continue;
                    }
                    let note_path = n.path.clone();
                    if note_path.parent() == Some(target_dir) {
                        skipped += 1;
                        continue;
                    }
                    let stem = note_path.file_name().unwrap_or_default();
                    let new_path = target_dir.join(stem);
                    moves.push((id, new_path));
                }
                NodeKind::Directory(d) => {
                    dir_count += 1;
                    // Expand directory to all contained notes.
                    let old_dir = d.path.clone();
                    let new_dir = target_dir.join(d.name.as_str());
                    for (note_id, new_note_path) in
                        collect_directory_notes(graph, id, &old_dir, &new_dir)
                    {
                        if seen.insert(note_id) {
                            moves.push((note_id, new_note_path));
                        }
                    }
                }
                _ => {}
            }
        }

        if moves.is_empty() {
            let total = selected.len();
            let msg = if dir_count > 0 {
                format!(
                    "all notes from {total} selection(s) are already in {}",
                    target_dir.display()
                )
            } else {
                format!(
                    "all {total} note(s) are already in {}",
                    target_dir.display()
                )
            };
            queue_toast(ctx, &msg, ToastStyle::Info);
            return;
        }

        let plan = match plan_multi_rename(graph, vault_root, &moves) {
            Ok(p) => p,
            Err(e) => {
                queue_toast(ctx, &format!("{e}"), ToastStyle::Error);
                return;
            }
        };
        if let Err(e) = apply_rename_plan(vault_root, &plan) {
            queue_toast(ctx, &format!("move failed: {e}"), ToastStyle::Error);
            return;
        }

        let moved = moves.len();
        let msg = if dir_count > 0 {
            format!(
                "moved {moved} note(s) from {dir_count} director{} to {}",
                if dir_count == 1 { "y" } else { "ies" },
                target_dir.display()
            )
        } else if skipped > 0 {
            format!(
                "moved {moved} note(s) to {} ({skipped} already there)",
                target_dir.display()
            )
        } else {
            format!("moved {moved} note(s) to {}", target_dir.display())
        };
        queue_toast(ctx, &msg, ToastStyle::Success);

        ctx.request_graph_refresh();
    }

    /// Consume one key while the periodic-leader chord is active.
    /// Period letters fire the open flow; any other key (including Esc
    /// and a re-press of `p`) cancels silently. The flag is cleared
    /// before the open flow so a toast from `run_periodic_open` lands
    /// cleanly in the normal-mode UI.
    /// Derive the folder the create flow should start in from the
    /// currently-selected row:
    /// - Note row → containing folder of that note.
    /// - Directory row → the directory itself (`""` for vault root).
    /// - Ghost row → parent of the path the ghost wikilink encodes
    ///   (bare wikilinks → vault root).
    /// - No selection / empty tree / no graph → vault root.
    fn create_folder_from_selection(&self) -> PathBuf {
        let Some(graph) = Self::graph_of(&self.snapshot) else {
            return PathBuf::new();
        };
        let v = self.active_view();
        let Some(row) = v.tree.rows().get(v.selected) else {
            return PathBuf::new();
        };
        match graph.node(row.note_id) {
            NodeKind::Note(n) => n.path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
            NodeKind::Directory(d) => d.path.clone(),
            NodeKind::Ghost(g) => Path::new(&g.raw)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default(),
            NodeKind::Task(t) => t
                .source_file
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default(),
            NodeKind::Paragraph(p) => p
                .source_file
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default(),
            NodeKind::Heading(h) => h
                .source_file
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default(),
        }
    }

    /// Feed a key to the active create flow. Returns
    /// `EventOutcome::NotHandled` if no create flow is active (the
    /// caller's normal keymap can run).
    fn selected_note_abs_path(&self, ctx: &TabCtx) -> Option<PathBuf> {
        let graph = Self::graph_of(&self.snapshot)?;
        let id = self.selected_note_id()?;
        match graph.node(id) {
            NodeKind::Note(n) => Some(ctx.vault.path.join(&n.path)),
            _ => None,
        }
    }

    /// Build and apply the rename plan for the given node. Called by
    /// the `Tab::graph_commit_rename` hook when the rename modal
    /// commits. Toasts on success or failure; on success, refreshes
    /// the graph in place.
    fn commit_rename(
        &mut self,
        ctx: &TabCtx,
        note_id: NoteId,
        is_directory: bool,
        source_rel: PathBuf,
        new_name: &str,
    ) {
        let Some(graph) = Self::graph_of(&self.snapshot) else {
            return;
        };
        let vault_root = &ctx.vault.path;
        // Reopen the rename modal with the typed-in name if commit fails
        // for any recoverable reason (target already exists, plan
        // failure, write failure). Mirrors the pre-migration UX where
        // `handle_rename_key` kept `rename_state` alive on error.
        let reopen_on_error = |ctx: &TabCtx, name: &str| {
            let state = if is_directory {
                GraphRenameState::for_directory(note_id, name, source_rel.clone())
            } else {
                GraphRenameState::for_note(note_id, name, source_rel.clone())
            };
            *ctx.pending_request.borrow_mut() =
                Some(AppRequest::OpenModal(Box::new(ActiveModal::Rename(state))));
        };
        // Local alias-struct so the rest of the function stays
        // structurally identical to its pre-migration form.
        struct Rs<'a> {
            note_id: NoteId,
            is_directory: bool,
            source_rel: &'a Path,
        }
        let rs = Rs {
            note_id,
            is_directory,
            source_rel: &source_rel,
        };

        if rs.is_directory {
            // Directory rename: collect all notes under old dir via BFS,
            // compute new paths, plan_multi_rename.
            let dir_path = rs.source_rel;
            let new_dir = dir_path.parent().unwrap_or(Path::new("")).join(new_name);
            if vault_root.join(&new_dir).exists() {
                queue_toast(
                    ctx,
                    &format!("target directory already exists: {}", new_dir.display()),
                    ToastStyle::Error,
                );
                reopen_on_error(ctx, new_name);
                return;
            }
            let moves = collect_directory_notes(graph, rs.note_id, dir_path, &new_dir);
            match plan_multi_rename(graph, vault_root, &moves) {
                Ok(plan) => {
                    if let Err(e) = apply_rename_plan(vault_root, &plan) {
                        queue_toast(ctx, &format!("rename failed: {e}"), ToastStyle::Error);
                        reopen_on_error(ctx, new_name);
                        return;
                    }
                    let n = moves.len();
                    queue_toast(
                        ctx,
                        &format!(
                            "renamed directory {} → {} ({} file{})",
                            dir_path.display(),
                            new_dir.display(),
                            n,
                            if n == 1 { "" } else { "s" }
                        ),
                        ToastStyle::Success,
                    );
                }
                Err(e) => {
                    queue_toast(ctx, &format!("{e}"), ToastStyle::Error);
                    reopen_on_error(ctx, new_name);
                    return;
                }
            }
        } else {
            // Note rename: plan_rename with new path in same directory.
            let new_path = rs.source_rel.parent().unwrap_or(Path::new("")).join(
                if new_name.ends_with(".md") {
                    PathBuf::from(new_name)
                } else {
                    PathBuf::from(format!("{new_name}.md"))
                },
            );
            match plan_rename(graph, vault_root, rs.note_id, &new_path) {
                Ok(plan) => {
                    if let Err(e) = apply_rename_plan(vault_root, &plan) {
                        queue_toast(ctx, &format!("rename failed: {e}"), ToastStyle::Error);
                        reopen_on_error(ctx, new_name);
                        return;
                    }
                    let old_display = rs.source_rel.display();
                    let new_display = new_path.display();
                    queue_toast(
                        ctx,
                        &format!("renamed {old_display} → {new_display}"),
                        ToastStyle::Success,
                    );
                }
                Err(e) => {
                    queue_toast(ctx, &format!("{e}"), ToastStyle::Error);
                    reopen_on_error(ctx, new_name);
                    return;
                }
            }
        }

        // Success: refresh the graph.
        ctx.request_graph_refresh();
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
    /// by the `Tab::graph_apply_preset` hook when the preset-picker
    /// modal commits.
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

    /// Resolve the focused `NodeKind::Task` row, run an `ops::*` mutation
    /// against its `(source_file, source_line)`, then rebuild the graph and
    /// restore the cursor to the same task. Mirrors the Tasks-tab
    /// `with_selected_task` / `refresh_after_mutation` pattern
    /// (graph-task-interaction §7.2). On a non-Task row it toasts and is a
    /// no-op. Returns `true` if the op ran.
    #[allow(clippy::too_many_lines)]
    /// The closure receives the vault's task format alongside the task's
    /// location. Ops are called without an `expected` guard here: the graph
    /// node carries `TaskData` (string-typed, lossy), not a faithful `Task`
    /// to compare against.
    fn with_focused_task<F>(&mut self, ctx: &mut TabCtx, verb: &str, op: F) -> bool
    where
        F: FnOnce(
            &std::path::Path,
            usize,
            chrono::NaiveDate,
            &dyn ft_core::task::format::TaskFormat,
        ) -> anyhow::Result<()>,
    {
        // Extract the focused task's identity up front so we drop the
        // immutable graph borrow before mutating `self` / refreshing.
        let (abs, anchor, today) = {
            let Some(graph) = Self::graph_of(&self.snapshot) else {
                return false;
            };
            let v = self.active_view();
            let Some(row) = v.tree.rows().get(v.selected) else {
                return false;
            };
            let NodeKind::Task(t) = graph.node(row.note_id) else {
                queue_toast(ctx, "select a task first", ToastStyle::Error);
                return false;
            };
            (
                ctx.vault.path.join(&t.source_file),
                (t.source_file.clone(), t.source_line),
                ctx.today,
            )
        };
        match op(&abs, anchor.1, today, ctx.vault.task_format()) {
            Ok(()) => {}
            Err(e) => {
                queue_toast(ctx, &format!("{verb} failed: {e}"), ToastStyle::Error);
                return false;
            }
        }
        // Refresh the graph and land back on the same task once the
        // rebuilt snapshot arrives.
        self.pending_task_anchor = Some(anchor);
        ctx.request_graph_refresh();
        true
    }

    /// Move the cursor to the row whose task matches `(source_file,
    /// source_line)`, if present in the active view's tree.
    fn restore_task_cursor(&mut self, anchor: &(std::path::PathBuf, usize)) {
        let Some(graph) = Self::graph_of(&self.snapshot) else {
            return;
        };
        // Find the row index without a mutable borrow, then set it.
        let v = self.active_view();
        let found = v.tree.rows().iter().position(|r| {
            matches!(graph.node(r.note_id), NodeKind::Task(t)
                if t.source_file == anchor.0 && t.source_line == anchor.1)
        });
        if let Some(idx) = found {
            self.active_view_mut().selected = idx;
        }
    }

    /// Rewrite the active view's query to the focused note's (or directory's,
    /// or task's source note's) task subtree (graph-task-edit-modal §5).
    /// Deduped by construction via the `HasTask`→top-level model fix.
    fn rewrite_view_to_note_tasks(&mut self, ctx: &mut TabCtx) {
        let Some(graph) = Self::graph_of(&self.snapshot) else {
            return;
        };
        let v = self.active_view();
        let Some(row) = v.tree.rows().get(v.selected) else {
            return;
        };
        let path = match graph.node(row.note_id) {
            NodeKind::Note(n) => n.path.clone(),
            NodeKind::Directory(d) => d.path.clone(),
            NodeKind::Task(t) => t.source_file.clone(),
            _ => {
                queue_toast(ctx, "select a note or directory first", ToastStyle::Error);
                return;
            }
        };
        let query = if path.as_os_str().is_empty() {
            r#"node where kind = Note; expand where edge.kind in {has-task, subtask} and to.kind in {Task};"#
                .to_string()
        } else {
            format!(
                r#"node where kind = Note and path = "{}"; expand where edge.kind in {{has-task, subtask}} and to.kind in {{Task}};"#,
                path.display()
            )
        };
        let graph = Self::graph_of(&self.snapshot);
        let view = &mut self.views[self.active];
        view.set_query_text(&query);
        view.apply_query(graph, ft_core::dates::today());
    }

    /// The `(source_file, source_line)` of the focused Task row, if any.
    fn focused_task_anchor(&self) -> Option<(PathBuf, usize)> {
        let graph = Self::graph_of(&self.snapshot)?;
        let v = self.active_view();
        let row = v.tree.rows().get(v.selected)?;
        match graph.node(row.note_id) {
            NodeKind::Task(t) => Some((t.source_file.clone(), t.source_line)),
            _ => None,
        }
    }

    /// The note path to seed a new top-level task's `target` field from the
    /// focused row: a Note's own path, or a Task's source note. A Directory
    /// (no concrete file) yields `None`, so the create popup falls back to
    /// the daily note.
    fn focused_seed_note(&self) -> Option<PathBuf> {
        let graph = Self::graph_of(&self.snapshot)?;
        let v = self.active_view();
        let row = v.tree.rows().get(v.selected)?;
        match graph.node(row.note_id) {
            NodeKind::Note(n) => Some(n.path.clone()),
            NodeKind::Task(t) => Some(t.source_file.clone()),
            _ => None,
        }
    }

    /// Build an `EditPopup` pre-populated from the focused Task, plus
    /// its `(path, line)` anchor, for the `graph.task-edit-popup` command
    /// (graph-task-edit-modal §2.5).
    fn focused_task_edit_state(
        &self,
    ) -> Option<(
        crate::tui::tabs::tasks::edit_popup::EditPopup,
        PathBuf,
        usize,
    )> {
        let graph = Self::graph_of(&self.snapshot)?;
        let v = self.active_view();
        let row = v.tree.rows().get(v.selected)?;
        let NodeKind::Task(t) = graph.node(row.note_id) else {
            return None;
        };
        let task = ft_core::task::Task {
            description: t.description.clone(),
            status: match t.status.as_str() {
                "Done" => Status::Done,
                "InProgress" => Status::InProgress,
                "Cancelled" => Status::Cancelled,
                _ => Status::Open,
            },
            priority: t.priority.as_deref().and_then(parse_priority),
            due: t
                .due
                .as_deref()
                .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()),
            scheduled: t
                .scheduled
                .as_deref()
                .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()),
            tags: t.tags.clone(),
            recurrence: None,
            source_file: t.source_file.clone(),
            source_line: t.source_line,
            ..Default::default()
        };
        Some((
            crate::tui::tabs::tasks::edit_popup::EditPopup::from_task(&task),
            t.source_file.clone(),
            t.source_line,
        ))
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

    fn graph_jump_to_nodes(&mut self, path: Vec<NoteId>) {
        self.jump_to_path(path);
    }

    fn graph_apply_preset(&mut self, dsl: String) {
        self.apply_preset_to_active_view(&dsl);
    }

    fn graph_focus_query_bar(&mut self, ctx: &TabCtx) {
        *ctx.pending_request.borrow_mut() =
            Some(AppRequest::OpenModal(Box::new(ActiveModal::QueryBar {
                view_id: self.active,
            })));
    }

    fn graph_query_bar_key(&mut self, view_id: usize, key: crossterm::event::KeyEvent) {
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

    fn graph_apply_query_bar(&mut self, view_id: usize) {
        if view_id >= self.views.len() {
            return;
        }
        self.active = view_id;
        let graph = Self::graph_of(&self.snapshot);
        self.views[self.active].apply_query(graph, ft_core::dates::today());
    }

    fn graph_commit_rename(
        &mut self,
        ctx: &TabCtx,
        note_id: NoteId,
        is_directory: bool,
        source_rel: PathBuf,
        new_name: String,
    ) {
        self.commit_rename(ctx, note_id, is_directory, source_rel, &new_name);
    }

    fn graph_confirm_related(
        &mut self,
        ctx: &TabCtx,
        target_path: PathBuf,
        selected_titles: Vec<String>,
    ) {
        self.confirm_related(ctx, target_path, selected_titles);
    }

    fn graph_move_confirm_source_from_tree(&mut self, ctx: &TabCtx) {
        self.confirm_source_from_tree(ctx);
    }

    fn graph_move_confirm_target_from_tree(&mut self, ctx: &TabCtx, carry: MoveCarry) {
        self.confirm_target_from_tree(ctx, carry);
    }

    fn graph_move_confirm_move_target(&mut self, ctx: &TabCtx, selected: HashSet<NoteId>) {
        self.confirm_move_target(ctx, selected);
    }

    fn graph_move_execute_multi_move(
        &mut self,
        ctx: &TabCtx,
        selected: HashSet<NoteId>,
        dir_path: PathBuf,
    ) {
        self.execute_multi_move(ctx, &selected, &dir_path);
    }

    fn graph_navigate_periodic(&mut self, ctx: &TabCtx, period: Period) {
        self.navigate_periodic(ctx, period);
    }

    fn graph_confirm_delete(&mut self, ctx: &TabCtx, target: PathBuf, is_directory: bool) {
        let vault_root = &ctx.vault.path;
        let rel = target
            .strip_prefix(vault_root)
            .unwrap_or(&target)
            .to_path_buf();

        let plan = match plan_delete(&rel, vault_root) {
            Ok(p) => p,
            Err(e) => {
                queue_toast(ctx, &format!("cannot delete: {e}"), ToastStyle::Error);
                return;
            }
        };

        match apply_delete(vault_root, &plan) {
            Ok(()) => {
                ctx.request_graph_refresh();
                if is_directory {
                    queue_toast(
                        ctx,
                        &format!("deleted {}/", rel.display()),
                        ToastStyle::Success,
                    );
                } else {
                    queue_toast(
                        ctx,
                        &format!("deleted {}", rel.display()),
                        ToastStyle::Success,
                    );
                }
            }
            Err(e) => {
                queue_toast(ctx, &format!("delete failed: {e}"), ToastStyle::Error);
            }
        }
    }

    fn graph_create_subdir(&mut self, ctx: &TabCtx, parent: PathBuf, name: String) {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            queue_toast(ctx, "name cannot be empty", ToastStyle::Error);
            return;
        }
        if trimmed.contains('/') || trimmed.contains('\\') {
            queue_toast(
                ctx,
                "name cannot contain path separators",
                ToastStyle::Error,
            );
            return;
        }
        let abs_dir = ctx.vault.path.join(&parent).join(trimmed);
        if abs_dir.exists() {
            let display = if parent.as_os_str().is_empty() {
                format!("{}/", trimmed)
            } else {
                format!("{}/{}/", parent.display(), trimmed)
            };
            queue_toast(
                ctx,
                &format!("directory already exists: {}", display),
                ToastStyle::Error,
            );
            return;
        }
        match std::fs::create_dir_all(&abs_dir) {
            Ok(()) => {
                let display = if parent.as_os_str().is_empty() {
                    format!("{}/", trimmed)
                } else {
                    format!("{}/{}/", parent.display(), trimmed)
                };
                // Refresh graph to pick up the new directory.
                ctx.request_graph_refresh();
                queue_toast(ctx, &format!("created {}", display), ToastStyle::Success);
            }
            Err(e) => {
                queue_toast(ctx, &format!("create failed: {e}"), ToastStyle::Error);
            }
        }
    }

    /// Service `AppRequest::GraphTaskEdit`: apply the validated popup
    /// fields via `ops::update_task_line`, refresh, restore cursor
    /// (graph-task-edit-modal §3.4).
    fn graph_task_edit(
        &mut self,
        ctx: &TabCtx,
        path: PathBuf,
        line: usize,
        fields: crate::tui::tabs::tasks::edit_popup::PopupFields,
    ) {
        let abs = ctx.vault.path.join(&path);
        let (description, due, scheduled, priority, tags, recurrence) = fields;
        // No `expected` guard: the graph node carries `TaskData`, not a
        // faithful `Task`, so there is nothing to compare against yet.
        match ops::update_task_line(&abs, line, ctx.vault.task_format(), None, |t| {
            t.description = description;
            t.due = due;
            t.scheduled = scheduled;
            t.priority = priority;
            t.tags = tags;
            t.recurrence = recurrence;
        }) {
            Ok(_) => {
                self.pending_task_anchor = Some((path, line));
                ctx.request_graph_refresh();
                queue_toast(ctx, "task updated", ToastStyle::Success);
            }
            Err(e) => queue_toast(ctx, &format!("edit failed: {e}"), ToastStyle::Error),
        }
    }

    /// Service `AppRequest::GraphTaskCommitCreate`: resolve the target
    /// file + insertion position, write the new task via `ops::create_task`,
    /// then refresh the graph and land the cursor on it. Mirrors the
    /// Tasks-tab `submit_popup_new` (graph-task-edit-modal §4.3).
    fn graph_task_commit_create(
        &mut self,
        ctx: &TabCtx,
        fields: crate::tui::tabs::tasks::edit_popup::PopupFields,
        target: String,
        subtask_parent: Option<(PathBuf, usize)>,
    ) {
        let (description, due, scheduled, priority, tags, recurrence) = fields;

        // Resolve target file + position. A subtask's parent file and
        // indentation win over the (blank) target field.
        let (resolved, position) = match &subtask_parent {
            Some((pfile, pline)) => (
                ctx.vault.path.join(pfile),
                Position::Subtask {
                    parent_line: *pline,
                },
            ),
            None => {
                // `target` may be `Path` or `Path#heading text`.
                let (target_path, heading): (Option<PathBuf>, Option<String>) = if target.is_empty()
                {
                    (None, None)
                } else {
                    let q = ft_core::search::Query::parse(&target);
                    let path = (!q.file_part.is_empty()).then(|| PathBuf::from(&q.file_part));
                    (path, q.heading_part)
                };
                let (today_n, now_n) = ft_core::dates::now_pair();
                let resolved =
                    match ctx
                        .vault
                        .ensure_target(ctx.today, target_path.as_deref(), today_n, now_n)
                    {
                        Ok(p) => p,
                        Err(e) => {
                            queue_toast(ctx, &format!("create failed: {e}"), ToastStyle::Error);
                            return;
                        }
                    };
                let position = match heading {
                    Some(h) => Position::UnderHeading(h),
                    None => ops::auto_position(
                        &resolved,
                        ctx.vault.config.config.tasks.default_section.as_deref(),
                    ),
                };
                (resolved, position)
            }
        };

        let input = CreateInput {
            description,
            status: Status::Open,
            priority,
            scheduled,
            due,
            tags,
            recurrence,
            ..Default::default()
        };

        match ops::create_task(
            &resolved,
            ctx.vault.task_format(),
            input,
            CreateOptions {
                position,
                force: false,
            },
        ) {
            Ok(outcome) => {
                let rel = ctx.vault.relativize(&resolved).to_path_buf();
                // Lands on the new task when it's already visible (a
                // top-level task in an expanded note, say); a subtask of
                // a collapsed parent stays hidden until the user expands.
                self.pending_task_anchor = Some((rel.clone(), outcome.line));
                ctx.request_graph_refresh();
                queue_toast(
                    ctx,
                    &format!("created {}:{}", rel.display(), outcome.line),
                    ToastStyle::Success,
                );
            }
            Err(e) => queue_toast(ctx, &format!("create failed: {e}"), ToastStyle::Error),
        }
    }

    fn commands(&self) -> &'static [CommandDef] {
        GRAPH_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &self.keymap
    }

    fn dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome {
        // Approximation; render's `scroll_to_selection` corrects.
        let vis = 20usize;
        match cmd.name {
            // Views
            "graph.add-view" => {
                self.add_view_with_presets(ctx);
                CommandOutcome::Handled
            }
            "graph.preset-pick" => {
                self.open_preset_picker_for_active_view(ctx);
                CommandOutcome::Handled
            }
            "graph.close-view" => {
                self.close_view();
                CommandOutcome::Handled
            }
            "graph.next-view" => {
                self.next_view();
                CommandOutcome::Handled
            }
            "graph.prev-view" => {
                self.prev_view();
                CommandOutcome::Handled
            }
            "graph.switch-view" => {
                if let Some(idx_str) = cmd.arg("index") {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        self.switch_view(idx);
                    }
                }
                CommandOutcome::Handled
            }
            // Cross-tab
            "graph.related" => {
                if let Some(note_id) = self.selected_note_id() {
                    if let Some(modal) = self.build_related_modal_for_id(note_id, ctx) {
                        *ctx.pending_request.borrow_mut() =
                            Some(AppRequest::OpenModal(Box::new(ActiveModal::Related(modal))));
                    }
                } else {
                    queue_toast(
                        ctx,
                        "select a Note row to open its Related panel",
                        ToastStyle::Error,
                    );
                }
                CommandOutcome::Handled
            }
            "graph.journal" => {
                let target = Self::graph_of(&self.snapshot).and_then(|graph| {
                    let row = self
                        .active_view()
                        .tree
                        .rows()
                        .get(self.active_view().selected)?;
                    match graph.node(row.note_id) {
                        NodeKind::Note(n) => {
                            Some(crate::tui::tab::JournalTarget::Note(n.path.clone()))
                        }
                        NodeKind::Ghost(g) => {
                            Some(crate::tui::tab::JournalTarget::Ghost(g.raw.clone()))
                        }
                        _ => None,
                    }
                });
                if let Some(target) = target {
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::JournalFor { target });
                } else {
                    queue_toast(
                        ctx,
                        "select a Note or Ghost row to open its journal",
                        ToastStyle::Error,
                    );
                }
                CommandOutcome::Handled
            }
            "graph.add-to-journal-sources" => {
                let targets: Vec<crate::tui::tab::JournalTarget> =
                    match Self::graph_of(&self.snapshot) {
                        Some(graph) => {
                            let v = self.active_view();
                            // Multi-selection drives the target list when
                            // non-empty; otherwise fall back to the cursor row.
                            let ids: Vec<ft_core::graph::NoteId> = if v.multi_selected.is_empty() {
                                v.tree
                                    .rows()
                                    .get(v.selected)
                                    .map(|r| vec![r.note_id])
                                    .unwrap_or_default()
                            } else {
                                v.multi_selected
                                    .iter()
                                    .filter_map(|k| graph.id_for_key(k))
                                    .collect()
                            };
                            ids.into_iter()
                                .filter_map(|id| match graph.node(id) {
                                    NodeKind::Note(n) => {
                                        Some(crate::tui::tab::JournalTarget::Note(n.path.clone()))
                                    }
                                    NodeKind::Ghost(g) => {
                                        Some(crate::tui::tab::JournalTarget::Ghost(g.raw.clone()))
                                    }
                                    _ => None,
                                })
                                .collect()
                        }
                        None => Vec::new(),
                    };
                if targets.is_empty() {
                    queue_toast(ctx, "no Note or Ghost rows selected", ToastStyle::Error);
                } else {
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::JournalAddSources {
                        targets,
                        default_mode: crate::tui::tab::AppendOrReplaceMode::Append,
                    });
                }
                CommandOutcome::Handled
            }
            // Query / search
            "graph.query-bar" => {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::OpenModal(Box::new(ActiveModal::QueryBar {
                        view_id: self.active,
                    })));
                CommandOutcome::Handled
            }
            "graph.rewrite-for-root" => {
                self.rewrite_query_for_root();
                CommandOutcome::Handled
            }
            "graph.search" => {
                if let (Some(g), Some(q)) = (
                    Self::graph_of(&self.snapshot),
                    self.active_view().query.as_ref(),
                ) {
                    let src = GraphSearchPickerSource::new(g, q, ctx.today);
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
                        ActiveModal::Search(SearchPickerModal::new(src)),
                    )));
                }
                CommandOutcome::Handled
            }
            // Navigation
            "graph.cursor-down" => {
                let Some(g) = Self::graph_of(&self.snapshot) else {
                    return CommandOutcome::Handled;
                };
                let v = &mut self.views[self.active];
                v.selected = v.tree.move_selection_down(v.selected);
                v.refresh_selected_path(g);
                v.scroll_to_selection(vis);
                CommandOutcome::Handled
            }
            "graph.cursor-up" => {
                let Some(g) = Self::graph_of(&self.snapshot) else {
                    return CommandOutcome::Handled;
                };
                let v = &mut self.views[self.active];
                v.selected = v.tree.move_selection_up(v.selected);
                v.refresh_selected_path(g);
                v.scroll_to_selection(vis);
                CommandOutcome::Handled
            }
            "graph.expand-or-collapse" => {
                let graph = Self::graph_of(&self.snapshot);
                let v = &mut self.views[self.active];
                if let (Some(g), Some(q)) = (graph, v.query.as_ref()) {
                    let path = v.path_to(v.selected, g);
                    let was_expanded = v
                        .tree
                        .rows()
                        .get(v.selected)
                        .map(|r| r.expanded)
                        .unwrap_or(false);
                    v.tree.expand_at(v.selected, g, q, ctx.today);
                    if was_expanded {
                        v.forget_expansion_subtree(&path);
                    } else if v.tree.rows().get(v.selected).is_some_and(|r| r.expanded) {
                        v.add_expansion_path(path);
                    }
                    v.scroll_to_selection(vis);
                }
                CommandOutcome::Handled
            }
            "graph.collapse-or-jump-parent" => {
                let Some(g) = Self::graph_of(&self.snapshot) else {
                    return CommandOutcome::Handled;
                };
                let v = &mut self.views[self.active];
                let expanded = v.tree.rows().get(v.selected).is_some_and(|r| r.expanded);
                if expanded {
                    let path = v.path_to(v.selected, g);
                    v.tree.collapse_at(v.selected);
                    v.forget_expansion_subtree(&path);
                    v.scroll_to_selection(vis);
                } else {
                    let depth = v.tree.rows().get(v.selected).map_or(0, |r| r.depth);
                    if depth > 0 {
                        let target = depth.saturating_sub(1);
                        let mut pos = v.selected;
                        while pos > 0 {
                            pos -= 1;
                            if v.tree.rows()[pos].depth == target {
                                v.selected = pos;
                                v.refresh_selected_path(g);
                                v.scroll_to_selection(vis);
                                break;
                            }
                        }
                    }
                }
                CommandOutcome::Handled
            }
            "graph.cursor-first" => {
                let Some(g) = Self::graph_of(&self.snapshot) else {
                    return CommandOutcome::Handled;
                };
                let v = &mut self.views[self.active];
                v.selected = 0;
                v.scroll_offset = 0;
                v.refresh_selected_path(g);
                CommandOutcome::Handled
            }
            "graph.cursor-last" => {
                let Some(g) = Self::graph_of(&self.snapshot) else {
                    return CommandOutcome::Handled;
                };
                let v = &mut self.views[self.active];
                v.selected = v.tree.len().saturating_sub(1);
                v.refresh_selected_path(g);
                v.scroll_to_selection(vis);
                CommandOutcome::Handled
            }
            "graph.cursor-half-page-down" => {
                let Some(g) = Self::graph_of(&self.snapshot) else {
                    return CommandOutcome::Handled;
                };
                let v = &mut self.views[self.active];
                let rows = vis.max(1);
                v.selected = (v.selected + rows / 2).min(v.tree.len().saturating_sub(1));
                v.scroll_offset = (v.scroll_offset + rows / 2).min(v.tree.len().saturating_sub(1));
                v.refresh_selected_path(g);
                v.scroll_to_selection(vis);
                CommandOutcome::Handled
            }
            "graph.cursor-half-page-up" => {
                let Some(g) = Self::graph_of(&self.snapshot) else {
                    return CommandOutcome::Handled;
                };
                let v = &mut self.views[self.active];
                let rows = vis.max(1);
                v.selected = v.selected.saturating_sub(rows / 2);
                v.scroll_offset = v.scroll_offset.saturating_sub(rows / 2);
                v.refresh_selected_path(g);
                v.scroll_to_selection(vis);
                CommandOutcome::Handled
            }
            // Notes
            "graph.open-in-editor" => {
                self.request_open_selected_in_editor(ctx);
                CommandOutcome::Handled
            }
            // Task interaction (graph-task-interaction §7.3).
            "graph.task-complete" => {
                self.with_focused_task(ctx, "complete", |path, line, today, format| {
                    match ops::complete_task(
                        path,
                        line,
                        format,
                        None,
                        CompleteOptions { on: today },
                    ) {
                        Ok(_) => Ok(()),
                        Err(ops::CompleteError::AlreadyDone { .. }) => Ok(()),
                        Err(e) => Err(e.into()),
                    }
                });
                CommandOutcome::Handled
            }
            "graph.task-cancel" => {
                self.with_focused_task(ctx, "cancel", |path, line, today, format| {
                    match ops::cancel_task(path, line, format, None, today) {
                        Ok(_) => Ok(()),
                        Err(ops::CancelError::AlreadyCancelled { .. }) => Ok(()),
                        Err(e) => Err(e.into()),
                    }
                });
                CommandOutcome::Handled
            }
            "graph.task-due-next" => {
                self.with_focused_task(ctx, "due+1", |path, line, today, format| {
                    nudge_task_field(path, line, format, TaskField::Due, 1, today)
                });
                CommandOutcome::Handled
            }
            "graph.task-due-prev" => {
                self.with_focused_task(ctx, "due-1", |path, line, today, format| {
                    nudge_task_field(path, line, format, TaskField::Due, -1, today)
                });
                CommandOutcome::Handled
            }
            "graph.task-scheduled-next" => {
                self.with_focused_task(ctx, "scheduled+1", |path, line, today, format| {
                    nudge_task_field(path, line, format, TaskField::Scheduled, 1, today)
                });
                CommandOutcome::Handled
            }
            "graph.task-scheduled-prev" => {
                self.with_focused_task(ctx, "scheduled-1", |path, line, today, format| {
                    nudge_task_field(path, line, format, TaskField::Scheduled, -1, today)
                });
                CommandOutcome::Handled
            }
            "graph.task-priority-next" => {
                self.with_focused_task(ctx, "priority+1", |path, line, _today, format| {
                    cycle_task_priority(path, line, format, 1)
                });
                CommandOutcome::Handled
            }
            "graph.task-priority-prev" => {
                self.with_focused_task(ctx, "priority-1", |path, line, _today, format| {
                    cycle_task_priority(path, line, format, -1)
                });
                CommandOutcome::Handled
            }
            "graph.task-due-today" => {
                self.with_focused_task(ctx, "due=today", |path, line, today, format| {
                    ops::update_task_line(path, line, format, None, |t| {
                        t.due = Some(today);
                    })
                    .map(|_| ())
                    .map_err(|e| e.into())
                });
                CommandOutcome::Handled
            }
            "graph.task-edit-popup" => {
                if let Some((popup, path, line)) = self.focused_task_edit_state() {
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
                        ActiveModal::TaskEdit(Box::new(TaskEditState { popup, path, line })),
                    )));
                } else {
                    queue_toast(ctx, "select a task first", ToastStyle::Error);
                }
                CommandOutcome::Handled
            }
            "graph.task-leader" => {
                let leader = TaskLeader {
                    seed_note: self.focused_seed_note(),
                    focused_task: self.focused_task_anchor(),
                };
                *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
                    ActiveModal::TaskLeader(Box::new(leader)),
                )));
                CommandOutcome::Handled
            }
            "graph.task-create" | "graph.task-new-subtask" => {
                // The leader modal (`a`) opens the create popup directly via
                // `ModalOutcome::OpenSibling`; these tab-level commands are
                // no-ops so `ft commands list` / docs still surface them.
                CommandOutcome::Handled
            }
            "graph.tasks-of-note" => {
                self.rewrite_view_to_note_tasks(ctx);
                CommandOutcome::Handled
            }
            "graph.open-in-obsidian" => {
                self.request_open_selected_in_obsidian(ctx);
                CommandOutcome::Handled
            }
            "graph.create-blank" => {
                // Ghost shortcut: create the note instantly at the ghost's path.
                if let (Some(graph), Some(row)) = (
                    Self::graph_of(&self.snapshot),
                    self.active_view()
                        .tree
                        .rows()
                        .get(self.active_view().selected),
                ) {
                    if let NodeKind::Ghost(g) = graph.node(row.note_id) {
                        let abs_path = ctx.vault.path.join(Path::new(&g.raw).with_extension("md"));
                        let title = Path::new(&g.raw)
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        if let Some(parent) = abs_path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let content = format!("# {title}\n");
                        if ft_core::fs::write_atomic(&abs_path, &content).is_ok() {
                            // Refresh graph to pick up the new note.
                            ctx.request_graph_refresh();
                            let rel = abs_path
                                .strip_prefix(&ctx.vault.path)
                                .unwrap_or(&abs_path)
                                .to_path_buf();
                            ctx.recents.record_open(&rel);
                            *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
                                path: abs_path,
                                line: 1,
                            });
                            queue_toast(
                                ctx,
                                &format!("created {}", rel.display()),
                                ToastStyle::Success,
                            );
                        } else {
                            queue_toast(ctx, "failed to create note", ToastStyle::Error);
                        }
                        return CommandOutcome::Handled;
                    }
                }
                let folder = self.create_folder_from_selection();
                let state = create::begin_filename_prompt(folder, None);
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::OpenModal(Box::new(ActiveModal::Create(state))));
                CommandOutcome::Handled
            }
            "graph.create-from-template" => {
                // Ghost shortcut: open template picker, commit to ghost path.
                if let (Some(graph), Some(row)) = (
                    Self::graph_of(&self.snapshot),
                    self.active_view()
                        .tree
                        .rows()
                        .get(self.active_view().selected),
                ) {
                    if let NodeKind::Ghost(g) = graph.node(row.note_id) {
                        let parent = Path::new(&g.raw)
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_default();
                        let filename = Path::new(&g.raw)
                            .file_name()
                            .map(|n| {
                                let s = n.to_string_lossy().into_owned();
                                if s.ends_with(".md") {
                                    s
                                } else {
                                    format!("{s}.md")
                                }
                            })
                            .unwrap_or_default();
                        let state =
                            create::begin_template_picking(ctx, Some(parent), Some(filename));
                        *ctx.pending_request.borrow_mut() =
                            Some(AppRequest::OpenModal(Box::new(ActiveModal::Create(state))));
                        return CommandOutcome::Handled;
                    }
                }
                let folder = self.create_folder_from_selection();
                let state = create::begin_template_picking(ctx, Some(folder), None);
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::OpenModal(Box::new(ActiveModal::Create(state))));
                CommandOutcome::Handled
            }
            "graph.append" => {
                let Some(target_path) = self.selected_note_abs_path(ctx) else {
                    queue_toast(
                        ctx,
                        "select a note first (A appends to the selected note)",
                        ToastStyle::Error,
                    );
                    return CommandOutcome::Handled;
                };
                let state = AppendState::begin_with_target(ctx, target_path, None);
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::OpenModal(Box::new(ActiveModal::Append(state))));
                CommandOutcome::Handled
            }
            "graph.quick-capture" => {
                let src = CapturePresetPickerSource::new(ctx.vault);
                let target = self.selected_note_abs_path(ctx);
                *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
                    ActiveModal::CapturePicker(CapturePickerModal::new(src, target)),
                )));
                CommandOutcome::Handled
            }
            "graph.move" => {
                *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
                    ActiveModal::MoveOuter(GraphMoveOuter::SourceFromTree),
                )));
                CommandOutcome::Handled
            }
            "graph.refresh" => {
                ctx.request_graph_refresh();
                CommandOutcome::Handled
            }
            "graph.rename-or-multi-move" => {
                // r with multi-selection enters multi-move; otherwise
                // opens the rename modal on the focused row.
                let selected = {
                    let v = self.active_view_mut();
                    if !v.multi_selected.is_empty() {
                        let s = std::mem::take(&mut v.multi_selected);
                        Some(s)
                    } else {
                        None
                    }
                };
                if let Some(keys) = selected {
                    // Modal API speaks NoteIds (resolved against the
                    // current live graph); drop any keys whose nodes
                    // have already vanished.
                    let graph = Self::graph_of(&self.snapshot);
                    let ids: HashSet<NoteId> = graph
                        .map(|g| keys.iter().filter_map(|k| g.id_for_key(k)).collect())
                        .unwrap_or_default();
                    if ids.is_empty() {
                        return CommandOutcome::Handled;
                    }
                    *ctx.pending_request.borrow_mut() =
                        Some(AppRequest::OpenModal(Box::new(ActiveModal::MoveOuter(
                            GraphMoveOuter::MoveTargetFromTree { selected: ids },
                        ))));
                    return CommandOutcome::Handled;
                }
                let graph = Self::graph_of(&self.snapshot);
                let v = self.active_view();
                let Some(row) = v.tree.rows().get(v.selected) else {
                    return CommandOutcome::Handled;
                };
                let modal = match graph.map(|g| g.node(row.note_id)) {
                    Some(NodeKind::Note(n)) => Some(GraphRenameState::for_note(
                        row.note_id,
                        &n.title,
                        n.path.clone(),
                    )),
                    Some(NodeKind::Directory(d)) if d.path.as_os_str().is_empty() => {
                        queue_toast(ctx, "cannot rename vault root", ToastStyle::Error);
                        None
                    }
                    Some(NodeKind::Directory(d)) => Some(GraphRenameState::for_directory(
                        row.note_id,
                        &d.name,
                        d.path.clone(),
                    )),
                    Some(NodeKind::Ghost(_)) => {
                        queue_toast(
                            ctx,
                            "cannot rename a ghost — create the note first",
                            ToastStyle::Error,
                        );
                        None
                    }
                    _ => None,
                };
                if let Some(state) = modal {
                    *ctx.pending_request.borrow_mut() =
                        Some(AppRequest::OpenModal(Box::new(ActiveModal::Rename(state))));
                }
                CommandOutcome::Handled
            }
            "graph.delete" => {
                let graph = Self::graph_of(&self.snapshot);
                let v = self.active_view();
                let Some(row) = v.tree.rows().get(v.selected) else {
                    return CommandOutcome::Handled;
                };
                match graph.map(|g| g.node(row.note_id)) {
                    Some(NodeKind::Note(n)) => {
                        let rel = n.path.to_string_lossy().into_owned();
                        let abs = ctx.vault.path.join(&n.path);
                        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
                            ActiveModal::ConfirmDelete(ConfirmDeleteState {
                                message: format!("Delete note `{rel}`?"),
                                target: abs,
                                is_directory: false,
                                focus: ConfirmChoice::No,
                            }),
                        )));
                    }
                    Some(NodeKind::Directory(d)) if d.path.as_os_str().is_empty() => {
                        queue_toast(ctx, "cannot delete vault root", ToastStyle::Error);
                    }
                    Some(NodeKind::Directory(d)) => {
                        let rel = d.path.to_string_lossy().into_owned();
                        let display = if rel.is_empty() {
                            "vault root".to_string()
                        } else {
                            rel.clone()
                        };
                        let abs = ctx.vault.path.join(&d.path);
                        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
                            ActiveModal::ConfirmDelete(ConfirmDeleteState {
                                message: format!(
                                    "Delete directory `{display}/` and all its contents?"
                                ),
                                target: abs,
                                is_directory: true,
                                focus: ConfirmChoice::No,
                            }),
                        )));
                    }
                    Some(NodeKind::Ghost(_)) => {
                        queue_toast(
                            ctx,
                            "cannot delete a ghost — it does not exist on disk",
                            ToastStyle::Error,
                        );
                    }
                    Some(NodeKind::Task(_)) => {
                        queue_toast(
                            ctx,
                            "cannot delete a task node — delete the task in its source file",
                            ToastStyle::Error,
                        );
                    }
                    _ => {}
                }
                CommandOutcome::Handled
            }
            "graph.create-subdir" => {
                let graph = Self::graph_of(&self.snapshot);
                let v = self.active_view();
                let Some(row) = v.tree.rows().get(v.selected) else {
                    return CommandOutcome::Handled;
                };
                match graph.map(|g| g.node(row.note_id)) {
                    Some(NodeKind::Directory(d)) => {
                        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
                            ActiveModal::CreateSubdir(CreateSubdirState {
                                parent: d.path.clone(),
                                buf: EditBuffer::default(),
                                error: None,
                            }),
                        )));
                    }
                    _ => {
                        queue_toast(ctx, "select a directory first", ToastStyle::Error);
                    }
                }
                CommandOutcome::Handled
            }
            // Periodic
            "graph.periodic-leader" => {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::OpenModal(Box::new(ActiveModal::PeriodicLeader)));
                CommandOutcome::Handled
            }
            "graph.today" => {
                self.navigate_periodic(ctx, Period::Daily);
                CommandOutcome::Handled
            }
            // Multi-select
            "graph.toggle-multi-select" => {
                let Some(g) = Self::graph_of(&self.snapshot) else {
                    return CommandOutcome::Handled;
                };
                let v = self.active_view();
                let Some(row) = v.tree.rows().get(v.selected) else {
                    return CommandOutcome::Handled;
                };
                let note_id = row.note_id;
                let (selectable, is_root) = match g.node(note_id) {
                    NodeKind::Note(_) => (true, false),
                    NodeKind::Directory(d) => (true, d.path.as_os_str().is_empty()),
                    _ => (false, false),
                };
                if selectable && !is_root {
                    let key = g.stable_key(note_id);
                    let v = &mut self.views[self.active];
                    if v.multi_selected.contains(&key) {
                        v.multi_selected.remove(&key);
                    } else {
                        v.multi_selected.insert(key);
                    }
                }
                CommandOutcome::Handled
            }
            "graph.clear-multi-select" => {
                let v = self.active_view_mut();
                if !v.multi_selected.is_empty() {
                    v.multi_selected.clear();
                    CommandOutcome::Handled
                } else {
                    // Esc with empty multi-selection falls through to
                    // (potentially) close other things; signal NotHandled.
                    CommandOutcome::NotHandled
                }
            }
            _ => CommandOutcome::NotHandled,
        }
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

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        // Before the first snapshot lands there is nothing to derive a
        // tree from — render a non-blocking loading line instead.
        if self.snapshot.is_none() && ctx.snapshot.is_none() {
            frame.render_widget(
                ratatui::widgets::Paragraph::new("building graph…")
                    .style(Style::default().fg(palette::DIM)),
                area,
            );
            return;
        }

        let [input_area, strip_area, tree_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(area);

        let input_mode = ctx.active_modal_name == Some("query-bar");

        // Extract view info before mutable borrow for tree rendering.
        let query_snippet = self.views[self.active].query_snippet();

        // ── Input bar ────────────────────────────────────────────────
        // The bar scrolls horizontally so a long query keeps the caret in
        // view. The hardware cursor is only positioned while editing.
        let prompt_style = if input_mode {
            Style::default().fg(palette::PRIMARY)
        } else {
            Style::default().fg(palette::DIM)
        };
        let cursor_mode = if input_mode {
            CursorMode::Hardware
        } else {
            CursorMode::None
        };
        render_inline_input(
            frame,
            input_area,
            InlineInput::new(&self.views[self.active].query_buf, cursor_mode)
                .prefix(Span::styled("> ", prompt_style))
                .text_style(prompt_style),
        );

        // ── View tab strip ───────────────────────────────────────────
        let mut spans: Vec<Span> = Vec::with_capacity(self.views.len() * 2);
        for (i, vw) in self.views.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" "));
            }
            let label = format!(" {}: {} ", i + 1, vw.query_snippet());
            let style = if i == self.active {
                Style::default()
                    .fg(palette::BLACK)
                    .bg(palette::PRIMARY)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette::DIM)
            };
            spans.push(Span::styled(label, style));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), strip_area);

        // ── Tree ─────────────────────────────────────────────────────
        let tree_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", query_snippet))
            .border_style(Style::default().fg(palette::PRIMARY));
        let inner_area = tree_block.inner(tree_area);
        frame.render_widget(tree_block, tree_area);

        let visible = inner_area.height.saturating_sub(1).max(1) as usize;
        let active = self.active;
        let v = &mut self.views[active];

        v.scroll_to_selection(visible);

        let items: Vec<ListItem> = v
            .tree
            .rows()
            .iter()
            .enumerate()
            .skip(v.scroll_offset)
            .take(visible)
            .map(|(i, row)| {
                let indent = "  ".repeat(row.depth);
                let indicator = if row.expanded {
                    '▼'
                } else if row.expandable {
                    '▶'
                } else {
                    ' '
                };
                let sel_marker = Self::graph_of(&self.snapshot)
                    .map(|g| v.multi_selected.contains(&g.stable_key(row.note_id)))
                    .unwrap_or(false);
                let sel_marker = if sel_marker { '●' } else { ' ' };
                let prefix = format!("{indent}{indicator} {sel_marker} ");
                let base_style = if i == v.selected {
                    Style::default()
                        .fg(palette::BLACK)
                        .bg(palette::PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(palette::WHITE)
                };
                let graph = Self::graph_of(&self.snapshot);
                let kind_color = graph
                    .map(|g| node_kind_color(g.node(row.note_id)))
                    .unwrap_or(palette::WHITE);
                // Selected row keeps the uniform BLACK-on-PRIMARY highlight;
                // overlaying the per-kind color (e.g. orange for Task) would
                // collide with the orange selection background.
                let kind_style = if i == v.selected {
                    base_style
                } else {
                    base_style.fg(kind_color)
                };
                let kind_span = Span::styled(row.kind_char.to_string(), kind_style);
                let display_span = Span::styled(row.display.clone(), kind_style);
                let space = Span::styled(" ", base_style);
                // Build a Line from multiple Spans so that type-color
                // is layered with selection highlighting.
                let line = Line::from(vec![
                    Span::styled(prefix, base_style),
                    kind_span,
                    space,
                    display_span,
                ]);
                ListItem::new(line)
            })
            .collect();

        frame.render_widget(List::new(items), inner_area);

        // Empty-state hint: shown when the active view's tree has no
        // navigable content (≤ 1 row) and the user isn't actively
        // typing. Disappears as soon as the user expands anything or
        // enters input mode.
        if v.tree.len() <= 1 && !input_mode && inner_area.height >= 2 {
            let hint_rect = Rect {
                y: inner_area.y + 1,
                height: 1,
                ..inner_area
            };
            let hint = Span::styled("press / to edit query", Style::default().fg(palette::DIM));
            frame.render_widget(Paragraph::new(Line::from(hint)), hint_rect);
        }

        // Error line overlays bottom of tree inner area.
        if let Some(ref err) = v.parse_error {
            if inner_area.height > 0 {
                let err_rect = Rect {
                    y: inner_area.y + inner_area.height.saturating_sub(1),
                    height: 1,
                    ..inner_area
                };
                let err_span = Span::styled(err.as_str(), Style::default().fg(palette::ERROR));
                frame.render_widget(Paragraph::new(Line::from(err_span)), err_rect);
            }
        }

        // Move-section overlay: rendered by `Modal::render` for
        // `ActiveModal::MoveOuter(...)` via the App-level modal driver
        // (extract-modal-driver §2 + migrate-move-outer-modal). No
        // tab-resident render arm here anymore.
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

/// Which date field a task nudge operates on (graph-task-interaction §7.3).
enum TaskField {
    Due,
    Scheduled,
}

/// Nudge a task's `due` or `scheduled` by `delta_days` (from `today` if
/// unset). Mirrors the Tasks-tab `nudge_field` helper.
fn nudge_task_field(
    path: &std::path::Path,
    line: usize,
    format: &dyn ft_core::task::format::TaskFormat,
    field: TaskField,
    delta_days: i64,
    today: chrono::NaiveDate,
) -> anyhow::Result<()> {
    use chrono::Duration;
    ops::update_task_line(path, line, format, None, move |t| {
        let current = match field {
            TaskField::Due => t.due,
            TaskField::Scheduled => t.scheduled,
        };
        let base = current.unwrap_or(today);
        let next = base + Duration::days(delta_days);
        match field {
            TaskField::Due => t.due = Some(next),
            TaskField::Scheduled => t.scheduled = Some(next),
        }
    })
    .map(|_| ())
    .map_err(anyhow::Error::from)
}

/// Cycle a task's priority forward (`dir = 1`) or backward (`dir = -1`)
/// through the priority cycle. Mirrors the Tasks-tab `cycle_priority`.
fn cycle_task_priority(
    path: &std::path::Path,
    line: usize,
    format: &dyn ft_core::task::format::TaskFormat,
    dir: i64,
) -> anyhow::Result<()> {
    const CYCLE: [Option<Priority>; 6] = [
        None,
        Some(Priority::Lowest),
        Some(Priority::Low),
        Some(Priority::Medium),
        Some(Priority::High),
        Some(Priority::Highest),
    ];
    ops::update_task_line(path, line, format, None, move |t| {
        let pos = CYCLE.iter().position(|p| *p == t.priority).unwrap_or(0) as i64;
        let len = CYCLE.len() as i64;
        let next = ((pos + dir).rem_euclid(len)) as usize;
        t.priority = CYCLE[next];
    })
    .map(|_| ())
    .map_err(anyhow::Error::from)
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

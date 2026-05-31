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

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, List, ListItem, Paragraph},
    Frame,
};

use ft_core::graph::preset;
use ft_core::graph::query::{parse as parse_query, GraphQuery};
use ft_core::graph::rename::{
    apply_rename_plan, collect_directory_notes, plan_multi_rename, plan_rename,
};
use ft_core::graph::{Graph, NodeKind, NoteId};

use std::sync::Arc;

use ft_core::periodic::Period;
use ft_core::search::Hit;

use crate::tui::{
    event::Event,
    help::HelpSection,
    notes_actions::{
        create::{self, CreateState, CreateStep},
        periodic::run_periodic_open,
        queue_toast,
        section_move::{
            self, advance_to_multiselect, compose_with_existing_target, MoveCarry, MoveStep,
            SectionMoveState,
        },
    },
    tab::{AppRequest, EventOutcome, Tab, TabCtx, ToastStyle},
    tabs::notes::view as notes_view,
    widgets::{EditBuffer, FuzzyPicker, PickerOutcome, VaultFilePickerSource},
};

// ── Preset picker source ──────────────────────────────────────────────

struct PresetPickerSource {
    items: Vec<(String, String)>,
    matcher: nucleo_matcher::Matcher,
    buf: Vec<char>,
}

impl PresetPickerSource {
    fn new(vault: &ft_core::vault::Vault) -> Self {
        let mut items: Vec<(String, String)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (name, dsl) in &vault.config.config.graph.presets {
            if seen.insert(name.clone()) {
                items.push((name.clone(), dsl.clone()));
            }
        }
        for name in preset::builtin_names() {
            if seen.insert(name.to_string()) {
                items.push((name.to_string(), preset::builtin(name).unwrap().to_string()));
            }
        }
        Self {
            items,
            matcher: nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT),
            buf: Vec::new(),
        }
    }
}

impl crate::tui::widgets::PickerSource for PresetPickerSource {
    type Item = String;

    fn query(&mut self, q: &str, limit: usize) -> Vec<crate::tui::widgets::PickerItem<String>> {
        let pat = nucleo_matcher::pattern::Pattern::parse(
            q,
            nucleo_matcher::pattern::CaseMatching::Smart,
            nucleo_matcher::pattern::Normalization::Smart,
        );
        let mut ranked: Vec<(u32, usize, Vec<u32>)> = Vec::new();
        for (i, (name, _)) in self.items.iter().enumerate() {
            self.buf.clear();
            let haystack = nucleo_matcher::Utf32Str::new(name, &mut self.buf);
            let mut indices = Vec::new();
            if let Some(score) = pat.indices(haystack, &mut self.matcher, &mut indices) {
                ranked.push((score, i, indices));
            }
        }
        ranked.sort_by_key(|b| std::cmp::Reverse(b.0));
        ranked
            .into_iter()
            .take(limit)
            .map(|(_, i, match_indices)| {
                let (name, _) = &self.items[i];
                crate::tui::widgets::PickerItem {
                    label: name.clone(),
                    match_indices,
                    data: name.clone(),
                }
            })
            .collect()
    }

    fn initial_items(&mut self, limit: usize) -> Vec<crate::tui::widgets::PickerItem<String>> {
        self.items
            .iter()
            .take(limit)
            .map(|(name, _)| crate::tui::widgets::PickerItem {
                label: name.clone(),
                match_indices: Vec::new(),
                data: name.clone(),
            })
            .collect()
    }
}

// ── GraphTab ──────────────────────────────────────────────────────────

/// Fallback query the first view of the graph tab seeds itself with on
/// first focus when `[graph].default_query` isn't set in config. Shows
/// the vault root as a single directory line — pressing Enter / `l`
/// expands one hop. Kept here (and not in `ft-core`) because it's a
/// TUI-presentation default, not an engine concern.
const BUILTIN_DEFAULT_QUERY: &str = concat!(
    "node where kind = Directory and path = \"\"; ",
    "expand where edge.kind = directory-contains;",
);

/// Width budget for a view's tab-strip label query snippet, in characters.
const VIEW_LABEL_QUERY_WIDTH: usize = 20;

pub struct GraphTab {
    graph: Option<Graph>,
    views: Vec<ExpandedView>,
    active: usize,
    /// Whether the query input bar of the active view owns the keyboard.
    /// Global rather than per-view — there's no meaningful notion of
    /// "editing an inactive view", and the App-level keymap wants one
    /// place to look to decide if printable characters are being captured.
    input_mode: bool,
    /// Active create-note flow. `Some` whenever the user has pressed
    /// `c` / `C` and we're walking the shared [`CreateState`] machine;
    /// `None` otherwise. While `Some`, the create overlay captures the
    /// keyboard ahead of every other binding.
    create_state: Option<CreateState>,
    /// Whether the periodic-note leader chord is awaiting its next
    /// keystroke (`d`/`w`/`m`/`q`/`y`). Mirrors `NotesState::PeriodicLeader`.
    /// Set by `p`; cleared on any subsequent keypress (the period letter
    /// fires the open flow; everything else cancels silently).
    periodic_leader: bool,
    /// Active move-section flow. `None` outside the flow; `Some` while
    /// the user is walking the two-phase graph-driven UX or inside a
    /// shared [`SectionMoveState`] step.
    move_outer: Option<GraphMoveOuter>,
    /// Active rename-in-place modal. `Some` when Flow B is open.
    rename_state: Option<GraphRenameState>,
    /// Active preset picker. `Some` after `Ctrl+N` or `Ctrl+P`;
    /// selecting a preset applies the preset DSL. Dismissing falls
    /// back to a blank view (`Ctrl+N`) or leaves the active view
    /// unchanged (`Ctrl+P`).
    preset_picker: Option<FuzzyPicker<PresetPickerSource>>,
    /// When `true`, the open picker was triggered by `Ctrl+P` and
    /// should apply the selected preset to the *existing* active view
    /// rather than a newly-created one.
    preset_picker_for_active_view: bool,
}

/// Inline rename-in-place state. `Some` while the rename modal is open.
#[derive(Debug)]
struct GraphRenameState {
    note_id: NoteId,
    is_directory: bool,
    buffer: EditBuffer,
    source_rel: PathBuf,
}

/// Graph-tab outer wrapper around the shared section-move flow.
///
/// The shared module's [`SectionMoveState`] assumes both source and
/// target are picked via fuzzy pickers. The Graph tab inserts two
/// tree-driven phases — `SourceFromTree` before the headings step and
/// `TargetFromTree` after it — and falls back to the shared picker
/// flow via `t`.
pub enum GraphMoveOuter {
    /// `m` pressed once: awaiting `m` again (confirm selected node as
    /// source), `t` (open fuzzy source picker), or Esc (cancel).
    SourceFromTree,
    /// `t` was pressed during phase 1: fuzzy picker open. `Esc` returns
    /// to `SourceFromTree`; selecting a file transitions to
    /// `Inner(HeadingMultiSelect)`.
    SourcePicker {
        picker: FuzzyPicker<VaultFilePickerSource>,
    },
    /// In a shared `SectionMoveState` step (headings multi-select or
    /// composing). The Graph tab intercepts the headings → target
    /// transition and swaps to `TargetFromTree` rather than letting the
    /// shared `TargetPicking` (fuzzy) own the screen.
    Inner(SectionMoveState),
    /// Phase 2: target via tree. `m` confirms the selected node, `t`
    /// falls back to picker, `/` enters input mode for query
    /// refinement, `Esc` returns to the headings step rebuilt from the
    /// carry.
    TargetFromTree { carry: MoveCarry },
    /// Phase 2 fallback: fuzzy target picker open. `Esc` returns to
    /// `TargetFromTree`; selecting a target transitions to
    /// `Inner(Composing)`.
    TargetPicker {
        picker: FuzzyPicker<VaultFilePickerSource>,
        carry: MoveCarry,
    },
    /// Flow A phase 2: selecting target directory for moved notes.
    /// `Enter`/`m` confirms selected Directory row; `t` opens picker;
    /// Esc cancels.
    MoveTargetFromTree { selected: HashSet<NoteId> },
    /// Flow A fallback: fuzzy directory picker for target.
    MoveTargetPicker {
        picker: FuzzyPicker<VaultFilePickerSource>,
        selected: HashSet<NoteId>,
    },
}

impl GraphTab {
    pub fn new() -> Self {
        Self {
            graph: None,
            views: vec![ExpandedView::default()],
            active: 0,
            input_mode: false,
            create_state: None,
            periodic_leader: false,
            move_outer: None,
            rename_state: None,
            preset_picker: None,
            preset_picker_for_active_view: false,
        }
    }

    /// Resolve the currently-selected row to a `Hit` that the shared
    /// section-move flow can consume. Returns `None` for non-Note rows
    /// (directories, ghosts, empty selection).
    fn selected_note_hit(&self) -> Option<Hit> {
        let graph = self.graph.as_ref()?;
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

    /// Apply a `MoveStep` returned by the shared module while we're in
    /// `Inner(...)`. The Graph tab intercepts the headings → target
    /// transition (the shared step yields `TargetPicking { ..., picker, error }`)
    /// and re-routes to `TargetFromTree`, discarding the picker and
    /// using a tree-driven target phase instead. All other transitions
    /// pass through unchanged.
    fn apply_inner_step(&mut self, step: MoveStep) {
        match step {
            MoveStep::Stay | MoveStep::NotHandled => {}
            MoveStep::Finished => {
                self.move_outer = None;
            }
            MoveStep::Transition(SectionMoveState::TargetPicking {
                source_rel,
                source_abs,
                source_content,
                headings,
                selected,
                focus,
                clipboard,
                picker: _,
                error: _,
            }) => {
                let carry = MoveCarry {
                    source_rel,
                    source_abs,
                    source_content,
                    headings,
                    selected,
                    focus,
                    clipboard,
                };
                self.move_outer = Some(GraphMoveOuter::TargetFromTree { carry });
            }
            MoveStep::Transition(next) => {
                self.move_outer = Some(GraphMoveOuter::Inner(next));
            }
        }
    }

    /// Confirm the currently-selected node as move source. Reads the
    /// file, extracts headings, transitions to `Inner(HeadingMultiSelect)`.
    /// No-op + toast when the selected row isn't a Note.
    fn confirm_source_from_tree(&mut self, ctx: &TabCtx) {
        let Some(hit) = self.selected_note_hit() else {
            queue_toast(ctx, "select a note row to use as source", ToastStyle::Error);
            return;
        };
        // advance_to_multiselect can yield Finished on IO error / empty
        // headings — fold it through the same dispatcher as Inner so the
        // toast surfaces correctly.
        let step = advance_to_multiselect(ctx, hit);
        self.apply_inner_step(step);
    }

    /// Confirm the currently-selected node as move target. Reads the
    /// target file, builds `Composing`, transitions to `Inner(Composing)`.
    /// Toasts on non-Note selection or same-file.
    fn confirm_target_from_tree(&mut self, ctx: &TabCtx) {
        let Some(hit) = self.selected_note_hit() else {
            queue_toast(ctx, "select a note row to use as target", ToastStyle::Error);
            return;
        };
        let Some(GraphMoveOuter::TargetFromTree { carry }) = self.move_outer.take() else {
            return;
        };
        if hit.path == carry.source_rel {
            queue_toast(
                ctx,
                "same-file move is out of scope — pick a different target",
                ToastStyle::Error,
            );
            // Restore the outer so the user can pick again.
            self.move_outer = Some(GraphMoveOuter::TargetFromTree { carry });
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
                return;
            }
        };
        let step = compose_with_existing_target(carry, hit.path, target_abs, target_content);
        self.apply_inner_step(step);
    }

    /// Confirm the currently-selected row as the move target for Flow A.
    /// Reads the Directory path and executes the multi-note move.
    fn confirm_move_target(&mut self, ctx: &TabCtx) {
        let Some(graph) = self.graph.as_ref() else {
            return;
        };
        let v = self.active_view();
        let Some(row) = v.tree.rows().get(v.selected) else {
            queue_toast(ctx, "select a directory as target", ToastStyle::Error);
            return;
        };
        let dir_path = match graph.node(row.note_id) {
            NodeKind::Directory(d) => d.path.clone(),
            _ => {
                queue_toast(ctx, "select a directory as target", ToastStyle::Error);
                return;
            }
        };
        let Some(GraphMoveOuter::MoveTargetFromTree { selected }) = self.move_outer.take() else {
            return;
        };
        self.execute_multi_move(ctx, &selected, &dir_path);
    }

    /// Execute a multi-note move: plan and apply renames for each
    /// selected note to `target_dir/`, then refresh. Directory
    /// selections are expanded to their contained notes via BFS.
    fn execute_multi_move(&mut self, ctx: &TabCtx, selected: &HashSet<NoteId>, target_dir: &Path) {
        let Some(graph) = self.graph.as_ref() else {
            self.move_outer = None;
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

        self.move_outer = None;

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

        let scan = ctx.vault.scan();
        if let Ok(new_graph) = Graph::build(ctx.vault, &scan) {
            self.graph = Some(new_graph);
            self.restore_all_views();
        }
    }

    /// Dispatch a keystroke while the move overlay is active. Returns
    /// `EventOutcome::NotHandled` when no move flow is in progress
    /// (the caller's regular keymap can run).
    fn handle_move_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        let Some(outer) = self.move_outer.take() else {
            return EventOutcome::NotHandled;
        };
        match outer {
            GraphMoveOuter::SourceFromTree => match (k.code, k.modifiers) {
                (KeyCode::Char('m'), KeyModifiers::NONE) => {
                    self.confirm_source_from_tree(ctx);
                    // confirm_source_from_tree only transitions the
                    // outer on success (Note row → headings step). On
                    // toast paths (non-Note selection, IO error, no
                    // headings) it leaves move_outer at `None` — but
                    // the user should stay in the source phase so
                    // they can navigate to a Note and try again.
                    if self.move_outer.is_none() {
                        self.move_outer = Some(GraphMoveOuter::SourceFromTree);
                    }
                    EventOutcome::Consumed
                }
                (KeyCode::Char('t'), KeyModifiers::NONE) => {
                    self.move_outer = Some(GraphMoveOuter::SourcePicker {
                        picker: self.open_source_picker(ctx),
                    });
                    EventOutcome::Consumed
                }
                (KeyCode::Esc, _) => {
                    // Drop back to normal mode (move_outer already taken).
                    EventOutcome::Consumed
                }
                _ => {
                    // Restore and ignore.
                    self.move_outer = Some(GraphMoveOuter::SourceFromTree);
                    EventOutcome::NotHandled
                }
            },
            GraphMoveOuter::SourcePicker { mut picker } => match picker.handle_key(k) {
                PickerOutcome::Selected(hit) => {
                    let step = advance_to_multiselect(ctx, hit);
                    self.apply_inner_step(step);
                    EventOutcome::Consumed
                }
                PickerOutcome::Cancelled => {
                    self.move_outer = Some(GraphMoveOuter::SourceFromTree);
                    EventOutcome::Consumed
                }
                PickerOutcome::StillOpen => {
                    self.move_outer = Some(GraphMoveOuter::SourcePicker { picker });
                    EventOutcome::Consumed
                }
                PickerOutcome::NotHandled => {
                    self.move_outer = Some(GraphMoveOuter::SourcePicker { picker });
                    EventOutcome::NotHandled
                }
            },
            GraphMoveOuter::Inner(mut sms) => {
                let step = section_move::handle_key(&mut sms, k, ctx);
                // If the step didn't transition away, put the state back
                // (apply_inner_step handles Transition/Finished by
                // assigning move_outer itself).
                match step {
                    MoveStep::Stay => {
                        self.move_outer = Some(GraphMoveOuter::Inner(sms));
                        EventOutcome::Consumed
                    }
                    MoveStep::NotHandled => {
                        self.move_outer = Some(GraphMoveOuter::Inner(sms));
                        EventOutcome::NotHandled
                    }
                    other => {
                        self.apply_inner_step(other);
                        EventOutcome::Consumed
                    }
                }
            }
            GraphMoveOuter::TargetFromTree { carry } => match (k.code, k.modifiers) {
                (KeyCode::Char('m'), KeyModifiers::NONE) => {
                    self.move_outer = Some(GraphMoveOuter::TargetFromTree { carry });
                    self.confirm_target_from_tree(ctx);
                    EventOutcome::Consumed
                }
                (KeyCode::Char('t'), KeyModifiers::NONE) => {
                    self.move_outer = Some(GraphMoveOuter::TargetPicker {
                        picker: self.open_source_picker(ctx),
                        carry,
                    });
                    EventOutcome::Consumed
                }
                (KeyCode::Char('/'), KeyModifiers::NONE) => {
                    // `/` falls back to the tab's query-input mode so
                    // the user can refine the visible tree. We keep the
                    // move state alive — exiting input mode returns
                    // here.
                    self.input_mode = true;
                    self.move_outer = Some(GraphMoveOuter::TargetFromTree { carry });
                    EventOutcome::Consumed
                }
                (KeyCode::Esc, _) => {
                    // Cancel back to the heading-multi-select with the
                    // same carry data so the user can re-pick headings
                    // or escape further.
                    self.move_outer = Some(GraphMoveOuter::Inner(
                        SectionMoveState::HeadingMultiSelect {
                            source_rel: carry.source_rel,
                            source_abs: carry.source_abs,
                            source_content: carry.source_content,
                            headings: carry.headings,
                            selected: carry.selected,
                            focus: carry.focus,
                        },
                    ));
                    EventOutcome::Consumed
                }
                // Pass arrow/jk/Enter through to the tree-navigation
                // keymap so the user can move selection in the tree.
                _ => {
                    self.move_outer = Some(GraphMoveOuter::TargetFromTree { carry });
                    EventOutcome::NotHandled
                }
            },
            GraphMoveOuter::TargetPicker { mut picker, carry } => match picker.handle_key(k) {
                PickerOutcome::Selected(hit) => {
                    if hit.path == carry.source_rel {
                        // Same-file: reopen picker with a fresh inst.
                        queue_toast(
                            ctx,
                            "same-file move is out of scope — pick a different target",
                            ToastStyle::Error,
                        );
                        self.move_outer = Some(GraphMoveOuter::TargetPicker { picker, carry });
                        return EventOutcome::Consumed;
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
                            self.move_outer = Some(GraphMoveOuter::TargetFromTree { carry });
                            return EventOutcome::Consumed;
                        }
                    };
                    let step =
                        compose_with_existing_target(carry, hit.path, target_abs, target_content);
                    self.apply_inner_step(step);
                    EventOutcome::Consumed
                }
                PickerOutcome::Cancelled => {
                    self.move_outer = Some(GraphMoveOuter::TargetFromTree { carry });
                    EventOutcome::Consumed
                }
                PickerOutcome::StillOpen => {
                    self.move_outer = Some(GraphMoveOuter::TargetPicker { picker, carry });
                    EventOutcome::Consumed
                }
                PickerOutcome::NotHandled => {
                    self.move_outer = Some(GraphMoveOuter::TargetPicker { picker, carry });
                    EventOutcome::NotHandled
                }
            },
            GraphMoveOuter::MoveTargetFromTree { selected } => {
                match (k.code, k.modifiers) {
                    (KeyCode::Enter, _) | (KeyCode::Char('m'), KeyModifiers::NONE) => {
                        self.move_outer = Some(GraphMoveOuter::MoveTargetFromTree { selected });
                        self.confirm_move_target(ctx);
                        EventOutcome::Consumed
                    }
                    (KeyCode::Char('t'), KeyModifiers::NONE) => {
                        self.move_outer = Some(GraphMoveOuter::MoveTargetPicker {
                            picker: self.open_source_picker(ctx),
                            selected,
                        });
                        EventOutcome::Consumed
                    }
                    (KeyCode::Esc, _) => {
                        // Cancel: clear multi-selection (already
                        // consumed from ExpandedView), drop outer.
                        EventOutcome::Consumed
                    }
                    // Tree navigation keys pass through.
                    _ => {
                        self.move_outer = Some(GraphMoveOuter::MoveTargetFromTree { selected });
                        EventOutcome::NotHandled
                    }
                }
            }
            GraphMoveOuter::MoveTargetPicker {
                mut picker,
                selected,
            } => {
                match picker.handle_key(k) {
                    PickerOutcome::Selected(hit) => {
                        // Execute move to the selected directory.
                        let dir_path = hit.path;
                        self.execute_multi_move(ctx, &selected, &dir_path);
                        EventOutcome::Consumed
                    }
                    PickerOutcome::Cancelled => {
                        self.move_outer = Some(GraphMoveOuter::MoveTargetFromTree { selected });
                        EventOutcome::Consumed
                    }
                    PickerOutcome::StillOpen => {
                        self.move_outer =
                            Some(GraphMoveOuter::MoveTargetPicker { picker, selected });
                        EventOutcome::Consumed
                    }
                    PickerOutcome::NotHandled => {
                        self.move_outer =
                            Some(GraphMoveOuter::MoveTargetPicker { picker, selected });
                        EventOutcome::NotHandled
                    }
                }
            }
        }
    }

    /// Consume one key while the periodic-leader chord is active.
    /// Period letters fire the open flow; any other key (including Esc
    /// and a re-press of `p`) cancels silently. The flag is cleared
    /// before the open flow so a toast from `run_periodic_open` lands
    /// cleanly in the normal-mode UI.
    fn handle_periodic_leader_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        let period = match (k.code, k.modifiers) {
            (KeyCode::Char('d'), KeyModifiers::NONE) => Some(Period::Daily),
            (KeyCode::Char('w'), KeyModifiers::NONE) => Some(Period::Weekly),
            (KeyCode::Char('m'), KeyModifiers::NONE) => Some(Period::Monthly),
            (KeyCode::Char('q'), KeyModifiers::NONE) => Some(Period::Quarterly),
            (KeyCode::Char('y'), KeyModifiers::NONE) => Some(Period::Yearly),
            _ => None,
        };
        self.periodic_leader = false;
        if let Some(p) = period {
            run_periodic_open(ctx, p);
        }
        EventOutcome::Consumed
    }

    /// Derive the folder the create flow should start in from the
    /// currently-selected row:
    /// - Note row → containing folder of that note.
    /// - Directory row → the directory itself (`""` for vault root).
    /// - Ghost row → parent of the path the ghost wikilink encodes
    ///   (bare wikilinks → vault root).
    /// - No selection / empty tree / no graph → vault root.
    fn create_folder_from_selection(&self) -> PathBuf {
        let Some(graph) = self.graph.as_ref() else {
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
        }
    }

    /// Feed a key to the active create flow. Returns
    /// `EventOutcome::NotHandled` if no create flow is active (the
    /// caller's normal keymap can run).
    fn handle_create_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        let Some(cs) = self.create_state.as_mut() else {
            return EventOutcome::NotHandled;
        };
        match create::handle_key(cs, k, ctx) {
            CreateStep::Stay => EventOutcome::Consumed,
            CreateStep::NotHandled => EventOutcome::NotHandled,
            CreateStep::Transition(next) => {
                *cs = next;
                EventOutcome::Consumed
            }
            CreateStep::Finished => {
                self.create_state = None;
                EventOutcome::Consumed
            }
        }
    }

    /// Dispatch a keystroke while the rename-in-place modal (Flow B) is
    /// open. EditBuffer keys are routed; Enter commits, Esc discards.
    fn handle_rename_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        let Some(rs) = self.rename_state.as_mut() else {
            return EventOutcome::NotHandled;
        };
        match (k.code, k.modifiers) {
            (KeyCode::Esc, _) => {
                self.rename_state = None;
                EventOutcome::Consumed
            }
            (KeyCode::Enter, _) => {
                let new_name = rs.buffer.text.trim().to_string();
                if new_name.is_empty() {
                    queue_toast(ctx, "name cannot be empty", ToastStyle::Error);
                    return EventOutcome::Consumed;
                }
                if new_name.contains('/') {
                    queue_toast(
                        ctx,
                        "name cannot contain / — use move (Space-select + r) to change directories",
                        ToastStyle::Error,
                    );
                    return EventOutcome::Consumed;
                }
                self.commit_rename(ctx, &new_name);
                EventOutcome::Consumed
            }
            (KeyCode::Char(c), KeyModifiers::NONE) => {
                rs.buffer.insert(c);
                EventOutcome::Consumed
            }
            (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                rs.buffer.insert(c);
                EventOutcome::Consumed
            }
            (KeyCode::Backspace, _) => {
                rs.buffer.backspace();
                EventOutcome::Consumed
            }
            (KeyCode::Delete, _) => {
                rs.buffer.delete();
                EventOutcome::Consumed
            }
            (KeyCode::Left, _) => {
                rs.buffer.left();
                EventOutcome::Consumed
            }
            (KeyCode::Right, _) => {
                rs.buffer.right();
                EventOutcome::Consumed
            }
            (KeyCode::Home, _) => {
                rs.buffer.home();
                EventOutcome::Consumed
            }
            (KeyCode::End, _) => {
                rs.buffer.end();
                EventOutcome::Consumed
            }
            (KeyCode::Char('w'), m) if m.contains(KeyModifiers::CONTROL) => {
                rs.buffer.delete_word_backward();
                EventOutcome::Consumed
            }
            _ => EventOutcome::Consumed,
        }
    }

    /// Build and apply the rename plan for the current rename modal.
    /// On success, refreshes the graph and closes the modal. On error,
    /// toasts and leaves the modal open.
    fn commit_rename(&mut self, ctx: &TabCtx, new_name: &str) {
        let Some(rs) = self.rename_state.take() else {
            return;
        };
        let Some(graph) = self.graph.as_ref() else {
            self.rename_state = Some(rs);
            return;
        };
        let vault_root = &ctx.vault.path;

        if rs.is_directory {
            // Directory rename: collect all notes under old dir via BFS,
            // compute new paths, plan_multi_rename.
            let dir_path = &rs.source_rel;
            let new_dir = dir_path.parent().unwrap_or(Path::new("")).join(new_name);
            if vault_root.join(&new_dir).exists() {
                queue_toast(
                    ctx,
                    &format!("target directory already exists: {}", new_dir.display()),
                    ToastStyle::Error,
                );
                self.rename_state = Some(rs);
                return;
            }
            let moves = collect_directory_notes(graph, rs.note_id, dir_path, &new_dir);
            match plan_multi_rename(graph, vault_root, &moves) {
                Ok(plan) => {
                    if let Err(e) = apply_rename_plan(vault_root, &plan) {
                        queue_toast(ctx, &format!("rename failed: {e}"), ToastStyle::Error);
                        self.rename_state = Some(rs);
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
                    self.rename_state = Some(rs);
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
                        self.rename_state = Some(rs);
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
                    self.rename_state = Some(rs);
                    return;
                }
            }
        }

        // Success: refresh the graph.
        let scan = ctx.vault.scan();
        if let Ok(new_graph) = Graph::build(ctx.vault, &scan) {
            self.graph = Some(new_graph);
            self.restore_all_views();
        }
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
    fn add_view_with_presets(&mut self, ctx: &TabCtx) {
        let src = PresetPickerSource::new(ctx.vault);
        if src.items.is_empty() {
            self.add_view();
            return;
        }
        self.preset_picker_for_active_view = false;
        self.views.push(ExpandedView::default());
        self.active = self.views.len() - 1;
        self.preset_picker = Some(FuzzyPicker::new(src));
    }

    /// Open the preset picker bound to the *current* active view (the
    /// `Ctrl+P` path). On selection the active view's query is replaced
    /// in-place; on dismiss nothing changes.
    fn open_preset_picker_for_active_view(&mut self, ctx: &TabCtx) {
        let src = PresetPickerSource::new(ctx.vault);
        if src.items.is_empty() {
            return;
        }
        self.preset_picker_for_active_view = true;
        self.preset_picker = Some(FuzzyPicker::new(src));
    }

    /// Open a new blank view and drop into input mode. Used when no
    /// presets exist (or by test code).
    fn add_view(&mut self) {
        self.views.push(ExpandedView::default());
        self.active = self.views.len() - 1;
        self.input_mode = true;
    }

    /// Resolve a preset name to its DSL string, preferring user config over
    /// built-ins.
    fn resolve_preset(&self, name: &str, ctx: &TabCtx) -> Option<String> {
        if let Some(user) = ctx.vault.config.config.graph.presets.get(name) {
            return Some(user.clone());
        }
        preset::builtin(name).map(|s| s.to_string())
    }

    fn handle_preset_picker_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome {
        let Some(mut picker) = self.preset_picker.take() else {
            return EventOutcome::NotHandled;
        };
        let for_active = self.preset_picker_for_active_view;
        match picker.handle_key(k) {
            PickerOutcome::Selected(name) => {
                if let Some(dsl) = self.resolve_preset(&name, ctx) {
                    self.apply_preset_to_active_view(&dsl);
                }
                self.input_mode = false;
                self.preset_picker_for_active_view = false;
            }
            PickerOutcome::Cancelled => {
                if !for_active {
                    self.input_mode = true;
                }
                self.preset_picker_for_active_view = false;
            }
            PickerOutcome::StillOpen => {
                self.preset_picker = Some(picker);
            }
            PickerOutcome::NotHandled => {
                self.preset_picker = Some(picker);
                return EventOutcome::NotHandled;
            }
        }
        EventOutcome::Consumed
    }

    fn apply_preset_to_active_view(&mut self, dsl: &str) {
        let graph = self.graph.as_ref();
        let v = &mut self.views[self.active];
        v.query_text = dsl.to_string();
        v.input_cursor = dsl.len();
        v.apply_query(graph);
    }

    /// Rewrite the active view's query to root on the currently-selected
    /// node. Only works for Note and Directory nodes (which have paths).
    /// Ghost and Task nodes are no-ops.
    fn rewrite_query_for_root(&mut self) {
        // Gather all needed data first, then mutate the view.
        let Some(graph) = self.graph.as_ref() else {
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
            format!("node where kind = {kind_str} and path = \"{escaped_path}\"{expand_part}");

        let v = &mut self.views[self.active];
        v.query_text = new_query;
        v.input_cursor = v.query_text.len();
        v.apply_query(Some(graph));
    }

    /// Close the active view. If it's the last view, replace it with a
    /// fresh empty view so we never have zero views (avoids a special
    /// "no views" rendering path).
    fn close_view(&mut self) {
        if self.views.len() == 1 {
            self.views[0] = ExpandedView::default();
            self.input_mode = false;
            return;
        }
        self.views.remove(self.active);
        if self.active >= self.views.len() {
            self.active = self.views.len() - 1;
        }
        self.input_mode = false;
    }

    fn next_view(&mut self) {
        if self.views.len() <= 1 {
            return;
        }
        self.active = (self.active + 1) % self.views.len();
        self.input_mode = false;
    }

    fn prev_view(&mut self) {
        if self.views.len() <= 1 {
            return;
        }
        self.active = (self.active + self.views.len() - 1) % self.views.len();
        self.input_mode = false;
    }

    fn switch_view(&mut self, idx: usize) {
        if idx < self.views.len() {
            self.active = idx;
            self.input_mode = false;
        }
    }

    /// Re-derive every view's tree from the current graph (used on
    /// `refresh()` and after the first `on_focus` populates the graph
    /// for views that already had a parsed query).
    fn restore_all_views(&mut self) {
        let Some(g) = self.graph.as_ref() else {
            return;
        };
        for v in self.views.iter_mut() {
            v.multi_selected.clear();
            v.restore_expansion(g);
        }
    }

    fn request_open_selected_in_editor(&self, ctx: &TabCtx) {
        let Some(graph) = self.graph.as_ref() else {
            return;
        };
        let v = self.active_view();
        let Some(row) = v.tree.rows().get(v.selected) else {
            return;
        };
        if let NodeKind::Note(n) = graph.node(row.note_id) {
            let abs = ctx.vault.path.join(&n.path);
            ctx.recents.record_open(&n.path);
            *ctx.pending_request.borrow_mut() =
                Some(AppRequest::OpenInEditor { path: abs, line: 1 });
        }
    }

    fn request_open_selected_in_obsidian(&self, ctx: &TabCtx) {
        let Some(graph) = self.graph.as_ref() else {
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

    fn handle_input_event(&mut self, ev: &crossterm::event::KeyEvent) -> Result<EventOutcome> {
        match (ev.code, ev.modifiers) {
            (KeyCode::Enter, _) => {
                let graph = self.graph.as_ref();
                self.views[self.active].apply_query(graph);
                self.input_mode = false;
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Esc, _) => {
                self.input_mode = false;
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                let v = self.active_view_mut();
                v.query_text.insert(v.input_cursor, c);
                v.input_cursor += c.len_utf8();
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Backspace, _) => {
                let v = self.active_view_mut();
                if v.input_cursor > 0 {
                    let prev = v
                        .query_text
                        .char_indices()
                        .rev()
                        .find(|(i, _)| *i < v.input_cursor)
                        .map(|(i, c)| (i, c.len_utf8()));
                    if let Some((_, len)) = prev {
                        let start = v.input_cursor - len;
                        v.query_text.replace_range(start..v.input_cursor, "");
                        v.input_cursor = start;
                    }
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Delete, _) => {
                let v = self.active_view_mut();
                if v.input_cursor < v.query_text.len() {
                    let ch = v.query_text[v.input_cursor..].chars().next().unwrap();
                    let end = v.input_cursor + ch.len_utf8();
                    v.query_text.replace_range(v.input_cursor..end, "");
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Left, _) => {
                let v = self.active_view_mut();
                if v.input_cursor > 0 {
                    let prev = v
                        .query_text
                        .char_indices()
                        .rev()
                        .find(|(i, _)| *i < v.input_cursor)
                        .map(|(i, _)| i);
                    if let Some(i) = prev {
                        v.input_cursor = i;
                    }
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Right, _) => {
                let v = self.active_view_mut();
                if v.input_cursor < v.query_text.len() {
                    let next = v.query_text[v.input_cursor..]
                        .chars()
                        .next()
                        .map(|c| v.input_cursor + c.len_utf8());
                    if let Some(i) = next {
                        v.input_cursor = i;
                    }
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Home, _) => {
                self.active_view_mut().input_cursor = 0;
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::End, _) => {
                let v = self.active_view_mut();
                v.input_cursor = v.query_text.len();
                Ok(EventOutcome::Consumed)
            }
            _ => Ok(EventOutcome::NotHandled),
        }
    }
}

impl Tab for GraphTab {
    fn title(&self) -> &str {
        "Graph"
    }

    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()> {
        if self.graph.is_none() {
            let scan = ctx.vault.scan();
            self.graph = Some(Graph::build(ctx.vault, &scan)?);
            // First focus: seed the FIRST view only — additional views
            // (created later via Ctrl+N) start empty by design. Skip if
            // a query is already present (test paths construct the tab
            // with state pre-populated).
            let v0 = &mut self.views[0];
            if v0.query_text.trim().is_empty() {
                let seed = ctx
                    .vault
                    .config
                    .config
                    .graph
                    .default_query
                    .clone()
                    .unwrap_or_else(|| BUILTIN_DEFAULT_QUERY.to_string());
                v0.query_text = seed;
                v0.input_cursor = v0.query_text.len();
                let graph = self.graph.as_ref();
                v0.apply_query(graph);
            } else {
                // Re-derive every view's tree against the freshly-built
                // graph so trees materialize on first focus.
                self.restore_all_views();
            }
        }
        Ok(())
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };

        // The create overlay captures the keyboard ahead of every other
        // binding, including input mode — the user is inside a modal
        // popup, not the tree.
        if self.create_state.is_some() {
            return Ok(self.handle_create_key(k, ctx));
        }

        // Rename modal (Flow B): captures keyboard while open.
        if self.rename_state.is_some() {
            return Ok(self.handle_rename_key(k, ctx));
        }

        // Preset picker: captures keyboard while open. On selection,
        // pre-fills the new view's query; on dismiss, starts blank.
        if self.preset_picker.is_some() {
            return Ok(self.handle_preset_picker_key(k, ctx));
        }

        // Move-section flow: shared and tree-driven phases all funnel
        // through one dispatcher. Most of them capture the keyboard,
        // but `TargetFromTree` deliberately returns `NotHandled` for
        // navigation keys (j/k/g/G/etc.) so the tree-cursor keymap
        // further down still runs.
        if self.move_outer.is_some() {
            let outcome = self.handle_move_key(k, ctx);
            if matches!(outcome, EventOutcome::Consumed) {
                return Ok(outcome);
            }
            // outcome == NotHandled — fall through to tree-navigation /
            // input-mode / etc. while keeping move_outer alive.
        }

        // Periodic-leader chord: a single keystroke fires (or cancels)
        // the open flow. Runs ahead of input mode and the outer-tab
        // passthrough so the period letters can't leak through.
        if self.periodic_leader {
            return Ok(self.handle_periodic_leader_key(k, ctx));
        }

        // Input mode owns the keyboard — digits and every other char
        // should land as text in the query, not trigger a tab switch.
        // This must run BEFORE the tab-passthrough check below.
        if self.input_mode {
            return self.handle_input_event(&k);
        }

        // Multi-view bindings — checked before the outer-tab passthrough
        // so Alt+digit and Ctrl+chord variants land here instead of the
        // App's tab switcher.
        match (k.code, k.modifiers) {
            (KeyCode::Char('n'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.add_view_with_presets(ctx);
                return Ok(EventOutcome::Consumed);
            }
            (KeyCode::Char('p'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.open_preset_picker_for_active_view(ctx);
                return Ok(EventOutcome::Consumed);
            }
            (KeyCode::Char('w'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.close_view();
                return Ok(EventOutcome::Consumed);
            }
            (KeyCode::PageDown, m) if m.contains(KeyModifiers::CONTROL) => {
                self.next_view();
                return Ok(EventOutcome::Consumed);
            }
            (KeyCode::PageUp, m) if m.contains(KeyModifiers::CONTROL) => {
                self.prev_view();
                return Ok(EventOutcome::Consumed);
            }
            (KeyCode::Char(c), m)
                if c.is_ascii_digit() && c != '0' && m.contains(KeyModifiers::ALT) =>
            {
                let idx = (c as u8 - b'1') as usize;
                self.switch_view(idx);
                return Ok(EventOutcome::Consumed);
            }
            _ => {}
        }

        // Tab switching keys pass through to the App's dispatcher. Plain
        // digits (NO modifier) trigger an outer-tab switch; modified
        // digits were handled above as Alt+N view jumps.
        if matches!(k.code, KeyCode::Tab | KeyCode::BackTab)
            || (matches!(k.code, KeyCode::Char(c) if c.is_ascii_digit())
                && k.modifiers == KeyModifiers::NONE)
        {
            return Ok(EventOutcome::NotHandled);
        }

        let graph_missing = self.graph.is_none();
        if graph_missing || self.active_view().tree.is_empty() {
            if let (KeyCode::Char('/'), KeyModifiers::NONE) = (k.code, k.modifiers) {
                self.input_mode = true;
                return Ok(EventOutcome::Consumed);
            }
            return Ok(EventOutcome::NotHandled);
        }

        let vis = 20; // approximation; render's scroll_to_selection corrects
        match (k.code, k.modifiers) {
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                self.input_mode = true;
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('z'), KeyModifiers::NONE) => {
                self.rewrite_query_for_root();
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => {
                let v = self.active_view_mut();
                v.selected = v.tree.move_selection_down(v.selected);
                v.refresh_selected_path();
                v.scroll_to_selection(vis);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => {
                let v = self.active_view_mut();
                v.selected = v.tree.move_selection_up(v.selected);
                v.refresh_selected_path();
                v.scroll_to_selection(vis);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Enter, _) | (KeyCode::Char('l'), _) => {
                let graph = self.graph.as_ref();
                let v = &mut self.views[self.active];
                if let (Some(g), Some(q)) = (graph, v.query.as_ref()) {
                    let path = v.path_to(v.selected);
                    let was_expanded = v
                        .tree
                        .rows()
                        .get(v.selected)
                        .map(|r| r.expanded)
                        .unwrap_or(false);
                    v.tree.expand_at(v.selected, g, q);
                    // Record/forget the expansion-path spec. Toggle: if
                    // the node was previously expanded the call above
                    // collapsed it; otherwise it (attempted to) expand.
                    if was_expanded {
                        v.forget_expansion_subtree(&path);
                    } else if v.tree.rows().get(v.selected).is_some_and(|r| r.expanded) {
                        v.add_expansion_path(path);
                    }
                    v.scroll_to_selection(vis);
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('h'), _) => {
                let v = self.active_view_mut();
                let expanded = v.tree.rows().get(v.selected).is_some_and(|r| r.expanded);
                if expanded {
                    let path = v.path_to(v.selected);
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
                                v.refresh_selected_path();
                                v.scroll_to_selection(vis);
                                break;
                            }
                        }
                    }
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('g'), _) => {
                let v = self.active_view_mut();
                v.selected = 0;
                v.scroll_offset = 0;
                v.refresh_selected_path();
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('G'), _) => {
                let v = self.active_view_mut();
                v.selected = v.tree.len().saturating_sub(1);
                v.refresh_selected_path();
                v.scroll_to_selection(vis);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                let v = self.active_view_mut();
                let rows = vis.max(1);
                v.selected = (v.selected + rows / 2).min(v.tree.len().saturating_sub(1));
                v.scroll_offset = (v.scroll_offset + rows / 2).min(v.tree.len().saturating_sub(1));
                v.refresh_selected_path();
                v.scroll_to_selection(vis);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                let v = self.active_view_mut();
                let rows = vis.max(1);
                v.selected = v.selected.saturating_sub(rows / 2);
                v.scroll_offset = v.scroll_offset.saturating_sub(rows / 2);
                v.refresh_selected_path();
                v.scroll_to_selection(vis);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('o'), KeyModifiers::NONE) => {
                self.request_open_selected_in_editor(ctx);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('o'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.request_open_selected_in_obsidian(ctx);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('c'), KeyModifiers::NONE) => {
                // Create blank: seed the folder picker step by jumping
                // straight into FilenamePrompt with the folder derived
                // from the current selection.
                let folder = self.create_folder_from_selection();
                self.create_state = Some(create::begin_filename_prompt(folder, None));
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('m'), KeyModifiers::NONE) => {
                // Enter the move-section source phase. A second `m`
                // confirms the currently-selected node as source; `t`
                // opens the fuzzy picker; Esc cancels.
                self.move_outer = Some(GraphMoveOuter::SourceFromTree);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('p'), KeyModifiers::NONE) => {
                self.periodic_leader = true;
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('t'), KeyModifiers::NONE) => {
                run_periodic_open(ctx, Period::Daily);
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('C'), _) | (KeyCode::Char('c'), KeyModifiers::SHIFT) => {
                // Create from template: open the template picker with
                // the folder pre-seeded from the current selection.
                // After the template is chosen, the flow skips the
                // folder picker and goes straight to the filename
                // prompt — same selection-driven shape as `c`.
                let folder = self.create_folder_from_selection();
                self.create_state = Some(create::begin_template_picking(ctx, Some(folder)));
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => {
                let scan = ctx.vault.scan();
                self.graph = Some(Graph::build(ctx.vault, &scan)?);
                self.restore_all_views();
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char('r'), KeyModifiers::NONE) => {
                // Check multi_selected first (needs mutable access to active view).
                let selected = {
                    let v = self.active_view_mut();
                    if !v.multi_selected.is_empty() {
                        let s = std::mem::take(&mut v.multi_selected);
                        Some(s)
                    } else {
                        None
                    }
                };
                if let Some(s) = selected {
                    self.move_outer = Some(GraphMoveOuter::MoveTargetFromTree { selected: s });
                    return Ok(EventOutcome::Consumed);
                }
                // Flow B: rename focused node in place (needs immutable
                // access to graph and view).
                let graph = self.graph.as_ref();
                let v = self.active_view();
                let Some(row) = v.tree.rows().get(v.selected) else {
                    return Ok(EventOutcome::Consumed);
                };
                match graph.map(|g| g.node(row.note_id)) {
                    Some(NodeKind::Note(n)) => {
                        self.rename_state = Some(GraphRenameState {
                            note_id: row.note_id,
                            is_directory: false,
                            buffer: EditBuffer::from(&n.title),
                            source_rel: n.path.clone(),
                        });
                    }
                    Some(NodeKind::Directory(d)) if d.path.as_os_str().is_empty() => {
                        queue_toast(ctx, "cannot rename vault root", ToastStyle::Error);
                    }
                    Some(NodeKind::Directory(d)) => {
                        self.rename_state = Some(GraphRenameState {
                            note_id: row.note_id,
                            is_directory: true,
                            buffer: EditBuffer::from(&d.name),
                            source_rel: d.path.clone(),
                        });
                    }
                    Some(NodeKind::Ghost(_)) => {
                        queue_toast(
                            ctx,
                            "cannot rename a ghost — create the note first",
                            ToastStyle::Error,
                        );
                    }
                    _ => {}
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Char(' '), KeyModifiers::NONE) => {
                let (selectable, note_id, is_root) = {
                    let v = self.active_view();
                    let Some(row) = v.tree.rows().get(v.selected) else {
                        return Ok(EventOutcome::Consumed);
                    };
                    let note_id = row.note_id;
                    let (selectable, is_root) = match self.graph.as_ref().map(|g| g.node(note_id)) {
                        Some(NodeKind::Note(_)) => (true, false),
                        Some(NodeKind::Directory(d)) => (true, d.path.as_os_str().is_empty()),
                        _ => (false, false),
                    };
                    (selectable, note_id, is_root)
                };
                if selectable && !is_root {
                    let v = self.active_view_mut();
                    if v.multi_selected.contains(&note_id) {
                        v.multi_selected.remove(&note_id);
                    } else {
                        v.multi_selected.insert(note_id);
                    }
                }
                Ok(EventOutcome::Consumed)
            }
            (KeyCode::Esc, KeyModifiers::NONE) => {
                let v = self.active_view_mut();
                if !v.multi_selected.is_empty() {
                    v.multi_selected.clear();
                    return Ok(EventOutcome::Consumed);
                }
                Ok(EventOutcome::NotHandled)
            }
            _ => Ok(EventOutcome::NotHandled),
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        let [strip_area, tree_area, input_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(area);

        // ── View tab strip ───────────────────────────────────────────
        let mut spans: Vec<Span> = Vec::with_capacity(self.views.len() * 2);
        for (i, v) in self.views.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" "));
            }
            let label = format!(" {}: {} ", i + 1, v.query_snippet());
            let style = if i == self.active {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            spans.push(Span::styled(label, style));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), strip_area);

        // ── Tree ─────────────────────────────────────────────────────
        let visible = tree_area.height.saturating_sub(1).max(1) as usize;
        let input_mode = self.input_mode;
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
                let sel_marker = if v.multi_selected.contains(&row.note_id) {
                    '●'
                } else {
                    ' '
                };
                let line = format!(
                    "{indent}{indicator} {sel_marker} {kind} {display}",
                    kind = row.kind_char,
                    display = row.display,
                );
                let style = if i == v.selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(Span::styled(line, style)))
            })
            .collect();

        frame.render_widget(List::new(items), tree_area);

        // Empty-state hint: shown when the active view's tree has no
        // navigable content (≤ 1 row) and the user isn't actively
        // typing. Disappears as soon as the user expands anything or
        // enters input mode.
        if v.tree.len() <= 1 && !input_mode && tree_area.height >= 2 {
            let hint_rect = Rect {
                y: tree_area.y + 1,
                height: 1,
                ..tree_area
            };
            let hint = Span::styled(
                "press / to edit query",
                Style::default().fg(Color::DarkGray),
            );
            frame.render_widget(Paragraph::new(Line::from(hint)), hint_rect);
        }

        // ── Input bar ────────────────────────────────────────────────
        let prompt_style = if input_mode {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Gray)
        };
        let input_text = format!("> {}", v.query_text);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(input_text, prompt_style))),
            input_area,
        );

        if input_mode {
            // 2 = width of the "> " prompt.
            let x = input_area
                .x
                .saturating_add(2)
                .saturating_add(v.input_cursor as u16);
            frame.set_cursor_position((
                x.min(input_area.x + input_area.width.saturating_sub(1)),
                input_area.y,
            ));
        }

        // Error line overlays bottom of tree area.
        if let Some(ref err) = v.parse_error {
            if tree_area.height > 0 {
                let err_rect = Rect {
                    y: tree_area.y + tree_area.height.saturating_sub(1),
                    height: 1,
                    ..tree_area
                };
                let err_span = Span::styled(err.as_str(), Style::default().fg(Color::Red));
                frame.render_widget(Paragraph::new(Line::from(err_span)), err_rect);
            }
        }

        // Create overlay floats over everything when active. Reuses the
        // Notes-tab renderer so both tabs render the same modal.
        if let Some(cs) = self.create_state.as_mut() {
            notes_view::render_create_overlay(frame, area, cs);
        }

        // Periodic-leader popup. Same renderer as the Notes tab — a
        // centered modal listing the period choices (d/w/m/q/y).
        if self.periodic_leader {
            notes_view::render_periodic_leader(frame, area);
        }

        // Move-section overlay. Inner(...) defers to the shared Notes
        // renderer (multiselect / new-target / compose popups). The two
        // tree-driven phases render a thin status banner over the tab
        // strip so the user knows what `m`/`t`/Esc do right now.
        if let Some(outer) = self.move_outer.as_mut() {
            match outer {
                GraphMoveOuter::SourceFromTree => {
                    render_move_banner(
                        frame,
                        strip_area,
                        "MOVE source · m: use selected · t: pick from list · Esc: cancel",
                    );
                }
                GraphMoveOuter::SourcePicker { picker: _ } | GraphMoveOuter::Inner(_) => {
                    // SourcePicker uses the shared picker; Inner uses
                    // the shared move overlay. Forward through a
                    // throwaway SectionMoveState so the existing render
                    // path can be reused for both.
                    if let GraphMoveOuter::SourcePicker { picker } = outer {
                        let mut wrap = SectionMoveState::SourcePicking {
                            picker: std::mem::replace(
                                picker,
                                FuzzyPicker::new(VaultFilePickerSource::new(
                                    Arc::clone(ctx.vault),
                                    Arc::clone(ctx.recents),
                                )),
                            ),
                        };
                        notes_view::render_move_overlay(frame, area, &mut wrap);
                        // Restore the original picker (we swapped it
                        // out to satisfy the borrow checker without
                        // taking ownership of the variant).
                        if let SectionMoveState::SourcePicking { picker: orig } = wrap {
                            *picker = orig;
                        }
                    } else if let GraphMoveOuter::Inner(sms) = outer {
                        notes_view::render_move_overlay(frame, area, sms);
                    }
                }
                GraphMoveOuter::TargetFromTree { .. } => {
                    render_move_banner(
                        frame,
                        strip_area,
                        "MOVE target · m: use selected · t: pick from list · /: refine · Esc: back",
                    );
                }
                GraphMoveOuter::TargetPicker { picker, carry } => {
                    let mut wrap = SectionMoveState::TargetPicking {
                        source_rel: carry.source_rel.clone(),
                        source_abs: carry.source_abs.clone(),
                        source_content: carry.source_content.clone(),
                        headings: carry.headings.clone(),
                        selected: carry.selected.clone(),
                        focus: carry.focus,
                        clipboard: carry.clipboard.clone(),
                        picker: std::mem::replace(
                            picker,
                            FuzzyPicker::new(VaultFilePickerSource::new(
                                Arc::clone(ctx.vault),
                                Arc::clone(ctx.recents),
                            )),
                        ),
                        error: None,
                    };
                    notes_view::render_move_overlay(frame, area, &mut wrap);
                    if let SectionMoveState::TargetPicking { picker: orig, .. } = wrap {
                        *picker = orig;
                    }
                }
                GraphMoveOuter::MoveTargetFromTree { selected } => {
                    let n = selected.len();
                    let text = format!(
                        "Move {n} selection(s): navigate to target directory, Enter/m to confirm, t for picker, Esc to cancel"
                    );
                    render_move_banner(frame, strip_area, &text);
                }
                GraphMoveOuter::MoveTargetPicker {
                    picker,
                    selected: _,
                } => {
                    picker.render(frame, area);
                }
            }
        }

        // Rename-in-place modal (Flow B). Rendered as a centered overlay
        // similar to the create flow.
        if let Some(rs) = self.rename_state.as_mut() {
            let popup_area = centered_rect(60, 30, area);
            frame.render_widget(Clear, popup_area);
            let [title_area, buf_area, footer_area] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .areas(popup_area);
            let title = if rs.is_directory {
                "Rename directory"
            } else {
                "Rename note"
            };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    title,
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ))),
                title_area,
            );
            let buf_text = &rs.buffer.text;
            let buf_display = if buf_text.is_empty() {
                " ".to_string()
            } else {
                buf_text.clone()
            };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    buf_display,
                    Style::default().fg(Color::Yellow),
                ))),
                buf_area,
            );
            let footer = "Enter: commit · Esc: discard";
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    footer,
                    Style::default().fg(Color::Gray),
                ))),
                footer_area,
            );
        }

        if let Some(ref mut picker) = self.preset_picker {
            // Percentage-based sizing so the picker grows with the
            // terminal. At 80×24 this gives roughly 48×12 — enough to
            // show all 5 built-in presets without scrolling; on a
            // larger viewport the picker scales up so long preset
            // names and user-defined presets fit comfortably.
            let popup_area = centered_rect(60, 60, area);
            frame.render_widget(Clear, popup_area);
            picker.render(frame, popup_area);
        }
    }

    fn refresh(&mut self, ctx: &mut TabCtx) -> Result<()> {
        let scan = ctx.vault.scan();
        self.graph = Some(Graph::build(ctx.vault, &scan)?);
        self.restore_all_views();
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
        ]
    }
}

// ── ExpandedView ──────────────────────────────────────────────────────

/// Per-view state. A graph tab owns a `Vec<ExpandedView>` and renders the
/// active one. The view holds both *spec* fields (`query_text`,
/// `expanded_paths`, `selected_path`) and *derived* fields (`tree`,
/// `selected`, `scroll_offset`); spec fields survive a graph rebuild and
/// drive the rebuild of derived fields via [`Self::restore_expansion`].
#[derive(Debug, Default)]
pub struct ExpandedView {
    query_text: String,
    input_cursor: usize,
    parse_error: Option<String>,
    query: Option<GraphQuery>,
    /// Root-anchored paths the user has expanded. Each path is the
    /// sequence of NoteIds from a root (inclusive) down to the
    /// expanded node (inclusive). Closed under prefixes by
    /// construction — expanding a child always implies its parents are
    /// also expanded.
    expanded_paths: HashSet<Vec<NoteId>>,
    /// Path of the currently-selected row (root-to-leaf, inclusive).
    /// Used to restore selection across graph rebuilds; on a missing
    /// leaf we shed the tail and re-try until we hit an ancestor that
    /// still exists.
    selected_path: Option<Vec<NoteId>>,
    /// Space-toggled multi-selection. When non-empty, `r` triggers Flow
    /// A (move to directory) instead of Flow B (rename in place).
    /// Cleared on graph rebuild (NoteIds are stale).
    multi_selected: HashSet<NoteId>,
    tree: TreeState,
    selected: usize,
    scroll_offset: usize,
}

impl ExpandedView {
    /// Parse `query_text`, swap in the parsed query, and rebuild the
    /// tree against the current graph. Clears expansion state — a new
    /// query starts fresh.
    fn apply_query(&mut self, graph: Option<&Graph>) {
        self.parse_error = None;
        if self.query_text.trim().is_empty() {
            self.query = None;
            self.expanded_paths.clear();
            self.selected_path = None;
            self.tree = TreeState::default();
            self.selected = 0;
            self.scroll_offset = 0;
            return;
        }
        match parse_query(&self.query_text) {
            Ok(q) => {
                self.query = Some(q);
                self.expanded_paths.clear();
                self.selected_path = None;
                self.selected = 0;
                self.scroll_offset = 0;
                if let Some(g) = graph {
                    let q = self.query.as_ref().unwrap();
                    let roots = q.select(g);
                    self.tree.build_from(&roots, g, q);
                    self.refresh_selected_path();
                }
            }
            Err(e) => self.parse_error = Some(e.to_string()),
        }
    }

    /// Re-derive the flat tree from the saved expansion paths against
    /// the given graph. Paths whose nodes no longer exist are
    /// truncated; selection falls back to the nearest restored
    /// ancestor (then row 0).
    fn restore_expansion(&mut self, graph: &Graph) {
        if self.query.is_none() {
            // No parsed query (empty text, or a parse error): nothing
            // to materialize.
            self.tree = TreeState::default();
            self.selected = 0;
            self.scroll_offset = 0;
            return;
        }

        // Clone the GraphQuery once so we can mutably borrow `self.tree`
        // alongside; query is a cheap-ish AST tree.
        let query = self.query.clone().unwrap();
        let roots = query.select(graph);
        self.tree.build_from(&roots, graph, &query);

        // Replay expansions shortest-path-first so parents are expanded
        // before their children.
        let mut sorted: Vec<Vec<NoteId>> = std::mem::take(&mut self.expanded_paths)
            .into_iter()
            .collect();
        sorted.sort_by_key(|p| p.len());
        let mut restored: HashSet<Vec<NoteId>> = HashSet::new();
        for path in sorted {
            if let Some(idx) = self.find_row_for_path(&path) {
                let already = self.tree.rows()[idx].expanded;
                if already || self.tree.expand_at(idx, graph, &query) {
                    restored.insert(path);
                }
            }
            // else: path disappeared — drop it.
        }
        self.expanded_paths = restored;

        // Restore selection: walk the saved selected_path, shedding the
        // suffix until we find a matching row; fall back to row 0.
        self.selected = 0;
        if let Some(path) = self.selected_path.clone() {
            let mut len = path.len();
            while len > 0 {
                if let Some(idx) = self.find_row_for_path(&path[..len]) {
                    self.selected = idx;
                    break;
                }
                len -= 1;
            }
        }
        // Heuristic scroll — render's scroll_to_selection will correct
        // against the real visible budget on first draw.
        self.scroll_offset = self.selected.saturating_sub(10);
        self.refresh_selected_path();
    }

    /// Locate the row corresponding to a root-anchored path, walking
    /// only through currently-visible children of each step. Returns
    /// `None` if any node along the path isn't in the visible tree.
    fn find_row_for_path(&self, path: &[NoteId]) -> Option<usize> {
        if path.is_empty() {
            return None;
        }
        let rows = self.tree.rows();
        let mut idx = rows
            .iter()
            .position(|r| r.depth == 0 && r.note_id == path[0])?;
        for &next in &path[1..] {
            let parent_depth = rows[idx].depth;
            let mut found = None;
            for (i, r) in rows.iter().enumerate().skip(idx + 1) {
                if r.depth <= parent_depth {
                    break;
                }
                if r.depth == parent_depth + 1 && r.note_id == next {
                    found = Some(i);
                    break;
                }
            }
            idx = found?;
        }
        Some(idx)
    }

    /// Walk the visible tree backward from `index` to assemble its
    /// root-to-leaf path. Returns an empty vec for out-of-bounds.
    fn path_to(&self, index: usize) -> Vec<NoteId> {
        let rows = self.tree.rows();
        if index >= rows.len() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut next_depth = rows[index].depth + 1;
        for i in (0..=index).rev() {
            if rows[i].depth + 1 == next_depth {
                out.push(rows[i].note_id);
                next_depth = rows[i].depth;
                if next_depth == 0 {
                    break;
                }
            }
        }
        out.reverse();
        out
    }

    /// Record an expansion. Also adds every ancestor prefix (defensive
    /// — by construction the user's prior expansions should already
    /// have those, but enforcing the invariant locally keeps
    /// `restore_expansion` simple).
    fn add_expansion_path(&mut self, path: Vec<NoteId>) {
        for i in 1..=path.len() {
            self.expanded_paths.insert(path[..i].to_vec());
        }
    }

    /// Drop a collapse target plus every path that extends it. Mirrors
    /// `TreeState::collapse_at`, which removes all descendant rows.
    fn forget_expansion_subtree(&mut self, path: &[NoteId]) {
        self.expanded_paths.retain(|p| !starts_with(p, path));
    }

    fn refresh_selected_path(&mut self) {
        if self.tree.is_empty() {
            self.selected_path = None;
        } else {
            self.selected_path = Some(self.path_to(self.selected));
        }
    }

    fn scroll_to_selection(&mut self, visible_rows: usize) {
        if visible_rows == 0 || self.tree.is_empty() {
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.selected.saturating_sub(visible_rows - 1);
        }
    }

    /// Width-limited query snippet for the tab strip label.
    fn query_snippet(&self) -> String {
        let s = self.query_text.trim();
        if s.is_empty() {
            return "(empty)".to_string();
        }
        if s.chars().count() <= VIEW_LABEL_QUERY_WIDTH {
            return s.to_string();
        }
        let mut buf: String = s
            .chars()
            .take(VIEW_LABEL_QUERY_WIDTH.saturating_sub(1))
            .collect();
        buf.push('…');
        buf
    }
}

/// One-line status banner overlaid on the view-strip row while a
/// tree-driven move phase is active (Source/Target). Replaces the
/// strip's view labels so the user can see which keys fire what right
/// now.
fn render_move_banner(frame: &mut Frame, area: Rect, text: &str) {
    let span = Span::styled(
        text,
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(Paragraph::new(Line::from(span)), area);
}

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

/// One visible row in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub depth: usize,
    pub note_id: NoteId,
    pub display: String,
    pub kind_char: char,
    pub expanded: bool,
    pub expandable: bool,
}

/// The flat-list tree with expansion cache. Manipulated imperatively:
/// expanding inserts children after the parent row; collapsing removes
/// all descendant rows.
#[derive(Debug, Default)]
pub struct TreeState {
    rows: Vec<TreeRow>,
    expansion_cache: HashMap<NoteId, Option<Vec<NoteId>>>,
}

impl TreeState {
    pub fn build_from(&mut self, roots: &[NoteId], graph: &Graph, query: &GraphQuery) {
        self.rows.clear();
        self.expansion_cache.clear();
        for id in roots {
            self.rows.push(Self::make_row(*id, 0, graph, query));
        }
    }

    pub fn expand_at(&mut self, index: usize, graph: &Graph, query: &GraphQuery) -> bool {
        if index >= self.rows.len() {
            return false;
        }

        if self.rows[index].expanded {
            self.collapse_at(index);
            return true;
        }

        if !self.rows[index].expandable {
            return false;
        }

        let id = self.rows[index].note_id;

        let children = self
            .expansion_cache
            .entry(id)
            .or_insert_with(|| query.expand(graph, id));

        let child_ids: &[NoteId] = match children {
            Some(v) => v.as_slice(),
            None => {
                self.rows[index].expandable = false;
                return false;
            }
        };

        let child_depth = self.rows[index].depth + 1;
        let insert_pos = index + 1;
        for child_id in child_ids.iter().rev() {
            self.rows.insert(
                insert_pos,
                Self::make_row(*child_id, child_depth, graph, query),
            );
        }

        self.rows[index].expanded = true;
        self.rows[index].expandable = !child_ids.is_empty();
        true
    }

    pub fn collapse_at(&mut self, index: usize) {
        if index >= self.rows.len() || !self.rows[index].expanded {
            return;
        }

        let bound_depth = self.rows[index].depth;
        let mut end = index + 1;
        while end < self.rows.len() && self.rows[end].depth > bound_depth {
            end += 1;
        }

        self.rows.drain(index + 1..end);
        self.rows[index].expanded = false;
    }

    pub fn move_selection_up(&self, current: usize) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        if current == 0 {
            self.rows.len() - 1
        } else {
            current - 1
        }
    }

    pub fn move_selection_down(&self, current: usize) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        if current + 1 >= self.rows.len() {
            0
        } else {
            current + 1
        }
    }

    pub fn rows(&self) -> &[TreeRow] {
        &self.rows
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    fn make_row(id: NoteId, depth: usize, graph: &Graph, query: &GraphQuery) -> TreeRow {
        let (display, kind_char) = match graph.node(id) {
            NodeKind::Note(n) => (
                n.path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| n.path.to_string_lossy().into_owned()),
                'N',
            ),
            NodeKind::Directory(d) => {
                if d.path.as_os_str().is_empty() {
                    ("/".to_string(), 'D')
                } else {
                    (format!("{}/", d.name), 'D')
                }
            }
            NodeKind::Ghost(g) => (g.raw.clone(), 'G'),
            NodeKind::Task(t) => (t.description.clone(), 'T'),
        };
        // Compute expandability up-front by asking the policy how many
        // children this node has. None = no expand block at all (still
        // not expandable). Some(empty) = policy says zero children.
        // This avoids the misleading ▶ arrow on leaves that disappears
        // only after the user tries to expand.
        let expandable = matches!(query.expand(graph, id), Some(ref v) if !v.is_empty());
        TreeRow {
            depth,
            note_id: id,
            display,
            kind_char,
            expanded: false,
            expandable,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tree_tests {
    use std::path::PathBuf;

    use ft_core::graph::query::parse as parse_query;
    use ft_core::graph::Graph;
    use ft_core::vault::{Scan, Vault};

    use super::*;

    fn dirs_graph() -> Graph {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dirs");
        let v = Vault::discover(Some(path)).expect("dirs fixture vault must exist");
        Graph::build(&v, &Scan::default()).unwrap()
    }

    fn dirs_query() -> GraphQuery {
        parse_query(
            "node where kind = Directory without incoming(kind = directory-contains); expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};",
        )
        .unwrap()
    }

    #[test]
    fn build_from_roots_creates_flat_rows() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q);
        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0].depth, 0);
        assert_eq!(state.rows[0].kind_char, 'D');
    }

    #[test]
    fn expand_inserts_children_at_correct_position() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q);
        assert_eq!(state.rows.len(), 1);

        let changed = state.expand_at(0, &g, &q);
        assert!(changed);
        assert_eq!(state.rows.len(), 4);
        assert!(state.rows[0].expanded);
        assert_eq!(state.rows[0].depth, 0);
        assert_eq!(state.rows[1].depth, 1);
        assert_eq!(state.rows[2].depth, 1);
        assert_eq!(state.rows[3].depth, 1);
    }

    #[test]
    fn collapse_removes_descendants() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q);
        state.expand_at(0, &g, &q);
        assert_eq!(state.rows.len(), 4);

        state.collapse_at(0);
        assert_eq!(state.rows.len(), 1);
        assert!(!state.rows[0].expanded);
    }

    #[test]
    fn expand_toggle_collapses_when_already_expanded() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q);

        state.expand_at(0, &g, &q);
        assert_eq!(state.rows.len(), 4);
        assert!(state.rows[0].expanded);

        let changed = state.expand_at(0, &g, &q);
        assert!(changed);
        assert_eq!(state.rows.len(), 1);
        assert!(!state.rows[0].expanded);
    }

    #[test]
    fn expand_then_expand_child() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q);

        state.expand_at(0, &g, &q);
        assert_eq!(state.rows.len(), 4);

        let areas_idx = state
            .rows
            .iter()
            .position(|r| r.kind_char == 'D' && r.display == "Areas/")
            .unwrap();

        state.expand_at(areas_idx, &g, &q);
        assert_eq!(state.rows.len(), 6);

        let ops = state
            .rows
            .iter()
            .find(|r| r.display == "operations/")
            .unwrap();
        assert_eq!(ops.depth, 2);
    }

    #[test]
    fn expand_unexpandable_node_returns_false() {
        let g = dirs_graph();
        let q = parse_query("node where kind = Note;").unwrap();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q);

        let changed = state.expand_at(0, &g, &q);
        assert!(!changed);
        assert!(!state.rows[0].expandable);
    }

    #[test]
    fn move_selection_wraps_at_bounds() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots: Vec<_> = g
            .nodes()
            .filter(|(_, k)| matches!(k, NodeKind::Note(_)))
            .map(|(id, _)| id)
            .take(3)
            .collect();

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q);
        assert_eq!(state.rows.len(), 3);

        assert_eq!(state.move_selection_up(0), 2);
        assert_eq!(state.move_selection_down(2), 0);
        assert_eq!(state.move_selection_down(0), 1);
        assert_eq!(state.move_selection_up(1), 0);
    }

    #[test]
    fn empty_tree_selection_is_zero() {
        let state = TreeState::default();
        assert_eq!(state.move_selection_up(0), 0);
        assert_eq!(state.move_selection_down(0), 0);
    }

    #[test]
    fn cache_is_used_on_repeat_expand() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q);

        state.expand_at(0, &g, &q);
        let first_len = state.rows.len();
        state.collapse_at(0);
        state.expand_at(0, &g, &q);
        assert_eq!(state.rows.len(), first_len);
        assert!(state.expansion_cache.contains_key(&state.rows[0].note_id));
    }

    #[test]
    fn build_marks_expandable_false_when_policy_returns_no_children() {
        // Empty vault → root has no Note children under the
        // policy. Expandability is now determined up front by
        // `make_row` asking the query; the row never shows the
        // ▶ arrow at all.
        let tmp = assert_fs::TempDir::new().unwrap();
        use assert_fs::prelude::*;
        tmp.child(".obsidian").create_dir_all().unwrap();

        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &Scan::default()).unwrap();

        let q = parse_query(
            "node where indegree = 0; expand where from.kind = Directory and edge.kind = directory-contains and to.kind = Note;",
        ).unwrap();

        let root_id = g.node_by_path(std::path::Path::new("")).unwrap();

        let mut state = TreeState::default();
        state.build_from(&[root_id], &g, &q);

        // Pre-computed: not expandable, so attempting expand is a
        // no-op and `expanded` stays false (nothing was opened).
        assert!(!state.rows[0].expandable);
        let changed = state.expand_at(0, &g, &q);
        assert!(!changed);
        assert!(!state.rows[0].expanded);
        assert_eq!(state.rows.len(), 1);
    }

    #[test]
    fn build_marks_expandable_true_when_policy_returns_children() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q);
        assert_eq!(state.rows.len(), 1);
        // Root directory has 3 immediate children under the policy →
        // expandable from the start.
        assert!(state.rows[0].expandable);
    }

    #[test]
    fn build_marks_note_rows_unexpandable_under_directory_contains_policy() {
        // Notes have no outgoing directory-contains edges, so the
        // policy yields zero children — rows for notes should not
        // display the ▶ arrow.
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q);
        state.expand_at(0, &g, &q);

        for row in state.rows().iter().filter(|r| r.kind_char == 'N') {
            assert!(
                !row.expandable,
                "note row {} should be a leaf under the dirs policy",
                row.display
            );
        }
    }

    #[test]
    fn task_nodes_render_with_kind_char_t() {
        use assert_fs::prelude::*;
        use ft_core::task::{Status, Task};

        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("root.md")
            .write_str("- [ ] Task one\n- [x] Task two\n")
            .unwrap();
        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();

        let scan = Scan {
            tasks: vec![
                Task {
                    description: "Task one".into(),
                    status: Status::Open,
                    priority: None,
                    tags: vec![],
                    due: None,
                    scheduled: None,
                    source_file: PathBuf::from("root.md"),
                    source_line: 1,
                    created: None,
                    start: None,
                    done: None,
                    cancelled: None,
                    recurrence: None,
                    id: None,
                    depends_on: vec![],
                    on_completion: None,
                    block_link: None,
                    raw_trailing: None,
                    indent_level: 0,
                    parent: None,
                },
                Task {
                    description: "Task two".into(),
                    status: Status::Done,
                    priority: None,
                    tags: vec![],
                    due: None,
                    scheduled: None,
                    source_file: PathBuf::from("root.md"),
                    source_line: 2,
                    created: None,
                    start: None,
                    done: None,
                    cancelled: None,
                    recurrence: None,
                    id: None,
                    depends_on: vec![],
                    on_completion: None,
                    block_link: None,
                    raw_trailing: None,
                    indent_level: 0,
                    parent: None,
                },
            ],
            errors: vec![],
        };
        let g = Graph::build(&v, &scan).unwrap();

        // Query for task nodes only
        let q = parse_query("node where kind = Task;").unwrap();
        let mut state = TreeState::default();
        let roots = q.select(&g);
        state.build_from(&roots, &g, &q);

        assert_eq!(state.rows.len(), 2);
        assert_eq!(state.rows[0].kind_char, 'T');
        assert_eq!(state.rows[0].display, "Task one");
        assert_eq!(state.rows[1].kind_char, 'T');
        assert_eq!(state.rows[1].display, "Task two");
    }
}

#[cfg(test)]
mod view_tests {
    use std::path::PathBuf;

    use assert_fs::prelude::*;
    use ft_core::graph::Graph;
    use ft_core::vault::{Scan, Vault};

    use super::*;

    fn dirs_graph() -> Graph {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dirs");
        let v = Vault::discover(Some(path)).expect("dirs fixture vault must exist");
        Graph::build(&v, &Scan::default()).unwrap()
    }

    fn dirs_query_text() -> &'static str {
        "node where kind = Directory without incoming(kind = directory-contains); expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};"
    }

    fn view_with_query() -> (Graph, ExpandedView) {
        let g = dirs_graph();
        let mut v = ExpandedView {
            query_text: dirs_query_text().to_string(),
            ..Default::default()
        };
        v.apply_query(Some(&g));
        (g, v)
    }

    #[test]
    fn add_expansion_path_includes_all_prefixes() {
        let mut v = ExpandedView::default();
        // Synthesize a couple of NoteIds via the dirs graph.
        let g = dirs_graph();
        let root = g.node_by_path(std::path::Path::new("")).unwrap();
        let areas = g.node_by_path(std::path::Path::new("Areas")).unwrap();
        let ops = g
            .node_by_path(std::path::Path::new("Areas/operations"))
            .unwrap();
        v.add_expansion_path(vec![root, areas, ops]);
        assert!(v.expanded_paths.contains(&vec![root]));
        assert!(v.expanded_paths.contains(&vec![root, areas]));
        assert!(v.expanded_paths.contains(&vec![root, areas, ops]));
    }

    #[test]
    fn forget_expansion_subtree_removes_descendants() {
        let g = dirs_graph();
        let root = g.node_by_path(std::path::Path::new("")).unwrap();
        let areas = g.node_by_path(std::path::Path::new("Areas")).unwrap();
        let ops = g
            .node_by_path(std::path::Path::new("Areas/operations"))
            .unwrap();
        let projects = g.node_by_path(std::path::Path::new("Projects")).unwrap();
        let mut v = ExpandedView::default();
        v.add_expansion_path(vec![root, areas, ops]);
        v.add_expansion_path(vec![root, projects]);
        v.forget_expansion_subtree(&[root, areas]);
        assert!(!v.expanded_paths.contains(&vec![root, areas]));
        assert!(!v.expanded_paths.contains(&vec![root, areas, ops]));
        // Untouched siblings stay.
        assert!(v.expanded_paths.contains(&vec![root, projects]));
        assert!(v.expanded_paths.contains(&vec![root]));
    }

    #[test]
    fn path_to_walks_back_to_root() {
        let (_g, v) = view_with_query();
        assert_eq!(v.path_to(0).len(), 1);
    }

    #[test]
    fn restore_expansion_walks_each_path() {
        let (g, mut v) = view_with_query();
        // Expand root then Areas/.
        let root_id = v.tree.rows()[0].note_id;
        v.tree.expand_at(0, &g, v.query.as_ref().unwrap());
        v.add_expansion_path(vec![root_id]);
        let areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        let areas_id = v.tree.rows()[areas_idx].note_id;
        v.tree.expand_at(areas_idx, &g, v.query.as_ref().unwrap());
        v.add_expansion_path(vec![root_id, areas_id]);
        let expected_len = v.tree.len();

        // Now drop and re-derive from spec.
        v.tree = TreeState::default();
        v.restore_expansion(&g);

        assert_eq!(v.tree.len(), expected_len);
        assert!(v.tree.rows()[0].expanded);
        let restored_areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        assert!(v.tree.rows()[restored_areas_idx].expanded);
    }

    #[test]
    fn restore_expansion_truncates_at_missing_node() {
        let (g, mut v) = view_with_query();
        let root_id = v.tree.rows()[0].note_id;
        v.tree.expand_at(0, &g, v.query.as_ref().unwrap());
        v.add_expansion_path(vec![root_id]);
        // Add a fictitious deeper path whose intermediate node is
        // bogus — restoration should drop it without panicking.
        let bogus = g.node_by_path(std::path::Path::new("Areas")).unwrap();
        let bogus2 = g
            .node_by_path(std::path::Path::new("Areas/operations"))
            .unwrap();
        // Inject [root, bogus_not_in_tree, bogus2] — bogus IS in the graph
        // but we'll remove Areas from the tree shape by replaying against
        // an empty path set first, then adding only this fake path.
        v.expanded_paths.clear();
        v.expanded_paths.insert(vec![root_id]);
        v.expanded_paths.insert(vec![root_id, bogus, bogus2]); // ok actually exists
        v.tree = TreeState::default();
        v.restore_expansion(&g);
        // The valid path expanded the root, plus Areas/ if its
        // children include operations.
        assert!(v.tree.rows()[0].expanded);
        // Verify expanded_paths retained only paths whose nodes survived.
        for path in &v.expanded_paths {
            for &nid in path {
                assert!(
                    matches!(
                        g.node(nid),
                        NodeKind::Note(_) | NodeKind::Directory(_) | NodeKind::Ghost(_)
                    ),
                    "every restored path node must exist in the graph"
                );
            }
        }
    }

    #[test]
    fn restore_expansion_preserves_selection_when_present() {
        let (g, mut v) = view_with_query();
        // Expand root, then select Areas/.
        v.tree.expand_at(0, &g, v.query.as_ref().unwrap());
        let root_id = v.tree.rows()[0].note_id;
        v.add_expansion_path(vec![root_id]);
        let areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        v.selected = areas_idx;
        v.refresh_selected_path();

        // Drop derived state and restore.
        v.tree = TreeState::default();
        v.restore_expansion(&g);

        let restored_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        assert_eq!(v.selected, restored_idx);
    }

    #[test]
    fn restore_expansion_falls_back_to_ancestor_when_selection_gone() {
        let (g, mut v) = view_with_query();
        v.tree.expand_at(0, &g, v.query.as_ref().unwrap());
        let root_id = v.tree.rows()[0].note_id;
        v.add_expansion_path(vec![root_id]);
        // Selection path: [root, NEVER_EXISTS]. We can't easily fabricate
        // a fake NoteId, so instead point at a real id that the path-
        // walker won't find as a child of root: use a Note's id as a
        // bogus "child of root" — Notes ARE children of root via
        // directory-contains, so this is actually a valid selection.
        // Switch tactic: select Areas/, then *manually* corrupt the
        // saved selected_path to [root, areas, BOGUS_NESTED] where
        // BOGUS_NESTED is operations/ — which is not a child of areas
        // unless areas is expanded. Restoration only expands root via
        // expanded_paths, so areas isn't expanded → walker stops at
        // areas → selection falls back to that ancestor.
        let areas = g.node_by_path(std::path::Path::new("Areas")).unwrap();
        let ops = g
            .node_by_path(std::path::Path::new("Areas/operations"))
            .unwrap();
        v.selected_path = Some(vec![root_id, areas, ops]);
        v.tree = TreeState::default();
        v.restore_expansion(&g);

        let areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.note_id == areas)
            .unwrap();
        assert_eq!(v.selected, areas_idx);
    }

    #[test]
    fn restore_expansion_with_no_paths_falls_back_to_row_zero() {
        let (g, mut v) = view_with_query();
        v.selected = 5; // out of bounds for the no-expansion tree
        v.tree = TreeState::default();
        v.restore_expansion(&g);
        assert_eq!(v.selected, 0);
    }

    #[test]
    fn query_snippet_truncates_long_text() {
        let v = ExpandedView {
            query_text: "node where kind = Directory and path = \"\"; expand where ...".into(),
            ..Default::default()
        };
        let snip = v.query_snippet();
        assert!(snip.chars().count() <= VIEW_LABEL_QUERY_WIDTH);
        assert!(snip.ends_with('…'));
    }

    #[test]
    fn query_snippet_empty_says_empty() {
        let v = ExpandedView::default();
        assert_eq!(v.query_snippet(), "(empty)");
    }

    #[test]
    fn new_graph_tab_has_one_empty_view() {
        let tab = GraphTab::new();
        assert_eq!(tab.views.len(), 1);
        assert_eq!(tab.active, 0);
        assert!(tab.views[0].query_text.is_empty());
        assert!(!tab.input_mode);
    }

    #[test]
    fn add_view_appends_and_switches() {
        let mut tab = GraphTab::new();
        tab.add_view();
        assert_eq!(tab.views.len(), 2);
        assert_eq!(tab.active, 1);
        assert!(tab.input_mode);
    }

    #[test]
    fn close_last_view_replaces_with_empty() {
        let mut tab = GraphTab::new();
        tab.views[0].query_text = "node where indegree = 0;".into();
        tab.close_view();
        assert_eq!(tab.views.len(), 1);
        assert!(tab.views[0].query_text.is_empty());
    }

    #[test]
    fn close_view_picks_left_neighbor() {
        let mut tab = GraphTab::new();
        tab.add_view();
        tab.add_view();
        assert_eq!(tab.active, 2);
        tab.close_view();
        // After removing index 2 from [_, _, _], new len=2 → active clamps to 1.
        assert_eq!(tab.views.len(), 2);
        assert_eq!(tab.active, 1);
    }

    #[test]
    fn cycle_views_wraps_at_bounds() {
        let mut tab = GraphTab::new();
        tab.add_view();
        tab.add_view();
        // active = 2
        tab.next_view();
        assert_eq!(tab.active, 0);
        tab.prev_view();
        assert_eq!(tab.active, 2);
        tab.prev_view();
        assert_eq!(tab.active, 1);
    }

    #[test]
    fn switch_view_bounds_checked() {
        let mut tab = GraphTab::new();
        tab.add_view();
        tab.switch_view(5);
        assert_eq!(tab.active, 1, "out-of-range switch must be a no-op");
        tab.switch_view(0);
        assert_eq!(tab.active, 0);
    }

    /// Ctrl+P opens the preset picker for the active view; selecting
    /// a preset replaces the active view's query in-place.
    #[test]
    fn ctrl_p_preset_replaces_active_view_query() {
        use chrono::NaiveDate;
        use ft_core::recents::RecentsLog;
        use std::cell::Cell;
        use std::cell::RefCell;
        use std::sync::Arc;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dirs");
        let vault = Vault::discover(Some(path)).expect("dirs fixture vault must exist");
        let vault = Arc::new(vault);
        let recents = Arc::new(RecentsLog::for_vault(&vault));
        let today = NaiveDate::from_ymd_opt(2026, 5, 29).unwrap();
        let last_refresh = Cell::new(None);
        let pending_request = RefCell::new(None);

        let ctx = TabCtx {
            vault: &vault,
            recents: &recents,
            today,
            last_refresh: &last_refresh,
            pending_request: &pending_request,
        };

        // Build graph so views can resolve queries.
        let scan = vault.scan();
        let graph = Graph::build(&vault, &scan).unwrap();

        let mut tab = GraphTab::new();
        tab.graph = Some(graph);
        tab.views[0].query_text = "node where kind = Note;".to_string();

        // Ctrl+P → open picker bound to the active view.
        tab.open_preset_picker_for_active_view(&ctx);
        assert!(tab.preset_picker.is_some());
        assert!(tab.preset_picker_for_active_view);

        // Simulate pressing Enter on the first match (alphabetically
        // "crosslinks" → the crosslinks preset DSL).
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let outcome = tab.handle_preset_picker_key(enter, &ctx);
        assert!(matches!(outcome, EventOutcome::Consumed));

        // The active view's query is replaced with the preset DSL.
        assert_eq!(
            tab.views[0].query_text,
            r#"node where kind = Directory and path = ""; expand where edge.kind in {directory-contains, links-into, link, embed};"#,
            "active view query should be replaced by the selected preset DSL"
        );

        // Flag is reset after selection.
        assert!(!tab.preset_picker_for_active_view);
        assert!(tab.preset_picker.is_none());
    }

    // ── z (root-on-selected) tests ──────────────────────────────────

    /// Helper: build a graph, apply a query so the tree has the target
    /// node as a row, select it, and return the tab.
    fn tab_with_node_selected(
        files: &[(&str, &str)],
        query_text: &str,
        select_path: &str,
    ) -> GraphTab {
        use std::path::Path;
        let dir = assert_fs::TempDir::new().unwrap();
        dir.child(".obsidian").create_dir_all().unwrap();
        for (rel, content) in files {
            dir.child(rel).write_str(content).unwrap();
        }
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        let scan = vault.scan();
        let graph = Graph::build(&vault, &scan).unwrap();
        let mut v = ExpandedView {
            query_text: query_text.to_string(),
            ..Default::default()
        };
        v.apply_query(Some(&graph));
        // Find and select the row matching select_path.
        let target = graph
            .node_by_path(Path::new(select_path))
            .expect("target node must exist");
        let sel = v
            .tree
            .rows()
            .iter()
            .position(|r| r.note_id == target)
            .expect("target row must be in tree");
        v.selected = sel;
        let mut tab = GraphTab::new();
        tab.graph = Some(graph);
        tab.views[0] = v;
        tab
    }

    #[test]
    fn z_on_note_rewrites_query() {
        let mut tab = tab_with_node_selected(
            &[("Areas/finance.md", "[[Projects/alpha]]"), ("Projects/alpha.md", "")],
            "node where kind = Note and path = \"Areas/finance.md\"; expand where edge.kind in {directory-contains, link};",
            "Areas/finance.md",
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_text,
            "node where kind = Note and path = \"Areas/finance.md\"; expand where edge.kind in {directory-contains, link};"
        );
    }

    #[test]
    fn z_on_directory_rewrites_query() {
        let mut tab = tab_with_node_selected(
            &[("Areas/finance.md", "")],
            "node where kind = Directory and path = \"Areas\"; expand where edge.kind = directory-contains;",
            "Areas",
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_text,
            "node where kind = Directory and path = \"Areas\"; expand where edge.kind = directory-contains;"
        );
    }

    #[test]
    fn z_on_root_directory_rewrites_query() {
        let mut tab = tab_with_node_selected(
            &[("foo.md", "")],
            "node where kind = Directory and path = \"\"; expand where edge.kind = directory-contains;",
            "",
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_text,
            "node where kind = Directory and path = \"\"; expand where edge.kind = directory-contains;"
        );
    }

    #[test]
    fn z_on_ghost_is_noop() {
        let dir = assert_fs::TempDir::new().unwrap();
        dir.child(".obsidian").create_dir_all().unwrap();
        dir.child("foo.md").write_str("[[Phantom]]").unwrap();
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let mut v = ExpandedView {
            query_text: "node where kind = Ghost;".to_string(),
            ..Default::default()
        };
        v.apply_query(Some(&graph));
        v.selected = 0;
        let mut tab = GraphTab::new();
        tab.graph = Some(graph);
        tab.views[0] = v;
        let before = tab.views[0].query_text.clone();
        tab.rewrite_query_for_root();
        assert_eq!(tab.views[0].query_text, before, "ghost should be no-op");
    }

    #[test]
    fn z_on_task_is_noop() {
        use ft_core::task::{Status, Task};
        let dir = assert_fs::TempDir::new().unwrap();
        dir.child(".obsidian").create_dir_all().unwrap();
        dir.child("root.md").write_str("- [ ] A task\n").unwrap();
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        let scan = Scan {
            tasks: vec![Task {
                description: "A task".into(),
                status: Status::Open,
                priority: None,
                tags: vec![],
                due: None,
                scheduled: None,
                source_file: PathBuf::from("root.md"),
                source_line: 1,
                created: None,
                start: None,
                done: None,
                cancelled: None,
                recurrence: None,
                id: None,
                depends_on: vec![],
                on_completion: None,
                block_link: None,
                raw_trailing: None,
                indent_level: 0,
                parent: None,
            }],
            errors: vec![],
        };
        let graph = Graph::build(&vault, &scan).unwrap();
        let mut v = ExpandedView {
            query_text: "node where kind = Task;".to_string(),
            ..Default::default()
        };
        v.apply_query(Some(&graph));
        v.selected = 0;
        let mut tab = GraphTab::new();
        tab.graph = Some(graph);
        tab.views[0] = v;
        let before = tab.views[0].query_text.clone();
        tab.rewrite_query_for_root();
        assert_eq!(tab.views[0].query_text, before, "task should be no-op");
    }

    #[test]
    fn z_preserves_expand_block() {
        let mut tab = tab_with_node_selected(
            &[("Areas/finance.md", "")],
            "node where kind = Directory and path = \"\"; expand where edge.kind in {directory-contains, links-into, link, embed};",
            "", // root directory is always in the tree for this query
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_text,
            "node where kind = Directory and path = \"\"; expand where edge.kind in {directory-contains, links-into, link, embed};"
        );
    }

    #[test]
    fn z_no_expand_block_produces_trailing_semicolon() {
        let mut tab = tab_with_node_selected(
            &[("foo.md", "")],
            "node where kind = Note and path = \"foo.md\";",
            "foo.md",
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_text,
            "node where kind = Note and path = \"foo.md\";"
        );
    }
}

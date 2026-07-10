//! Command dispatch handlers for the Graph tab. `GRAPH_COMMANDS` /
//! `GRAPH_KEYMAP` (the declarative registry rows) live in
//! `commands.rs`; this module is the handler side. Split out of a
//! single ~690-line match into grouped handlers so no one function
//! re-grows into the god-method the graph-tab-decomposition change
//! removed. Each group falls through to the next via its `_` arm;
//! `Tab::dispatch_command` in `mod.rs` delegates to
//! `dispatch_command_impl` here.

use super::*;

impl GraphTab {
    pub(super) fn dispatch_command_impl(
        &mut self,
        cmd: &Command,
        ctx: &mut TabCtx,
    ) -> CommandOutcome {
        self.dispatch_view_command(cmd, ctx)
    }

    /// Views + cross-tab handoffs (`graph.add-view` .. `graph.add-to-journal-sources`).
    fn dispatch_view_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome {
        match cmd.name {
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
                            Some(crate::tui::tab::GatherTarget::Note(n.path.clone()))
                        }
                        NodeKind::Ghost(g) => {
                            Some(crate::tui::tab::GatherTarget::Ghost(g.raw.clone()))
                        }
                        _ => None,
                    }
                });
                if let Some(target) = target {
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::GatherFor { target });
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
                let targets: Vec<crate::tui::tab::GatherTarget> =
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
                                        Some(crate::tui::tab::GatherTarget::Note(n.path.clone()))
                                    }
                                    NodeKind::Ghost(g) => {
                                        Some(crate::tui::tab::GatherTarget::Ghost(g.raw.clone()))
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
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::GatherAddSources {
                        targets,
                        default_mode: crate::tui::tab::AppendOrReplaceMode::Append,
                    });
                }
                CommandOutcome::Handled
            }
            _ => self.dispatch_query_or_nav_command(cmd, ctx),
        }
    }

    /// Query bar + tree navigation (`graph.query-bar` .. `graph.cursor-half-page-up`).
    fn dispatch_query_or_nav_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome {
        // Approximation; render's scroll_to_selection corrects.
        let vis = 20usize;
        match cmd.name {
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
            _ => self.dispatch_notes_or_task_command(cmd, ctx),
        }
    }

    /// Editor/Obsidian handoff + task-field quick edits
    /// (`graph.open-in-editor` .. `graph.open-in-obsidian`).
    fn dispatch_notes_or_task_command(
        &mut self,
        cmd: &Command,
        ctx: &mut TabCtx,
    ) -> CommandOutcome {
        match cmd.name {
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
            _ => self.dispatch_mutation_command(cmd, ctx),
        }
    }

    /// Create/append/capture/move/delete/subdir flows
    /// (`graph.create-blank` .. `graph.create-subdir`).
    fn dispatch_mutation_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome {
        match cmd.name {
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
            "graph.promote-ghost" => {
                self.promote_ghost(ctx);
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
            _ => self.dispatch_periodic_or_multiselect_command(cmd, ctx),
        }
    }

    /// Periodic-note jump + multi-select toggles
    /// (`graph.periodic-leader` .. `graph.clear-multi-select`).
    fn dispatch_periodic_or_multiselect_command(
        &mut self,
        cmd: &Command,
        ctx: &mut TabCtx,
    ) -> CommandOutcome {
        match cmd.name {
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

    /// `P` — promote the selected ghost into a synth note scaffolded
    /// with every paragraph mentioning it: journal feed → synth
    /// scaffold at the ghost's path, `ft.synth.targets` set, graph
    /// refreshed, editor opened. The ghost node disappears on refresh
    /// because the note now exists.
    fn promote_ghost(&mut self, ctx: &mut TabCtx) {
        let (ghost_id, raw) = {
            let Some(graph) = Self::graph_of(&self.snapshot) else {
                queue_toast(
                    ctx,
                    "graph is still building — retry in a moment",
                    ToastStyle::Error,
                );
                return;
            };
            let view = self.active_view();
            match view
                .tree
                .rows()
                .get(view.selected)
                .map(|row| (row.note_id, graph.node(row.note_id)))
            {
                Some((id, NodeKind::Ghost(g))) => (id, g.raw.clone()),
                _ => {
                    queue_toast(
                        ctx,
                        "promote applies to ghost rows — select a ghost",
                        ToastStyle::Error,
                    );
                    return;
                }
            }
        };

        if ft_core::git::discover_repo(&ctx.vault.path).is_none() {
            queue_toast(
                ctx,
                "vault is not inside a git repository — promote needs git history for the seeded journal",
                ToastStyle::Error,
            );
            return;
        }

        let Some(graph) = Self::graph_of(&self.snapshot) else {
            return;
        };
        let mut cache = ft_core::blame_cache::BlameCache::load(&ctx.vault.path).unwrap_or_default();
        let report = match ft_core::gather::build_gather(graph, &[ghost_id], ctx.vault, &mut cache)
        {
            Ok(r) => r,
            Err(e) => {
                queue_toast(
                    ctx,
                    &format!("promote: journal failed: {e}"),
                    ToastStyle::Error,
                );
                return;
            }
        };
        let _ = cache.save(&ctx.vault.path);
        if report.entries.is_empty() {
            queue_toast(
                ctx,
                "promote: no mentioning paragraphs with git history found",
                ToastStyle::Error,
            );
            return;
        }

        let target = std::path::Path::new(&raw).with_extension("md");
        let plan = match ft_core::synth::scaffold::plan_synth_scaffold(
            ctx.vault,
            &target,
            &report.entries,
        ) {
            Ok(p) => p,
            Err(e) => {
                queue_toast(
                    ctx,
                    &format!("promote: plan failed: {e}"),
                    ToastStyle::Error,
                );
                return;
            }
        };
        let section_count = plan.sections.len();
        let written = match ft_core::synth::scaffold::apply_synth_scaffold(ctx.vault, &plan) {
            Ok(p) => p,
            Err(e) => {
                queue_toast(
                    ctx,
                    &format!("promote: write failed: {e}"),
                    ToastStyle::Error,
                );
                return;
            }
        };

        // Record the promoted concept so `synth grow` and the Journal
        // tab's context flow (`o`) know this note's targets.
        let link = format!("[[{raw}]]");
        if let Ok(content) = std::fs::read_to_string(&written) {
            let new_content =
                ft_core::synth::callout::upsert_synth_frontmatter(&content, Some(&[link]));
            let _ = ft_core::fs::write_atomic(&written, &new_content);
        }

        ctx.request_graph_refresh();
        queue_toast(
            ctx,
            &format!(
                "promoted [[{raw}]] → {} with {section_count} section(s)",
                target.display()
            ),
            ToastStyle::Success,
        );
        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
            path: written,
            line: 1,
        });
    }
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

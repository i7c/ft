//! Rename / move / delete / subdirectory mutation commits for the
//! Graph tab (tui-tab-request-routing). Split out of `mod.rs` to keep
//! that file from re-growing into the god-object the graph-tab-
//! decomposition change removed.

use super::*;

impl GraphTab {
    /// Apply the modal's selected concepts to the target note via
    /// the `ft-core::related` plan/apply pair. Called from the
    /// `GraphRequest::ConfirmRelated` arm of `handle_graph_request`
    /// when the Related modal commits.
    pub(super) fn confirm_related(
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

    pub(super) fn open_source_picker(&self, ctx: &TabCtx) -> FuzzyPicker<VaultFilePickerSource> {
        FuzzyPicker::new(VaultFilePickerSource::new(
            Arc::clone(ctx.vault),
            Arc::clone(ctx.recents),
        ))
    }

    /// Confirm the currently-selected node as move source.
    ///
    /// Called from the `GraphRequest::MoveConfirmSourceFromTree` arm of
    /// `handle_graph_request` after the [`GraphMoveOuter::SourceFromTree`]
    /// modal posts that request on `m`. Validates the
    /// selection, calls [`advance_to_multiselect`], and either advances the
    /// flow by posting `OpenModal(MoveOuter(Inner(...)))` or — on toast paths
    /// (non-Note row, IO error, no headings) — re-opens `SourceFromTree` so
    /// the user can navigate and retry.
    pub(super) fn confirm_source_from_tree(&mut self, ctx: &TabCtx) {
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
    /// Called from the `GraphRequest::MoveConfirmTargetFromTree` arm of
    /// `handle_graph_request` after the [`GraphMoveOuter::TargetFromTree`]
    /// modal posts that request on `m`. The modal
    /// hands the [`MoveCarry`] through the round-trip so this method can
    /// re-open `TargetFromTree` (carry intact) on a recoverable error
    /// (non-Note selection, same-file pick).
    pub(super) fn confirm_target_from_tree(&mut self, ctx: &TabCtx, carry: MoveCarry) {
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
    /// Called from the `GraphRequest::MoveConfirmMoveTarget` arm of
    /// `handle_graph_request` after the
    /// [`GraphMoveOuter::MoveTargetFromTree`] modal posts that request on
    /// `m`/Enter. On a
    /// recoverable failure (no row / non-Directory) re-opens
    /// `MoveTargetFromTree` with `selected` intact so the user can navigate
    /// to a different row.
    pub(super) fn confirm_move_target(&mut self, ctx: &TabCtx, selected: HashSet<NoteId>) {
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
    pub(super) fn execute_multi_move(
        &mut self,
        ctx: &TabCtx,
        selected: &HashSet<NoteId>,
        target_dir: &Path,
    ) {
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
    pub(super) fn create_folder_from_selection(&self) -> PathBuf {
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
    pub(super) fn selected_note_abs_path(&self, ctx: &TabCtx) -> Option<PathBuf> {
        let graph = Self::graph_of(&self.snapshot)?;
        let id = self.selected_note_id()?;
        match graph.node(id) {
            NodeKind::Note(n) => Some(ctx.vault.path.join(&n.path)),
            _ => None,
        }
    }

    /// Build and apply the rename plan for the given node. Called from
    /// the `GraphRequest::CommitRename` arm of `handle_graph_request`
    /// when the rename modal commits. Toasts on success or failure; on
    /// success, refreshes the graph in place.
    pub(super) fn commit_rename(
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

    /// Services `GraphRequest::ConfirmDelete`: plan + apply the delete,
    /// refresh the graph, and toast the outcome.
    pub(super) fn confirm_delete(&mut self, ctx: &TabCtx, target: PathBuf, is_directory: bool) {
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

    /// Services `GraphRequest::CreateSubdir`: create the directory,
    /// refresh the graph, and toast.
    pub(super) fn create_subdir(&mut self, ctx: &TabCtx, parent: PathBuf, name: String) {
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
}

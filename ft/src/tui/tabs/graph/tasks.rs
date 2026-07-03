//! Task-popup edit/create commit logic for the Graph tab
//! (graph-task-edit-modal, tui-tab-request-routing). Split out of
//! `mod.rs` to keep that file from re-growing into the god-object
//! the graph-tab-decomposition change removed.

use super::*;

impl GraphTab {
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
    pub(super) fn with_focused_task<F>(&mut self, ctx: &mut TabCtx, verb: &str, op: F) -> bool
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
    pub(super) fn restore_task_cursor(&mut self, anchor: &(std::path::PathBuf, usize)) {
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
    pub(super) fn rewrite_view_to_note_tasks(&mut self, ctx: &mut TabCtx) {
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
    pub(super) fn focused_task_anchor(&self) -> Option<(PathBuf, usize)> {
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
    pub(super) fn focused_seed_note(&self) -> Option<PathBuf> {
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
    pub(super) fn focused_task_edit_state(
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

    /// Services `GraphRequest::TaskEdit`: apply the validated popup
    /// fields via `ops::update_task_line`, refresh, restore cursor
    /// (graph-task-edit-modal §3.4).
    pub(super) fn task_edit(
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

    /// Services `GraphRequest::TaskCommitCreate`: resolve the target
    /// file + insertion position, write the new task via `ops::create_task`,
    /// then refresh the graph and land the cursor on it. Mirrors the
    /// Tasks-tab `submit_popup_new` (graph-task-edit-modal §4.3).
    pub(super) fn task_commit_create(
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
}

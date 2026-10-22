//! Modal state hosted by the Tasks tab: the task-preset picker, the
//! retag picker, and the task-move picker. Kept next to the tab because
//! each reaches `SearchView`/`Config` types and posts a
//! Tasks-targeted `AppRequest`. A parallel of the Graph tab's
//! `tabs/graph/modals.rs` `PresetPickerSource` / `PresetPickerModal`,
//! reading the *task* preset maps (`Config::presets` +
//! `query::preset::builtin`) rather than the graph maps.

use std::path::PathBuf;

use ft_core::query::preset;
use ft_core::task::ops::{self, MoveSource, MoveTarget};
use ft_core::task::Task;

use super::color::TaskColor;
use ratatui::layout::Rect;
use ratatui::widgets::Clear;
use ratatui::Frame;

use crate::tui::command::{CommandDef, CommandOutcome};
use crate::tui::event::Event;
use crate::tui::help::HelpSection;
use crate::tui::keymap::KeyMap;
use crate::tui::modal_commands as mc;
use crate::tui::tab::{AppRequest, ParentChoice, TabCtx, TasksRequest, ToastStyle};
use crate::tui::widgets::{
    FuzzyPicker, PickerItem, PickerOutcome, PickerSource, VaultFilePickerSource,
};

/// Fuzzy-picker source over task presets: user-defined
/// `Config::presets` (shadowing) followed by the built-ins from
/// `ft_core::query::preset::builtin`. A parallel of the Graph tab's
/// `PresetPickerSource` against the task preset maps.
pub struct TaskPresetPickerSource {
    pub(crate) items: Vec<(String, String)>,
    matcher: nucleo_matcher::Matcher,
    buf: Vec<char>,
}

impl TaskPresetPickerSource {
    pub(crate) fn new(vault: &ft_core::vault::Vault) -> Self {
        let mut items: Vec<(String, String)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (name, dsl) in &vault.config.config.tasks.presets {
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

impl PickerSource for TaskPresetPickerSource {
    type Item = String;

    fn query(&mut self, q: &str, limit: usize) -> Vec<PickerItem<String>> {
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
                PickerItem {
                    label: name.clone(),
                    match_indices,
                    data: name.clone(),
                }
            })
            .collect()
    }

    fn initial_items(&mut self, limit: usize) -> Vec<PickerItem<String>> {
        self.items
            .iter()
            .take(limit)
            .map(|(name, _)| PickerItem {
                label: name.clone(),
                match_indices: Vec::new(),
                data: name.clone(),
            })
            .collect()
    }
}

/// Modal wrapper around the task-preset picker (parallel of the Graph
/// tab's `PresetPickerModal`). On `Enter`: resolve the picked preset
/// name to its DSL string (user `Config::presets` first, then
/// `query::preset::builtin`), post
/// `AppRequest::Tasks(TasksRequest::ApplyPreset(dsl))`, and return
/// `Closed`. On `Esc`: return `Closed` with no request (the active
/// view's query is unchanged).
pub struct TaskPresetPickerModal {
    inner: FuzzyPicker<TaskPresetPickerSource>,
}

impl TaskPresetPickerModal {
    pub(crate) fn new(source: TaskPresetPickerSource) -> Self {
        Self {
            inner: FuzzyPicker::new(source),
        }
    }
}

impl crate::tui::modal::Modal for TaskPresetPickerModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> crate::tui::modal::ModalOutcome {
        let Event::Key(k) = ev else {
            return crate::tui::modal::ModalOutcome::NotHandled;
        };
        match self.inner.handle_key(k) {
            PickerOutcome::Selected(name) => {
                let dsl = ctx
                    .vault
                    .config
                    .config
                    .tasks
                    .presets
                    .get(&name)
                    .cloned()
                    .or_else(|| preset::builtin(&name).map(|s| s.to_string()));
                if let Some(dsl) = dsl {
                    *ctx.pending_request.borrow_mut() =
                        Some(AppRequest::Tasks(TasksRequest::ApplyPreset(dsl)));
                }
                crate::tui::modal::ModalOutcome::Closed
            }
            PickerOutcome::Cancelled => crate::tui::modal::ModalOutcome::Closed,
            PickerOutcome::StillOpen => crate::tui::modal::ModalOutcome::Consumed,
            PickerOutcome::NotHandled => crate::tui::modal::ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        let popup_area = super::edit_popup::centered_rect(60, 60, area);
        frame.render_widget(Clear, popup_area);
        self.inner.render(frame, popup_area);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Task preset picker",
            &[
                ("Type", "filter"),
                ("↑ / ↓", "navigate"),
                ("Enter", "apply preset"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "task-preset-picker"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::TASK_PRESET_PICKER_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::TASK_PRESET_PICKER_KEYMAP
    }

    fn dispatch_command(
        &mut self,
        _cmd: &crate::tui::command::Command,
        _ctx: &TabCtx,
    ) -> CommandOutcome {
        CommandOutcome::NotHandled
    }
}

/// Fuzzy-picker source over the retag tag list (`config.tasks.retag_tags`).
/// A parallel of [`TaskPresetPickerSource`] but over a fixed, flat list of
/// bare tag names. Items are labeled with a leading `#` for readability;
/// the data carried on selection is the bare name (no `#`).
pub struct TaskRetagPickerSource {
    pub(crate) items: Vec<String>,
    matcher: nucleo_matcher::Matcher,
    buf: Vec<char>,
}

impl TaskRetagPickerSource {
    pub(crate) fn new(vault: &ft_core::vault::Vault) -> Self {
        let items: Vec<String> = vault.config.config.tasks.retag_tags.clone();
        Self {
            items,
            matcher: nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT),
            buf: Vec::new(),
        }
    }
}

impl PickerSource for TaskRetagPickerSource {
    type Item = String;

    fn query(&mut self, q: &str, limit: usize) -> Vec<PickerItem<String>> {
        let pat = nucleo_matcher::pattern::Pattern::parse(
            q,
            nucleo_matcher::pattern::CaseMatching::Smart,
            nucleo_matcher::pattern::Normalization::Smart,
        );
        let mut ranked: Vec<(u32, usize, Vec<u32>)> = Vec::new();
        for (i, name) in self.items.iter().enumerate() {
            // Match against the bare name; the `#` label is display-only.
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
            .map(|(_, i, match_indices)| PickerItem {
                label: format!("#{}", self.items[i]),
                match_indices,
                data: self.items[i].clone(),
            })
            .collect()
    }

    fn initial_items(&mut self, limit: usize) -> Vec<PickerItem<String>> {
        self.items
            .iter()
            .take(limit)
            .map(|name| PickerItem {
                label: format!("#{name}"),
                match_indices: Vec::new(),
                data: name.clone(),
            })
            .collect()
    }
}

/// Modal wrapper around the retag picker. On `Enter`: post
/// `AppRequest::Tasks(TasksRequest::RetagSelected(name))` with the bare
/// tag name and return `Closed`. On `Esc`: return `Closed` with no request
/// (the selected task is unchanged).
pub struct TaskRetagPickerModal {
    inner: FuzzyPicker<TaskRetagPickerSource>,
}

impl TaskRetagPickerModal {
    pub(crate) fn new(source: TaskRetagPickerSource) -> Self {
        Self {
            inner: FuzzyPicker::new(source),
        }
    }
}

impl crate::tui::modal::Modal for TaskRetagPickerModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> crate::tui::modal::ModalOutcome {
        let Event::Key(k) = ev else {
            return crate::tui::modal::ModalOutcome::NotHandled;
        };
        match self.inner.handle_key(k) {
            PickerOutcome::Selected(name) => {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::Tasks(TasksRequest::RetagSelected(name)));
                crate::tui::modal::ModalOutcome::Closed
            }
            PickerOutcome::Cancelled => crate::tui::modal::ModalOutcome::Closed,
            PickerOutcome::StillOpen => crate::tui::modal::ModalOutcome::Consumed,
            PickerOutcome::NotHandled => crate::tui::modal::ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        let popup_area = super::edit_popup::centered_rect(60, 60, area);
        frame.render_widget(Clear, popup_area);
        self.inner.render(frame, popup_area);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Task retag picker",
            &[
                ("Type", "filter"),
                ("↑ / ↓", "navigate"),
                ("Enter", "apply tag"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "task-retag-picker"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::TASK_RETAG_PICKER_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::TASK_RETAG_PICKER_KEYMAP
    }

    fn dispatch_command(
        &mut self,
        _cmd: &crate::tui::command::Command,
        _ctx: &TabCtx,
    ) -> CommandOutcome {
        CommandOutcome::NotHandled
    }
}

// ── Task parent picker ───────────────────────────────────────────────

/// Fuzzy-picker source over fixed `(label, choice)` entries — a task's
/// parents (plus "all parents" when unlinking), or the task colors.
pub struct ChoicePickSource<T> {
    items: Vec<(String, T)>,
    matcher: nucleo_matcher::Matcher,
    buf: Vec<char>,
}

impl<T> ChoicePickSource<T> {
    pub(crate) fn new(items: Vec<(String, T)>) -> Self {
        Self {
            items,
            matcher: nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT),
            buf: Vec::new(),
        }
    }
}

impl<T: Clone> PickerSource for ChoicePickSource<T> {
    type Item = T;

    fn query(&mut self, q: &str, limit: usize) -> Vec<PickerItem<T>> {
        let pat = nucleo_matcher::pattern::Pattern::parse(
            q,
            nucleo_matcher::pattern::CaseMatching::Smart,
            nucleo_matcher::pattern::Normalization::Smart,
        );
        let mut ranked: Vec<(u32, usize, Vec<u32>)> = Vec::new();
        for (i, (label, _)) in self.items.iter().enumerate() {
            self.buf.clear();
            let haystack = nucleo_matcher::Utf32Str::new(label, &mut self.buf);
            let mut indices = Vec::new();
            if let Some(score) = pat.indices(haystack, &mut self.matcher, &mut indices) {
                ranked.push((score, i, indices));
            }
        }
        ranked.sort_by_key(|b| std::cmp::Reverse(b.0));
        ranked
            .into_iter()
            .take(limit)
            .map(|(_, i, match_indices)| PickerItem {
                label: self.items[i].0.clone(),
                match_indices,
                data: self.items[i].1.clone(),
            })
            .collect()
    }

    fn initial_items(&mut self, limit: usize) -> Vec<PickerItem<T>> {
        self.items
            .iter()
            .take(limit)
            .map(|(label, choice)| PickerItem {
                label: label.clone(),
                match_indices: Vec::new(),
                data: choice.clone(),
            })
            .collect()
    }
}

/// Picker over a task's parents (`U` with several references, `I` with
/// several ticket parents). On `Enter` posts
/// `AppRequest::Tasks(TasksRequest::ParentPicked { task, choice })`; on
/// `Esc` closes with no write.
pub struct TaskParentPickModal {
    task: ft_core::task::TaskKey,
    title: &'static str,
    inner: FuzzyPicker<ChoicePickSource<ParentChoice>>,
}

impl TaskParentPickModal {
    pub(crate) fn new(
        task: ft_core::task::TaskKey,
        title: &'static str,
        source: ChoicePickSource<ParentChoice>,
    ) -> Self {
        Self {
            task,
            title,
            inner: FuzzyPicker::new(source),
        }
    }
}

impl crate::tui::modal::Modal for TaskParentPickModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> crate::tui::modal::ModalOutcome {
        let Event::Key(k) = ev else {
            return crate::tui::modal::ModalOutcome::NotHandled;
        };
        match self.inner.handle_key(k) {
            PickerOutcome::Selected(choice) => {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::Tasks(TasksRequest::ParentPicked {
                        task: self.task.clone(),
                        choice,
                    }));
                crate::tui::modal::ModalOutcome::Closed
            }
            PickerOutcome::Cancelled => crate::tui::modal::ModalOutcome::Closed,
            PickerOutcome::StillOpen => crate::tui::modal::ModalOutcome::Consumed,
            PickerOutcome::NotHandled => crate::tui::modal::ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        let popup_area = super::edit_popup::centered_rect(60, 40, area);
        frame.render_widget(Clear, popup_area);
        self.inner.render(frame, popup_area);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            self.title,
            &[
                ("Type", "filter"),
                ("↑ / ↓", "navigate"),
                ("Enter", "choose"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "task-parent-pick"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::TASK_PARENT_PICK_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::TASK_PARENT_PICK_KEYMAP
    }

    fn dispatch_command(
        &mut self,
        _cmd: &crate::tui::command::Command,
        _ctx: &TabCtx,
    ) -> CommandOutcome {
        CommandOutcome::NotHandled
    }
}

// ── Task color picker ────────────────────────────────────────────────

/// Picker over the six task colors plus `none` (`m` on the Tasks tab).
/// On `Enter` posts `AppRequest::Tasks(TasksRequest::ColorPicked(..))`;
/// on `Esc` closes with no write.
pub struct TaskColorPickModal {
    inner: FuzzyPicker<ChoicePickSource<Option<TaskColor>>>,
}

impl TaskColorPickModal {
    pub(crate) fn new() -> Self {
        let items = TaskColor::ALL
            .into_iter()
            .map(|c| (c.name().to_string(), Some(c)))
            .chain(std::iter::once(("none".to_string(), None)))
            .collect();
        Self {
            inner: FuzzyPicker::new(ChoicePickSource::new(items)),
        }
    }
}

impl crate::tui::modal::Modal for TaskColorPickModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> crate::tui::modal::ModalOutcome {
        let Event::Key(k) = ev else {
            return crate::tui::modal::ModalOutcome::NotHandled;
        };
        match self.inner.handle_key(k) {
            PickerOutcome::Selected(color) => {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::Tasks(TasksRequest::ColorPicked(color)));
                crate::tui::modal::ModalOutcome::Closed
            }
            PickerOutcome::Cancelled => crate::tui::modal::ModalOutcome::Closed,
            PickerOutcome::StillOpen => crate::tui::modal::ModalOutcome::Consumed,
            PickerOutcome::NotHandled => crate::tui::modal::ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        let popup_area = super::edit_popup::centered_rect(40, 50, area);
        frame.render_widget(Clear, popup_area);
        self.inner.render(frame, popup_area);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Task color picker",
            &[
                ("Type", "filter"),
                ("↑ / ↓", "navigate"),
                ("Enter", "set color"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "task-color-pick"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::TASK_COLOR_PICK_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::TASK_COLOR_PICK_KEYMAP
    }

    fn dispatch_command(
        &mut self,
        _cmd: &crate::tui::command::Command,
        _ctx: &TabCtx,
    ) -> CommandOutcome {
        CommandOutcome::NotHandled
    }
}

// ── Task-move picker ─────────────────────────────────────────────────

/// Modal wrapper around the file+heading fuzzy picker for the
/// `tasks.move` flow. Captures the source task identity at open time
/// (path + line + scanned `Task` for the `LineChanged` guard); on
/// `Enter` builds a `MoveTarget` from the picked `Hit` and runs
/// `ops::plan_move` + `ops::apply_move_plan`, then requests a graph
/// refresh. Same-file targets are rejected with a toast and the picker
/// stays open. On `Esc` the modal closes with no write.
///
/// Reuses `VaultFilePickerSource` (the same picker new-task creation
/// uses) rather than introducing a new source — the `Hit` already
/// carries `path` + optional `heading`, which is exactly the
/// `MoveTarget::{Append, UnderHeading}` shape.
pub struct TaskMoveModal {
    inner: FuzzyPicker<VaultFilePickerSource>,
    /// Absolute path of the file the source task currently lives in.
    source_path: PathBuf,
    /// 1-indexed source line of the task to move.
    source_line: usize,
    /// The scanned task at `source_line`, passed as `expected` so
    /// `plan_move` fails with `MoveError::LineChanged` if the line
    /// shifted on disk.
    task: Task,
}

impl TaskMoveModal {
    pub(crate) fn new(ctx: &TabCtx, source_path: PathBuf, source_line: usize, task: Task) -> Self {
        let source = VaultFilePickerSource::new(
            std::sync::Arc::clone(ctx.vault),
            std::sync::Arc::clone(ctx.recents),
        );
        Self {
            inner: FuzzyPicker::new(source),
            source_path,
            source_line,
            task,
        }
    }
}

/// Build a `MoveTarget` from a picker `Hit`: `UnderHeading` when the
/// hit carries a heading, `Append` otherwise. The path is resolved
/// absolute against the vault root. Pure / testable in isolation from
/// the modal state.
pub(crate) fn hit_to_move_target(
    hit: &ft_core::search::Hit,
    vault_root: &std::path::Path,
) -> MoveTarget {
    let abs = vault_root.join(&hit.path);
    match &hit.heading {
        Some(h) => MoveTarget::UnderHeading(abs, h.text.clone()),
        None => MoveTarget::Append(abs),
    }
}

impl crate::tui::modal::Modal for TaskMoveModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> crate::tui::modal::ModalOutcome {
        let Event::Key(k) = ev else {
            return crate::tui::modal::ModalOutcome::NotHandled;
        };
        match self.inner.handle_key(k) {
            PickerOutcome::Selected(hit) => {
                let target = hit_to_move_target(&hit, &ctx.vault.path);

                // Same-file guard: reject and keep the picker open so
                // the user can pick a different target.
                if target.path() == self.source_path {
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast {
                        text: "can't move to the same file — pick a different target".into(),
                        style: ToastStyle::Error,
                    });
                    return crate::tui::modal::ModalOutcome::Consumed;
                }

                let source = MoveSource {
                    path: self.source_path.clone(),
                    line: self.source_line,
                    expected: Some(self.task.clone()),
                };
                let format = ctx.vault.task_format();
                match ops::plan_move(&[source], &target, format) {
                    Ok(mut plan) => {
                        let retargeted = super::search::retarget(ctx, &mut plan);
                        let applied = retargeted
                            .clone()
                            .and_then(|_| ops::apply_move_plan(&plan).map_err(|e| e.to_string()));
                        if let Err(e) = applied {
                            *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast {
                                text: format!("move failed: {e}"),
                                style: ToastStyle::Error,
                            });
                            ctx.request_graph_refresh();
                            return crate::tui::modal::ModalOutcome::Closed;
                        }
                        let label = match &target {
                            MoveTarget::UnderHeading(p, h) => {
                                format!("moved to {}#{}", p.display(), h)
                            }
                            MoveTarget::Append(p) | MoveTarget::UnderTask { path: p, .. } => {
                                format!("moved to {}", p.display())
                            }
                        };
                        let label = format!(
                            "{label}{}",
                            super::search::retarget_note(retargeted.unwrap_or(0))
                        );
                        *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast {
                            text: label,
                            style: ToastStyle::Success,
                        });
                        ctx.request_graph_refresh();
                        crate::tui::modal::ModalOutcome::Closed
                    }
                    Err(e) => {
                        // `LineChanged` (drift) and other plan errors:
                        // surface, refresh, close. The user should rescan.
                        *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast {
                            text: format!("{e}"),
                            style: ToastStyle::Error,
                        });
                        ctx.request_graph_refresh();
                        crate::tui::modal::ModalOutcome::Closed
                    }
                }
            }
            PickerOutcome::Cancelled => crate::tui::modal::ModalOutcome::Closed,
            PickerOutcome::StillOpen => crate::tui::modal::ModalOutcome::Consumed,
            PickerOutcome::NotHandled => crate::tui::modal::ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        let popup_area = super::edit_popup::centered_rect(60, 60, area);
        frame.render_widget(Clear, popup_area);
        self.inner.render(frame, popup_area);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Task move",
            &[
                ("Type", "filter files / headings"),
                ("↑ / ↓", "navigate"),
                ("Enter", "move task to file / heading"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "task-move"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::TASK_MOVE_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::TASK_MOVE_KEYMAP
    }

    fn dispatch_command(
        &mut self,
        _cmd: &crate::tui::command::Command,
        _ctx: &TabCtx,
    ) -> CommandOutcome {
        CommandOutcome::NotHandled
    }
}

//! Modal state hosted by the Tasks tab: the task-preset picker. Kept
//! next to the tab because it reaches `SearchView`/`Config::presets`
//! types and posts a Tasks-targeted `AppRequest`. A parallel of the
//! Graph tab's `tabs/graph/modals.rs` `PresetPickerSource` /
//! `PresetPickerModal`, reading the *task* preset maps
//! (`Config::presets` + `query::preset::builtin`) rather than the graph
//! maps.

use ft_core::query::preset;
use ratatui::layout::Rect;
use ratatui::widgets::Clear;
use ratatui::Frame;

use crate::tui::command::{CommandDef, CommandOutcome};
use crate::tui::event::Event;
use crate::tui::help::HelpSection;
use crate::tui::keymap::KeyMap;
use crate::tui::modal_commands as mc;
use crate::tui::tab::{AppRequest, TabCtx, TasksRequest};
use crate::tui::widgets::{FuzzyPicker, PickerItem, PickerOutcome, PickerSource};

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
        for (name, dsl) in &vault.config.config.presets {
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

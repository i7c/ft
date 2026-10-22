use anyhow::Result;
use ratatui::{layout::Rect, Frame};

use crate::tui::{
    event::Event,
    tab::{EventOutcome, TabCtx},
};

/// A view inside the Tasks tab. Views are listed in the sidebar dropdown and
/// the active one renders into the viewport. v1 ships only "Search"; "Board"
/// and "Calendar" are explicitly out of scope.
pub trait View {
    #[allow(dead_code)]
    fn title(&self) -> &str;

    fn on_focus(&mut self, _ctx: &mut TabCtx) -> Result<()> {
        Ok(())
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome>;

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx);

    /// Called (via the owning tab) when a new graph snapshot installs
    /// while this tab is active. Default: no-op.
    fn on_graph_ready(&mut self, _ctx: &mut TabCtx) {}

    fn refresh(&mut self, _ctx: &mut TabCtx) -> Result<()> {
        Ok(())
    }

    /// Replace the view's query with a preset DSL and recompute. Raised
    /// by the task-preset-picker modal via `TasksTab::handle_tasks_request`.
    /// Default: no-op (only `SearchView` overrides). Mirrors how
    /// `on_graph_ready` / `refresh` are default-no-op `View` methods
    /// so the owning tab can call through `Box<dyn View>` without
    /// downcasting.
    fn apply_preset(&mut self, _dsl: &str, _ctx: &mut TabCtx) {}

    /// Retag the selected task with `tag` (bare name, no leading `#`),
    /// swapping out any prior tag from `config.tasks.retag_tags`. Raised
    /// by the task-retag-picker modal via `TasksTab::handle_tasks_request`.
    /// Default: no-op (only `SearchView` overrides).
    fn apply_retag(&mut self, _tag: &str, _ctx: &mut TabCtx) {}

    /// Select `key` (revealing its ancestors) once the next snapshot is
    /// adopted. Default: no-op.
    fn select_after_refresh(&mut self, _key: ft_core::task::TaskKey) {}

    /// Set or clear the color of the cut set / selected task (the
    /// task-color-pick modal's choice). Default: no-op.
    fn color_picked(
        &mut self,
        _color: Option<crate::tui::tabs::tasks::color::TaskColor>,
        _ctx: &mut TabCtx,
    ) {
    }

    /// Replace the view's idle bindings (the tab's keymap with user
    /// overrides applied). Default: no-op.
    fn set_keymap(&mut self, _keymap: crate::tui::keymap::KeyMap) {}

    /// Act on the task-parent-pick modal's `choice` for `task`. Default:
    /// no-op.
    fn parent_picked(
        &mut self,
        _task: ft_core::task::TaskKey,
        _choice: crate::tui::tab::ParentChoice,
        _ctx: &mut TabCtx,
    ) {
    }
}

use anyhow::Result;
use chrono::NaiveDate;
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
    fn apply_preset(&mut self, _dsl: &str, _today: NaiveDate) {}
}

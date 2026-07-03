//! Rendering for the Graph tab. Split out of `mod.rs` to keep that
//! file from re-growing into the god-object the graph-tab-
//! decomposition change removed. `Tab::render` in `mod.rs` delegates
//! to `render_impl` here — a single `impl Tab for GraphTab` block
//! can't span files, but the inherent method it delegates to can live
//! anywhere in the `tabs::graph` module tree.

use super::*;

impl GraphTab {
    pub(super) fn render_impl(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
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
}

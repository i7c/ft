//! `Search` tab — live paragraph search over the shared snapshot's
//! search index.
//!
//! The query box at the top parses the query DSL on every keystroke
//! (`/` enters edit mode; `Enter`/`Esc` leave it — results update live
//! while typing). The box's title carries the parse state (`AND/ANY ·
//! sort: … · N results · M selected`), visually distinct from the query
//! text inside. Below the box — whose bottom border is the solid
//! separator — sits a feed-split: the result list in a compact top
//! viewport (max 10 rows, scrolls past that; rows carry path · line
//! range · heading path · matched labels, no body text; grouped under
//! one header row per note by default, `g` toggles flat), and the
//! focused paragraph's body as wrapped text in a preview pane at the
//! bottom with the query's matches highlighted.
//! `Space` multi-selects paragraphs (on a header: the whole note);
//! `s`/`S` ship the selection (or all results) into a synth note via the
//! shared send-to-synth flow; `a` toggles all/any; `o` cycles relevance
//! ↔ date sort; `c` clears back to the fresh-tab empty state; `Enter`
//! opens the source at the paragraph.
//!
//! The index lives in the shared snapshot (`GraphSnapshot::search`) and
//! is rebuilt by the App's background worker on generation change — the
//! tab never scans or builds indexes itself. Queries run synchronously
//! against the in-memory index: sub-millisecond at vault scale, so no
//! worker round-trip per keystroke.

use std::collections::HashSet;
use std::sync::LazyLock;

use anyhow::Result;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, ListItem, Paragraph},
    Frame,
};

use ft_core::blame_cache::BlameCache;
use ft_core::search::{
    fill_highlights, group, parse_query, search, search_with_dates, SearchCtx, SearchResult, Sort,
};

use crate::tui::command::{Command, CommandDef, CommandOutcome, CommandScope};
use crate::tui::event::Event;
use crate::tui::help::HelpSection;
use crate::tui::keymap::{KeyChord, KeyMap};
use crate::tui::palette;
use crate::tui::synth_send::{SynthSendFlow, SynthSendHost};
use crate::tui::tab::{AppRequest, EventOutcome, Tab, TabCtx, TabKind};
use crate::tui::widgets::{
    inline_markdown_spans, render_feed_split, render_inline_input, wrap_line, CursorMode,
    EditBuffer, InlineInput,
};

// ── Commands ─────────────────────────────────────────────────────────

pub(crate) static SEARCH_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "search.edit-query",
        description: "Edit the search query (live results as you type)",
        scope: CommandScope::Tab("search"),
        group: "Query",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "search.toggle-any",
        description: "Toggle any-match (OR) vs all-match (AND) across terms",
        scope: CommandScope::Tab("search"),
        group: "Query",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "search.cycle-sort",
        description: "Cycle result sort between relevance and date (newest first)",
        scope: CommandScope::Tab("search"),
        group: "Query",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "search.toggle-grouping",
        description: "Toggle results grouped by note vs one flat row per paragraph",
        scope: CommandScope::Tab("search"),
        group: "Query",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "search.result-up",
        description: "Move the cursor up one row",
        scope: CommandScope::Tab("search"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "search.result-down",
        description: "Move the cursor down one row",
        scope: CommandScope::Tab("search"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "search.toggle-selection",
        description: "Toggle multi-select on the current row",
        scope: CommandScope::Tab("search"),
        group: "Selection",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "search.open-source",
        description: "Open the source note at the paragraph in $EDITOR",
        scope: CommandScope::Tab("search"),
        group: "Open",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "search.send-to-synth-existing",
        description: "Append selected (or all) results to an existing synth note",
        scope: CommandScope::Tab("search"),
        group: "Synth",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "search.send-to-synth-new",
        description: "Create a new synth note from selected (or all) results",
        scope: CommandScope::Tab("search"),
        group: "Synth",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "search.reload",
        description: "Re-run the query against the current index",
        scope: CommandScope::Tab("search"),
        group: "Source",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "search.clear",
        description: "Clear the query and results (back to the empty state)",
        scope: CommandScope::Tab("search"),
        group: "Source",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
];

pub(crate) static SEARCH_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("/", "search.edit-query")
        .bind("a", "search.toggle-any")
        .bind("o", "search.cycle-sort")
        .bind("g", "search.toggle-grouping")
        .bind("Up", "search.result-up")
        .bind("k", "search.result-up")
        .bind("Down", "search.result-down")
        .bind("j", "search.result-down")
        .bind("Space", "search.toggle-selection")
        .bind("Enter", "search.open-source")
        .bind("s", "search.send-to-synth-existing")
        .bind("S", "search.send-to-synth-new")
        .bind("R", "search.reload")
        .bind("c", "search.clear")
});

pub struct SearchTab {
    /// The query input line. Editable while `editing` is true; the
    /// results re-query on every edit.
    input: EditBuffer,
    /// True while the input line owns the keyboard (`/` enters, Esc or
    /// Enter leaves). Results update live while typing.
    editing: bool,
    /// any-mode: OR across clauses instead of the default AND.
    any: bool,
    sort: Sort,
    /// Current results, in the active sort order.
    results: Vec<SearchResult>,
    /// Group results under one header row per note.
    grouped: bool,
    /// The list rows derived from `results` and `grouped`.
    rows: Vec<Row>,
    /// 0-indexed cursor into `rows`.
    cursor: usize,
    /// Multi-select indices into `results` (stable across `g`).
    selected: HashSet<usize>,
    /// Lazy blame cache for the date sort (kept warm across queries).
    cache: Option<BlameCache>,
    /// The last parsed query (drives the results list). `None` before
    /// the first query or after the input is cleared.
    last_query: Option<ft_core::search::SearchQuery>,
    /// Last query error, if any.
    last_error: Option<String>,
    /// Shared send-to-synth state machine.
    synth_send: SynthSendFlow,
    keymap: KeyMap,
}

impl Default for SearchTab {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchTab {
    pub fn new() -> Self {
        Self {
            input: EditBuffer::default(),
            editing: false,
            any: false,
            sort: Sort::Relevance,
            results: Vec::new(),
            grouped: true,
            rows: Vec::new(),
            cursor: 0,
            selected: HashSet::new(),
            cache: None,
            last_query: None,
            last_error: None,
            synth_send: SynthSendFlow::new(),
            keymap: SEARCH_KEYMAP.clone(),
        }
    }

    pub fn with_keymap_overlay(mut self, overlay: &crate::tui::keymap::KeymapOverlay) -> Self {
        self.keymap = SEARCH_KEYMAP.with_overlay(overlay);
        self
    }

    /// Install a query (from the Graph tab's handoff or tests) and re-run it.
    pub fn queue_query(&mut self, query: String, any: bool) {
        self.input = EditBuffer::from(&query);
        self.any = any;
        self.editing = true;
        self.requery();
    }

    /// Re-run the current query against the snapshot's search index.
    /// Synchronous: the index is in-memory and immutable after build.
    fn requery(&mut self) {
        self.last_error = None;
        let query = parse_query(&self.input.text, self.any);
        self.results.clear();
        self.rows.clear();
        self.selected.clear();
        self.cursor = 0;
        if query.is_empty() {
            self.last_query = None;
            return;
        }
        // The index is fetched on demand here (via TabCtx at the call
        // site), so this fn only reshapes state from `query`.
        self.last_query = Some(query);
    }

    fn run_query(&mut self, ctx: &TabCtx) {
        let Some(snap) = ctx.snapshot.clone() else {
            return;
        };
        let Some(query) = self.last_query.clone() else {
            return;
        };
        let index = &snap.search;
        let sctx = SearchCtx {
            graph: Some(&snap.graph),
            presets: Some(&ctx.vault.config.config.graph.presets),
        };
        let outcome = if self.sort == Sort::Date || query.has_date_filters() {
            let mut cache = self.cache.take().unwrap_or_default();
            let r = search_with_dates(index, &query, sctx, ctx.vault, &mut cache, self.sort)
                .map(|d| d.results);
            self.cache = Some(cache);
            r
        } else {
            search(index, &query, sctx)
        };
        match outcome {
            Ok(results) => self.results = results,
            Err(e) => {
                self.results.clear();
                self.last_error = Some(e.to_string());
            }
        }
        self.selected.clear();
        self.cursor = 0;
        self.rebuild_rows(false);
    }

    /// Derive the list rows from `results`. With `keep_focus` the cursor
    /// follows the focused result into the new layout; otherwise it
    /// returns to the top.
    fn rebuild_rows(&mut self, keep_focus: bool) {
        let focus = if keep_focus {
            self.cursor_result()
        } else {
            None
        };
        self.rows.clear();
        if self.grouped {
            for g in group(&self.results) {
                self.rows.push(Row::Header {
                    path: g.path.display().to_string(),
                    members: g.members.clone(),
                });
                self.rows.extend(g.members.into_iter().map(Row::Item));
            }
        } else {
            self.rows.extend((0..self.results.len()).map(Row::Item));
        }
        self.cursor = focus
            .and_then(|i| self.rows.iter().position(|r| *r == Row::Item(i)))
            .unwrap_or(0);
    }

    /// The result the cursor stands for: the row's result, or a header's
    /// first member.
    fn cursor_result(&self) -> Option<usize> {
        match self.rows.get(self.cursor)? {
            Row::Item(i) => Some(*i),
            Row::Header { members, .. } => members.first().copied(),
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let len = self.rows.len() as isize;
        self.cursor = ((self.cursor as isize + delta).clamp(0, len - 1)) as usize;
    }

    fn toggle_selection(&mut self) {
        match self.rows.get(self.cursor) {
            Some(Row::Item(i)) if !self.selected.remove(i) => {
                self.selected.insert(*i);
            }
            Some(Row::Item(_)) => {}
            Some(Row::Header { members, .. }) => {
                if members.iter().all(|i| self.selected.contains(i)) {
                    for i in members {
                        self.selected.remove(i);
                    }
                } else {
                    self.selected.extend(members.iter().copied());
                }
            }
            None => {}
        }
    }

    fn toggle_grouping(&mut self) {
        self.grouped = !self.grouped;
        self.rebuild_rows(true);
    }

    fn toggle_any(&mut self) {
        self.any = !self.any;
        self.requery();
    }

    fn cycle_sort(&mut self) {
        self.sort = match self.sort {
            Sort::Relevance => Sort::Date,
            Sort::Date => Sort::Relevance,
        };
        self.requery();
    }

    fn open_selected(&self, ctx: &TabCtx) {
        let Some(result) = self.cursor_result().and_then(|i| self.results.get(i)) else {
            return;
        };
        let abs = ctx.vault.path.join(&result.path);
        ctx.recents.record_open(&result.path);
        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
            path: abs,
            line: result.line_start as usize,
        });
    }
}

/// The Search tab supplies its results (selected, or all) as sources.
impl SynthSendHost for SearchTab {
    fn synth_sources(
        &mut self,
        _ctx: &mut TabCtx,
        _target: &std::path::Path,
    ) -> Vec<ft_core::synth::source::SynthSource> {
        let chosen: Vec<&SearchResult> = if self.selected.is_empty() {
            self.results.iter().collect()
        } else {
            let mut v: Vec<usize> = self.selected.iter().copied().collect();
            v.sort_unstable();
            v.into_iter().filter_map(|i| self.results.get(i)).collect()
        };
        chosen
            .into_iter()
            .map(|r| ft_core::synth::source::SynthSource {
                source_path: r.path.clone(),
                line_start: r.line_start,
                line_end: r.line_end,
                body: r.body.clone(),
            })
            .collect()
    }

    fn on_synth_committed(&mut self, ctx: &mut TabCtx, _target: &std::path::Path) {
        // The vault changed; the snapshot refresh re-derives the index.
        ctx.request_graph_refresh();
    }
}

impl Tab for SearchTab {
    fn queue_search_query(&mut self, query: String, any: bool) {
        self.queue_query(query, any);
    }

    fn title(&self) -> &str {
        "Search"
    }

    fn kind(&self) -> TabKind {
        TabKind::Search
    }

    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()> {
        if self.last_query.is_some() {
            self.run_query(ctx);
        }
        Ok(())
    }

    fn on_graph_ready(&mut self, ctx: &mut TabCtx) {
        if self.last_query.is_some() {
            self.run_query(ctx);
        }
    }

    fn commands(&self) -> &'static [CommandDef] {
        SEARCH_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &self.keymap
    }

    fn dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome {
        match cmd.name {
            "search.edit-query" => {
                self.editing = true;
                CommandOutcome::Handled
            }
            "search.toggle-any" => {
                self.toggle_any();
                self.run_query(ctx);
                CommandOutcome::Handled
            }
            "search.cycle-sort" => {
                self.cycle_sort();
                self.run_query(ctx);
                CommandOutcome::Handled
            }
            "search.toggle-grouping" => {
                self.toggle_grouping();
                CommandOutcome::Handled
            }
            "search.result-up" => {
                self.move_cursor(-1);
                CommandOutcome::Handled
            }
            "search.result-down" => {
                self.move_cursor(1);
                CommandOutcome::Handled
            }
            "search.toggle-selection" => {
                self.toggle_selection();
                CommandOutcome::Handled
            }
            "search.open-source" => {
                self.open_selected(ctx);
                CommandOutcome::Handled
            }
            "search.send-to-synth-existing" => {
                if !self.results.is_empty() {
                    self.synth_send.open_existing(ctx);
                }
                CommandOutcome::Handled
            }
            "search.send-to-synth-new" => {
                if !self.results.is_empty() {
                    self.synth_send.open_new(ctx);
                }
                CommandOutcome::Handled
            }
            "search.reload" => {
                self.run_query(ctx);
                CommandOutcome::Handled
            }
            "search.clear" => {
                self.input = EditBuffer::default();
                self.editing = false;
                self.any = false;
                self.sort = Sort::Relevance;
                self.results.clear();
                self.rows.clear();
                self.selected.clear();
                self.cursor = 0;
                self.last_query = None;
                self.last_error = None;
                CommandOutcome::Handled
            }
            _ => CommandOutcome::NotHandled,
        }
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };

        if self.synth_send.is_active() {
            // Take the flow out of `self` so the host borrow (`self`) is
            // disjoint from the flow's mutable borrow.
            let mut flow = std::mem::take(&mut self.synth_send);
            let outcome = flow.handle_key(k, ctx, self);
            self.synth_send = flow;
            return Ok(outcome);
        }

        if self.editing {
            use crossterm::event::KeyCode;
            match k.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.editing = false;
                    return Ok(EventOutcome::Consumed);
                }
                _ => {
                    let before = self.input.text.clone();
                    let _ = self.input.handle_event(k);
                    if self.input.text != before {
                        self.requery();
                        self.run_query(ctx);
                    }
                    return Ok(EventOutcome::Consumed);
                }
            }
        }

        let chord = KeyChord::from_key_event(k);
        let Some(cmd) = self.keymap.lookup(chord).cloned() else {
            return Ok(EventOutcome::NotHandled);
        };
        Ok(match self.dispatch_command(&cmd, ctx) {
            CommandOutcome::Handled => EventOutcome::Consumed,
            CommandOutcome::NotHandled => EventOutcome::NotHandled,
        })
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        let block = Block::default().borders(Borders::ALL).title(" Search ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Query box (3 rows: title border + input row + bottom border —
        // the bottom border doubles as the solid separator from the
        // feed split below). Its title carries the parse state so the
        // status stays visually distinct from the query text inside.
        let (query_area, feed_area) = split_query_box(inner);
        let mut status = format!(
            "{} · sort: {} · {}",
            if self.any { "ANY" } else { "AND" },
            match self.sort {
                Sort::Relevance => "relevance",
                Sort::Date => "date",
            },
            if self.grouped { "grouped" } else { "flat" }
        );
        let filters = self.last_query.as_ref().map_or(0, |q| q.filters().count());
        if filters > 0 {
            status.push_str(&format!(
                " · {filters} filter{}",
                if filters == 1 { "" } else { "s" }
            ));
        }
        let qtitle = Line::from(Span::styled(
            format!(
                " {status} ({} result{}, {} selected) ",
                self.results.len(),
                if self.results.len() == 1 { "" } else { "s" },
                self.selected.len()
            ),
            Style::default().fg(palette::PRIMARY),
        ));
        let qblock = Block::default().borders(Borders::ALL).title(qtitle);
        let qinner = qblock.inner(query_area);
        frame.render_widget(qblock, query_area);
        // Clear the input row first so a shortened query can't leave
        // the previous frame's longer text behind.
        frame.render_widget(Clear, qinner);
        let prefix = if self.editing { "> " } else { "/ " };
        render_inline_input(
            frame,
            qinner,
            InlineInput {
                buf: &self.input,
                prefix: vec![Span::styled(prefix, Style::default().fg(palette::PRIMARY))],
                placeholder: Some(Span::styled(
                    "type a query — substring, =word, ~fuzzy, \"phrase\", [[link]], -exclude, path: tag: near:[[…]]",
                    Style::default().fg(palette::DIM),
                )),
                text_style: Style::default().fg(palette::WHITE),
                cursor: if self.editing {
                    CursorMode::Bar(Style::default())
                } else {
                    CursorMode::None
                },
            },
        );

        if let Some(err) = self.last_error.as_deref() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("error: {err}"),
                    Style::default().fg(palette::ERROR),
                ))),
                feed_area,
            );
            self.synth_send.render(frame, area);
            return;
        }

        if self.results.is_empty() {
            let text = if self.input.text.trim().is_empty() {
                "press / to start typing — results update live"
            } else {
                "no matches — try =word, ~fuzzy, or [[link]] syntax"
            };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    text,
                    Style::default().fg(palette::DIM),
                ))),
                feed_area,
            );
            self.synth_send.render(frame, area);
            return;
        }

        // Compact list rows — no paragraph body (the preview pane below
        // shows it) — padded to the full row width so a shorter row can't
        // leave the previous frame's longer text behind. Grouped: a header
        // per note, then `L{start}-{end}  {heading path}  {labels}` rows;
        // flat: the path leads every row.
        let mut items: Vec<ListItem<'_>> = Vec::with_capacity(self.rows.len());
        for row in &self.rows {
            let mut spans = match row {
                Row::Header { path, members } => {
                    let all = members.iter().all(|i| self.selected.contains(i));
                    vec![
                        Span::styled(
                            if all { "[*] " } else { "    " },
                            Style::default().fg(palette::DIM),
                        ),
                        Span::styled(
                            format!("{path} ({})", members.len()),
                            Style::default()
                                .fg(palette::PRIMARY)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]
                }
                Row::Item(i) => {
                    let r = &self.results[*i];
                    let mark = if self.selected.contains(i) {
                        "[*] "
                    } else {
                        "    "
                    };
                    let loc = if self.grouped {
                        format!("  L{}-{}", r.line_start, r.line_end)
                    } else {
                        format!("{} L{}-{}", r.path.display(), r.line_start, r.line_end)
                    };
                    let mut spans = vec![
                        Span::styled(mark, Style::default().fg(palette::DIM)),
                        Span::styled(loc, Style::default().fg(palette::PRIMARY)),
                    ];
                    if !r.heading_path.is_empty() {
                        spans.push(Span::styled(
                            format!("  {}", r.heading_path.join(" › ")),
                            Style::default().fg(palette::SECONDARY),
                        ));
                    }
                    if !r.matched.is_empty() {
                        spans.push(Span::styled(
                            format!("  {}", r.matched.join(" ")),
                            Style::default().fg(palette::DIM),
                        ));
                    }
                    spans
                }
            };
            let width: usize = spans.iter().map(|s| s.width()).sum();
            let pad = (feed_area.width as usize).saturating_sub(width);
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
            items.push(ListItem::new(Line::from(spans)));
        }

        // Highlights are computed for the focused paragraph only (it is
        // the one the preview shows).
        let focus = self.cursor_result();
        if let (Some(i), Some(snap), Some(query)) =
            (focus, ctx.snapshot.as_ref(), self.last_query.as_ref())
        {
            fill_highlights(
                &snap.search,
                query,
                std::slice::from_mut(&mut self.results[i]),
            );
        }

        // Preview pane: a header band (path · lines · matched labels ·
        // score, plus the blame date when dated) then the wrapped
        // paragraph body with the shared inline markdown styling and the
        // query's matches highlighted.
        let mut preview_header: Vec<Line<'_>> = Vec::new();
        let mut preview_body: Vec<Line<'_>> = Vec::new();
        if let Some(row) = focus.and_then(|i| self.results.get(i)) {
            let mut h = format!(
                "{} · L{}–{}",
                row.path.display(),
                row.line_start,
                row.line_end
            );
            if !row.heading_path.is_empty() {
                h.push_str(&format!(" · {}", row.heading_path.join(" › ")));
            }
            if !row.matched.is_empty() {
                h.push_str(&format!(" · {}", row.matched.join(" · ")));
            }
            h.push_str(&format!(" · score {:.2}", row.score));
            if let Some(d) = row.date {
                h.push_str(&format!(" · {d}"));
            }
            preview_header.push(Line::from(Span::styled(
                h,
                Style::default()
                    .fg(palette::PRIMARY)
                    .bg(palette::ENTRY_HEADER_BG)
                    .add_modifier(Modifier::BOLD),
            )));
            let needles: Vec<&str> = row
                .highlights
                .iter()
                .map(|h| &row.body[h.clone()])
                .collect();
            for body_line in row.body.lines() {
                for wrapped in wrap_line(body_line, feed_area.width as usize) {
                    preview_body.push(Line::from(highlight_spans(
                        inline_markdown_spans(&wrapped),
                        &needles,
                    )));
                }
            }
        }

        render_feed_split(
            frame,
            feed_area,
            items,
            self.cursor,
            &HashSet::new(),
            &preview_header,
            &preview_body,
        );
        self.synth_send.render(frame, area);
    }

    fn help_sections(&self) -> Vec<HelpSection> {
        vec![
            HelpSection::new(
                "Query",
                &[
                    ("/", "edit the query (live results as you type)"),
                    ("Esc / Enter", "leave the query editor"),
                    ("a", "toggle all-match (AND) ↔ any-match (OR)"),
                    ("o", "cycle sort: relevance ↔ date"),
                    ("g", "toggle grouped by note ↔ flat"),
                ],
            ),
            HelpSection::new(
                "Results",
                &[
                    ("↑ / ↓ · j / k", "select prev / next row"),
                    ("Space", "toggle multi-select (on a note header: all of it)"),
                    ("Enter", "open the source note at the paragraph"),
                ],
            ),
            HelpSection::new(
                "Synth",
                &[
                    ("s", "append selected (or all) results to a synth note"),
                    ("S", "create a new synth note from the results"),
                ],
            ),
            HelpSection::new(
                "Source",
                &[
                    ("R", "re-run the query"),
                    ("c", "clear the query and results"),
                ],
            ),
        ]
    }
}

/// One list row: a note header (grouped mode) or one result.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Header { path: String, members: Vec<usize> },
    Item(usize),
}

/// Re-style every occurrence of a highlighted string inside `spans`
/// (the preview's wrapped, markdown-styled lines no longer share byte
/// offsets with the paragraph, so matches are found by text).
fn highlight_spans(spans: Vec<Span<'static>>, needles: &[&str]) -> Vec<Span<'static>> {
    if needles.is_empty() {
        return spans;
    }
    let hl = Style::default()
        .fg(palette::BLACK)
        .bg(palette::SECONDARY)
        .add_modifier(Modifier::BOLD);
    let mut out = Vec::with_capacity(spans.len());
    for span in spans {
        let text = span.content.to_string();
        let mut marks: Vec<(usize, usize)> = Vec::new();
        for n in needles.iter().filter(|n| !n.is_empty()) {
            marks.extend(text.match_indices(n).map(|(at, m)| (at, at + m.len())));
        }
        if marks.is_empty() {
            out.push(span);
            continue;
        }
        marks.sort_unstable();
        let mut at = 0;
        for (s, e) in marks {
            if s < at {
                continue;
            }
            if s > at {
                out.push(Span::styled(text[at..s].to_string(), span.style));
            }
            out.push(Span::styled(text[s..e].to_string(), span.style.patch(hl)));
            at = e;
        }
        if at < text.len() {
            out.push(Span::styled(text[at..].to_string(), span.style));
        }
    }
    out
}

/// Split the inner area into a 3-row query box and the feed split.
fn split_query_box(area: Rect) -> (Rect, Rect) {
    use ratatui::layout::{Constraint, Direction, Layout};
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    (rows[0], rows[1])
}

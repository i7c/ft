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
use std::sync::LazyLock;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use ft_core::graph::delete::{apply_delete, plan_delete};
use ft_core::graph::preset;
use ft_core::graph::query::{parse as parse_query, GraphQuery};
use ft_core::graph::rename::{
    apply_rename_plan, collect_directory_notes, plan_multi_rename, plan_rename,
};
use ft_core::graph::{Graph, NodeKey, NodeKind, NoteId};

use std::sync::Arc;

use ft_core::periodic::Period;
use ft_core::search::Hit;
use ft_core::task::ops::{self, CompleteOptions, CreateInput, CreateOptions, Position};
use ft_core::task::{Priority, Status};

use crate::tui::{
    command::{Command, CommandDef, CommandOutcome, CommandScope},
    event::Event,
    help::HelpSection,
    keymap::{KeyChord, KeyMap},
    modal::{
        ActiveModal, ConfirmChoice, ConfirmDeleteState, CreateSubdirState, Modal, ModalOutcome,
    },
    modal_commands as mc,
    notes_actions::{
        append::AppendState,
        capture::{self, CapturePresetPickerSource},
        create, queue_toast,
        section_move::{
            self, advance_to_multiselect, compose_with_existing_target, MoveCarry, MoveStep,
            SectionMoveState,
        },
    },
    palette,
    tab::{AppRequest, EventOutcome, Tab, TabCtx, ToastStyle},
    tabs::notes::view as notes_view,
    tabs::tasks::edit_popup::EditPopup,
    widgets::{
        render_inline_input, render_scroll_list, CursorMode, EditBuffer, FuzzyPicker, InlineInput,
        PickerOutcome, ScrollListOpts, VaultFilePickerSource,
    },
};

// ── Preset picker source ──────────────────────────────────────────────

pub struct PresetPickerSource {
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

// ── Search-in-tree picker ─────────────────────────────────────────────

/// One reachable node in the active view's policy-induced subgraph,
/// pre-computed at picker-open time. `path` is the shortest BFS path from
/// some root to `id` (inclusive of both endpoints); `leaf` is the same
/// string `TreeState::make_row` puts in `TreeRow.display`; `breadcrumb`
/// is the ancestor leafs joined with `/`.
#[derive(Debug, Clone)]
struct Candidate {
    path: Vec<NoteId>,
    leaf: String,
    breadcrumb: String,
    kind_char: char,
}

/// Render `path[..len-1]` as a path-like breadcrumb. Directory leafs end
/// with `/` and the vault root's leaf is `/`; naïve `join("/")` produces
/// doubled separators. This walker trims trailing slashes from each leaf
/// and prepends a single `/` when the ancestor chain starts at the root,
/// so `[root, Areas, operations]` renders `/Areas/operations` (not
/// `//Areas//operations/`).
fn format_breadcrumb(graph: &Graph, path: &[NoteId], today: chrono::NaiveDate) -> String {
    if path.len() <= 1 {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::with_capacity(path.len() - 1);
    for &aid in &path[..path.len() - 1] {
        let (s, _) = leaf_display(graph, aid, today);
        parts.push(s.trim_end_matches('/').to_string());
    }
    let rooted = parts.first().map(|s| s.is_empty()).unwrap_or(false);
    if rooted {
        format!("/{}", parts[1..].join("/"))
    } else {
        parts.join("/")
    }
}

/// BFS from `query.select(graph)` following `query.expand(graph, id)` as
/// the successor function. Cycles are handled by a visited set. Each
/// node is emitted at most once, at its shortest distance from a root;
/// ties resolved by BFS visit order (which itself depends on `query`'s
/// root ordering and the sorted child order in `query.expand`).
fn collect_search_candidates(
    graph: &Graph,
    query: &GraphQuery,
    today: chrono::NaiveDate,
) -> Vec<Candidate> {
    use std::collections::VecDeque;

    let roots = query.select(graph);
    let mut visited: HashSet<NoteId> = HashSet::with_capacity(roots.len());
    let mut queue: VecDeque<(NoteId, Vec<NoteId>)> = VecDeque::new();
    for r in &roots {
        if visited.insert(*r) {
            queue.push_back((*r, vec![*r]));
        }
    }

    let mut out: Vec<Candidate> = Vec::new();
    while let Some((id, path)) = queue.pop_front() {
        let (leaf, kind_char) = leaf_display(graph, id, today);
        let breadcrumb = format_breadcrumb(graph, &path, today);
        out.push(Candidate {
            path: path.clone(),
            leaf,
            breadcrumb,
            kind_char,
        });
        if let Some(children) = query.expand(graph, id) {
            for child in children {
                if visited.insert(child) {
                    let mut child_path = path.clone();
                    child_path.push(child);
                    queue.push_back((child, child_path));
                }
            }
        }
    }
    out
}

pub struct GraphSearchPickerSource {
    candidates: Vec<Candidate>,
    matcher: nucleo_matcher::Matcher,
    buf: Vec<char>,
}

impl GraphSearchPickerSource {
    fn new(graph: &Graph, query: &GraphQuery, today: chrono::NaiveDate) -> Self {
        Self {
            candidates: collect_search_candidates(graph, query, today),
            matcher: nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT),
            buf: Vec::new(),
        }
    }

    /// Build the rendered label string for a candidate: leaf, separator,
    /// breadcrumb. Pure so the test for label-format invariants doesn't
    /// have to construct a picker.
    fn format_label(c: &Candidate) -> String {
        if c.breadcrumb.is_empty() {
            c.leaf.clone()
        } else {
            format!("{}  ·  {}", c.leaf, c.breadcrumb)
        }
    }
}

impl crate::tui::widgets::PickerSource for GraphSearchPickerSource {
    type Item = Vec<NoteId>;

    fn query(
        &mut self,
        q: &str,
        limit: usize,
    ) -> Vec<crate::tui::widgets::PickerItem<Vec<NoteId>>> {
        let pat = nucleo_matcher::pattern::Pattern::parse(
            q,
            nucleo_matcher::pattern::CaseMatching::Smart,
            nucleo_matcher::pattern::Normalization::Smart,
        );
        let mut ranked: Vec<(u32, usize, Vec<u32>)> = Vec::new();
        for (i, c) in self.candidates.iter().enumerate() {
            let haystack_str = if c.breadcrumb.is_empty() {
                c.leaf.clone()
            } else {
                format!("{} {}", c.leaf, c.breadcrumb)
            };
            self.buf.clear();
            let haystack = nucleo_matcher::Utf32Str::new(&haystack_str, &mut self.buf);
            let mut indices = Vec::new();
            if let Some(score) = pat.indices(haystack, &mut self.matcher, &mut indices) {
                ranked.push((score, i, indices));
            }
        }
        ranked.sort_by_key(|b| std::cmp::Reverse(b.0));
        ranked
            .into_iter()
            .take(limit)
            .map(|(_, i, raw_indices)| {
                let c = &self.candidates[i];
                let leaf_chars = c.leaf.chars().count() as u32;
                // Highlight indices in the haystack `"{leaf} {breadcrumb}"`
                // line up with `format_label`'s `"{leaf}  ·  {breadcrumb}"`
                // only inside the leaf portion (positions < leaf_chars).
                // Drop matches that land in the breadcrumb to avoid
                // misaligned highlights — the separator widths differ.
                let match_indices: Vec<u32> = raw_indices
                    .into_iter()
                    .filter(|idx| *idx < leaf_chars)
                    .collect();
                crate::tui::widgets::PickerItem {
                    label: GraphSearchPickerSource::format_label(c),
                    match_indices,
                    data: c.path.clone(),
                }
            })
            .collect()
    }

    fn initial_items(&mut self, limit: usize) -> Vec<crate::tui::widgets::PickerItem<Vec<NoteId>>> {
        self.candidates
            .iter()
            .take(limit)
            .map(|c| crate::tui::widgets::PickerItem {
                label: GraphSearchPickerSource::format_label(c),
                match_indices: Vec::new(),
                data: c.path.clone(),
            })
            .collect()
    }
}

// ── CapturePickerModal ────────────────────────────────────────────────

/// Modal wrapper around the quick-capture preset picker
/// (extract-modal-driver §4). Carries an optional `target_note_override`
/// so the modal can pass the selected note (if any) into
/// [`capture::try_execute_preset`] without reaching back into the host
/// tab's selection state.
///
/// On `Enter`:
/// - `Executed` → return `Closed`. The preset committed via the
///   `AppRequest::OpenInEditor` it queued.
/// - `NeedsVars(state)` → return `OpenSibling(ActiveModal::CaptureVar(state))`.
///   First real use of `OpenSibling`; the modal driver swaps the slot
///   in one event-loop iteration so the user goes straight from the
///   picker selection to the first var prompt.
/// - `Err(msg)` → queue an error toast and return `Closed`.
pub struct CapturePickerModal {
    inner: FuzzyPicker<CapturePresetPickerSource>,
    target_note_override: Option<PathBuf>,
}

impl CapturePickerModal {
    pub fn new(source: CapturePresetPickerSource, target_note_override: Option<PathBuf>) -> Self {
        Self {
            inner: FuzzyPicker::new(source),
            target_note_override,
        }
    }
}

impl Modal for CapturePickerModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match self.inner.handle_key(k) {
            PickerOutcome::Selected(name) => {
                match capture::try_execute_preset(ctx, &name, self.target_note_override.clone()) {
                    Ok(capture::CaptureResult::Executed) => ModalOutcome::Closed,
                    Ok(capture::CaptureResult::NeedsVars(vs)) => {
                        ModalOutcome::OpenSibling(Box::new(ActiveModal::CaptureVar(vs)))
                    }
                    Err(e) => {
                        queue_toast(ctx, &e, ToastStyle::Error);
                        ModalOutcome::Closed
                    }
                }
            }
            PickerOutcome::Cancelled => ModalOutcome::Closed,
            PickerOutcome::StillOpen => ModalOutcome::Consumed,
            PickerOutcome::NotHandled => ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, _ctx: &TabCtx) {
        notes_view::render_picker_popup(
            frame,
            area,
            " quick capture · preset ",
            &mut self.inner,
            &[("Enter", "run"), ("Esc", "cancel")],
            None,
        );
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Quick capture",
            &[
                ("Type", "filter"),
                ("↑ / ↓", "navigate"),
                ("Enter", "run preset"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "capture-picker"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::CAPTURE_PICKER_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::CAPTURE_PICKER_KEYMAP
    }
}

// ── PresetPickerModal ─────────────────────────────────────────────────

/// Modal wrapper around the preset picker (extract-modal-driver §4).
/// Two open paths: `Ctrl+N` opens with `for_active_view = false`
/// (caller pre-pushed a blank view); `Ctrl+P` opens with
/// `for_active_view = true` (applies to existing active view).
///
/// On `Enter`: resolve the picked preset name to its DSL string, post
/// `AppRequest::GraphApplyPreset(dsl)` and return `Closed`.
///
/// On `Esc` with `for_active_view = false`: the pre-pushed blank view
/// drops into edit mode via `AppRequest::GraphFocusQueryBar`. With
/// `for_active_view = true`: no action — just close.
pub struct PresetPickerModal {
    inner: FuzzyPicker<PresetPickerSource>,
    for_active_view: bool,
}

impl PresetPickerModal {
    pub fn new(source: PresetPickerSource, for_active_view: bool) -> Self {
        Self {
            inner: FuzzyPicker::new(source),
            for_active_view,
        }
    }
}

impl Modal for PresetPickerModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match self.inner.handle_key(k) {
            PickerOutcome::Selected(name) => {
                let dsl = ctx
                    .vault
                    .config
                    .config
                    .graph
                    .presets
                    .get(&name)
                    .cloned()
                    .or_else(|| preset::builtin(&name).map(|s| s.to_string()));
                if let Some(dsl) = dsl {
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphApplyPreset(dsl));
                }
                ModalOutcome::Closed
            }
            PickerOutcome::Cancelled => {
                if !self.for_active_view {
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphFocusQueryBar);
                }
                ModalOutcome::Closed
            }
            PickerOutcome::StillOpen => ModalOutcome::Consumed,
            PickerOutcome::NotHandled => ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, _ctx: &TabCtx) {
        let popup_area = centered_rect(60, 60, area);
        frame.render_widget(Clear, popup_area);
        self.inner.render(frame, popup_area);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Preset picker",
            &[
                ("Type", "filter"),
                ("↑ / ↓", "navigate"),
                ("Enter", "apply preset"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "preset-picker"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::PRESET_PICKER_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::PRESET_PICKER_KEYMAP
    }
}

// ── SearchPickerModal ─────────────────────────────────────────────────

/// Modal wrapper around the in-tree fuzzy search picker
/// (extract-modal-driver §4). Owns the [`FuzzyPicker`] for the
/// duration of the modal's lifetime; on `Enter` posts
/// [`AppRequest::GraphJumpToNodes`] back to the Graph tab so the
/// cursor jumps to the chosen node, auto-expanding ancestors.
pub struct SearchPickerModal {
    inner: FuzzyPicker<GraphSearchPickerSource>,
}

impl SearchPickerModal {
    pub fn new(source: GraphSearchPickerSource) -> Self {
        Self {
            inner: FuzzyPicker::new(source),
        }
    }
}

impl Modal for SearchPickerModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match self.inner.handle_key(k) {
            PickerOutcome::Selected(path) => {
                *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphJumpToNodes(path));
                ModalOutcome::Closed
            }
            PickerOutcome::Cancelled => ModalOutcome::Closed,
            PickerOutcome::StillOpen => ModalOutcome::Consumed,
            PickerOutcome::NotHandled => ModalOutcome::NotHandled,
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, _ctx: &TabCtx) {
        let popup_area = centered_rect(60, 60, area);
        frame.render_widget(Clear, popup_area);
        let [picker_area, footer_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(popup_area);
        self.inner.render(frame, picker_area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Enter: jump · Esc: cancel",
                Style::default().fg(palette::DIM),
            ))),
            footer_area,
        );
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Graph search",
            &[
                ("Type", "filter"),
                ("↑ / ↓", "navigate"),
                ("Enter", "jump to node"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "search"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::SEARCH_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::SEARCH_KEYMAP
    }
}

// ── GraphTab ──────────────────────────────────────────────────────────

/// Fallback query the first view of the graph tab seeds itself with on
/// first focus when `[graph].default_query` isn't set in config. Shows
/// the vault root as a single directory line — pressing Enter / `l`
/// expands one hop. Kept here (and not in `ft-core`) because it's a
/// TUI-presentation default, not an engine concern.
const BUILTIN_DEFAULT_QUERY: &str = concat!(
    "node where path = \"\"; ",
    "expand where edge.kind in {directory-contains, link, embed};",
);

/// Width budget for a view's tab-strip label query snippet, in characters.
const VIEW_LABEL_QUERY_WIDTH: usize = 20;

// ── Commands ─────────────────────────────────────────────────────────

/// Every command the Graph tab exposes through the command/keymap
/// layer. Modal-launch commands (`graph.create-blank`, `graph.append`,
/// `graph.quick-capture`, `graph.move`, `graph.rename`, `graph.related`,
/// `graph.search`, `graph.preset-pick`) are tagged `opens_modal: true`
/// — `ft do` rejects them since they need interactive input.
pub(crate) static GRAPH_COMMANDS: &[CommandDef] = &[
    // Multi-view bindings
    CommandDef {
        name: "graph.add-view",
        description: "Add a new view (pick preset or blank)",
        scope: CommandScope::Tab("graph"),
        group: "Views",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.preset-pick",
        description: "Load a preset into the active view",
        scope: CommandScope::Tab("graph"),
        group: "Views",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.close-view",
        description: "Close the active view",
        scope: CommandScope::Tab("graph"),
        group: "Views",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.next-view",
        description: "Switch to the next view",
        scope: CommandScope::Tab("graph"),
        group: "Views",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.prev-view",
        description: "Switch to the previous view",
        scope: CommandScope::Tab("graph"),
        group: "Views",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.switch-view",
        description: "Switch to the view at the given 0-based index",
        scope: CommandScope::Tab("graph"),
        group: "Views",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.related",
        description: "Open the Related panel for the selected note",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.journal",
        description: "Open the Journal tab for the selected note or ghost",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.add-to-journal-sources",
        description: "Append selected (or cursor) Note/Ghost rows to the Journal tab's sources",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    // Query bar
    CommandDef {
        name: "graph.query-bar",
        description: "Open the query bar to edit the active view's query",
        scope: CommandScope::Tab("graph"),
        group: "Query",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.rewrite-for-root",
        description: "Re-root the active view's query on the selected node",
        scope: CommandScope::Tab("graph"),
        group: "Query",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.search",
        description: "Open the in-tree fuzzy search picker",
        scope: CommandScope::Tab("graph"),
        group: "Query",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    // Navigation
    CommandDef {
        name: "graph.cursor-down",
        description: "Move the cursor down one row",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.cursor-up",
        description: "Move the cursor up one row",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.expand-or-collapse",
        description: "Expand the selected node (or collapse if already expanded)",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.collapse-or-jump-parent",
        description: "Collapse the selected node (or jump to parent)",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.cursor-first",
        description: "Jump to the first row",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.cursor-last",
        description: "Jump to the last row",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.cursor-half-page-down",
        description: "Move the cursor down half a page",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.cursor-half-page-up",
        description: "Move the cursor up half a page",
        scope: CommandScope::Tab("graph"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    // Notes — open / create / append / capture / move / rename
    CommandDef {
        name: "graph.open-in-editor",
        description: "Open the selected note in $EDITOR",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.open-in-obsidian",
        description: "Open the selected note in Obsidian",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.create-blank",
        description: "Create a new note (blank) in the selected folder",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.create-from-template",
        description: "Create a new note from a template",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.append",
        description: "Append a template to the selected note",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.quick-capture",
        description: "Quick capture (run a preset)",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.move",
        description: "Enter the move-section flow (source from selected)",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.rename-or-multi-move",
        description: "Rename the selected node (or move multi-selection)",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.refresh",
        description: "Refresh the graph from disk",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.delete",
        description: "Delete the selected note or directory",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.create-subdir",
        description: "Create a subdirectory under the selected directory",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    // Periodic notes
    CommandDef {
        name: "graph.periodic-leader",
        description: "Navigate to periodic note in graph (then d/w/m/q/y)",
        scope: CommandScope::Tab("graph"),
        group: "Periodic notes",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.today",
        description: "Navigate to today's daily note in graph",
        scope: CommandScope::Tab("graph"),
        group: "Periodic notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    // Tasks — interaction verbs on focused Task rows
    // (graph-task-interaction §7). All are no-ops (toast) on non-Task rows.
    CommandDef {
        name: "graph.task-complete",
        description: "Complete the focused task",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-cancel",
        description: "Cancel the focused task",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-due-next",
        description: "Advance the focused task's due date by one day",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-due-prev",
        description: "Move the focused task's due date back one day",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-scheduled-next",
        description: "Advance the focused task's scheduled date by one day",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-scheduled-prev",
        description: "Move the focused task's scheduled date back one day",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-priority-next",
        description: "Cycle the focused task's priority up",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-priority-prev",
        description: "Cycle the focused task's priority down",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-due-today",
        description: "Set the focused task's due date to today",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-edit-popup",
        description: "Open the task edit form on the focused task",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-leader",
        description: "Task-create leader (then c=create, s=subtask)",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-create",
        description: "Create a new top-level task (via the task leader)",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.task-new-subtask",
        description: "Create a subtask under the focused task (via the task leader)",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.tasks-of-note",
        description: "Rewrite the view to the focused note's task subtree",
        scope: CommandScope::Tab("graph"),
        group: "Tasks",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    // Multi-select
    CommandDef {
        name: "graph.toggle-multi-select",
        description: "Toggle multi-selection on the focused row",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "graph.clear-multi-select",
        description: "Clear the multi-selection (Esc)",
        scope: CommandScope::Tab("graph"),
        group: "Notes",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
];

/// Default keymap for the Graph tab. Per-modal flows are routed
/// through the App-level `ActiveModal` slot and bypass this keymap
/// entirely (the modal driver dispatches keys to the modal first).
pub(crate) static GRAPH_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        // Views
        .bind("Ctrl+n", "graph.add-view")
        .bind("Ctrl+p", "graph.preset-pick")
        .bind("Ctrl+w", "graph.close-view")
        .bind("Ctrl+PageDown", "graph.next-view")
        .bind("Ctrl+PageUp", "graph.prev-view")
        // Cross-tab
        .bind("R", "graph.related")
        .bind("J", "graph.journal")
        .bind("Ctrl+j", "graph.add-to-journal-sources")
        // Query bar / search
        .bind("/", "graph.query-bar")
        .bind("z", "graph.rewrite-for-root")
        .bind("f", "graph.search")
        // Navigation — vim + arrow aliases
        .bind("j", "graph.cursor-down")
        .bind("Down", "graph.cursor-down")
        .bind("k", "graph.cursor-up")
        .bind("Up", "graph.cursor-up")
        .bind("Enter", "graph.expand-or-collapse")
        .bind("l", "graph.expand-or-collapse")
        .bind("h", "graph.collapse-or-jump-parent")
        .bind("g", "graph.cursor-first")
        .bind("G", "graph.cursor-last")
        .bind("Ctrl+d", "graph.cursor-half-page-down")
        .bind("Ctrl+u", "graph.cursor-half-page-up")
        // Notes
        .bind("o", "graph.open-in-editor")
        .bind("Ctrl+o", "graph.open-in-obsidian")
        .bind("c", "graph.create-blank")
        .bind("C", "graph.create-from-template")
        .bind("A", "graph.append")
        .bind("Q", "graph.quick-capture")
        .bind("m", "graph.move")
        .bind("r", "graph.rename-or-multi-move")
        .bind("Ctrl+r", "graph.refresh")
        .bind("d", "graph.delete")
        .bind("n", "graph.create-subdir")
        // Periodic
        .bind("p", "graph.periodic-leader")
        .bind("t", "graph.today")
        // Tasks — interaction verbs on focused Task rows
        // (graph-task-interaction §7). `p`/`t` are taken by the periodic
        // flow, so priority cycles on `=`/`-` and due-today on `T`.
        .bind("x", "graph.task-complete")
        .bind("X", "graph.task-cancel")
        .bind("]", "graph.task-due-next")
        .bind("[", "graph.task-due-prev")
        .bind("}", "graph.task-scheduled-next")
        .bind("{", "graph.task-scheduled-prev")
        .bind("=", "graph.task-priority-next")
        .bind("-", "graph.task-priority-prev")
        .bind("T", "graph.task-due-today")
        .bind("e", "graph.task-edit-popup")
        .bind("a", "graph.task-leader")
        .bind("v", "graph.tasks-of-note")
        // Multi-select
        .bind("Space", "graph.toggle-multi-select")
        .bind("Esc", "graph.clear-multi-select")
        // Alt+1..9 → switch view (with `index` arg)
        .bind_with_args("Alt+1", "graph.switch-view", &[("index", "0")])
        .bind_with_args("Alt+2", "graph.switch-view", &[("index", "1")])
        .bind_with_args("Alt+3", "graph.switch-view", &[("index", "2")])
        .bind_with_args("Alt+4", "graph.switch-view", &[("index", "3")])
        .bind_with_args("Alt+5", "graph.switch-view", &[("index", "4")])
        .bind_with_args("Alt+6", "graph.switch-view", &[("index", "5")])
        .bind_with_args("Alt+7", "graph.switch-view", &[("index", "6")])
        .bind_with_args("Alt+8", "graph.switch-view", &[("index", "7")])
        .bind_with_args("Alt+9", "graph.switch-view", &[("index", "8")])
});

pub struct GraphTab {
    graph: Option<Graph>,
    views: Vec<ExpandedView>,
    active: usize,
    /// Vault-relative path of a note whose Related modal should open
    /// on the next focus once the graph is built. Set by
    /// [`crate::tui::App`] when the TUI was launched via
    /// `ft notes update-related`.
    queued_related_path: Option<PathBuf>,
    /// Effective keymap: static defaults overlaid with user config.
    keymap: crate::tui::keymap::KeyMap,
}

/// Inline rename-in-place state — the modal owns its edit buffer and
/// the node identity. Migrated through `ActiveModal` in
/// extract-modal-driver §4; commits via `AppRequest::GraphCommitRename`
/// so the host can plan/apply/refresh against the in-memory graph.
#[derive(Debug)]
pub struct GraphRenameState {
    note_id: NoteId,
    is_directory: bool,
    buffer: EditBuffer,
    source_rel: PathBuf,
}

impl GraphRenameState {
    pub fn for_note(note_id: NoteId, title: &str, source_rel: PathBuf) -> Self {
        Self {
            note_id,
            is_directory: false,
            buffer: EditBuffer::from(title),
            source_rel,
        }
    }

    pub fn for_directory(note_id: NoteId, name: &str, source_rel: PathBuf) -> Self {
        Self {
            note_id,
            is_directory: true,
            buffer: EditBuffer::from(name),
            source_rel,
        }
    }
}

impl Modal for GraphRenameState {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match k.code {
            KeyCode::Esc => ModalOutcome::Closed,
            KeyCode::Enter => {
                let new_name = self.buffer.text.trim().to_string();
                if new_name.is_empty() {
                    queue_toast(ctx, "name cannot be empty", ToastStyle::Error);
                    return ModalOutcome::Consumed;
                }
                if new_name.contains('/') {
                    queue_toast(
                        ctx,
                        "name cannot contain / — use move (Space-select + r) to change directories",
                        ToastStyle::Error,
                    );
                    return ModalOutcome::Consumed;
                }
                *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphCommitRename {
                    note_id: self.note_id,
                    is_directory: self.is_directory,
                    source_rel: self.source_rel.clone(),
                    new_name,
                });
                // The modal closes here; if the host's commit hits a
                // recoverable error (target exists, plan failure, etc.)
                // it re-opens the modal via OpenModal so the user can
                // edit the name and retry — preserving the
                // pre-migration UX (`reopen_on_error` in `commit_rename`).
                ModalOutcome::Closed
            }
            // All edits + cursor moves + readline chords (Ctrl+A/E,
            // Alt+B/F/D, etc.) go through the buffer's EDIT_KEYMAP.
            // Unrecognised chords are still Consumed so they don't
            // leak through to tab- or global-level bindings.
            _ => {
                let _ = self.buffer.handle_event(k);
                ModalOutcome::Consumed
            }
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, _ctx: &TabCtx) {
        let popup_area = centered_rect(60, 30, area);
        frame.render_widget(Clear, popup_area);
        let [title_area, buf_area, footer_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(popup_area);
        let title = if self.is_directory {
            "Rename directory"
        } else {
            "Rename note"
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                title,
                Style::default()
                    .fg(palette::BLACK)
                    .bg(palette::WHITE)
                    .add_modifier(Modifier::BOLD),
            ))),
            title_area,
        );
        let buf_text = &self.buffer.text;
        let buf_display = if buf_text.is_empty() {
            " ".to_string()
        } else {
            buf_text.clone()
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                buf_display,
                Style::default().fg(palette::SECONDARY),
            ))),
            buf_area,
        );
        let footer = "Enter: commit · Esc: discard";
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                footer,
                Style::default().fg(palette::DIM),
            ))),
            footer_area,
        );
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Rename",
            &[
                ("Type", "edit name"),
                ("Enter", "commit"),
                ("Esc", "discard"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "rename"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::RENAME_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::RENAME_KEYMAP
    }
}

// ── TaskEdit modal (graph-task-edit-modal §2) ──────────────────────────

/// Full-form task edit popup hosted on the Graph tab. Wraps the shared
/// [`EditPopup`] in edit mode plus the task's `(path, line)` identity so
/// the commit can post `AppRequest::GraphTaskEdit`. Render + validation
/// reuse the Tasks-tab helpers lifted into `edit_popup`.
pub struct TaskEditState {
    pub popup: crate::tui::tabs::tasks::edit_popup::EditPopup,
    pub path: PathBuf,
    pub line: usize,
}

impl Modal for TaskEditState {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        // Ctrl+S submits regardless of focused field; Enter submits too.
        let submit = (k.code == KeyCode::Char('s') && k.modifiers.contains(KeyModifiers::CONTROL))
            || k.code == KeyCode::Enter;
        if submit {
            return self.commit(ctx);
        }
        match k.code {
            KeyCode::Esc => ModalOutcome::Closed,
            KeyCode::Tab => {
                self.popup.focus = self.popup.next_field();
                ModalOutcome::Consumed
            }
            KeyCode::BackTab => {
                self.popup.focus = self.popup.prev_field();
                ModalOutcome::Consumed
            }
            KeyCode::Down => {
                self.popup.focus = self.popup.next_field();
                ModalOutcome::Consumed
            }
            KeyCode::Up => {
                self.popup.focus = self.popup.prev_field();
                ModalOutcome::Consumed
            }
            _ => {
                let _ = self.popup.focused_buffer_mut().handle_event(k);
                ModalOutcome::Consumed
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        crate::tui::tabs::tasks::edit_popup::render_edit_popup(frame, area, &mut self.popup);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Edit task",
            &[
                ("Tab / Shift+Tab", "next / prev field"),
                ("↑ / ↓", "navigate fields"),
                ("Ctrl+S / Enter", "save"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "task-edit"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::TASK_EDIT_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::TASK_EDIT_KEYMAP
    }

    fn dispatch_command(&mut self, _cmd: &Command, _ctx: &TabCtx) -> CommandOutcome {
        CommandOutcome::NotHandled
    }
}

impl TaskEditState {
    /// Validate the popup fields and post `GraphTaskEdit`. Mirrors the
    /// Tasks-tab `submit_popup` validation, minus the target/move field
    /// (edits don't move the task).
    fn commit(&mut self, ctx: &TabCtx) -> ModalOutcome {
        use crate::tui::tabs::tasks::edit_popup::{
            merge_tags_into_description, parse_optional_date, parse_priority, parse_tags_field,
            EditField,
        };
        let due = match parse_optional_date(&self.popup.due.text, ctx.today) {
            Ok(v) => v,
            Err(e) => {
                self.popup.error = Some(format!("due: {e}"));
                self.popup.focus = EditField::Due;
                return ModalOutcome::Consumed;
            }
        };
        let scheduled = match parse_optional_date(&self.popup.scheduled.text, ctx.today) {
            Ok(v) => v,
            Err(e) => {
                self.popup.error = Some(format!("scheduled: {e}"));
                self.popup.focus = EditField::Scheduled;
                return ModalOutcome::Consumed;
            }
        };
        let priority = match parse_priority(&self.popup.priority.text) {
            Ok(v) => v,
            Err(e) => {
                self.popup.error = Some(e);
                self.popup.focus = EditField::Priority;
                return ModalOutcome::Consumed;
            }
        };
        let recurrence = self.popup.recurrence.text.trim();
        let recurrence = (!recurrence.is_empty()).then(|| recurrence.to_string());
        let raw_description = self.popup.description.text.trim().to_string();
        let tags = parse_tags_field(&self.popup.tags.text);
        let description = merge_tags_into_description(&raw_description, &tags);
        if description.is_empty() {
            self.popup.error = Some("description is empty".into());
            self.popup.focus = EditField::Description;
            return ModalOutcome::Consumed;
        }
        *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphTaskEdit {
            path: self.path.clone(),
            line: self.line,
            fields: (description, due, scheduled, priority, tags, recurrence),
        });
        ModalOutcome::Closed
    }
}

// ── TaskLeader modal (graph-task-edit-modal §4) ───────────────────────

/// Two-key leader (`a` then `c`/`s`) for creating tasks from the Graph
/// tab. Seeded at open time with the focused row's note path and (if a
/// Task is focused) its `(file, line)` so `c`/`s` can open the create
/// popup with the right target/parent. Mirrors `PeriodicLeader`: any
/// other key closes it.
pub struct TaskLeader {
    /// Note path to seed the new task's `target` field with (the focused
    /// Note, or a focused Task's source note). `None` falls back to the
    /// daily note at commit time.
    pub seed_note: Option<PathBuf>,
    /// The focused Task's `(source_file, source_line)`, used as the parent
    /// when creating a subtask. `None` → `s` toasts "select a task first".
    pub focused_task: Option<(PathBuf, usize)>,
}

impl TaskLeader {
    /// Build the seeded create popup the leader hands off to. Top-level
    /// seeds the `target` field from `seed_note`; subtask leaves it blank
    /// (the parent's file wins on commit).
    fn create_modal(&self, subtask_parent: Option<(PathBuf, usize)>) -> ActiveModal {
        let mut popup = EditPopup::new_blank();
        if subtask_parent.is_none() {
            if let Some(p) = &self.seed_note {
                popup.target = EditBuffer::from(&p.display().to_string());
            }
        }
        ActiveModal::TaskCreate(Box::new(TaskCreateState {
            popup,
            subtask_parent,
        }))
    }
}

impl Modal for TaskLeader {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match k.code {
            KeyCode::Char('c') => ModalOutcome::OpenSibling(Box::new(self.create_modal(None))),
            KeyCode::Char('s') => match self.focused_task.clone() {
                Some(parent) => {
                    ModalOutcome::OpenSibling(Box::new(self.create_modal(Some(parent))))
                }
                None => {
                    queue_toast(ctx, "select a task first", ToastStyle::Error);
                    ModalOutcome::Closed
                }
            },
            _ => ModalOutcome::Closed,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        use ratatui::widgets::Clear;
        let area = crate::tui::tabs::tasks::edit_popup::centered_rect(40, 12, area);
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" task: c=create · s=subtask · Esc=cancel ")
            .style(Style::default().bg(palette::BLACK));
        frame.render_widget(block, area);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Task create",
            &[
                ("c", "new top-level task"),
                ("s", "new subtask"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "task-leader"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::TASK_LEADER_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::TASK_LEADER_KEYMAP
    }
}

// ── TaskCreate modal (graph-task-edit-modal §4) ───────────────────────

/// Full-form task *create* popup hosted on the Graph tab. Wraps the
/// shared [`EditPopup`] in New mode plus an optional subtask parent.
/// Render + validation reuse the Tasks-tab helpers; on `Ctrl+S` it posts
/// `AppRequest::GraphTaskCommitCreate`, which the Graph tab services via
/// `ops::create_task`. `Enter` on the `target` field opens the file
/// picker (matching the Tasks-tab create flow), so only `Ctrl+S` submits.
pub struct TaskCreateState {
    pub popup: EditPopup,
    pub subtask_parent: Option<(PathBuf, usize)>,
}

impl Modal for TaskCreateState {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        use crate::tui::tabs::tasks::edit_popup::{
            handle_target_picker_key, open_target_picker, EditField,
        };
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };

        // While the target picker is open every key routes to it.
        if self.popup.target_picker.is_some() {
            handle_target_picker_key(&mut self.popup, k);
            return ModalOutcome::Consumed;
        }

        // Ctrl+S submits regardless of focused field.
        if k.code == KeyCode::Char('s') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return self.commit(ctx);
        }

        // On the target field, Enter or a printable char opens the file
        // picker (seeded with that keystroke) — never inserts inline.
        if self.popup.focus == EditField::Target {
            match (k.code, k.modifiers) {
                (KeyCode::Enter, _) => {
                    open_target_picker(&mut self.popup, ctx, None);
                    return ModalOutcome::Consumed;
                }
                (KeyCode::Char(c), m)
                    if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
                {
                    open_target_picker(&mut self.popup, ctx, Some(c));
                    return ModalOutcome::Consumed;
                }
                _ => {}
            }
        }

        match k.code {
            KeyCode::Esc => ModalOutcome::Closed,
            KeyCode::Tab | KeyCode::Down => {
                self.popup.focus = self.popup.next_field();
                ModalOutcome::Consumed
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.popup.focus = self.popup.prev_field();
                ModalOutcome::Consumed
            }
            _ => {
                let _ = self.popup.focused_buffer_mut().handle_event(k);
                ModalOutcome::Consumed
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        crate::tui::tabs::tasks::edit_popup::render_edit_popup(frame, area, &mut self.popup);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Create task",
            &[
                ("Tab / Shift+Tab", "next / prev field"),
                ("Enter", "pick target file (on target field)"),
                ("Ctrl+S", "create"),
                ("Esc", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "task-create"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::TASK_CREATE_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::TASK_CREATE_KEYMAP
    }

    fn dispatch_command(&mut self, _cmd: &Command, _ctx: &TabCtx) -> CommandOutcome {
        CommandOutcome::NotHandled
    }
}

impl TaskCreateState {
    /// Validate the popup fields and post `GraphTaskCommitCreate`. Mirrors
    /// the Tasks-tab `submit_popup` validation; disk resolution (target /
    /// duplicate) happens in the Graph-tab servicing hook and surfaces as
    /// a toast on error.
    fn commit(&mut self, ctx: &TabCtx) -> ModalOutcome {
        use crate::tui::tabs::tasks::edit_popup::{
            merge_tags_into_description, parse_optional_date, parse_priority, parse_tags_field,
            EditField,
        };
        let due = match parse_optional_date(&self.popup.due.text, ctx.today) {
            Ok(v) => v,
            Err(e) => {
                self.popup.error = Some(format!("due: {e}"));
                self.popup.focus = EditField::Due;
                return ModalOutcome::Consumed;
            }
        };
        let scheduled = match parse_optional_date(&self.popup.scheduled.text, ctx.today) {
            Ok(v) => v,
            Err(e) => {
                self.popup.error = Some(format!("scheduled: {e}"));
                self.popup.focus = EditField::Scheduled;
                return ModalOutcome::Consumed;
            }
        };
        let priority = match parse_priority(&self.popup.priority.text) {
            Ok(v) => v,
            Err(e) => {
                self.popup.error = Some(e);
                self.popup.focus = EditField::Priority;
                return ModalOutcome::Consumed;
            }
        };
        let recurrence = self.popup.recurrence.text.trim();
        let recurrence = (!recurrence.is_empty()).then(|| recurrence.to_string());
        let raw_description = self.popup.description.text.trim().to_string();
        let tags = parse_tags_field(&self.popup.tags.text);
        let description = merge_tags_into_description(&raw_description, &tags);
        if description.is_empty() {
            self.popup.error = Some("description is empty".into());
            self.popup.focus = EditField::Description;
            return ModalOutcome::Consumed;
        }
        *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphTaskCommitCreate {
            fields: (description, due, scheduled, priority, tags, recurrence),
            target: self.popup.target.text.trim().to_string(),
            subtask_parent: self.subtask_parent.clone(),
        });
        ModalOutcome::Closed
    }
}

/// Related panel modal state. Built on `R` keypress against a Note
/// row (or via `ft notes update-related`). A unified read + write
/// surface: it shows the scored concepts (`ft notes related` prints
/// the same data) and optionally commits checked concepts to the
/// note's `## Related` section. Splits scored concepts into two
/// visual groups: entries already in N's Related section
/// (non-interactive, marked) followed by suggested candidates the
/// user toggles with Space. Note-only — ghost rows toast (a ghost
/// has no file to write; reading-for-ghosts is via `ft notes related`).
#[derive(Debug)]
pub struct RelatedModal {
    /// The note whose Related panel is open.
    target_path: PathBuf,
    target_title: String,
    /// Concepts already in the Related section (alias links inside
    /// the section's body). Rendered as non-interactive "✓" rows.
    already: Vec<ft_core::related::RelatedScore>,
    /// Candidates not yet in the Related section. The cursor moves
    /// through this slice; Space toggles `checked` membership.
    candidates: Vec<ft_core::related::RelatedScore>,
    /// Titles the user has checked for inclusion. Keyed by title
    /// (graph NoteIds aren't durable across rebuilds, but titles
    /// are good enough for this short-lived UI state).
    checked: HashSet<String>,
    cursor: usize,
}

impl RelatedModal {
    /// Move cursor through the candidate list. No-op when there are
    /// no candidates (already-in-related rows are non-interactive).
    fn move_cursor(&mut self, delta: isize) {
        if self.candidates.is_empty() {
            return;
        }
        let len = self.candidates.len() as isize;
        let new = (self.cursor as isize + delta).clamp(0, len - 1);
        self.cursor = new as usize;
    }

    fn toggle_current(&mut self) {
        let Some(c) = self.candidates.get(self.cursor) else {
            return;
        };
        let key = c.title.clone();
        if !self.checked.remove(&key) {
            self.checked.insert(key);
        }
    }

    /// Collected concept titles in the same order they appear in
    /// `candidates` (deterministic).
    fn selected_titles(&self) -> Vec<String> {
        self.candidates
            .iter()
            .filter(|c| self.checked.contains(&c.title))
            .map(|c| c.title.clone())
            .collect()
    }
}

impl Modal for RelatedModal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        match (k.code, k.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('q'), KeyModifiers::NONE) => ModalOutcome::Closed,
            (KeyCode::Enter, _) => {
                let titles = self.selected_titles();
                if !titles.is_empty() {
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphConfirmRelated {
                        target_path: self.target_path.clone(),
                        selected_titles: titles,
                    });
                }
                ModalOutcome::Closed
            }
            (KeyCode::Char(' '), _) => {
                self.toggle_current();
                ModalOutcome::Consumed
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.move_cursor(-1);
                ModalOutcome::Consumed
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.move_cursor(1);
                ModalOutcome::Consumed
            }
            _ => ModalOutcome::Consumed,
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, _ctx: &TabCtx) {
        render_related_modal(frame, area, self);
    }

    fn keymap_help(&self) -> HelpSection {
        HelpSection::new(
            "Related",
            &[
                ("↑/↓ · j/k", "move cursor"),
                ("Space", "toggle"),
                ("Enter", "commit"),
                ("Esc / q", "cancel"),
            ],
        )
    }

    fn name(&self) -> &'static str {
        "related"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::RELATED_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::RELATED_KEYMAP
    }
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

/// Build the fuzzy file/directory picker used by the move flow's
/// `t`-fallback. Pulled out of [`GraphTab`] so [`GraphMoveOuter`]'s
/// `Modal` impl can spawn pickers without borrowing the tab.
fn open_move_file_picker(ctx: &TabCtx) -> FuzzyPicker<VaultFilePickerSource> {
    FuzzyPicker::new(VaultFilePickerSource::new(
        Arc::clone(ctx.vault),
        Arc::clone(ctx.recents),
    ))
}

impl Modal for GraphMoveOuter {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome {
        let Event::Key(k) = ev else {
            return ModalOutcome::NotHandled;
        };
        // Take the variant by value so each branch can move owned
        // fields into its handler. `*self` is restored on
        // `Consumed`/`NotHandled` paths; `Closed`/`OpenSibling` paths
        // discard whatever's left in `*self` (the App swaps the slot).
        let prev = std::mem::replace(self, GraphMoveOuter::SourceFromTree);
        match prev {
            GraphMoveOuter::SourceFromTree => self.handle_source_from_tree(k, ctx),
            GraphMoveOuter::SourcePicker { picker } => self.handle_source_picker(picker, k, ctx),
            GraphMoveOuter::Inner(sms) => self.handle_inner(sms, k, ctx),
            GraphMoveOuter::TargetFromTree { carry } => self.handle_target_from_tree(carry, k, ctx),
            GraphMoveOuter::TargetPicker { picker, carry } => {
                self.handle_target_picker(picker, carry, k, ctx)
            }
            GraphMoveOuter::MoveTargetFromTree { selected } => {
                self.handle_move_target_from_tree(selected, k, ctx)
            }
            GraphMoveOuter::MoveTargetPicker { picker, selected } => {
                self.handle_move_target_picker(picker, selected, k, ctx)
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
        // Reconstruct the tab's strip area (top 1 row of the body) so
        // banners overwrite the view-tab strip exactly as the
        // pre-migration render arm did.
        let strip_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        match self {
            GraphMoveOuter::SourceFromTree => {
                render_move_banner(
                    frame,
                    strip_area,
                    "MOVE source · m: use selected · t: pick from list · Esc: cancel",
                );
            }
            GraphMoveOuter::SourcePicker { picker } => {
                // Forward through a throwaway `SectionMoveState::SourcePicking`
                // so the shared move overlay handles the picker chrome.
                // `mem::replace` lets us hand the picker over by value
                // without taking ownership of the variant.
                let mut wrap = SectionMoveState::SourcePicking {
                    picker: std::mem::replace(picker, open_move_file_picker(ctx)),
                };
                notes_view::render_move_overlay(frame, area, &mut wrap);
                if let SectionMoveState::SourcePicking { picker: orig } = wrap {
                    *picker = orig;
                }
            }
            GraphMoveOuter::Inner(sms) => {
                notes_view::render_move_overlay(frame, area, sms);
            }
            GraphMoveOuter::TargetFromTree { .. } => {
                render_move_banner(
                    frame,
                    strip_area,
                    "MOVE target · m: use selected · t: pick from list · Esc: back",
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
                    picker: std::mem::replace(picker, open_move_file_picker(ctx)),
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
            GraphMoveOuter::MoveTargetPicker { picker, .. } => {
                picker.render(frame, area);
            }
        }
    }

    fn keymap_help(&self) -> HelpSection {
        match self {
            GraphMoveOuter::SourceFromTree => HelpSection::new(
                "Move section · source",
                &[
                    ("m", "use selected as source"),
                    ("t", "pick source from list"),
                    ("Esc", "cancel"),
                ],
            ),
            GraphMoveOuter::SourcePicker { .. } => HelpSection::new(
                "Move section · source picker",
                &[
                    ("Type", "filter"),
                    ("↑ / ↓", "navigate"),
                    ("Enter", "pick source"),
                    ("Esc", "back to tree"),
                ],
            ),
            GraphMoveOuter::Inner(_) => HelpSection::new(
                "Move section",
                &[
                    ("Space", "toggle"),
                    ("↑ / ↓", "navigate"),
                    ("Enter", "confirm step"),
                    ("Esc", "cancel / back"),
                ],
            ),
            GraphMoveOuter::TargetFromTree { .. } => HelpSection::new(
                "Move section · target",
                &[
                    ("m", "use selected as target"),
                    ("t", "pick target from list"),
                    ("/", "refine tree"),
                    ("Esc", "back to headings"),
                ],
            ),
            GraphMoveOuter::TargetPicker { .. } => HelpSection::new(
                "Move section · target picker",
                &[
                    ("Type", "filter"),
                    ("↑ / ↓", "navigate"),
                    ("Enter", "pick target"),
                    ("Esc", "back to tree"),
                ],
            ),
            GraphMoveOuter::MoveTargetFromTree { .. } => HelpSection::new(
                "Move · target directory",
                &[
                    ("Enter / m", "confirm directory"),
                    ("t", "pick directory from list"),
                    ("Esc", "cancel"),
                ],
            ),
            GraphMoveOuter::MoveTargetPicker { .. } => HelpSection::new(
                "Move · directory picker",
                &[
                    ("Type", "filter"),
                    ("↑ / ↓", "navigate"),
                    ("Enter", "confirm directory"),
                    ("Esc", "back to tree"),
                ],
            ),
        }
    }

    fn name(&self) -> &'static str {
        "move"
    }

    fn commands(&self) -> &'static [CommandDef] {
        mc::MOVE_OUTER_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &mc::MOVE_OUTER_KEYMAP
    }
}

impl GraphMoveOuter {
    fn handle_source_from_tree(&mut self, k: KeyEvent, ctx: &TabCtx) -> ModalOutcome {
        match (k.code, k.modifiers) {
            (KeyCode::Char('m'), KeyModifiers::NONE) => {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::GraphMoveConfirmSourceFromTree);
                // The host hook re-opens SourceFromTree on a toast
                // path or advances to `Inner(...)` on success.
                ModalOutcome::Closed
            }
            (KeyCode::Char('t'), KeyModifiers::NONE) => ModalOutcome::OpenSibling(Box::new(
                ActiveModal::MoveOuter(GraphMoveOuter::SourcePicker {
                    picker: open_move_file_picker(ctx),
                }),
            )),
            (KeyCode::Esc, _) => ModalOutcome::Closed,
            _ => {
                *self = GraphMoveOuter::SourceFromTree;
                ModalOutcome::NotHandled
            }
        }
    }

    fn handle_source_picker(
        &mut self,
        mut picker: FuzzyPicker<VaultFilePickerSource>,
        k: KeyEvent,
        ctx: &TabCtx,
    ) -> ModalOutcome {
        match picker.handle_key(k) {
            PickerOutcome::Selected(hit) => match advance_to_multiselect(ctx, hit) {
                MoveStep::Transition(inner) => ModalOutcome::OpenSibling(Box::new(
                    ActiveModal::MoveOuter(GraphMoveOuter::Inner(inner)),
                )),
                // Toast was queued by advance_to_multiselect; drop back
                // to the tree-driven source phase so the user can
                // pick a different note.
                MoveStep::Finished => ModalOutcome::OpenSibling(Box::new(ActiveModal::MoveOuter(
                    GraphMoveOuter::SourceFromTree,
                ))),
                MoveStep::Stay | MoveStep::NotHandled => {
                    *self = GraphMoveOuter::SourcePicker { picker };
                    ModalOutcome::Consumed
                }
            },
            PickerOutcome::Cancelled => ModalOutcome::OpenSibling(Box::new(
                ActiveModal::MoveOuter(GraphMoveOuter::SourceFromTree),
            )),
            PickerOutcome::StillOpen => {
                *self = GraphMoveOuter::SourcePicker { picker };
                ModalOutcome::Consumed
            }
            PickerOutcome::NotHandled => {
                *self = GraphMoveOuter::SourcePicker { picker };
                ModalOutcome::NotHandled
            }
        }
    }

    fn handle_inner(
        &mut self,
        mut sms: SectionMoveState,
        k: KeyEvent,
        ctx: &TabCtx,
    ) -> ModalOutcome {
        // Drive the shared section-move state machine directly so we
        // can inspect the returned `MoveStep` and intercept the
        // `HeadingMultiSelect → TargetPicking` transition (we replace
        // the shared `TargetPicking` with our tree-driven
        // `TargetFromTree { carry }` instead).
        let step = section_move::handle_key(&mut sms, k, ctx);
        match step {
            MoveStep::Stay => {
                *self = GraphMoveOuter::Inner(sms);
                ModalOutcome::Consumed
            }
            MoveStep::NotHandled => {
                *self = GraphMoveOuter::Inner(sms);
                ModalOutcome::NotHandled
            }
            MoveStep::Finished => ModalOutcome::Closed,
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
                ModalOutcome::OpenSibling(Box::new(ActiveModal::MoveOuter(
                    GraphMoveOuter::TargetFromTree { carry },
                )))
            }
            MoveStep::Transition(next) => {
                *self = GraphMoveOuter::Inner(next);
                ModalOutcome::Consumed
            }
        }
    }

    fn handle_target_from_tree(
        &mut self,
        carry: MoveCarry,
        k: KeyEvent,
        ctx: &TabCtx,
    ) -> ModalOutcome {
        match (k.code, k.modifiers) {
            (KeyCode::Char('m'), KeyModifiers::NONE) => {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::GraphMoveConfirmTargetFromTree {
                        carry: Box::new(carry),
                    });
                ModalOutcome::Closed
            }
            (KeyCode::Char('t'), KeyModifiers::NONE) => ModalOutcome::OpenSibling(Box::new(
                ActiveModal::MoveOuter(GraphMoveOuter::TargetPicker {
                    picker: open_move_file_picker(ctx),
                    carry,
                }),
            )),
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                // `/` cancels the move flow and opens the host's
                // query bar on the active view. With the modal
                // driver there can only be one active modal at a
                // time, so the pre-migration UX (carry preserved
                // across the bar's lifetime) is no longer
                // expressible — the carry is dropped here. The host
                // hook picks the correct `view_id`.
                let _ = carry;
                *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphFocusQueryBar);
                ModalOutcome::Closed
            }
            (KeyCode::Esc, _) => {
                // Cancel back to the heading-multi-select with the
                // same carry data so the user can re-pick headings.
                ModalOutcome::OpenSibling(Box::new(ActiveModal::MoveOuter(GraphMoveOuter::Inner(
                    SectionMoveState::HeadingMultiSelect {
                        source_rel: carry.source_rel,
                        source_abs: carry.source_abs,
                        source_content: carry.source_content,
                        headings: carry.headings,
                        selected: carry.selected,
                        focus: carry.focus,
                    },
                ))))
            }
            _ => {
                // Pass tree-navigation keys through; keep self alive.
                *self = GraphMoveOuter::TargetFromTree { carry };
                ModalOutcome::NotHandled
            }
        }
    }

    fn handle_target_picker(
        &mut self,
        mut picker: FuzzyPicker<VaultFilePickerSource>,
        carry: MoveCarry,
        k: KeyEvent,
        ctx: &TabCtx,
    ) -> ModalOutcome {
        match picker.handle_key(k) {
            PickerOutcome::Selected(hit) => {
                if hit.path == carry.source_rel {
                    // Same-file: reopen picker with same instance.
                    queue_toast(
                        ctx,
                        "same-file move is out of scope — pick a different target",
                        ToastStyle::Error,
                    );
                    *self = GraphMoveOuter::TargetPicker { picker, carry };
                    return ModalOutcome::Consumed;
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
                        return ModalOutcome::OpenSibling(Box::new(ActiveModal::MoveOuter(
                            GraphMoveOuter::TargetFromTree { carry },
                        )));
                    }
                };
                match compose_with_existing_target(carry, hit.path, target_abs, target_content) {
                    MoveStep::Transition(inner) => ModalOutcome::OpenSibling(Box::new(
                        ActiveModal::MoveOuter(GraphMoveOuter::Inner(inner)),
                    )),
                    MoveStep::Finished => ModalOutcome::Closed,
                    MoveStep::Stay | MoveStep::NotHandled => ModalOutcome::Closed,
                }
            }
            PickerOutcome::Cancelled => ModalOutcome::OpenSibling(Box::new(
                ActiveModal::MoveOuter(GraphMoveOuter::TargetFromTree { carry }),
            )),
            PickerOutcome::StillOpen => {
                *self = GraphMoveOuter::TargetPicker { picker, carry };
                ModalOutcome::Consumed
            }
            PickerOutcome::NotHandled => {
                *self = GraphMoveOuter::TargetPicker { picker, carry };
                ModalOutcome::NotHandled
            }
        }
    }

    fn handle_move_target_from_tree(
        &mut self,
        selected: HashSet<NoteId>,
        k: KeyEvent,
        ctx: &TabCtx,
    ) -> ModalOutcome {
        match (k.code, k.modifiers) {
            (KeyCode::Enter, _) | (KeyCode::Char('m'), KeyModifiers::NONE) => {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::GraphMoveConfirmMoveTarget { selected });
                ModalOutcome::Closed
            }
            (KeyCode::Char('t'), KeyModifiers::NONE) => ModalOutcome::OpenSibling(Box::new(
                ActiveModal::MoveOuter(GraphMoveOuter::MoveTargetPicker {
                    picker: open_move_file_picker(ctx),
                    selected,
                }),
            )),
            (KeyCode::Esc, _) => {
                // Cancel: multi-selection was already taken by the
                // tab's `r` arm; just drop the modal.
                ModalOutcome::Closed
            }
            _ => {
                // Tree navigation keys pass through to the tab.
                *self = GraphMoveOuter::MoveTargetFromTree { selected };
                ModalOutcome::NotHandled
            }
        }
    }

    fn handle_move_target_picker(
        &mut self,
        mut picker: FuzzyPicker<VaultFilePickerSource>,
        selected: HashSet<NoteId>,
        k: KeyEvent,
        ctx: &TabCtx,
    ) -> ModalOutcome {
        match picker.handle_key(k) {
            PickerOutcome::Selected(hit) => {
                // Hand off to the host so it can plan + apply the
                // multi-rename to the chosen directory.
                *ctx.pending_request.borrow_mut() = Some(AppRequest::GraphMoveExecuteMultiMove {
                    selected,
                    dir_path: hit.path,
                });
                ModalOutcome::Closed
            }
            PickerOutcome::Cancelled => ModalOutcome::OpenSibling(Box::new(
                ActiveModal::MoveOuter(GraphMoveOuter::MoveTargetFromTree { selected }),
            )),
            PickerOutcome::StillOpen => {
                *self = GraphMoveOuter::MoveTargetPicker { picker, selected };
                ModalOutcome::Consumed
            }
            PickerOutcome::NotHandled => {
                *self = GraphMoveOuter::MoveTargetPicker { picker, selected };
                ModalOutcome::NotHandled
            }
        }
    }
}

impl GraphTab {
    pub fn new() -> Self {
        Self {
            graph: None,
            views: vec![ExpandedView::default()],
            active: 0,
            queued_related_path: None,
            keymap: GRAPH_KEYMAP.clone(),
        }
    }

    /// Return a new `GraphTab` with the given keymap overlay applied.
    pub fn with_keymap_overlay(mut self, overlay: &crate::tui::keymap::KeymapOverlay) -> Self {
        self.keymap = GRAPH_KEYMAP.with_overlay(overlay);
        self
    }

    /// Return the `NoteId` of the currently-selected Note row, or
    /// `None` for non-Note rows (directories, ghosts, paragraphs).
    fn selected_note_id(&self) -> Option<NoteId> {
        let graph = self.graph.as_ref()?;
        let v = self.active_view();
        let row = v.tree.rows().get(v.selected)?;
        matches!(graph.node(row.note_id), NodeKind::Note(_)).then_some(row.note_id)
    }

    /// Handle a key while the Related panel modal is open.
    /// Compute scores and build the Related panel modal for the
    /// note at `note_path`. Returns `None` when the path doesn't
    /// resolve to a real note. Caller is responsible for posting
    /// `AppRequest::OpenModal(Related(...))`.
    fn build_related_modal_for_path(&self, note_path: &Path, ctx: &TabCtx) -> Option<RelatedModal> {
        let graph = self.graph.as_ref()?;
        let note_id = graph.note_by_path(note_path)?;
        self.build_related_modal_for_id(note_id, ctx)
    }

    /// Build the Related panel modal for a known `NoteId`. Toasts on
    /// errors (non-note row, scoring failure). Ghost rows are
    /// rejected here — a ghost has no file to write, so the panel is
    /// Note-only; ghost reading is via `ft notes related`.
    fn build_related_modal_for_id(&self, note_id: NoteId, ctx: &TabCtx) -> Option<RelatedModal> {
        let graph = self.graph.as_ref()?;
        let NodeKind::Note(note_data) = graph.node(note_id) else {
            queue_toast(
                ctx,
                "select a Note row — ghosts are read via `ft notes related`",
                ToastStyle::Error,
            );
            return None;
        };
        let target_path = note_data.path.clone();
        let target_title = note_data.title.clone();
        let scores = match ft_core::related::score_related(graph, note_id, ctx.vault) {
            Ok(s) => s,
            Err(e) => {
                queue_toast(ctx, &format!("scoring failed: {e}"), ToastStyle::Error);
                return None;
            }
        };
        let (already, candidates): (Vec<_>, Vec<_>) =
            scores.into_iter().partition(|s| s.already_in_related);
        Some(RelatedModal {
            target_path,
            target_title,
            already,
            candidates,
            checked: HashSet::new(),
            cursor: 0,
        })
    }

    /// Apply the modal's selected concepts to the target note via
    /// the `ft-core::related` plan/apply pair. Called by
    /// `Tab::graph_confirm_related` when the Related modal commits.
    fn confirm_related(
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

    /// Confirm the currently-selected node as move source.
    ///
    /// Called by [`Tab::graph_move_confirm_source_from_tree`] after the
    /// [`GraphMoveOuter::SourceFromTree`] modal posts
    /// [`AppRequest::GraphMoveConfirmSourceFromTree`] on `m`. Validates the
    /// selection, calls [`advance_to_multiselect`], and either advances the
    /// flow by posting `OpenModal(MoveOuter(Inner(...)))` or — on toast paths
    /// (non-Note row, IO error, no headings) — re-opens `SourceFromTree` so
    /// the user can navigate and retry.
    fn confirm_source_from_tree(&mut self, ctx: &TabCtx) {
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
    /// Called by [`Tab::graph_move_confirm_target_from_tree`] after the
    /// [`GraphMoveOuter::TargetFromTree`] modal posts
    /// [`AppRequest::GraphMoveConfirmTargetFromTree`] on `m`. The modal
    /// hands the [`MoveCarry`] through the round-trip so this method can
    /// re-open `TargetFromTree` (carry intact) on a recoverable error
    /// (non-Note selection, same-file pick).
    fn confirm_target_from_tree(&mut self, ctx: &TabCtx, carry: MoveCarry) {
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
    /// Called by [`Tab::graph_move_confirm_move_target`] after the
    /// [`GraphMoveOuter::MoveTargetFromTree`] modal posts
    /// [`AppRequest::GraphMoveConfirmMoveTarget`] on `m`/Enter. On a
    /// recoverable failure (no row / non-Directory) re-opens
    /// `MoveTargetFromTree` with `selected` intact so the user can navigate
    /// to a different row.
    fn confirm_move_target(&mut self, ctx: &TabCtx, selected: HashSet<NoteId>) {
        let Some(graph) = self.graph.as_ref() else {
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
    fn execute_multi_move(&mut self, ctx: &TabCtx, selected: &HashSet<NoteId>, target_dir: &Path) {
        let Some(graph) = self.graph.as_ref() else {
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

        let scan = ctx.vault.scan();
        if let Ok(new_graph) = Graph::build(ctx.vault, &scan) {
            self.graph = Some(new_graph);
            self.restore_all_views();
        }
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
            NodeKind::Paragraph(p) => p
                .source_file
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default(),
        }
    }

    /// Feed a key to the active create flow. Returns
    /// `EventOutcome::NotHandled` if no create flow is active (the
    /// caller's normal keymap can run).
    fn selected_note_abs_path(&self, ctx: &TabCtx) -> Option<PathBuf> {
        let graph = self.graph.as_ref()?;
        let id = self.selected_note_id()?;
        match graph.node(id) {
            NodeKind::Note(n) => Some(ctx.vault.path.join(&n.path)),
            _ => None,
        }
    }

    /// Build and apply the rename plan for the given node. Called by
    /// the `Tab::graph_commit_rename` hook when the rename modal
    /// commits. Toasts on success or failure; on success, refreshes
    /// the graph in place.
    fn commit_rename(
        &mut self,
        ctx: &TabCtx,
        note_id: NoteId,
        is_directory: bool,
        source_rel: PathBuf,
        new_name: &str,
    ) {
        let Some(graph) = self.graph.as_ref() else {
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
    /// `Ctrl+N` path: push a blank view, then open the preset picker
    /// with `for_active_view = false`. If no presets exist, just push
    /// the blank view and drop into input mode (no picker to open).
    fn add_view_with_presets(&mut self, ctx: &TabCtx) {
        let src = PresetPickerSource::new(ctx.vault);
        if src.items.is_empty() {
            self.add_view();
            return;
        }
        self.views.push(ExpandedView::default());
        self.active = self.views.len() - 1;
        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
            ActiveModal::PresetPicker(PresetPickerModal::new(src, false)),
        )));
    }

    /// `Ctrl+P` path: open the preset picker bound to the *current*
    /// active view. On selection the active view's query is replaced
    /// in-place; on dismiss nothing changes.
    fn open_preset_picker_for_active_view(&mut self, ctx: &TabCtx) {
        let src = PresetPickerSource::new(ctx.vault);
        if src.items.is_empty() {
            return;
        }
        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
            ActiveModal::PresetPicker(PresetPickerModal::new(src, true)),
        )));
    }

    /// Open a new blank view. Used when no presets exist (or by test
    /// code). The caller is responsible for posting
    /// `OpenModal(QueryBar)` if they want the new view to drop into
    /// input mode (the production `Ctrl+N` path does this; test code
    /// often doesn't).
    fn add_view(&mut self) {
        self.views.push(ExpandedView::default());
        self.active = self.views.len() - 1;
    }

    /// Apply a preset DSL string to the currently-active view. Called
    /// by the `Tab::graph_apply_preset` hook when the preset-picker
    /// modal commits.
    fn apply_preset_to_active_view(&mut self, dsl: &str) {
        let graph = self.graph.as_ref();
        let v = &mut self.views[self.active];
        v.set_query_text(dsl);
        v.apply_query(graph, ft_core::dates::today());
    }

    /// Land the cursor on the node at the end of `path`, auto-expanding
    /// every ancestor along the way. Writes the path components into
    /// `expanded_paths` and stores the full path in `selected_path` so the
    /// jump survives a subsequent graph refresh, then re-runs
    /// `restore_expansion` to materialize the tree.
    pub fn jump_to_path(&mut self, path: Vec<NoteId>) {
        if path.is_empty() {
            return;
        }
        let Some(graph) = self.graph.as_ref() else {
            return;
        };
        let key_path: Vec<NodeKey> = path.iter().map(|id| graph.stable_key(*id)).collect();
        let v = &mut self.views[self.active];
        if key_path.len() > 1 {
            v.add_expansion_path(key_path[..key_path.len() - 1].to_vec());
        }
        v.selected_path = Some(key_path);
        v.restore_expansion(graph, ft_core::dates::today());
        // Approximate visible-rows budget; render's scroll_to_selection
        // corrects against the real area on the next draw.
        v.scroll_to_selection(20);
    }

    /// BFS from the active query's roots to `target`, returning the
    /// shortest path (root-to-target inclusive) on first hit. Returns
    /// `None` if `target` is not reachable. Reuses the visited-set
    /// pattern from [`collect_search_candidates`] but stops early.
    fn find_node_path(&self, target: NoteId) -> Option<Vec<NoteId>> {
        use std::collections::VecDeque;

        let graph = self.graph.as_ref()?;
        let v = self.active_view();
        let query = v.query.as_ref()?;

        let roots = query.select(graph);
        let mut visited: HashSet<NoteId> = HashSet::with_capacity(roots.len());
        let mut queue: VecDeque<(NoteId, Vec<NoteId>)> = VecDeque::new();

        for r in &roots {
            if visited.insert(*r) {
                if *r == target {
                    return Some(vec![*r]);
                }
                queue.push_back((*r, vec![*r]));
            }
        }

        while let Some((id, path)) = queue.pop_front() {
            if let Some(children) = query.expand(graph, id) {
                for child in children {
                    if visited.insert(child) {
                        let mut child_path = path.clone();
                        child_path.push(child);
                        if child == target {
                            return Some(child_path);
                        }
                        queue.push_back((child, child_path));
                    }
                }
            }
        }
        None
    }

    /// Navigate to the periodic note for `period` within the active
    /// view's tree. Resolves the expected path (no file creation),
    /// looks up the NoteId, runs BFS from the query roots, and either
    /// jumps the cursor via [`jump_to_path`] or queues a toast when
    /// the note is unreachable.
    fn navigate_periodic(&mut self, ctx: &TabCtx, period: Period) {
        let Some(graph) = self.graph.as_ref() else {
            return;
        };
        let pn = &ctx.vault.config.config.periodic_notes;
        let cfg = match period {
            Period::Daily => pn.daily.as_ref(),
            Period::Weekly => pn.weekly.as_ref(),
            Period::Monthly => pn.monthly.as_ref(),
            Period::Quarterly => pn.quarterly.as_ref(),
            Period::Yearly => pn.yearly.as_ref(),
        };
        let Some(cfg) = cfg else {
            queue_toast(
                ctx,
                &format!("{} not configured", period.as_str()),
                ToastStyle::Error,
            );
            return;
        };

        let abs_path =
            match ft_core::periodic::resolve_periodic_path(&ctx.vault.path, cfg, ctx.today) {
                Ok(p) => p,
                Err(e) => {
                    queue_toast(ctx, &format!("{e}"), ToastStyle::Error);
                    return;
                }
            };

        let rel = match abs_path.strip_prefix(&ctx.vault.path) {
            Ok(p) => p.to_path_buf(),
            Err(_) => {
                queue_toast(ctx, "periodic note is outside the vault", ToastStyle::Error);
                return;
            }
        };

        let Some(note_id) = graph.note_by_path(&rel) else {
            queue_toast(
                ctx,
                &format!(
                    "{} note is not in the current graph results",
                    period.as_str()
                ),
                ToastStyle::Info,
            );
            return;
        };

        match self.find_node_path(note_id) {
            Some(path) => self.jump_to_path(path),
            None => {
                queue_toast(
                    ctx,
                    &format!(
                        "{} note is not in the current graph results",
                        period.as_str()
                    ),
                    ToastStyle::Info,
                );
            }
        }
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
            format!("node where kind in {{{kind_str}}} and path = \"{escaped_path}\"{expand_part}");

        let v = &mut self.views[self.active];
        v.set_query_text(new_query);
        v.apply_query(Some(graph), ft_core::dates::today());
    }

    /// Close the active view. If it's the last view, replace it with a
    /// fresh empty view so we never have zero views (avoids a special
    /// "no views" rendering path).
    fn close_view(&mut self) {
        if self.views.len() == 1 {
            self.views[0] = ExpandedView::default();
            return;
        }
        self.views.remove(self.active);
        if self.active >= self.views.len() {
            self.active = self.views.len() - 1;
        }
    }

    fn next_view(&mut self) {
        if self.views.len() <= 1 {
            return;
        }
        self.active = (self.active + 1) % self.views.len();
    }

    fn prev_view(&mut self) {
        if self.views.len() <= 1 {
            return;
        }
        self.active = (self.active + self.views.len() - 1) % self.views.len();
    }

    fn switch_view(&mut self, idx: usize) {
        if idx < self.views.len() {
            self.active = idx;
        }
    }

    /// Re-derive every view's tree from the current graph (used on
    /// `refresh()` and after the first `on_focus` populates the graph
    /// for views that already had a parsed query). Drops only the
    /// `multi_selected` keys whose underlying nodes have actually
    /// disappeared; existing marks survive a graph rebuild.
    fn restore_all_views(&mut self) {
        let Some(g) = self.graph.as_ref() else {
            return;
        };
        for v in self.views.iter_mut() {
            v.multi_selected.retain(|k| g.id_for_key(k).is_some());
            v.restore_expansion(g, ft_core::dates::today());
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
        match graph.node(row.note_id) {
            NodeKind::Note(n) => {
                let abs = ctx.vault.path.join(&n.path);
                ctx.recents.record_open(&n.path);
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::OpenInEditor { path: abs, line: 1 });
            }
            // graph-task-interaction §7.4: open the task's owning note at
            // the task's line, so the user lands on the task in context.
            NodeKind::Task(t) => {
                let abs = ctx.vault.path.join(&t.source_file);
                ctx.recents.record_open(&t.source_file);
                *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
                    path: abs,
                    line: t.source_line,
                });
            }
            _ => {}
        }
    }

    /// Resolve the focused `NodeKind::Task` row, run an `ops::*` mutation
    /// against its `(source_file, source_line)`, then rebuild the graph and
    /// restore the cursor to the same task. Mirrors the Tasks-tab
    /// `with_selected_task` / `refresh_after_mutation` pattern
    /// (graph-task-interaction §7.2). On a non-Task row it toasts and is a
    /// no-op. Returns `true` if the op ran.
    #[allow(clippy::too_many_lines)]
    fn with_focused_task<F>(&mut self, ctx: &mut TabCtx, verb: &str, op: F) -> bool
    where
        F: FnOnce(&std::path::Path, usize, chrono::NaiveDate) -> anyhow::Result<()>,
    {
        // Extract the focused task's identity up front so we drop the
        // immutable graph borrow before mutating `self` / refreshing.
        let (abs, anchor, today) = {
            let Some(graph) = self.graph.as_ref() else {
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
        match op(&abs, anchor.1, today) {
            Ok(()) => {}
            Err(e) => {
                queue_toast(ctx, &format!("{verb} failed: {e}"), ToastStyle::Error);
                return false;
            }
        }
        // Refresh the graph and try to land back on the same task.
        let scan = ctx.vault.scan();
        if let Ok(new_graph) = Graph::build(ctx.vault, &scan) {
            self.graph = Some(new_graph);
            self.restore_all_views();
            self.restore_task_cursor(&anchor);
        }
        true
    }

    /// Move the cursor to the row whose task matches `(source_file,
    /// source_line)`, if present in the active view's tree.
    fn restore_task_cursor(&mut self, anchor: &(std::path::PathBuf, usize)) {
        let Some(graph) = self.graph.as_ref() else {
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
    fn rewrite_view_to_note_tasks(&mut self, ctx: &mut TabCtx) {
        let Some(graph) = self.graph.as_ref() else {
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
        let graph = self.graph.as_ref();
        let view = &mut self.views[self.active];
        view.set_query_text(&query);
        view.apply_query(graph, ft_core::dates::today());
    }

    /// The `(source_file, source_line)` of the focused Task row, if any.
    fn focused_task_anchor(&self) -> Option<(PathBuf, usize)> {
        let graph = self.graph.as_ref()?;
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
    fn focused_seed_note(&self) -> Option<PathBuf> {
        let graph = self.graph.as_ref()?;
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
    fn focused_task_edit_state(
        &self,
    ) -> Option<(
        crate::tui::tabs::tasks::edit_popup::EditPopup,
        PathBuf,
        usize,
    )> {
        let graph = self.graph.as_ref()?;
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
}

impl Tab for GraphTab {
    fn title(&self) -> &str {
        "Graph"
    }

    fn host_popup_open(&self) -> bool {
        // The graph query bar is the only EditBuffer this tab mounts
        // behind a modal forwarder; check the active view's buffer.
        self.views
            .get(self.active)
            .map(|v| v.query_buf.popup_is_open())
            .unwrap_or(false)
    }

    #[cfg(test)]
    fn set_focused_buffer_completion_for_test(
        &mut self,
        provider: Box<dyn crate::tui::widgets::CompletionProvider>,
    ) {
        if let Some(v) = self.views.get_mut(self.active) {
            v.query_buf.set_completion(provider);
        }
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
            if v0.query_buf.text.trim().is_empty() {
                let seed = ctx
                    .vault
                    .config
                    .config
                    .graph
                    .default_query
                    .clone()
                    .unwrap_or_else(|| BUILTIN_DEFAULT_QUERY.to_string());
                v0.set_query_text(seed);
                let graph = self.graph.as_ref();
                v0.apply_query(graph, ft_core::dates::today());
            } else {
                // Re-derive every view's tree against the freshly-built
                // graph so trees materialize on first focus.
                self.restore_all_views();
            }
        }
        // If a queued Related modal was requested before the graph
        // existed (e.g. `ft notes update-related <note>`), open it now.
        if let Some(path) = self.queued_related_path.take() {
            if let Some(modal) = self.build_related_modal_for_path(&path, ctx) {
                *ctx.pending_request.borrow_mut() =
                    Some(AppRequest::OpenModal(Box::new(ActiveModal::Related(modal))));
            }
        }
        Ok(())
    }

    fn queue_related_modal(&mut self, note_path: &Path) {
        self.queued_related_path = Some(note_path.to_path_buf());
    }

    fn graph_jump_to_nodes(&mut self, path: Vec<NoteId>) {
        self.jump_to_path(path);
    }

    fn graph_apply_preset(&mut self, dsl: String) {
        self.apply_preset_to_active_view(&dsl);
    }

    fn graph_focus_query_bar(&mut self, ctx: &TabCtx) {
        *ctx.pending_request.borrow_mut() =
            Some(AppRequest::OpenModal(Box::new(ActiveModal::QueryBar {
                view_id: self.active,
            })));
    }

    fn graph_query_bar_key(&mut self, view_id: usize, key: crossterm::event::KeyEvent) {
        // Per-key forwarding from the `QueryBar` modal into the view's
        // `EditBuffer`. Ignores keys for non-active views so a
        // `view_id` racing a view-close becomes a no-op rather than a
        // panic.
        //
        // The buffer's `handle_event` runs the chord through
        // `EDIT_KEYMAP` (readline bindings: Ctrl+A/E/K/W, Alt+B/F/D,
        // …) and falls back to printable-char insert. Anything the
        // buffer didn't recognise is dropped here — the QueryBar modal
        // already consumed it at the modal layer so it never reaches
        // tab- or global-level bindings.
        if view_id >= self.views.len() {
            return;
        }
        self.active = view_id;
        let _ = self.active_view_mut().query_buf.handle_event(key);
    }

    fn graph_apply_query_bar(&mut self, view_id: usize) {
        if view_id >= self.views.len() {
            return;
        }
        self.active = view_id;
        let graph = self.graph.as_ref();
        self.views[self.active].apply_query(graph, ft_core::dates::today());
    }

    fn graph_commit_rename(
        &mut self,
        ctx: &TabCtx,
        note_id: NoteId,
        is_directory: bool,
        source_rel: PathBuf,
        new_name: String,
    ) {
        self.commit_rename(ctx, note_id, is_directory, source_rel, &new_name);
    }

    fn graph_confirm_related(
        &mut self,
        ctx: &TabCtx,
        target_path: PathBuf,
        selected_titles: Vec<String>,
    ) {
        self.confirm_related(ctx, target_path, selected_titles);
    }

    fn graph_move_confirm_source_from_tree(&mut self, ctx: &TabCtx) {
        self.confirm_source_from_tree(ctx);
    }

    fn graph_move_confirm_target_from_tree(&mut self, ctx: &TabCtx, carry: MoveCarry) {
        self.confirm_target_from_tree(ctx, carry);
    }

    fn graph_move_confirm_move_target(&mut self, ctx: &TabCtx, selected: HashSet<NoteId>) {
        self.confirm_move_target(ctx, selected);
    }

    fn graph_move_execute_multi_move(
        &mut self,
        ctx: &TabCtx,
        selected: HashSet<NoteId>,
        dir_path: PathBuf,
    ) {
        self.execute_multi_move(ctx, &selected, &dir_path);
    }

    fn graph_navigate_periodic(&mut self, ctx: &TabCtx, period: Period) {
        self.navigate_periodic(ctx, period);
    }

    fn graph_confirm_delete(&mut self, ctx: &TabCtx, target: PathBuf, is_directory: bool) {
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
                let scan = ctx.vault.scan();
                if let Ok(g) = Graph::build(ctx.vault, &scan) {
                    self.graph = Some(g);
                    self.restore_all_views();
                }
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

    fn graph_create_subdir(&mut self, ctx: &TabCtx, parent: PathBuf, name: String) {
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
                let scan = ctx.vault.scan();
                if let Ok(g) = Graph::build(ctx.vault, &scan) {
                    self.graph = Some(g);
                    self.restore_all_views();
                }
                queue_toast(ctx, &format!("created {}", display), ToastStyle::Success);
            }
            Err(e) => {
                queue_toast(ctx, &format!("create failed: {e}"), ToastStyle::Error);
            }
        }
    }

    /// Service `AppRequest::GraphTaskEdit`: apply the validated popup
    /// fields via `ops::update_task_line`, refresh, restore cursor
    /// (graph-task-edit-modal §3.4).
    fn graph_task_edit(
        &mut self,
        ctx: &TabCtx,
        path: PathBuf,
        line: usize,
        fields: crate::tui::tabs::tasks::edit_popup::PopupFields,
    ) {
        let abs = ctx.vault.path.join(&path);
        let (description, due, scheduled, priority, tags, recurrence) = fields;
        match ops::update_task_line(&abs, line, |t| {
            t.description = description;
            t.due = due;
            t.scheduled = scheduled;
            t.priority = priority;
            t.tags = tags;
            t.recurrence = recurrence;
        }) {
            Ok(_) => {
                let scan = ctx.vault.scan();
                if let Ok(g) = Graph::build(ctx.vault, &scan) {
                    self.graph = Some(g);
                    self.restore_all_views();
                    self.restore_task_cursor(&(path, line));
                }
                queue_toast(ctx, "task updated", ToastStyle::Success);
            }
            Err(e) => queue_toast(ctx, &format!("edit failed: {e}"), ToastStyle::Error),
        }
    }

    /// Service `AppRequest::GraphTaskCommitCreate`: resolve the target
    /// file + insertion position, write the new task via `ops::create_task`,
    /// then refresh the graph and land the cursor on it. Mirrors the
    /// Tasks-tab `submit_popup_new` (graph-task-edit-modal §4.3).
    fn graph_task_commit_create(
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
            input,
            CreateOptions {
                position,
                force: false,
            },
        ) {
            Ok(outcome) => {
                let rel = ctx.vault.relativize(&resolved).to_path_buf();
                let scan = ctx.vault.scan();
                if let Ok(g) = Graph::build(ctx.vault, &scan) {
                    self.graph = Some(g);
                    self.restore_all_views();
                    // Lands on the new task when it's already visible (a
                    // top-level task in an expanded note, say); a subtask of
                    // a collapsed parent stays hidden until the user expands.
                    self.restore_task_cursor(&(rel.clone(), outcome.line));
                }
                queue_toast(
                    ctx,
                    &format!("created {}:{}", rel.display(), outcome.line),
                    ToastStyle::Success,
                );
            }
            Err(e) => queue_toast(ctx, &format!("create failed: {e}"), ToastStyle::Error),
        }
    }

    fn commands(&self) -> &'static [CommandDef] {
        GRAPH_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &self.keymap
    }

    fn dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome {
        // Approximation; render's `scroll_to_selection` corrects.
        let vis = 20usize;
        match cmd.name {
            // Views
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
                let target = self.graph.as_ref().and_then(|graph| {
                    let row = self
                        .active_view()
                        .tree
                        .rows()
                        .get(self.active_view().selected)?;
                    match graph.node(row.note_id) {
                        NodeKind::Note(n) => {
                            Some(crate::tui::tab::JournalTarget::Note(n.path.clone()))
                        }
                        NodeKind::Ghost(g) => {
                            Some(crate::tui::tab::JournalTarget::Ghost(g.raw.clone()))
                        }
                        _ => None,
                    }
                });
                if let Some(target) = target {
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::JournalFor { target });
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
                let targets: Vec<crate::tui::tab::JournalTarget> = match self.graph.as_ref() {
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
                                    Some(crate::tui::tab::JournalTarget::Note(n.path.clone()))
                                }
                                NodeKind::Ghost(g) => {
                                    Some(crate::tui::tab::JournalTarget::Ghost(g.raw.clone()))
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
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::JournalAddSources {
                        targets,
                        default_mode: crate::tui::tab::AppendOrReplaceMode::Append,
                    });
                }
                CommandOutcome::Handled
            }
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
                if let (Some(g), Some(q)) = (self.graph.as_ref(), self.active_view().query.as_ref())
                {
                    let src = GraphSearchPickerSource::new(g, q, ctx.today);
                    *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
                        ActiveModal::Search(SearchPickerModal::new(src)),
                    )));
                }
                CommandOutcome::Handled
            }
            // Navigation
            "graph.cursor-down" => {
                let Some(g) = self.graph.as_ref() else {
                    return CommandOutcome::Handled;
                };
                let v = &mut self.views[self.active];
                v.selected = v.tree.move_selection_down(v.selected);
                v.refresh_selected_path(g);
                v.scroll_to_selection(vis);
                CommandOutcome::Handled
            }
            "graph.cursor-up" => {
                let Some(g) = self.graph.as_ref() else {
                    return CommandOutcome::Handled;
                };
                let v = &mut self.views[self.active];
                v.selected = v.tree.move_selection_up(v.selected);
                v.refresh_selected_path(g);
                v.scroll_to_selection(vis);
                CommandOutcome::Handled
            }
            "graph.expand-or-collapse" => {
                let graph = self.graph.as_ref();
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
                let Some(g) = self.graph.as_ref() else {
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
                let Some(g) = self.graph.as_ref() else {
                    return CommandOutcome::Handled;
                };
                let v = &mut self.views[self.active];
                v.selected = 0;
                v.scroll_offset = 0;
                v.refresh_selected_path(g);
                CommandOutcome::Handled
            }
            "graph.cursor-last" => {
                let Some(g) = self.graph.as_ref() else {
                    return CommandOutcome::Handled;
                };
                let v = &mut self.views[self.active];
                v.selected = v.tree.len().saturating_sub(1);
                v.refresh_selected_path(g);
                v.scroll_to_selection(vis);
                CommandOutcome::Handled
            }
            "graph.cursor-half-page-down" => {
                let Some(g) = self.graph.as_ref() else {
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
                let Some(g) = self.graph.as_ref() else {
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
            // Notes
            "graph.open-in-editor" => {
                self.request_open_selected_in_editor(ctx);
                CommandOutcome::Handled
            }
            // Task interaction (graph-task-interaction §7.3).
            "graph.task-complete" => {
                self.with_focused_task(
                    ctx,
                    "complete",
                    |path, line, today| match ops::complete_task(
                        path,
                        line,
                        CompleteOptions { on: today },
                    ) {
                        Ok(_) => Ok(()),
                        Err(ops::CompleteError::AlreadyDone { .. }) => Ok(()),
                        Err(e) => Err(e.into()),
                    },
                );
                CommandOutcome::Handled
            }
            "graph.task-cancel" => {
                self.with_focused_task(ctx, "cancel", |path, line, today| {
                    match ops::cancel_task(path, line, today) {
                        Ok(_) => Ok(()),
                        Err(ops::CancelError::AlreadyCancelled { .. }) => Ok(()),
                        Err(e) => Err(e.into()),
                    }
                });
                CommandOutcome::Handled
            }
            "graph.task-due-next" => {
                self.with_focused_task(ctx, "due+1", |path, line, today| {
                    nudge_task_field(path, line, TaskField::Due, 1, today)
                });
                CommandOutcome::Handled
            }
            "graph.task-due-prev" => {
                self.with_focused_task(ctx, "due-1", |path, line, today| {
                    nudge_task_field(path, line, TaskField::Due, -1, today)
                });
                CommandOutcome::Handled
            }
            "graph.task-scheduled-next" => {
                self.with_focused_task(ctx, "scheduled+1", |path, line, today| {
                    nudge_task_field(path, line, TaskField::Scheduled, 1, today)
                });
                CommandOutcome::Handled
            }
            "graph.task-scheduled-prev" => {
                self.with_focused_task(ctx, "scheduled-1", |path, line, today| {
                    nudge_task_field(path, line, TaskField::Scheduled, -1, today)
                });
                CommandOutcome::Handled
            }
            "graph.task-priority-next" => {
                self.with_focused_task(ctx, "priority+1", |path, line, _today| {
                    cycle_task_priority(path, line, 1)
                });
                CommandOutcome::Handled
            }
            "graph.task-priority-prev" => {
                self.with_focused_task(ctx, "priority-1", |path, line, _today| {
                    cycle_task_priority(path, line, -1)
                });
                CommandOutcome::Handled
            }
            "graph.task-due-today" => {
                self.with_focused_task(ctx, "due=today", |path, line, today| {
                    ops::update_task_line(path, line, |t| {
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
            "graph.create-blank" => {
                // Ghost shortcut: create the note instantly at the ghost's path.
                if let (Some(graph), Some(row)) = (
                    self.graph.as_ref(),
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
                            let scan = ctx.vault.scan();
                            if let Ok(g) = Graph::build(ctx.vault, &scan) {
                                self.graph = Some(g);
                                self.restore_all_views();
                            }
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
                    self.graph.as_ref(),
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
                let scan = ctx.vault.scan();
                if let Ok(g) = Graph::build(ctx.vault, &scan) {
                    self.graph = Some(g);
                    self.restore_all_views();
                }
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
                    let graph = self.graph.as_ref();
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
                let graph = self.graph.as_ref();
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
                let graph = self.graph.as_ref();
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
                let graph = self.graph.as_ref();
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
                let Some(g) = self.graph.as_ref() else {
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

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };

        // App-global Tab cycling & plain-digit tab-switch must beat the
        // tab keymap so the user can switch tabs from anywhere. Modified
        // digits (Alt+N) are view jumps and ARE in the keymap.
        if matches!(k.code, KeyCode::Tab | KeyCode::BackTab)
            || (matches!(k.code, KeyCode::Char(c) if c.is_ascii_digit())
                && k.modifiers == KeyModifiers::NONE)
        {
            return Ok(EventOutcome::NotHandled);
        }

        // Graph-missing or empty-tree gate: most keys would no-op or
        // toast because they need a selected row. Keep the gate, but
        // still let view-management and the query-bar through so the
        // user can recover from an empty result (e.g. Ctrl+P to pick a
        // different preset, Ctrl+W to close the view).
        let graph_missing = self.graph.is_none();
        let chord = KeyChord::from_key_event(k);
        let cmd = self.keymap.lookup(chord).cloned();
        if graph_missing || self.active_view().tree.is_empty() {
            let allowed = cmd.as_ref().is_some_and(|c| empty_tree_allows(c.name));
            if !allowed {
                return Ok(EventOutcome::NotHandled);
            }
        }

        // Tab keymap → dispatch_command.
        let Some(cmd) = cmd else {
            return Ok(EventOutcome::NotHandled);
        };
        Ok(match self.dispatch_command(&cmd, ctx) {
            CommandOutcome::Handled => EventOutcome::Consumed,
            CommandOutcome::NotHandled => EventOutcome::NotHandled,
        })
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx) {
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
                let sel_marker = self
                    .graph
                    .as_ref()
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
                let graph = self.graph.as_ref();
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
                    ("f", "search & jump to node in current view"),
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
                    ("A", "append template to selected note"),
                    ("Q", "quick capture (run a preset)"),
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
            HelpSection::new(
                "Related",
                &[
                    ("Shift+R", "open Related panel for the selected note"),
                    ("Space", "toggle candidate (in modal)"),
                    ("Enter", "append checked concepts (in modal)"),
                    ("Esc / q", "close modal without writing"),
                ],
            ),
            HelpSection::new(
                "Cross-tab",
                &[
                    ("Shift+J", "open Journal tab for the selected note"),
                    ("Ctrl+J", "append selected (or cursor) to Journal sources"),
                ],
            ),
            HelpSection::new(
                "Tasks (on a Task row)",
                &[
                    ("x", "complete task"),
                    ("Shift+X", "cancel task"),
                    ("] / [", "due date +1 / -1 day"),
                    ("} / {", "scheduled date +1 / -1 day"),
                    ("= / -", "cycle priority up / down"),
                    ("Shift+T", "set due date to today"),
                    ("e", "edit task (full form)"),
                    ("o", "open source note at the task line"),
                ],
            ),
            HelpSection::new(
                "Tasks (any row)",
                &[
                    ("a", "leader → c = new task · s = new subtask"),
                    ("v", "view the focused note's task subtree"),
                ],
            ),
        ]
    }

    #[cfg(test)]
    fn selected_is_note_for_test(&self) -> bool {
        self.selected_note_id().is_some()
    }
}

// ── ExpandedView ──────────────────────────────────────────────────────

/// Per-view state. A graph tab owns a `Vec<ExpandedView>` and renders the
/// active one. The view holds both *spec* fields (`query_buf`,
/// `expanded_paths`, `selected_path`) and *derived* fields (`tree`,
/// `selected`, `scroll_offset`); spec fields survive a graph rebuild and
/// drive the rebuild of derived fields via [`Self::restore_expansion`].
#[derive(Debug, Default)]
pub struct ExpandedView {
    /// Editable query text + cursor. Single source of truth for the
    /// query bar; the `QueryBar` modal forwards every key event into
    /// this buffer.
    query_buf: EditBuffer,
    parse_error: Option<String>,
    query: Option<GraphQuery>,
    /// Root-anchored paths the user has expanded. Each path is the
    /// sequence of [`NodeKey`]s from a root (inclusive) down to the
    /// expanded node (inclusive). Closed under prefixes by
    /// construction — expanding a child always implies its parents are
    /// also expanded. `NodeKey` (path-based) is used instead of
    /// `NoteId` so the set survives `Graph::build`.
    expanded_paths: HashSet<Vec<NodeKey>>,
    /// Path of the currently-selected row (root-to-leaf, inclusive).
    /// Used to restore selection across graph rebuilds; on a missing
    /// leaf we shed the tail and re-try until we hit an ancestor that
    /// still exists.
    selected_path: Option<Vec<NodeKey>>,
    /// Space-toggled multi-selection. When non-empty, `r` triggers Flow
    /// A (move to directory) instead of Flow B (rename in place).
    /// Stored as `NodeKey` so the marker set survives a graph rebuild
    /// (e.g. a sibling file is deleted while the marks stand).
    multi_selected: HashSet<NodeKey>,
    tree: TreeState,
    selected: usize,
    scroll_offset: usize,
}

impl ExpandedView {
    /// Replace the query text with `s` and place the cursor at the end.
    /// Used by seeding paths (preset apply, default query, `z`
    /// rewrites) — never on a user keystroke.
    fn set_query_text(&mut self, s: impl AsRef<str>) {
        self.query_buf = EditBuffer::from(s.as_ref());
    }

    /// Parse the editable query text, swap in the parsed query, and
    /// rebuild the tree against the current graph. Clears expansion
    /// state — a new query starts fresh.
    fn apply_query(&mut self, graph: Option<&Graph>, today: chrono::NaiveDate) {
        self.parse_error = None;
        if self.query_buf.text.trim().is_empty() {
            self.query = None;
            self.expanded_paths.clear();
            self.selected_path = None;
            self.tree = TreeState::default();
            self.selected = 0;
            self.scroll_offset = 0;
            return;
        }
        match parse_query(&self.query_buf.text) {
            Ok(q) => {
                self.query = Some(q);
                self.expanded_paths.clear();
                self.selected_path = None;
                self.selected = 0;
                self.scroll_offset = 0;
                if let Some(g) = graph {
                    let q = self.query.as_ref().unwrap();
                    let roots = q.select(g);
                    self.tree.build_from(&roots, g, q, today);
                    self.refresh_selected_path(g);
                }
            }
            Err(e) => self.parse_error = Some(e.to_string()),
        }
    }

    /// Re-derive the flat tree from the saved expansion paths against
    /// the given graph. Paths whose nodes no longer exist are dropped;
    /// selection falls back to the nearest restored ancestor (then
    /// row 0). `scroll_offset` is preserved — the next render's
    /// `scroll_to_selection(visible)` only moves the view if the new
    /// `selected` actually ended up off-screen.
    fn restore_expansion(&mut self, graph: &Graph, today: chrono::NaiveDate) {
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
        self.tree.build_from(&roots, graph, &query, today);

        // Replay expansions shortest-path-first so parents are expanded
        // before their children.
        let mut sorted: Vec<Vec<NodeKey>> = std::mem::take(&mut self.expanded_paths)
            .into_iter()
            .collect();
        sorted.sort_by_key(|p| p.len());
        let mut restored: HashSet<Vec<NodeKey>> = HashSet::new();
        for path in sorted {
            if let Some(idx) = self.find_row_for_path(&path, graph) {
                let already = self.tree.rows()[idx].expanded;
                if already || self.tree.expand_at(idx, graph, &query, today) {
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
                if let Some(idx) = self.find_row_for_path(&path[..len], graph) {
                    self.selected = idx;
                    break;
                }
                len -= 1;
            }
        }
        // Preserve `scroll_offset` deliberately — only the next render
        // (which knows the real visible budget) is allowed to scroll,
        // and only when `selected` is off-screen. This keeps the
        // viewport pinned across editor close / rename / delete.
        self.refresh_selected_path(graph);
    }

    /// Locate the row corresponding to a root-anchored path, walking
    /// only through currently-visible children of each step. `path` is
    /// expressed in build-stable [`NodeKey`]s; each step is converted
    /// to a current-build `NoteId` once via `graph.id_for_key`. Returns
    /// `None` if any step doesn't resolve or isn't in the visible tree.
    fn find_row_for_path(&self, path: &[NodeKey], graph: &Graph) -> Option<usize> {
        if path.is_empty() {
            return None;
        }
        // Resolve every key once; if any step is missing in the new
        // graph, the path is dead.
        let ids: Vec<NoteId> = path
            .iter()
            .map(|k| graph.id_for_key(k))
            .collect::<Option<Vec<_>>>()?;
        let rows = self.tree.rows();
        let mut idx = rows
            .iter()
            .position(|r| r.depth == 0 && r.note_id == ids[0])?;
        for &next in &ids[1..] {
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
    /// root-to-leaf path, expressed in build-stable [`NodeKey`]s.
    /// Returns an empty vec for out-of-bounds.
    fn path_to(&self, index: usize, graph: &Graph) -> Vec<NodeKey> {
        let rows = self.tree.rows();
        if index >= rows.len() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut next_depth = rows[index].depth + 1;
        for i in (0..=index).rev() {
            if rows[i].depth + 1 == next_depth {
                out.push(graph.stable_key(rows[i].note_id));
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
    fn add_expansion_path(&mut self, path: Vec<NodeKey>) {
        for i in 1..=path.len() {
            self.expanded_paths.insert(path[..i].to_vec());
        }
    }

    /// Drop a collapse target plus every path that extends it. Mirrors
    /// `TreeState::collapse_at`, which removes all descendant rows.
    fn forget_expansion_subtree(&mut self, path: &[NodeKey]) {
        self.expanded_paths.retain(|p| !starts_with(p, path));
    }

    fn refresh_selected_path(&mut self, graph: &Graph) {
        if self.tree.is_empty() {
            self.selected_path = None;
        } else {
            self.selected_path = Some(self.path_to(self.selected, graph));
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
        let s = self.query_buf.text.trim();
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
fn render_related_modal(frame: &mut Frame, area: Rect, modal: &RelatedModal) {
    let popup_area = centered_rect(70, 70, area);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Related: {} ", modal.target_title))
        .style(Style::default());
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // The `already`-in-Related rows are non-interactive, so they live in
    // a fixed header band above the scrolling candidate list (capped so a
    // long `already` list can't starve the candidates of rows). Only the
    // candidates scroll, with the cursor kept in view.
    let already_rows = (modal.already.len() as u16).min(inner.height / 3);
    let [summary_area, already_area, list_area, footer_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(already_rows),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let header_text = if modal.candidates.is_empty() && modal.already.is_empty() {
        "no co-occurring concepts found".to_string()
    } else {
        format!(
            "{} already in Related · {} candidate(s)",
            modal.already.len(),
            modal.candidates.len()
        )
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            header_text,
            Style::default().fg(palette::DIM),
        ))),
        summary_area,
    );

    if already_rows > 0 {
        let already_lines: Vec<Line> = modal
            .already
            .iter()
            .map(|s| {
                Line::from(vec![
                    Span::styled("  ✓  ", Style::default().fg(palette::SUCCESS)),
                    Span::styled(
                        format!("[[{}]]", s.title),
                        Style::default().fg(palette::DIM),
                    ),
                    Span::styled(
                        format!("  ({})", s.score),
                        Style::default().fg(palette::DIM),
                    ),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(already_lines), already_area);
    }

    let items: Vec<ListItem> = modal
        .candidates
        .iter()
        .map(|s| {
            let checked = modal.checked.contains(&s.title);
            let marker = if checked { "[x]" } else { "[ ]" };
            ListItem::new(Line::from(vec![
                Span::raw(format!("{marker} ")),
                Span::raw(format!("[[{}]]", s.title)),
                Span::styled(
                    format!("  ({})", s.score),
                    Style::default().fg(palette::DIM),
                ),
            ]))
        })
        .collect();
    let selected = (!modal.candidates.is_empty()).then_some(modal.cursor);
    render_scroll_list(
        frame,
        list_area,
        items,
        selected,
        ScrollListOpts {
            highlight_symbol: "▶ ",
            highlight_style: Style::default().add_modifier(Modifier::REVERSED),
            scrollbar: true,
        },
    );

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Space: toggle · Enter: confirm · Esc/q: cancel",
            Style::default().fg(palette::DIM),
        ))),
        footer_area,
    );
}

fn render_move_banner(frame: &mut Frame, area: Rect, text: &str) {
    let span = Span::styled(
        text,
        Style::default()
            .fg(palette::BLACK)
            .bg(palette::PRIMARY)
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
    pub fn build_from(
        &mut self,
        roots: &[NoteId],
        graph: &Graph,
        query: &GraphQuery,
        today: chrono::NaiveDate,
    ) {
        self.rows.clear();
        self.expansion_cache.clear();
        for id in roots {
            self.rows.push(Self::make_row(*id, 0, graph, query, today));
        }
    }

    pub fn expand_at(
        &mut self,
        index: usize,
        graph: &Graph,
        query: &GraphQuery,
        today: chrono::NaiveDate,
    ) -> bool {
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
                Self::make_row(*child_id, child_depth, graph, query, today),
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

    fn make_row(
        id: NoteId,
        depth: usize,
        graph: &Graph,
        query: &GraphQuery,
        today: chrono::NaiveDate,
    ) -> TreeRow {
        let (display, kind_char) = leaf_display(graph, id, today);
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

/// Commands that remain usable when the active view's tree is empty
/// (or before the graph has been built). Everything else needs a
/// selected row or query result and is gated off until the user
/// recovers the view via one of these.
fn empty_tree_allows(name: &str) -> bool {
    matches!(
        name,
        "graph.add-view"
            | "graph.preset-pick"
            | "graph.close-view"
            | "graph.next-view"
            | "graph.prev-view"
            | "graph.switch-view"
            | "graph.query-bar"
            | "graph.refresh"
    )
}

/// Parse a `TaskData` date string (YYYY-MM-DD) into a `NaiveDate`.
/// Used by [`leaf_display`] to render relative due/scheduled labels.
fn parse_task_date(s: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
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
    field: TaskField,
    delta_days: i64,
    today: chrono::NaiveDate,
) -> anyhow::Result<()> {
    use chrono::Duration;
    ops::update_task_line(path, line, move |t| {
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
fn cycle_task_priority(path: &std::path::Path, line: usize, dir: i64) -> anyhow::Result<()> {
    const CYCLE: [Option<Priority>; 6] = [
        None,
        Some(Priority::Lowest),
        Some(Priority::Low),
        Some(Priority::Medium),
        Some(Priority::High),
        Some(Priority::Highest),
    ];
    ops::update_task_line(path, line, move |t| {
        let pos = CYCLE.iter().position(|p| *p == t.priority).unwrap_or(0) as i64;
        let len = CYCLE.len() as i64;
        let next = ((pos + dir).rem_euclid(len)) as usize;
        t.priority = CYCLE[next];
    })
    .map(|_| ())
    .map_err(anyhow::Error::from)
}

/// Parse a priority DSL string ("Highest"/"High"/… ) into a `Priority`.
/// Mirrors the `as_str` spelling the graph exposes.
fn parse_priority(s: &str) -> Option<Priority> {
    match s {
        "Highest" => Some(Priority::Highest),
        "High" => Some(Priority::High),
        "Medium" => Some(Priority::Medium),
        "Low" => Some(Priority::Low),
        "Lowest" => Some(Priority::Lowest),
        _ => None,
    }
}

/// Foreground color for a node kind, used to visually differentiate types
/// in the tree view. Palette inspired by the Monokai theme.
fn node_kind_color(kind: &NodeKind) -> Color {
    match kind {
        NodeKind::Note(_) => Color::Rgb(166, 210, 50), // warm green
        NodeKind::Directory(_) => Color::Rgb(80, 190, 200), // warm cyan
        NodeKind::Ghost(_) => palette::DIM,            // warm gray
        NodeKind::Task(_) => palette::PRIMARY,         // orange
        NodeKind::Paragraph(_) => Color::Rgb(210, 150, 100), // warm tan/purple
    }
}

/// Leaf row text + kind char for a node. Single source of truth shared by
/// `TreeState::make_row` (tree rendering) and `collect_search_candidates`
/// (jump-to-node picker), so search labels always match what's visible in
/// the tree.
fn leaf_display(graph: &Graph, id: NoteId, today: chrono::NaiveDate) -> (String, char) {
    use crate::tui::tabs::tasks::edit_popup::relative_date;
    match graph.node(id) {
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
        NodeKind::Task(t) => {
            let marker = match t.status.as_str() {
                "Open" => "[ ]",
                "Done" => "[x]",
                "InProgress" => "[/]",
                "Cancelled" => "[-]",
                _ => "[ ]",
            };
            let mut s = format!("{marker} {}", t.description);
            if let Some(due) = t.due.as_deref().and_then(parse_task_date) {
                s.push_str(&format!("  📅 {}", relative_date(due, today)));
            }
            if let Some(sched) = t.scheduled.as_deref().and_then(parse_task_date) {
                s.push_str(&format!("  ⏳ {}", relative_date(sched, today)));
            }
            if let Some(p) = t.priority.as_deref() {
                let mark = match p {
                    "Highest" => "⏫",
                    "High" => "⏫",
                    "Medium" => "🔼",
                    "Low" => "🔽",
                    "Lowest" => "🔽",
                    _ => "",
                };
                if !mark.is_empty() {
                    s.push_str(&format!("  {mark}"));
                }
            }
            (s, 'T')
        }
        NodeKind::Paragraph(p) => {
            let snippet: String = p.text.chars().take(60).collect();
            let trunc = if p.text.chars().count() > 60 {
                format!("{snippet}…")
            } else {
                snippet
            };
            if p.line_start == p.line_end {
                (
                    format!("{}:{}  {trunc}", p.source_file.display(), p.line_start),
                    'P',
                )
            } else {
                (
                    format!(
                        "{}:{}-{}  {trunc}",
                        p.source_file.display(),
                        p.line_start,
                        p.line_end
                    ),
                    'P',
                )
            }
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

    /// Pinned "today" for graph-tab tests, so task relative-date labels in
    /// snapshots don't drift with the wall clock. Matches `fixed_clock`.
    const FT_TEST_TODAY: chrono::NaiveDate = match chrono::NaiveDate::from_ymd_opt(2026, 5, 12) {
        Some(d) => d,
        None => panic!("invalid test date"),
    };

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
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);
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
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);
        assert_eq!(state.rows.len(), 1);

        let changed = state.expand_at(0, &g, &q, FT_TEST_TODAY);
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
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);
        state.expand_at(0, &g, &q, FT_TEST_TODAY);
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
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);

        state.expand_at(0, &g, &q, FT_TEST_TODAY);
        assert_eq!(state.rows.len(), 4);
        assert!(state.rows[0].expanded);

        let changed = state.expand_at(0, &g, &q, FT_TEST_TODAY);
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
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);

        state.expand_at(0, &g, &q, FT_TEST_TODAY);
        assert_eq!(state.rows.len(), 4);

        let areas_idx = state
            .rows
            .iter()
            .position(|r| r.kind_char == 'D' && r.display == "Areas/")
            .unwrap();

        state.expand_at(areas_idx, &g, &q, FT_TEST_TODAY);
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
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);

        let changed = state.expand_at(0, &g, &q, FT_TEST_TODAY);
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
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);
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
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);

        state.expand_at(0, &g, &q, FT_TEST_TODAY);
        let first_len = state.rows.len();
        state.collapse_at(0);
        state.expand_at(0, &g, &q, FT_TEST_TODAY);
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
        state.build_from(&[root_id], &g, &q, FT_TEST_TODAY);

        // Pre-computed: not expandable, so attempting expand is a
        // no-op and `expanded` stays false (nothing was opened).
        assert!(!state.rows[0].expandable);
        let changed = state.expand_at(0, &g, &q, FT_TEST_TODAY);
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
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);
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
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);
        state.expand_at(0, &g, &q, FT_TEST_TODAY);

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
                    source_file: PathBuf::from("root.md"),
                    source_line: 1,
                    ..Default::default()
                },
                Task {
                    description: "Task two".into(),
                    status: Status::Done,
                    source_file: PathBuf::from("root.md"),
                    source_line: 2,
                    ..Default::default()
                },
            ],
            errors: vec![],
        };
        let g = Graph::build(&v, &scan).unwrap();

        // Query for task nodes only
        let q = parse_query("node where kind = Task;").unwrap();
        let mut state = TreeState::default();
        let roots = q.select(&g);
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);

        assert_eq!(state.rows.len(), 2);
        assert_eq!(state.rows[0].kind_char, 'T');
        assert_eq!(state.rows[0].display, "[ ] Task one");
        assert_eq!(state.rows[1].kind_char, 'T');
        assert_eq!(state.rows[1].display, "[x] Task two");
    }
}

#[cfg(test)]
mod view_tests {
    use std::path::PathBuf;

    use assert_fs::prelude::*;
    use ft_core::graph::Graph;
    use ft_core::vault::{Scan, Vault};

    use super::*;

    /// Pinned "today" for graph-tab tests, so task relative-date labels in
    /// snapshots don't drift with the wall clock. Matches `fixed_clock`.
    const FT_TEST_TODAY: chrono::NaiveDate = match chrono::NaiveDate::from_ymd_opt(2026, 5, 12) {
        Some(d) => d,
        None => panic!("invalid test date"),
    };

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
            query_buf: EditBuffer::from(dirs_query_text()),
            ..Default::default()
        };
        v.apply_query(Some(&g), FT_TEST_TODAY);
        (g, v)
    }

    /// Test helper: vault-relative path → `NodeKey::Directory`.
    fn dir_key(p: &str) -> NodeKey {
        NodeKey::Directory(std::path::PathBuf::from(p))
    }

    #[test]
    fn add_expansion_path_includes_all_prefixes() {
        let mut v = ExpandedView::default();
        let root = dir_key("");
        let areas = dir_key("Areas");
        let ops = dir_key("Areas/operations");
        v.add_expansion_path(vec![root.clone(), areas.clone(), ops.clone()]);
        assert!(v.expanded_paths.contains(&vec![root.clone()]));
        assert!(v
            .expanded_paths
            .contains(&vec![root.clone(), areas.clone()]));
        assert!(v.expanded_paths.contains(&vec![root, areas, ops]));
    }

    #[test]
    fn forget_expansion_subtree_removes_descendants() {
        let root = dir_key("");
        let areas = dir_key("Areas");
        let ops = dir_key("Areas/operations");
        let projects = dir_key("Projects");
        let mut v = ExpandedView::default();
        v.add_expansion_path(vec![root.clone(), areas.clone(), ops.clone()]);
        v.add_expansion_path(vec![root.clone(), projects.clone()]);
        v.forget_expansion_subtree(&[root.clone(), areas.clone()]);
        assert!(!v
            .expanded_paths
            .contains(&vec![root.clone(), areas.clone()]));
        assert!(!v.expanded_paths.contains(&vec![root.clone(), areas, ops]));
        // Untouched siblings stay.
        assert!(v.expanded_paths.contains(&vec![root.clone(), projects]));
        assert!(v.expanded_paths.contains(&vec![root]));
    }

    #[test]
    fn path_to_walks_back_to_root() {
        let (g, v) = view_with_query();
        assert_eq!(v.path_to(0, &g).len(), 1);
    }

    #[test]
    fn restore_expansion_walks_each_path() {
        let (g, mut v) = view_with_query();
        // Expand root then Areas/.
        let root_key = g.stable_key(v.tree.rows()[0].note_id);
        v.tree
            .expand_at(0, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        v.add_expansion_path(vec![root_key.clone()]);
        let areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        let areas_key = g.stable_key(v.tree.rows()[areas_idx].note_id);
        v.tree
            .expand_at(areas_idx, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        v.add_expansion_path(vec![root_key, areas_key]);
        let expected_len = v.tree.len();

        // Now drop and re-derive from spec.
        v.tree = TreeState::default();
        v.restore_expansion(&g, FT_TEST_TODAY);

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
        let root_key = g.stable_key(v.tree.rows()[0].note_id);
        v.tree
            .expand_at(0, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        v.add_expansion_path(vec![root_key.clone()]);
        // Inject a fictitious deeper path whose intermediate key
        // doesn't appear as a child of root in the visible tree —
        // restoration should drop it without panicking.
        let stale = NodeKey::Directory(std::path::PathBuf::from("does/not/exist"));
        v.expanded_paths.clear();
        v.expanded_paths.insert(vec![root_key.clone()]);
        v.expanded_paths
            .insert(vec![root_key, stale.clone(), dir_key("Areas/operations")]);
        v.tree = TreeState::default();
        v.restore_expansion(&g, FT_TEST_TODAY);
        // The valid path expanded the root.
        assert!(v.tree.rows()[0].expanded);
        // Verify expanded_paths retained only paths whose keys resolve.
        for path in &v.expanded_paths {
            for key in path {
                assert!(
                    g.id_for_key(key).is_some(),
                    "every restored path key must resolve in the graph"
                );
                assert_ne!(key, &stale, "stale key must have been dropped");
            }
        }
    }

    #[test]
    fn restore_expansion_preserves_selection_when_present() {
        let (g, mut v) = view_with_query();
        // Expand root, then select Areas/.
        v.tree
            .expand_at(0, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        let root_key = g.stable_key(v.tree.rows()[0].note_id);
        v.add_expansion_path(vec![root_key]);
        let areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        v.selected = areas_idx;
        v.refresh_selected_path(&g);

        // Drop derived state and restore.
        v.tree = TreeState::default();
        v.restore_expansion(&g, FT_TEST_TODAY);

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
        v.tree
            .expand_at(0, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        let root_key = g.stable_key(v.tree.rows()[0].note_id);
        v.add_expansion_path(vec![root_key.clone()]);
        // selected_path = [root, areas, ops]. Restoration only expands
        // root via expanded_paths, so areas isn't expanded → walker
        // stops at areas → selection falls back to that ancestor.
        let areas = g.node_by_path(std::path::Path::new("Areas")).unwrap();
        v.selected_path = Some(vec![
            root_key,
            dir_key("Areas"),
            dir_key("Areas/operations"),
        ]);
        v.tree = TreeState::default();
        v.restore_expansion(&g, FT_TEST_TODAY);

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
        v.restore_expansion(&g, FT_TEST_TODAY);
        assert_eq!(v.selected, 0);
    }

    /// Regression: a freshly-built `Graph` assigns new `NodeIndex`
    /// values (per the `NoteId` doc-comment, IDs aren't stable across
    /// builds). Expansion must survive the rebuild by way of the
    /// path-based `NodeKey`s, so the user-perceived "tree collapses
    /// after delete / rename / git-sync" bug doesn't return.
    #[test]
    fn restore_expansion_survives_full_rebuild() {
        let (g, mut v) = view_with_query();
        // Expand root and Areas/.
        let root_key = g.stable_key(v.tree.rows()[0].note_id);
        v.tree
            .expand_at(0, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        v.add_expansion_path(vec![root_key.clone()]);
        let areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        let areas_key = g.stable_key(v.tree.rows()[areas_idx].note_id);
        v.tree
            .expand_at(areas_idx, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        v.add_expansion_path(vec![root_key, areas_key]);
        v.selected = areas_idx;
        v.refresh_selected_path(&g);
        let expanded_count_before = v.tree.len();

        // Drop the old graph entirely and build a new one against the
        // same vault. NoteIds in the new graph are not guaranteed
        // equal to those in `g`, but `NodeKey`s are.
        drop(g);
        let g2 = dirs_graph();
        v.tree = TreeState::default();
        v.restore_expansion(&g2, FT_TEST_TODAY);

        // Both expansions land in the rebuilt tree.
        assert_eq!(v.tree.len(), expanded_count_before);
        assert!(v.tree.rows()[0].expanded, "root stays expanded");
        let restored_areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        assert!(
            v.tree.rows()[restored_areas_idx].expanded,
            "Areas/ stays expanded after rebuild"
        );
        // Selection landed back on Areas/.
        assert_eq!(v.selected, restored_areas_idx);
    }

    #[test]
    fn query_snippet_truncates_long_text() {
        let v = ExpandedView {
            query_buf: EditBuffer::from(
                "node where kind = Directory and path = \"\"; expand where ...",
            ),
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
        assert!(tab.views[0].query_buf.text.is_empty());
    }

    #[test]
    fn add_view_appends_and_switches() {
        let mut tab = GraphTab::new();
        tab.add_view();
        assert_eq!(tab.views.len(), 2);
        assert_eq!(tab.active, 1);
        // Input-mode focus is now expressed via `OpenModal(QueryBar)`
        // posted by the production `Ctrl+N` path; `add_view` itself
        // is a pure state-mutator and no longer sets a flag.
    }

    #[test]
    fn close_last_view_replaces_with_empty() {
        let mut tab = GraphTab::new();
        tab.views[0].query_buf.text = "node where indegree = 0;".into();
        tab.close_view();
        assert_eq!(tab.views.len(), 1);
        assert!(tab.views[0].query_buf.text.is_empty());
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
    /// a preset replaces the active view's query in-place. Updated
    /// for extract-modal-driver §4: the picker is now an `ActiveModal`
    /// variant and commits via `AppRequest::GraphApplyPreset`.
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
            active_modal_name: None,
            host_popup_open: false,
        };

        // Build graph so views can resolve queries.
        let scan = vault.scan();
        let graph = Graph::build(&vault, &scan).unwrap();

        let mut tab = GraphTab::new();
        tab.graph = Some(graph);
        tab.views[0].query_buf.text = "node where kind = Note;".to_string();

        // Ctrl+P → tab posts OpenModal(PresetPicker(... for_active_view=true ...)).
        tab.open_preset_picker_for_active_view(&ctx);
        let req = pending_request
            .borrow_mut()
            .take()
            .expect("Ctrl+P must queue an OpenModal request");
        let mut modal = match req {
            AppRequest::OpenModal(m) => match *m {
                ActiveModal::PresetPicker(p) => p,
                other => panic!("expected PresetPicker, got {:?}", other.name()),
            },
            other => panic!("expected OpenModal, got {other:?}"),
        };

        // Feed Enter to the modal: should commit by posting GraphApplyPreset.
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let outcome = modal.handle_event(Event::Key(enter), &ctx);
        assert!(
            matches!(outcome, ModalOutcome::Closed),
            "Enter on a selected row must close the modal"
        );

        // The modal queued GraphApplyPreset(dsl). Apply it via the tab hook.
        let req = pending_request
            .borrow_mut()
            .take()
            .expect("Enter must queue GraphApplyPreset");
        match req {
            AppRequest::GraphApplyPreset(dsl) => tab.graph_apply_preset(dsl),
            other => panic!("expected GraphApplyPreset, got {other:?}"),
        }

        // The active view's query is replaced with the preset DSL.
        // `fs` is the first preset alphabetically, so the picker lands on it.
        assert_eq!(
            tab.views[0].query_buf.text,
            r#"node where path = ""; expand where edge.kind in {directory-contains};"#,
            "active view query should be replaced by the selected preset DSL"
        );
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
            query_buf: EditBuffer::from(query_text),
            ..Default::default()
        };
        v.apply_query(Some(&graph), FT_TEST_TODAY);
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
            "node where kind in {Note} and path = \"Areas/finance.md\"; expand where edge.kind in {directory-contains, link};",
            "Areas/finance.md",
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_buf.text,
            "node where kind in {Note} and path = \"Areas/finance.md\"; expand where edge.kind in {directory-contains, link};"
        );
    }

    #[test]
    fn z_on_directory_rewrites_query() {
        let mut tab = tab_with_node_selected(
            &[("Areas/finance.md", "")],
            "node where kind in {Directory} and path = \"Areas\"; expand where edge.kind in {directory-contains};",
            "Areas",
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_buf.text,
            "node where kind in {Directory} and path = \"Areas\"; expand where edge.kind in {directory-contains};"
        );
    }

    #[test]
    fn z_on_root_directory_rewrites_query() {
        let mut tab = tab_with_node_selected(
            &[("foo.md", "")],
            "node where kind in {Directory} and path = \"\"; expand where edge.kind in {directory-contains};",
            "",
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_buf.text,
            "node where kind in {Directory} and path = \"\"; expand where edge.kind in {directory-contains};"
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
            query_buf: EditBuffer::from("node where kind = Ghost;"),
            ..Default::default()
        };
        v.apply_query(Some(&graph), FT_TEST_TODAY);
        v.selected = 0;
        let mut tab = GraphTab::new();
        tab.graph = Some(graph);
        tab.views[0] = v;
        let before = tab.views[0].query_buf.text.clone();
        tab.rewrite_query_for_root();
        assert_eq!(tab.views[0].query_buf.text, before, "ghost should be no-op");
    }

    #[test]
    fn z_on_task_is_noop() {
        use ft_core::task::Task;
        let dir = assert_fs::TempDir::new().unwrap();
        dir.child(".obsidian").create_dir_all().unwrap();
        dir.child("root.md").write_str("- [ ] A task\n").unwrap();
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        let scan = Scan {
            tasks: vec![Task {
                description: "A task".into(),
                source_file: PathBuf::from("root.md"),
                source_line: 1,
                ..Default::default()
            }],
            errors: vec![],
        };
        let graph = Graph::build(&vault, &scan).unwrap();
        let mut v = ExpandedView {
            query_buf: EditBuffer::from("node where kind = Task;"),
            ..Default::default()
        };
        v.apply_query(Some(&graph), FT_TEST_TODAY);
        v.selected = 0;
        let mut tab = GraphTab::new();
        tab.graph = Some(graph);
        tab.views[0] = v;
        let before = tab.views[0].query_buf.text.clone();
        tab.rewrite_query_for_root();
        assert_eq!(tab.views[0].query_buf.text, before, "task should be no-op");
    }

    #[test]
    fn z_preserves_expand_block() {
        let mut tab = tab_with_node_selected(
            &[("Areas/finance.md", "")],
            "node where kind in {Directory} and path = \"\"; expand where edge.kind in {directory-contains, links-into, link, embed};",
            "", // root directory is always in the tree for this query
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_buf.text,
            "node where kind in {Directory} and path = \"\"; expand where edge.kind in {directory-contains, links-into, link, embed};"
        );
    }

    #[test]
    fn z_no_expand_block_produces_trailing_semicolon() {
        let mut tab = tab_with_node_selected(
            &[("foo.md", "")],
            "node where kind in {Note} and path = \"foo.md\";",
            "foo.md",
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_buf.text,
            "node where kind in {Note} and path = \"foo.md\";"
        );
    }
}

#[cfg(test)]
mod search_tests {
    use std::path::PathBuf;

    use assert_fs::prelude::*;
    use ft_core::graph::query::parse as parse_query;
    use ft_core::graph::Graph;
    use ft_core::vault::{Scan, Vault};

    use super::*;
    use crate::tui::widgets::PickerSource;

    /// Pinned "today" for graph-tab tests, so task relative-date labels in
    /// snapshots don't drift with the wall clock. Matches `fixed_clock`.
    const FT_TEST_TODAY: chrono::NaiveDate = match chrono::NaiveDate::from_ymd_opt(2026, 5, 12) {
        Some(d) => d,
        None => panic!("invalid test date"),
    };

    fn dirs_graph() -> Graph {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dirs");
        let v = Vault::discover(Some(path)).expect("dirs fixture vault must exist");
        Graph::build(&v, &Scan::default()).unwrap()
    }

    fn dirs_query() -> GraphQuery {
        parse_query(
            "node where kind = Directory and path = \"\"; expand where edge.kind = directory-contains;",
        )
        .unwrap()
    }

    // 7.1
    #[test]
    fn collect_finds_root_and_deeper_with_shortest_paths() {
        let g = dirs_graph();
        let q = dirs_query();
        let candidates = collect_search_candidates(&g, &q, FT_TEST_TODAY);

        let root_id = g.node_by_path(std::path::Path::new("")).unwrap();
        let root = candidates
            .iter()
            .find(|c| c.path == vec![root_id])
            .expect("root candidate present");
        assert_eq!(root.leaf, "/");
        assert!(root.breadcrumb.is_empty());

        let areas_id = g.node_by_path(std::path::Path::new("Areas")).unwrap();
        let areas = candidates
            .iter()
            .find(|c| *c.path.last().unwrap() == areas_id)
            .expect("Areas candidate present");
        assert_eq!(areas.path, vec![root_id, areas_id]);
        assert_eq!(areas.leaf, "Areas/");
        assert_eq!(areas.breadcrumb, "/");

        let ops_id = g
            .node_by_path(std::path::Path::new("Areas/operations"))
            .unwrap();
        let ops = candidates
            .iter()
            .find(|c| *c.path.last().unwrap() == ops_id)
            .expect("Areas/operations candidate present");
        assert_eq!(ops.path, vec![root_id, areas_id, ops_id]);
        assert_eq!(ops.leaf, "operations/");
        assert_eq!(ops.breadcrumb, "/Areas");
    }

    // 7.2
    #[test]
    fn bfs_terminates_on_cycle() {
        // Build a tiny vault with two notes that link to each other:
        // a.md → [[b]], b.md → [[a]]. With an expand policy that
        // follows Link edges and no max_depth, naive traversal would
        // loop. BFS with the visited set must return ≤ 2 candidates.
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("a.md").write_str("[[b]]\n").unwrap();
        tmp.child("b.md").write_str("[[a]]\n").unwrap();
        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&vault, &Scan::default()).unwrap();
        let q = parse_query(
            "node where kind = Note and path = \"a.md\"; expand where edge.kind = link;",
        )
        .unwrap();

        let candidates = collect_search_candidates(&g, &q, FT_TEST_TODAY);
        // a (depth 0) and b (depth 1); BFS must terminate.
        assert_eq!(candidates.len(), 2);
        let depths: Vec<usize> = candidates.iter().map(|c| c.path.len()).collect();
        assert!(depths.contains(&1));
        assert!(depths.contains(&2));
    }

    // 7.3
    #[test]
    fn no_expand_block_yields_only_roots() {
        let g = dirs_graph();
        // Two roots, no expand block.
        let q = parse_query("node where kind = Note;").unwrap();
        let candidates = collect_search_candidates(&g, &q, FT_TEST_TODAY);
        assert!(!candidates.is_empty(), "dirs fixture has at least one note");
        // Every candidate's path has length 1 (it's a root).
        assert!(
            candidates.iter().all(|c| c.path.len() == 1),
            "without expand, every candidate is a root"
        );
        // Exactly equal to `query.select(graph)` length.
        assert_eq!(candidates.len(), q.select(&g).len());
    }

    // 7.4 — the factor-out can't drift since make_row literally calls
    // leaf_display, but we still assert it for every node in the dirs
    // graph so any future divergence (e.g. someone re-inlining) is
    // caught.
    #[test]
    fn leaf_display_matches_make_row_for_every_node() {
        let g = dirs_graph();
        let q = dirs_query();
        for (id, _) in g.nodes() {
            let row = TreeState::make_row(id, 0, &g, &q, FT_TEST_TODAY);
            let (display, kind_char) = leaf_display(&g, id, FT_TEST_TODAY);
            assert_eq!(row.display, display, "display mismatch for {:?}", id);
            assert_eq!(row.kind_char, kind_char, "kind mismatch for {:?}", id);
        }
    }

    /// graph-task-interaction §D6: a Task row shows the status marker,
    /// description, and compact relative due/scheduled + priority when set.
    #[test]
    fn leaf_display_task_shows_dates_and_priority() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        let v = ft_core::vault::Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        // Due 3 days ago (relative to FT_TEST_TODAY = 2026-05-12).
        let scan = ft_core::vault::Scan {
            tasks: vec![ft_core::task::Task {
                description: "Fix login bug".into(),
                status: ft_core::task::Status::Open,
                priority: Some(ft_core::task::Priority::High),
                due: Some(chrono::NaiveDate::from_ymd_opt(2026, 5, 9).unwrap()),
                source_file: PathBuf::from("root.md"),
                source_line: 1,
                ..Default::default()
            }],
            errors: vec![],
        };
        let g = ft_core::graph::Graph::build(&v, &scan).unwrap();
        let task_id = g.task_by_loc(Path::new("root.md"), 1).unwrap();
        let (display, kind) = leaf_display(&g, task_id, FT_TEST_TODAY);
        assert_eq!(kind, 'T');
        assert!(display.starts_with("[ ] Fix login bug"), "got: {display}");
        assert!(
            display.contains("📅 3d ago"),
            "expected relative due: {display}"
        );
        assert!(
            display.contains("⏫"),
            "expected high priority marker: {display}"
        );
    }

    /// A Task with no dates/priority renders just the marker + description.
    #[test]
    fn leaf_display_task_omits_absent_fields() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        let v = ft_core::vault::Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let scan = ft_core::vault::Scan {
            tasks: vec![ft_core::task::Task {
                description: "Plain task".into(),
                source_file: PathBuf::from("root.md"),
                source_line: 1,
                ..Default::default()
            }],
            errors: vec![],
        };
        let g = ft_core::graph::Graph::build(&v, &scan).unwrap();
        let task_id = g.task_by_loc(Path::new("root.md"), 1).unwrap();
        let (display, kind) = leaf_display(&g, task_id, FT_TEST_TODAY);
        assert_eq!(kind, 'T');
        assert_eq!(display, "[ ] Plain task", "got: {display}");
    }

    // 7.5
    #[test]
    fn nucleo_ranks_leaf_match_over_unrelated() {
        // Synthesize two candidates with known haystacks; pick the
        // first NoteId from the dirs graph as a stand-in id (we don't
        // actually use it for matching).
        let g = dirs_graph();
        let some_id = g.nodes().next().unwrap().0;
        let mut src = GraphSearchPickerSource {
            candidates: vec![
                Candidate {
                    path: vec![some_id, some_id],
                    leaf: "bar".to_string(),
                    breadcrumb: "foo".to_string(),
                    kind_char: 'D',
                },
                Candidate {
                    path: vec![some_id, some_id],
                    leaf: "quux".to_string(),
                    breadcrumb: "foo".to_string(),
                    kind_char: 'D',
                },
            ],
            matcher: nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT),
            buf: Vec::new(),
        };
        let items = src.query("bar", 10);
        assert!(!items.is_empty(), "matcher must produce at least one row");
        // First (highest-ranked) item is the `bar` candidate, not `quux`.
        assert!(items[0].label.starts_with("bar"));
    }

    // 7.6
    #[test]
    fn jump_to_path_lands_cursor_at_target_with_ancestors_expanded() {
        let g = dirs_graph();
        let root_id = g.node_by_path(std::path::Path::new("")).unwrap();
        let areas_id = g.node_by_path(std::path::Path::new("Areas")).unwrap();
        let ops_id = g
            .node_by_path(std::path::Path::new("Areas/operations"))
            .unwrap();
        let shifts_id = g
            .note_by_path(std::path::Path::new("Areas/operations/shifts.md"))
            .unwrap();

        let mut tab = GraphTab::new();
        tab.graph = Some(g);
        tab.views[0].query_buf.text =
            "node where kind = Directory and path = \"\"; expand where edge.kind = directory-contains;"
                .to_string();
        let graph_ref = tab.graph.as_ref().unwrap();
        tab.views[0].apply_query(Some(graph_ref), FT_TEST_TODAY);

        let path = vec![root_id, areas_id, ops_id, shifts_id];
        tab.jump_to_path(path.clone());

        let v = &tab.views[0];
        let row = v.tree.rows().get(v.selected).expect("a row is selected");
        assert_eq!(row.note_id, shifts_id, "cursor landed on shifts.md");
        assert_eq!(row.depth, 3, "shifts.md is at depth 3");
        // The view stores paths as build-stable `NodeKey`s; rebuild
        // the expected key path the same way `jump_to_path` does.
        let g_ref = tab.graph.as_ref().unwrap();
        let key_path: Vec<NodeKey> = path.iter().map(|id| g_ref.stable_key(*id)).collect();
        assert_eq!(v.selected_path.as_deref(), Some(key_path.as_slice()));
        // Ancestors are recorded in expanded_paths (closed under prefixes).
        assert!(v.expanded_paths.contains(&vec![key_path[0].clone()]));
        assert!(v
            .expanded_paths
            .contains(&vec![key_path[0].clone(), key_path[1].clone()]));
        assert!(v.expanded_paths.contains(&vec![
            key_path[0].clone(),
            key_path[1].clone(),
            key_path[2].clone(),
        ]));
        // Target itself is NOT in expanded_paths.
        assert!(!v.expanded_paths.contains(&key_path));
    }
}

#[cfg(test)]
mod nav_tests {
    use std::path::PathBuf;

    use assert_fs::prelude::*;
    use ft_core::graph::Graph;
    use ft_core::vault::{Scan, Vault};

    use super::*;

    /// Pinned "today" for graph-tab tests, so task relative-date labels in
    /// snapshots don't drift with the wall clock. Matches `fixed_clock`.
    const FT_TEST_TODAY: chrono::NaiveDate = match chrono::NaiveDate::from_ymd_opt(2026, 5, 12) {
        Some(d) => d,
        None => panic!("invalid test date"),
    };

    fn dirs_graph() -> Graph {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dirs");
        let v = Vault::discover(Some(path)).expect("dirs fixture vault must exist");
        Graph::build(&v, &Scan::default()).unwrap()
    }

    fn tab_with_query(graph: Graph, query_text: &str) -> GraphTab {
        let mut v = ExpandedView {
            query_buf: EditBuffer::from(query_text),
            ..Default::default()
        };
        v.apply_query(Some(&graph), FT_TEST_TODAY);
        GraphTab {
            graph: Some(graph),
            views: vec![v],
            active: 0,
            queued_related_path: None,
            keymap: GRAPH_KEYMAP.clone(),
        }
    }

    // ── find_node_path ─────────────────────────────────────────────

    #[test]
    fn find_node_path_reachable_target() {
        let g = dirs_graph();
        let tab = tab_with_query(g, "node where kind = Directory and path = \"\"; expand where edge.kind = directory-contains;");

        let target = tab
            .graph
            .as_ref()
            .unwrap()
            .node_by_path(std::path::Path::new("Areas"))
            .unwrap();
        let path = tab.find_node_path(target);
        assert!(path.is_some(), "Areas should be reachable");
        let path = path.unwrap();
        // Path should be: root → Areas
        assert_eq!(path.len(), 2, "path has 2 nodes: root, Areas");
    }

    #[test]
    fn find_node_path_unreachable_target() {
        // Use a query that only selects a specific directory —
        // other directories not connected via the expand policy
        // are unreachable.
        let g = dirs_graph();
        let tab = tab_with_query(g, "node where kind = Directory and path = \"Areas\";");
        // "Projects" is a different directory, not reachable via
        // directory-contains from just the "Areas" root with no expand.
        let target = tab
            .graph
            .as_ref()
            .unwrap()
            .node_by_path(std::path::Path::new("Projects"))
            .unwrap();
        let path = tab.find_node_path(target);
        assert!(
            path.is_none(),
            "Projects should be unreachable from Areas-only root"
        );
    }

    #[test]
    fn find_node_path_root_is_target() {
        let g = dirs_graph();
        let tab = tab_with_query(g, "node where kind = Directory and path = \"\"; expand where edge.kind = directory-contains;");
        let root = tab
            .graph
            .as_ref()
            .unwrap()
            .node_by_path(std::path::Path::new(""))
            .unwrap();
        let path = tab.find_node_path(root);
        assert!(path.is_some(), "root should be found");
        let path = path.unwrap();
        assert_eq!(path.len(), 1, "root path should be length 1");
        assert_eq!(path[0], root);
    }

    #[test]
    fn find_node_path_shortest_path_wins() {
        // With a link-graph expand policy, a note reachable via
        // multiple paths should return the shortest (BFS).
        let (_dir, vault) = link_vault_for_shortest_path();
        let g = Graph::build(&vault, &Scan::default()).unwrap();
        let tab = tab_with_query(
            g,
            "node where kind = Note and path = \"A.md\"; expand where edge.kind in {links-into, link, embed};",
        );

        // A links to C, and A links to D which links to C.
        // The BFS should find the shorter path A→C.
        let c_id = tab
            .graph
            .as_ref()
            .unwrap()
            .node_by_path(std::path::Path::new("C.md"))
            .unwrap();
        let path = tab
            .find_node_path(c_id)
            .expect("C should be reachable from A");
        // Path should be A→C (length 2) not A→D→C (length 3).
        assert_eq!(path.len(), 2, "shortest path should be A→C (length 2)");
        // Verify path starts at A and ends at C.
        let a_id = tab
            .graph
            .as_ref()
            .unwrap()
            .node_by_path(std::path::Path::new("A.md"))
            .unwrap();
        assert_eq!(path.first(), Some(&a_id));
        assert_eq!(path.last(), Some(&c_id));
    }

    fn link_vault_for_shortest_path() -> (assert_fs::TempDir, Vault) {
        let dir = assert_fs::TempDir::new().unwrap();
        dir.child(".obsidian").create_dir_all().unwrap();
        // A links to C, A links to D, D links to C.
        // Shortest A→C is direct (A→C), not A→D→C.
        dir.child("A.md").write_str("[[C]]\n[[D]]\n").unwrap();
        dir.child("C.md").write_str("").unwrap();
        dir.child("D.md").write_str("[[C]]\n").unwrap();
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        (dir, vault)
    }
}

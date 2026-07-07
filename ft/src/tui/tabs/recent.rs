//! `History` tab — interactive surface for
//! [`ft_core::recent::build_recent`].
//!
//! Where the Journal tab is target-centric ("what mentions `[[X]]`?"),
//! History is time-shaped: a whole-vault, reverse-chronological feed of
//! the paragraphs edited within a window (default `7d`). It reuses the
//! Journal tab's row renderer and send-to-synth overlay
//! ([`crate::tui::tabs::gather`]'s `pub(crate)` helpers) and the shared
//! section-move modal.
//!
//! Row actions: `Enter` opens the source note in `$EDITOR`; `Space`
//! multi-selects; `s` / `S` ship the selection (or the whole feed) into a
//! synth note as protected `[!ft-source]` callouts; `m` opens the
//! section-move modal seeded to the row's note. `R` reloads.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::LazyLock;

use anyhow::Result;
use chrono::Duration;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, ListItem, Paragraph},
    Frame,
};

use ft_core::blame_cache::BlameCache;
use ft_core::gather::GatherEntry;
use ft_core::git::discover_repo;
use ft_core::pulse::WindowRange;
use ft_core::recent::{build_recent, RecentEntry, RecentOptions};
use ft_core::synth::scaffold::{apply_synth_scaffold, plan_synth_scaffold};

use crate::tui::command::{Command, CommandDef, CommandOutcome, CommandScope};
use crate::tui::event::Event;
use crate::tui::keymap::{KeyChord, KeyMap};
use crate::tui::notes_actions::create::enumerate_vault_folders;
use crate::tui::notes_actions::queue_toast;
use crate::tui::palette;
use crate::tui::tab::{AppRequest, EventOutcome, Tab, TabCtx, TabKind, ToastStyle};
use crate::tui::tabs::gather::{
    citation_badge_line, citation_detail_line, inline_markdown_spans, mark_note_as_synth,
    pad_to_width, render_synth_send, wrap_line, NonSynthChoice, SynthSendState,
};
use crate::tui::widgets::{
    render_feed_split, EditBuffer, FuzzyPicker, PathListPickerSource, PickerOutcome,
    VaultFilePickerSource,
};

// ── Commands ─────────────────────────────────────────────────────────

pub(crate) static RECENT_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "recent.reload",
        description: "Rebuild the recently-edited feed",
        scope: CommandScope::Tab("recent"),
        group: "Feed",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "recent.cursor-up",
        description: "Move the cursor up one entry",
        scope: CommandScope::Tab("recent"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "recent.cursor-down",
        description: "Move the cursor down one entry",
        scope: CommandScope::Tab("recent"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "recent.cursor-first",
        description: "Move the cursor to the first entry",
        scope: CommandScope::Tab("recent"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "recent.cursor-last",
        description: "Move the cursor to the last entry",
        scope: CommandScope::Tab("recent"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "recent.cursor-half-page-down",
        description: "Move the cursor down half a page (10 entries)",
        scope: CommandScope::Tab("recent"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "recent.cursor-half-page-up",
        description: "Move the cursor up half a page (10 entries)",
        scope: CommandScope::Tab("recent"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "recent.open-selected",
        description: "Open the selected entry's note in $EDITOR",
        scope: CommandScope::Tab("recent"),
        group: "Open",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "recent.toggle-entry-selection",
        description: "Toggle multi-select on the entry under the cursor",
        scope: CommandScope::Tab("recent"),
        group: "Selection",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "recent.toggle-uncited",
        description: "Filter to entries not yet cited in any synth note",
        scope: CommandScope::Tab("recent"),
        group: "Filter",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "recent.send-to-synth-existing",
        description: "Pick an existing note to append the selected (or all) entries to",
        scope: CommandScope::Tab("recent"),
        group: "Synth",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "recent.send-to-synth-new",
        description: "Create a new synth note for the selected (or all) entries",
        scope: CommandScope::Tab("recent"),
        group: "Synth",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "recent.move-section",
        description: "Move the selected entry's section into another note",
        scope: CommandScope::Tab("recent"),
        group: "Edit",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
];

pub(crate) static RECENT_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("R", "recent.reload")
        .bind("Up", "recent.cursor-up")
        .bind("k", "recent.cursor-up")
        .bind("Down", "recent.cursor-down")
        .bind("j", "recent.cursor-down")
        .bind("g", "recent.cursor-first")
        .bind("G", "recent.cursor-last")
        .bind("Ctrl+d", "recent.cursor-half-page-down")
        .bind("Ctrl+u", "recent.cursor-half-page-up")
        .bind("Enter", "recent.open-selected")
        .bind("Space", "recent.toggle-entry-selection")
        .bind("u", "recent.toggle-uncited")
        .bind("s", "recent.send-to-synth-existing")
        .bind("S", "recent.send-to-synth-new")
        .bind("m", "recent.move-section")
});

pub struct RecentTab {
    /// Window the feed is built for. Defaults to the last 7 days.
    window: WindowRange,
    /// When `true`, the feed keeps only entries not yet cited
    /// byte-identically in any synth note (stale citations stay).
    /// Mirrors the CLI's `--uncited`.
    uncited_only: bool,
    /// The currently-displayed feed.
    entries: Vec<RecentEntry>,
    /// Per-entry multi-selection (indices into `entries`).
    entry_selected: HashSet<usize>,
    /// 0-indexed cursor into `entries`.
    selected: usize,
    /// Lazy-loaded blame cache, preserved across rebuilds this session.
    cache: Option<BlameCache>,
    /// Generation of the snapshot the current feed was derived from, so
    /// a background rebuild re-derives on the next focus / graph-ready.
    built_generation: Option<u64>,
    /// Last load error, shown as a one-line banner.
    last_error: Option<String>,
    /// Send-to-synth overlay state (reuses the Journal tab's enum). `s`
    /// opens the existing-note picker; `S` the folder→title create flow.
    synth_send: Option<SynthSendState>,
    keymap: KeyMap,
}

impl Default for RecentTab {
    fn default() -> Self {
        Self::new()
    }
}

impl RecentTab {
    pub fn new() -> Self {
        Self {
            window: WindowRange::Since(Duration::days(7)),
            uncited_only: false,
            entries: Vec::new(),
            entry_selected: HashSet::new(),
            selected: 0,
            cache: None,
            built_generation: None,
            last_error: None,
            synth_send: None,
            keymap: RECENT_KEYMAP.clone(),
        }
    }

    pub fn with_keymap_overlay(mut self, overlay: &crate::tui::keymap::KeymapOverlay) -> Self {
        self.keymap = RECENT_KEYMAP.with_overlay(overlay);
        self
    }

    /// Rebuild the feed from the shared snapshot for the current window.
    /// Single seam — every path that changes what's shown funnels here.
    fn rebuild(&mut self, ctx: &mut TabCtx) {
        if discover_repo(&ctx.vault.path).is_none() {
            self.last_error =
                Some("vault is not inside a git repository — recent needs git history".to_string());
            self.entries.clear();
            self.entry_selected.clear();
            self.selected = 0;
            return;
        }
        let Some(snap) = ctx.snapshot.as_ref() else {
            self.last_error =
                Some("graph is still building — press R to retry in a moment".to_string());
            self.entries.clear();
            return;
        };
        let graph = &snap.graph;
        let generation = snap.generation;

        if self.cache.is_none() {
            self.cache = Some(BlameCache::load(&ctx.vault.path).unwrap_or_default());
        }
        let cache = self.cache.as_mut().expect("just initialized");
        let cfg = ctx.vault.config.config.synth.clone();
        let opts = RecentOptions::default();

        let report = match build_recent(graph, ctx.vault, &self.window, &cfg, &opts, cache) {
            Ok(r) => r,
            Err(e) => {
                self.last_error = Some(format!("build_recent failed: {e}"));
                self.entries.clear();
                return;
            }
        };
        let _ = cache.save(&ctx.vault.path);

        if !report.skipped_blame.is_empty() {
            let first = report
                .skipped_blame
                .first()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let msg = if report.skipped_blame.len() == 1 {
                format!("blame skipped 1 file: {first}")
            } else {
                format!(
                    "blame skipped {} files (e.g. {first})",
                    report.skipped_blame.len()
                )
            };
            queue_toast(ctx, &msg, ToastStyle::Info);
        }

        self.last_error = None;
        self.entries = report.entries;
        if self.uncited_only {
            let citations = &ctx.snapshot.as_ref().expect("checked above").citations;
            self.entries.retain(|e| {
                !citations
                    .lookup(&e.source_path, (e.line_start, e.line_end), &e.section_text)
                    .is_cited()
            });
        }
        self.entry_selected.clear();
        self.selected = 0;
        self.built_generation = Some(generation);
    }

    fn toggle_uncited(&mut self, ctx: &mut TabCtx) {
        self.uncited_only = !self.uncited_only;
        self.rebuild(ctx);
    }

    /// Re-derive the feed when the installed snapshot's generation has
    /// moved past the one we last built from (background catch-up).
    fn rebuild_if_stale(&mut self, ctx: &mut TabCtx) {
        let current = ctx.snapshot.as_ref().map(|s| s.generation);
        if current.is_some() && current != self.built_generation {
            self.rebuild(ctx);
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let len = self.entries.len() as isize;
        let new = (self.selected as isize + delta).clamp(0, len - 1);
        self.selected = new as usize;
    }

    fn jump_first(&mut self) {
        self.selected = 0;
    }

    fn jump_last(&mut self) {
        if !self.entries.is_empty() {
            self.selected = self.entries.len() - 1;
        }
    }

    fn toggle_entry_selection(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        if !self.entry_selected.remove(&self.selected) {
            self.entry_selected.insert(self.selected);
        }
    }

    fn request_open_selected(&self, ctx: &TabCtx) {
        let Some(entry) = self.entries.get(self.selected) else {
            return;
        };
        let abs = ctx.vault.path.join(&entry.source_path);
        ctx.recents.record_open(&entry.source_path);
        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
            path: abs,
            line: entry.line_start as usize,
        });
    }

    /// Open the section-move modal seeded to the selected row's note.
    fn open_move_for_selected(&self, ctx: &TabCtx) {
        let Some(entry) = self.entries.get(self.selected) else {
            return;
        };
        let source_rel = entry.source_path.clone();
        if let Some(state) =
            crate::tui::notes_actions::section_move::begin_for_source(ctx, source_rel)
        {
            *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
                crate::tui::modal::ActiveModal::SectionMove(state),
            )));
        }
    }

    /// `s` — open the existing-note picker for the synth send.
    fn open_send_to_existing(&mut self, ctx: &TabCtx) {
        if self.entries.is_empty() {
            return;
        }
        let source = VaultFilePickerSource::new(Arc::clone(ctx.vault), Arc::clone(ctx.recents));
        self.synth_send = Some(SynthSendState::PickExisting {
            picker: FuzzyPicker::new(source),
            new_only: false,
        });
    }

    /// `S` — open the folder picker for the create-new synth flow.
    fn open_send_to_new(&mut self, ctx: &TabCtx) {
        if self.entries.is_empty() {
            return;
        }
        let folders = enumerate_vault_folders(ctx.vault);
        let source = PathListPickerSource::new(folders);
        self.synth_send = Some(SynthSendState::PickFolder(FuzzyPicker::new(source)));
    }

    /// Entries to ship to the scaffold, as `GatherEntry` (the type the
    /// core scaffold planner consumes). Selected rows when any are
    /// selected, otherwise the whole feed. `matched` is always empty —
    /// History has no link target.
    fn entries_to_send(&self) -> Vec<GatherEntry> {
        let chosen: Vec<&RecentEntry> = if self.entry_selected.is_empty() {
            self.entries.iter().collect()
        } else {
            self.entries
                .iter()
                .enumerate()
                .filter(|(i, _)| self.entry_selected.contains(i))
                .map(|(_, e)| e)
                .collect()
        };
        chosen
            .into_iter()
            .map(|e| GatherEntry {
                source_title: e.source_title.clone(),
                source_path: e.source_path.clone(),
                line_start: e.line_start,
                line_end: e.line_end,
                section_text: e.section_text.clone(),
                date: e.date,
                matched: Vec::new(),
            })
            .collect()
    }

    /// Drive the send-to-synth overlay. History omits the Journal tab's
    /// `new_only` watermark flow — `new_only` is always `false` here.
    fn handle_synth_send_key(&mut self, k: KeyEvent, ctx: &mut TabCtx) -> EventOutcome {
        let Some(state) = self.synth_send.take() else {
            return EventOutcome::NotHandled;
        };
        match state {
            SynthSendState::PickExisting { mut picker, .. } => match picker.handle_key(k) {
                PickerOutcome::Selected(hit) => self.on_existing_picked(ctx, hit.path),
                PickerOutcome::Cancelled => {}
                PickerOutcome::StillOpen | PickerOutcome::NotHandled => {
                    self.synth_send = Some(SynthSendState::PickExisting {
                        picker,
                        new_only: false,
                    });
                }
            },
            SynthSendState::NonSynthPrompt { path, focus, .. } => match k.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.commit_send(ctx, &path, matches_mark(focus)),
                KeyCode::Char('a') | KeyCode::Char('A') => self.commit_send(ctx, &path, false),
                KeyCode::Char('m') | KeyCode::Char('M') => self.commit_send(ctx, &path, true),
                KeyCode::Char('c') | KeyCode::Char('C') => {}
                KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                    let next = match focus {
                        NonSynthChoice::AppendAnyway => NonSynthChoice::MarkAndAppend,
                        NonSynthChoice::MarkAndAppend => NonSynthChoice::AppendAnyway,
                    };
                    self.synth_send = Some(SynthSendState::NonSynthPrompt {
                        path,
                        focus: next,
                        new_only: false,
                    });
                }
                _ => {
                    self.synth_send = Some(SynthSendState::NonSynthPrompt {
                        path,
                        focus,
                        new_only: false,
                    });
                }
            },
            // The context-note picker is a Journal-tab flow; History
            // never constructs it.
            SynthSendState::PickContextNote(_) => {}
            SynthSendState::PickFolder(mut picker) => match picker.handle_key(k) {
                PickerOutcome::Selected(folder) => {
                    let folder = if folder == Path::new(".") {
                        PathBuf::new()
                    } else {
                        folder
                    };
                    self.synth_send = Some(SynthSendState::TitlePrompt {
                        folder,
                        buf: EditBuffer::default(),
                        error: None,
                    });
                }
                PickerOutcome::Cancelled => {}
                PickerOutcome::StillOpen | PickerOutcome::NotHandled => {
                    self.synth_send = Some(SynthSendState::PickFolder(picker));
                }
            },
            SynthSendState::TitlePrompt {
                folder, mut buf, ..
            } => match k.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let title = buf.text.trim().to_string();
                    if title.is_empty() {
                        self.synth_send = Some(SynthSendState::TitlePrompt {
                            folder,
                            buf,
                            error: Some("title is required".into()),
                        });
                    } else {
                        let filename = if title.ends_with(".md") {
                            title
                        } else {
                            format!("{title}.md")
                        };
                        let target = if folder.as_os_str().is_empty() {
                            PathBuf::from(&filename)
                        } else {
                            folder.join(&filename)
                        };
                        self.commit_send(ctx, &target, false);
                    }
                }
                _ => {
                    let _ = buf.handle_event(k);
                    self.synth_send = Some(SynthSendState::TitlePrompt {
                        folder,
                        buf,
                        error: None,
                    });
                }
            },
        }
        EventOutcome::Consumed
    }

    /// Existing note picked → send directly if it's already a synth note,
    /// else raise the append-anyway / mark-and-append prompt.
    fn on_existing_picked(&mut self, ctx: &mut TabCtx, path: PathBuf) {
        let abs = ctx.vault.path.join(&path);
        let is_synth = std::fs::read_to_string(&abs)
            .map(|c| ft_core::synth::callout::is_synth_note(&c))
            .unwrap_or(false);
        if is_synth {
            self.commit_send(ctx, &path, false);
        } else {
            self.synth_send = Some(SynthSendState::NonSynthPrompt {
                path,
                focus: NonSynthChoice::MarkAndAppend,
                new_only: false,
            });
        }
    }

    /// Plan + apply the synth scaffold and hand off to `$EDITOR`.
    fn commit_send(&mut self, ctx: &mut TabCtx, vault_rel_path: &Path, mark_synth: bool) {
        let entries = self.entries_to_send();
        if entries.is_empty() {
            queue_toast(ctx, "send-to-synth: no entries to send", ToastStyle::Error);
            return;
        }
        if mark_synth {
            if let Err(e) = mark_note_as_synth(&ctx.vault.path.join(vault_rel_path)) {
                queue_toast(
                    ctx,
                    &format!("could not add ft-synth marker: {e}"),
                    ToastStyle::Error,
                );
                return;
            }
        }
        let plan = match plan_synth_scaffold(ctx.vault, vault_rel_path, &entries) {
            Ok(p) => p,
            Err(e) => {
                queue_toast(ctx, &format!("synth plan failed: {e}"), ToastStyle::Error);
                return;
            }
        };
        let written = match apply_synth_scaffold(ctx.vault, &plan) {
            Ok(p) => p,
            Err(e) => {
                queue_toast(ctx, &format!("synth write failed: {e}"), ToastStyle::Error);
                return;
            }
        };
        queue_toast(
            ctx,
            &format!(
                "{} {} synth section(s) to {}",
                if plan.create { "created" } else { "appended" },
                plan.sections.len(),
                vault_rel_path.display()
            ),
            ToastStyle::Success,
        );
        // The synth note changed on disk; refresh the shared snapshot.
        ctx.request_graph_refresh();
        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
            path: written,
            line: 1,
        });
    }
}

/// Map a non-synth prompt focus to the `mark_synth` boolean.
fn matches_mark(focus: NonSynthChoice) -> bool {
    matches!(focus, NonSynthChoice::MarkAndAppend)
}

impl Tab for RecentTab {
    fn title(&self) -> &str {
        "Recent"
    }

    fn kind(&self) -> TabKind {
        TabKind::Recent
    }

    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()> {
        self.rebuild_if_stale(ctx);
        Ok(())
    }

    fn on_graph_ready(&mut self, ctx: &mut TabCtx) {
        self.rebuild(ctx);
    }

    fn commands(&self) -> &'static [CommandDef] {
        RECENT_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &self.keymap
    }

    fn dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome {
        match cmd.name {
            "recent.reload" => {
                self.rebuild(ctx);
                CommandOutcome::Handled
            }
            "recent.cursor-up" => {
                self.move_selection(-1);
                CommandOutcome::Handled
            }
            "recent.cursor-down" => {
                self.move_selection(1);
                CommandOutcome::Handled
            }
            "recent.cursor-first" => {
                self.jump_first();
                CommandOutcome::Handled
            }
            "recent.cursor-last" => {
                self.jump_last();
                CommandOutcome::Handled
            }
            "recent.cursor-half-page-down" => {
                self.move_selection(10);
                CommandOutcome::Handled
            }
            "recent.cursor-half-page-up" => {
                self.move_selection(-10);
                CommandOutcome::Handled
            }
            "recent.open-selected" => {
                self.request_open_selected(ctx);
                CommandOutcome::Handled
            }
            "recent.toggle-entry-selection" => {
                self.toggle_entry_selection();
                CommandOutcome::Handled
            }
            "recent.toggle-uncited" => {
                self.toggle_uncited(ctx);
                CommandOutcome::Handled
            }
            "recent.send-to-synth-existing" => {
                self.open_send_to_existing(ctx);
                CommandOutcome::Handled
            }
            "recent.send-to-synth-new" => {
                self.open_send_to_new(ctx);
                CommandOutcome::Handled
            }
            "recent.move-section" => {
                self.open_move_for_selected(ctx);
                CommandOutcome::Handled
            }
            _ => CommandOutcome::NotHandled,
        }
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };
        if self.synth_send.is_some() {
            return Ok(self.handle_synth_send_key(k, ctx));
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
        render_history(
            frame,
            area,
            &self.entries,
            self.uncited_only,
            ctx.snapshot.as_ref().map(|s| &s.citations),
            self.selected,
            &self.entry_selected,
            self.last_error.as_deref(),
        );
        if let Some(state) = self.synth_send.as_mut() {
            render_synth_send(frame, area, state);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_history(
    frame: &mut Frame,
    area: Rect,
    entries: &[RecentEntry],
    uncited_only: bool,
    citations: Option<&ft_core::synth::citations::CitationIndex>,
    selected: usize,
    entry_selected: &HashSet<usize>,
    last_error: Option<&str>,
) {
    let title = if uncited_only {
        format!(" Recent ({} entries) [filter: uncited] ", entries.len())
    } else {
        format!(" Recent ({} entries) ", entries.len())
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if entries.is_empty() {
        let mut lines = vec![Line::from(Span::styled(
            "no paragraphs edited in the window — press R to reload",
            Style::default().fg(palette::DIM),
        ))];
        if let Some(err) = last_error {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("error: {err}"),
                Style::default().fg(palette::ERROR),
            )));
        }
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    let wrap_width = inner.width as usize;

    // Compact list rows: one line per entry. `{date} {title}` plus an
    // inline citation badge when present. Multi-select marker `●`;
    // cursor highlight is owned by `render_feed_split`'s list widget.
    let mut list_rows: Vec<ListItem<'_>> = Vec::with_capacity(entries.len());
    for (i, e) in entries.iter().enumerate() {
        let marker = if entry_selected.contains(&i) {
            "● "
        } else {
            "  "
        };
        let mut text = format!("{marker}{}  {}", e.date, e.source_title);
        if let Some(index) = citations {
            let state = index.lookup(&e.source_path, (e.line_start, e.line_end), &e.section_text);
            if let Some((badge, _)) = citation_badge_line(&state, None) {
                text.push(' ');
                text.push_str(&badge);
            }
        }
        let row_style = Style::default().fg(palette::PRIMARY);
        list_rows.push(ListItem::new(Line::from(Span::styled(
            pad_to_width(&text, wrap_width),
            row_style,
        ))));
    }

    // Preview header + body for the selected entry. The header is
    // visually distinct (BOLD on the entry-header band) and carries the
    // full citation detail (which note(s) cite it; staleness).
    let mut preview_header: Vec<Line<'_>> = Vec::new();
    if let Some(e) = entries.get(selected) {
        let header_span = Span::styled(
            format!(
                "{}  ·  {}  ·  L{}–{}",
                e.source_title, e.date, e.line_start, e.line_end
            ),
            Style::default()
                .fg(palette::PRIMARY)
                .bg(palette::ENTRY_HEADER_BG)
                .add_modifier(Modifier::BOLD),
        );
        preview_header.push(Line::from(header_span));
        if let Some(index) = citations {
            let state = index.lookup(&e.source_path, (e.line_start, e.line_end), &e.section_text);
            if let Some((detail, style)) = citation_detail_line(&state, None) {
                preview_header.push(Line::from(Span::styled(format!("    ↳ {detail}"), style)));
            }
        }
    }

    let mut preview_body: Vec<Line<'_>> = Vec::new();
    if let Some(e) = entries.get(selected) {
        for body_line in e.section_text.lines() {
            for wrapped in wrap_line(body_line, wrap_width) {
                preview_body.push(Line::from(inline_markdown_spans(&wrapped)));
            }
        }
    }

    render_feed_split(
        frame,
        inner,
        list_rows,
        selected,
        entry_selected,
        &preview_header,
        &preview_body,
    );

    if let Some(err) = last_error {
        let banner_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        frame.render_widget(Clear, banner_area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("error: {err}"),
                Style::default().fg(palette::ERROR),
            ))),
            banner_area,
        );
    }
}

//! `Journal` tab — interactive surface for `ft_core::journal::build_journal`.
//!
//! The user picks a note via the shared fuzzy picker, then scrolls the
//! reverse-chronological feed of paragraph mentions. `Enter` opens the
//! source note in `$EDITOR` at the paragraph's first line. State
//! (the target note + its loaded entries) persists across tab switches;
//! `R` reloads, `c` clears back to the empty-state prompt.
//!
//! Cross-tab entry: the graph tab's `Shift+J` keybinding raises
//! [`crate::tui::tab::AppRequest::JournalFor`]; the App services that
//! by calling [`JournalTab::queue_journal_for`] and switching the
//! active tab. The queued target (a note path or a ghost name) is
//! consumed on the next `on_focus` and turned into a load.
//!
//! `BlameCache` is held in the tab so subsequent loads in the same
//! session warm up; the on-disk file at `.ft/cache/blame.msgpack` is
//! refreshed best-effort after every successful `build_journal` call.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::LazyLock;

use anyhow::Result;
use crossterm::event::KeyEvent;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use ft_core::blame_cache::BlameCache;
use ft_core::git::discover_repo;
use ft_core::graph::{Graph, NodeKind, NoteId};
use ft_core::journal::{build_journal, JournalEntry};
use ft_core::link_review::compute_link_review;
use ft_core::search::Hit;
use ft_core::synth::scaffold::{apply_synth_scaffold, plan_synth_scaffold};

use crate::tui::command::{Command, CommandDef, CommandOutcome, CommandScope};
use crate::tui::event::Event;
use crate::tui::help::HelpSection;
use crate::tui::keymap::{KeyChord, KeyMap};
use crate::tui::notes_actions::create::enumerate_vault_folders;
use crate::tui::palette;
use crate::tui::tab::{
    AppRequest, EventOutcome, JournalTarget, JournalWindow, MultiTargetRequest, Tab, TabCtx,
    ToastStyle,
};
use crate::tui::widgets::{
    EditBuffer, FuzzyPicker, PathListPickerSource, PickerOutcome, VaultFilePickerSource,
};

// ── Commands ─────────────────────────────────────────────────────────

/// Every action the Journal tab exposes through the command/keymap
/// layer. Pulled out of the tab impl so the registry can include them
/// at build time and `?` / `ft commands list` can introspect them.
pub(crate) static JOURNAL_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "journal.open-picker",
        description: "Open the fuzzy note picker to choose a journal source",
        scope: CommandScope::Tab("journal"),
        group: "Source",
        args_schema: &[],
        // Picker captures the keyboard for the duration of its session,
        // so `ft do` can't reasonably drive it headlessly.
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "journal.reload",
        description: "Reload the current note's journal",
        scope: CommandScope::Tab("journal"),
        group: "Source",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "journal.clear",
        description: "Clear the current journal and return to the picker prompt",
        scope: CommandScope::Tab("journal"),
        group: "Source",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "journal.cursor-up",
        description: "Move the cursor up one entry",
        scope: CommandScope::Tab("journal"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "journal.cursor-down",
        description: "Move the cursor down one entry",
        scope: CommandScope::Tab("journal"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "journal.cursor-first",
        description: "Move the cursor to the first entry",
        scope: CommandScope::Tab("journal"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "journal.cursor-last",
        description: "Move the cursor to the last entry",
        scope: CommandScope::Tab("journal"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "journal.cursor-half-page-down",
        description: "Move the cursor down half a page (10 entries)",
        scope: CommandScope::Tab("journal"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "journal.cursor-half-page-up",
        description: "Move the cursor up half a page (10 entries)",
        scope: CommandScope::Tab("journal"),
        group: "Navigation",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "journal.open-selected",
        description: "Open the selected entry's note in $EDITOR",
        scope: CommandScope::Tab("journal"),
        group: "Open",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "journal.toggle-entry-selection",
        description: "Toggle multi-select on the entry under the cursor",
        scope: CommandScope::Tab("journal"),
        group: "Selection",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "journal.toggle-in-window",
        description: "Filter to entries touched by the window (multi-target with window only)",
        scope: CommandScope::Tab("journal"),
        group: "Filter",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "journal.send-to-synth-existing",
        description: "Pick an existing note to append the selected (or all) entries to",
        scope: CommandScope::Tab("journal"),
        group: "Synth",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
    CommandDef {
        name: "journal.send-to-synth-new",
        description: "Create a new synth note for the selected (or all) entries",
        scope: CommandScope::Tab("journal"),
        group: "Synth",
        args_schema: &[],
        opens_modal: true,
        is_primary: false,
    },
];

/// Default keymap for the Journal tab. Aliases (e.g. `Up`/`k`,
/// `Down`/`j`) bind the same command to multiple chords. The
/// picker-open state captures keys before this keymap is consulted
/// (see `handle_event`).
pub(crate) static JOURNAL_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        // Source
        .bind("/", "journal.open-picker")
        .bind("R", "journal.reload")
        .bind("c", "journal.clear")
        // Navigation — vim aliases
        .bind("Up", "journal.cursor-up")
        .bind("k", "journal.cursor-up")
        .bind("Down", "journal.cursor-down")
        .bind("j", "journal.cursor-down")
        .bind("g", "journal.cursor-first")
        .bind("G", "journal.cursor-last")
        .bind("Ctrl+d", "journal.cursor-half-page-down")
        .bind("Ctrl+u", "journal.cursor-half-page-up")
        // Open
        .bind("Enter", "journal.open-selected")
        // Multi-target + synth
        .bind("Space", "journal.toggle-entry-selection")
        .bind("w", "journal.toggle-in-window")
        .bind("s", "journal.send-to-synth-existing")
        .bind("S", "journal.send-to-synth-new")
});

/// Send-to-synth multi-step flow state. Active only when the user has
/// pressed `s` (append-to-existing) or `S` (create-new); when `None`
/// the tab handles keys normally.
///
/// Both branches converge to a `(target_path, plan_create)` decision
/// that drives `plan_synth_scaffold` / `apply_synth_scaffold` and the
/// editor handoff.
pub enum SynthSendState {
    /// `s` — fuzzy picker over every `.md` in the vault.
    PickExisting(FuzzyPicker<VaultFilePickerSource>),
    /// User picked a real note but its frontmatter lacks
    /// `ft-synth: true`. Inline 3-way prompt: append anyway, mark and
    /// append, or cancel.
    NonSynthPrompt {
        path: PathBuf,
        focus: NonSynthChoice,
    },
    /// `S` — fuzzy picker over every vault folder. `.` selects the
    /// vault root.
    PickFolder(FuzzyPicker<PathListPickerSource>),
    /// `S` step 2 — typed title prompt; folder is the picked folder.
    /// The title's `.md` extension is added on submit if missing.
    TitlePrompt {
        folder: PathBuf,
        buf: EditBuffer,
        error: Option<String>,
    },
}

/// User's choice when sending to an existing non-synth note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonSynthChoice {
    /// Append protected sections without touching frontmatter.
    AppendAnyway,
    /// Insert/upgrade frontmatter to include `ft-synth: true`, then
    /// append.
    MarkAndAppend,
}

pub struct JournalTab {
    /// What's currently loaded. `None` puts the tab in its empty-state
    /// prompt. `Note(path)` for a real note, `Ghost(raw)` for an
    /// unresolved-link concept.
    target: Option<JournalTarget>,
    /// In multi-target mode, the full list of selected targets and the
    /// optional window that produced them. `target` is set to the first
    /// entry so existing single-target rendering paths keep working;
    /// multi-target rendering branches on `multi_targets.len() > 1`.
    multi_targets: Vec<JournalTarget>,
    /// Captured window from a Review-tab handoff. Enables the
    /// `--in-window` toggle (`w`).
    window: Option<JournalWindow>,
    /// When `true` and `window.is_some()`, the rendered feed is filtered
    /// down to entries whose paragraph lines overlap an added-line in
    /// the window.
    in_window_only: bool,
    /// The currently-displayed feed.
    entries: Vec<JournalEntry>,
    /// Per-entry display titles for the matched badge, parallel to
    /// `entries`. Resolved at load-time because `entry.matched` carries
    /// `NoteId`s that belong to the load-time graph, not the one the
    /// renderer has access to.
    entry_matched_titles: Vec<Vec<String>>,
    /// Per-entry selection (for `s` → send-to-synth). Indices into
    /// `entries`. Cleared on every fresh load.
    entry_selected: std::collections::HashSet<usize>,
    /// 0-indexed cursor into `entries`. Saturating-clamped on load.
    selected: usize,
    /// 0-indexed scroll offset (in entries, not lines). Adjusted at
    /// render time when `selected` would otherwise fall offscreen.
    scroll_offset: usize,
    /// Active fuzzy picker. `Some` while the picker overlay owns the
    /// keyboard; cleared on selection or `Esc`.
    picker: Option<FuzzyPicker<VaultFilePickerSource>>,
    /// Queued target from a cross-tab jump. Consumed by `on_focus` to
    /// kick off a load.
    queued_for: Option<JournalTarget>,
    /// Queued multi-target request from the Review tab. Consumed by
    /// `on_focus`; takes precedence over `queued_for` when both are set.
    queued_multi: Option<MultiTargetRequest>,
    /// Lazy-loaded blame cache; preserved across loads within the
    /// tab's session.
    cache: Option<BlameCache>,
    /// Last load error, if any. Shown as a single-line banner so the
    /// user knows why the feed didn't change. Cleared on next
    /// successful load or `c`.
    last_error: Option<String>,
    /// Send-to-synth multi-step state. `Some` while the picker or
    /// prompt overlay owns the keyboard; cleared on completion or
    /// `Esc`.
    synth_send: Option<SynthSendState>,
    keymap: crate::tui::keymap::KeyMap,
}

impl Default for JournalTab {
    fn default() -> Self {
        Self::new()
    }
}

impl JournalTab {
    pub fn new() -> Self {
        Self {
            target: None,
            multi_targets: Vec::new(),
            window: None,
            in_window_only: false,
            entries: Vec::new(),
            entry_matched_titles: Vec::new(),
            entry_selected: std::collections::HashSet::new(),
            selected: 0,
            scroll_offset: 0,
            picker: None,
            queued_for: None,
            queued_multi: None,
            cache: None,
            last_error: None,
            synth_send: None,
            keymap: JOURNAL_KEYMAP.clone(),
        }
    }

    pub fn with_keymap_overlay(mut self, overlay: &crate::tui::keymap::KeymapOverlay) -> Self {
        self.keymap = JOURNAL_KEYMAP.with_overlay(overlay);
        self
    }

    /// Run `build_journal` for `target` and replace `entries`. The
    /// blame cache is loaded from disk on first use and saved
    /// best-effort after a successful build. Accepts both notes and
    /// ghosts — the engine treats either symmetrically once a
    /// `NoteId` is in hand.
    fn load_for(&mut self, target: JournalTarget, ctx: &mut TabCtx) {
        if discover_repo(&ctx.vault.path).is_none() {
            self.last_error = Some(
                "vault is not inside a git repository — journal needs git history".to_string(),
            );
            self.target = Some(target);
            self.entries.clear();
            self.selected = 0;
            self.scroll_offset = 0;
            return;
        }

        // Build a fresh graph; the App-level graph belongs to the Graph
        // tab and isn't easily reachable from here.
        let scan = ctx.vault.scan();
        let graph = match Graph::build(ctx.vault, &scan) {
            Ok(g) => g,
            Err(e) => {
                self.last_error = Some(format!("graph build failed: {e}"));
                self.target = Some(target);
                self.entries.clear();
                return;
            }
        };

        let resolved = match &target {
            JournalTarget::Note(path) => graph.note_by_path(path),
            JournalTarget::Ghost(raw) => graph.ghost_by_raw(raw),
        };
        let Some(note_id) = resolved else {
            self.last_error = Some(format!("target not found in graph: {}", target.label()));
            self.target = Some(target);
            self.entries.clear();
            return;
        };

        if self.cache.is_none() {
            self.cache = Some(BlameCache::load(&ctx.vault.path).unwrap_or_default());
        }
        let cache = self.cache.as_mut().expect("just initialized");

        // Pass `vault.path` as the git CWD: paragraph paths are
        // vault-relative, and `git -C <vault>` finds the enclosing repo
        // even when the vault is a subdirectory of it.
        let vault_path = ctx.vault.path.clone();
        match build_journal(&graph, &[note_id], ctx.vault, &vault_path, cache) {
            Ok(report) => {
                self.last_error = None;
                self.target = Some(target);
                // Single-target mode → drop multi-target state.
                self.multi_targets.clear();
                self.window = None;
                self.in_window_only = false;
                self.entries = report.entries;
                self.entry_matched_titles = vec![vec![]; self.entries.len()];
                self.entry_selected.clear();
                self.selected = 0;
                self.scroll_offset = 0;
                if !report.skipped_blame.is_empty() {
                    let first = report
                        .skipped_blame
                        .first()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    let extra = report.skipped_blame.len().saturating_sub(1);
                    let msg = if extra == 0 {
                        format!("blame skipped 1 file: {first}")
                    } else {
                        format!(
                            "blame skipped {} files (e.g. {first})",
                            report.skipped_blame.len()
                        )
                    };
                    crate::tui::notes_actions::queue_toast(ctx, &msg, ToastStyle::Info);
                }
                // Best-effort cache save — failures are logged via toast
                // and otherwise non-fatal.
                if let Err(e) = cache.save(&ctx.vault.path) {
                    crate::tui::notes_actions::queue_toast(
                        ctx,
                        &format!("blame cache save: {e}"),
                        ToastStyle::Info,
                    );
                }
            }
            Err(e) => {
                self.last_error = Some(format!("build_journal failed: {e}"));
                self.target = Some(target);
                self.entries.clear();
            }
        }
    }

    /// Multi-target counterpart of [`Self::load_for`]. Runs
    /// `build_journal` over `request.targets` and stashes the window
    /// so the `w` key can later toggle in-window filtering.
    fn load_for_multi(&mut self, request: MultiTargetRequest, ctx: &mut TabCtx) {
        if discover_repo(&ctx.vault.path).is_none() {
            self.last_error = Some(
                "vault is not inside a git repository — journal needs git history".to_string(),
            );
            self.entries.clear();
            return;
        }

        let scan = ctx.vault.scan();
        let graph = match Graph::build(ctx.vault, &scan) {
            Ok(g) => g,
            Err(e) => {
                self.last_error = Some(format!("graph build failed: {e}"));
                self.entries.clear();
                return;
            }
        };

        // Resolve every target to a NoteId in this fresh graph.
        let mut ids: Vec<NoteId> = Vec::with_capacity(request.targets.len());
        let mut unresolved: Vec<String> = Vec::new();
        for t in &request.targets {
            let id = match t {
                JournalTarget::Note(p) => graph.note_by_path(p),
                JournalTarget::Ghost(raw) => graph.ghost_by_raw(raw),
            };
            match id {
                Some(i) => ids.push(i),
                None => unresolved.push(t.label()),
            }
        }
        if ids.is_empty() {
            self.last_error = Some(format!(
                "no Journal-multi targets resolved in current graph: {}",
                unresolved.join(", ")
            ));
            self.entries.clear();
            return;
        }

        if self.cache.is_none() {
            self.cache = Some(BlameCache::load(&ctx.vault.path).unwrap_or_default());
        }
        let cache = self.cache.as_mut().expect("just initialized");
        let vault_path = ctx.vault.path.clone();
        let report = match build_journal(&graph, &ids, ctx.vault, &vault_path, cache) {
            Ok(r) => r,
            Err(e) => {
                self.last_error = Some(format!("build_journal failed: {e}"));
                self.entries.clear();
                return;
            }
        };
        let _ = cache.save(&ctx.vault.path);

        // Resolve every entry's `matched` NoteIds to display titles
        // while the load-time graph is still in scope.
        let entry_matched_titles: Vec<Vec<String>> = report
            .entries
            .iter()
            .map(|e| {
                e.matched
                    .iter()
                    .map(|id| match graph.node(*id) {
                        NodeKind::Note(n) => n.title.clone(),
                        NodeKind::Ghost(g) => g.raw.clone(),
                        _ => String::new(),
                    })
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .collect();

        self.last_error = None;
        self.target = request.targets.first().cloned();
        self.multi_targets = request.targets;
        self.window = request.window;
        self.in_window_only = false;
        self.entries = report.entries;
        self.entry_matched_titles = entry_matched_titles;
        self.entry_selected.clear();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Recompute and apply the in-window filter against the current
    /// `entries`. Reloads from scratch (cheap) so toggling the filter
    /// on/off restores the full list without storing a shadow copy.
    fn refresh_after_filter_toggle(&mut self, ctx: &mut TabCtx) {
        // We need the unfiltered list as the basis; the cleanest path is
        // to re-run `load_for_multi` with the same request.
        let request = MultiTargetRequest {
            targets: self.multi_targets.clone(),
            window: self.window.clone(),
        };
        let want_in_window = self.in_window_only;
        self.load_for_multi(request, ctx);
        if want_in_window {
            self.apply_in_window_filter(ctx);
        }
    }

    /// Drop entries whose paragraph lines don't overlap any added line
    /// from `self.window`. No-op when `window` is `None`.
    fn apply_in_window_filter(&mut self, ctx: &mut TabCtx) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let scan = ctx.vault.scan();
        let graph = match Graph::build(ctx.vault, &scan) {
            Ok(g) => g,
            Err(_) => return,
        };
        let cfg = ctx.vault.config.config.synth.clone();
        let core_window = window.to_core();
        let review =
            match compute_link_review(&graph, ctx.vault, &ctx.vault.path, &core_window, &cfg) {
                Ok(r) => r,
                Err(_) => return,
            };
        // Filter `entries` and the parallel `entry_matched_titles`
        // together so they stay aligned.
        let added = &review.added_lines;
        let keep: Vec<bool> = self
            .entries
            .iter()
            .map(|e| {
                added
                    .get(&e.source_path)
                    .is_some_and(|lines| (e.line_start..=e.line_end).any(|ln| lines.contains(&ln)))
            })
            .collect();
        let mut idx = 0;
        self.entries.retain(|_| {
            let k = keep[idx];
            idx += 1;
            k
        });
        let mut idx = 0;
        self.entry_matched_titles.retain(|_| {
            let k = keep[idx];
            idx += 1;
            k
        });
        self.entry_selected.clear();
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
    }

    fn toggle_in_window(&mut self, ctx: &mut TabCtx) {
        if self.window.is_none() || self.multi_targets.len() <= 1 {
            return; // toggle only applies in multi-target mode with a window
        }
        self.in_window_only = !self.in_window_only;
        self.refresh_after_filter_toggle(ctx);
    }

    fn toggle_entry_selection(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        if !self.entry_selected.remove(&self.selected) {
            self.entry_selected.insert(self.selected);
        }
    }

    /// Drive the synth send-to flow per its current state. Returns
    /// `Consumed` whenever a `synth_send` is active; the caller in
    /// `handle_event` will exit early without further keymap lookup.
    fn handle_synth_send_key(&mut self, k: KeyEvent, ctx: &mut TabCtx) -> EventOutcome {
        use crossterm::event::KeyCode;
        let Some(state) = self.synth_send.take() else {
            return EventOutcome::NotHandled;
        };
        match state {
            SynthSendState::PickExisting(mut picker) => match picker.handle_key(k) {
                PickerOutcome::Selected(hit) => self.on_existing_picked(ctx, hit.path),
                PickerOutcome::Cancelled => {}
                PickerOutcome::StillOpen => {
                    self.synth_send = Some(SynthSendState::PickExisting(picker));
                }
                PickerOutcome::NotHandled => {
                    self.synth_send = Some(SynthSendState::PickExisting(picker));
                }
            },
            SynthSendState::NonSynthPrompt { path, focus } => match k.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.commit_non_synth_choice(ctx, &path, focus),
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    self.commit_non_synth_choice(ctx, &path, NonSynthChoice::AppendAnyway);
                }
                KeyCode::Char('m') | KeyCode::Char('M') => {
                    self.commit_non_synth_choice(ctx, &path, NonSynthChoice::MarkAndAppend);
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {}
                KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                    let next = match focus {
                        NonSynthChoice::AppendAnyway => NonSynthChoice::MarkAndAppend,
                        NonSynthChoice::MarkAndAppend => NonSynthChoice::AppendAnyway,
                    };
                    self.synth_send = Some(SynthSendState::NonSynthPrompt { path, focus: next });
                }
                _ => {
                    self.synth_send = Some(SynthSendState::NonSynthPrompt { path, focus });
                }
            },
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
                PickerOutcome::StillOpen => {
                    self.synth_send = Some(SynthSendState::PickFolder(picker));
                }
                PickerOutcome::NotHandled => {
                    self.synth_send = Some(SynthSendState::PickFolder(picker));
                }
            },
            SynthSendState::TitlePrompt {
                folder,
                mut buf,
                error: _,
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
                        // Create-new: `apply_synth_scaffold` will write
                        // frontmatter and content. No need to mark.
                        self.commit_send(ctx, &target, false);
                    }
                }
                KeyCode::Char(c) => {
                    buf.insert(c);
                    self.synth_send = Some(SynthSendState::TitlePrompt {
                        folder,
                        buf,
                        error: None,
                    });
                }
                KeyCode::Backspace => {
                    buf.backspace();
                    self.synth_send = Some(SynthSendState::TitlePrompt {
                        folder,
                        buf,
                        error: None,
                    });
                }
                KeyCode::Left => {
                    buf.left();
                    self.synth_send = Some(SynthSendState::TitlePrompt {
                        folder,
                        buf,
                        error: None,
                    });
                }
                KeyCode::Right => {
                    buf.right();
                    self.synth_send = Some(SynthSendState::TitlePrompt {
                        folder,
                        buf,
                        error: None,
                    });
                }
                KeyCode::Home => {
                    buf.home();
                    self.synth_send = Some(SynthSendState::TitlePrompt {
                        folder,
                        buf,
                        error: None,
                    });
                }
                KeyCode::End => {
                    buf.end();
                    self.synth_send = Some(SynthSendState::TitlePrompt {
                        folder,
                        buf,
                        error: None,
                    });
                }
                _ => {
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

    /// Existing note picked → check its frontmatter and either send
    /// directly (synth-marked) or open the NonSynthPrompt.
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
            });
        }
    }

    fn commit_non_synth_choice(&mut self, ctx: &mut TabCtx, path: &Path, choice: NonSynthChoice) {
        let mark = matches!(choice, NonSynthChoice::MarkAndAppend);
        self.commit_send(ctx, path, mark);
    }

    /// `s` — open the existing-note fuzzy picker.
    fn open_send_to_existing(&mut self, ctx: &TabCtx) {
        if self.entries.is_empty() {
            return;
        }
        let source = VaultFilePickerSource::new(Arc::clone(ctx.vault), Arc::clone(ctx.recents));
        self.synth_send = Some(SynthSendState::PickExisting(FuzzyPicker::new(source)));
    }

    /// `S` — open the folder fuzzy picker for the create-new flow.
    fn open_send_to_new(&mut self, ctx: &TabCtx) {
        if self.entries.is_empty() {
            return;
        }
        let folders = enumerate_vault_folders(ctx.vault);
        let source = PathListPickerSource::new(folders);
        self.synth_send = Some(SynthSendState::PickFolder(FuzzyPicker::new(source)));
    }

    /// Entries to ship to the scaffold: selected entries when any are
    /// selected, otherwise the full feed.
    fn entries_to_send(&self) -> Vec<JournalEntry> {
        if self.entry_selected.is_empty() {
            self.entries.clone()
        } else {
            self.entries
                .iter()
                .enumerate()
                .filter(|(i, _)| self.entry_selected.contains(i))
                .map(|(_, e)| e.clone())
                .collect()
        }
    }

    /// Perform the actual scaffold + handoff. `vault_rel_path` is the
    /// vault-relative target; `mark_synth` ensures the on-disk file's
    /// frontmatter includes `ft-synth: true` before the scaffold is
    /// applied (no-op when the file already has the marker or is being
    /// created — `apply_synth_scaffold` writes the marker fresh).
    fn commit_send(&mut self, ctx: &mut TabCtx, vault_rel_path: &Path, mark_synth: bool) {
        let entries = self.entries_to_send();
        if entries.is_empty() {
            crate::tui::notes_actions::queue_toast(
                ctx,
                "send-to-synth: no entries to send",
                ToastStyle::Error,
            );
            return;
        }

        if mark_synth {
            if let Err(e) = mark_note_as_synth(&ctx.vault.path.join(vault_rel_path)) {
                crate::tui::notes_actions::queue_toast(
                    ctx,
                    &format!("could not add ft-synth marker: {e}"),
                    ToastStyle::Error,
                );
                return;
            }
        }

        let plan = match plan_synth_scaffold(ctx.vault, &ctx.vault.path, vault_rel_path, &entries) {
            Ok(p) => p,
            Err(e) => {
                crate::tui::notes_actions::queue_toast(
                    ctx,
                    &format!("synth plan failed: {e}"),
                    ToastStyle::Error,
                );
                return;
            }
        };
        let written = match apply_synth_scaffold(ctx.vault, &plan) {
            Ok(p) => p,
            Err(e) => {
                crate::tui::notes_actions::queue_toast(
                    ctx,
                    &format!("synth write failed: {e}"),
                    ToastStyle::Error,
                );
                return;
            }
        };
        crate::tui::notes_actions::queue_toast(
            ctx,
            &format!(
                "{} {} synth section(s) to {}",
                if plan.create { "created" } else { "appended" },
                plan.sections.len(),
                vault_rel_path.display()
            ),
            ToastStyle::Success,
        );
        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
            path: written,
            line: 1,
        });
    }

    fn open_picker(&mut self, ctx: &TabCtx) {
        let source = VaultFilePickerSource::new(Arc::clone(ctx.vault), Arc::clone(ctx.recents));
        self.picker = Some(FuzzyPicker::new(source));
    }

    fn handle_picker_key(&mut self, k: KeyEvent, ctx: &mut TabCtx) -> EventOutcome {
        let Some(mut picker) = self.picker.take() else {
            return EventOutcome::NotHandled;
        };
        match picker.handle_key(k) {
            PickerOutcome::Selected(hit) => {
                let Hit { path, .. } = hit;
                self.load_for(JournalTarget::Note(path), ctx);
                EventOutcome::Consumed
            }
            PickerOutcome::Cancelled => EventOutcome::Consumed,
            PickerOutcome::StillOpen => {
                self.picker = Some(picker);
                EventOutcome::Consumed
            }
            PickerOutcome::NotHandled => {
                self.picker = Some(picker);
                EventOutcome::NotHandled
            }
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
        self.scroll_offset = 0;
    }

    fn jump_last(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = self.entries.len() - 1;
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
}

impl Tab for JournalTab {
    fn title(&self) -> &str {
        "Journal"
    }

    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()> {
        // Multi-target queue takes precedence; clear the single-note
        // queue without executing it (spec: "Both slots set prefers
        // multi-target").
        if let Some(request) = self.queued_multi.take() {
            self.queued_for = None;
            self.load_for_multi(request, ctx);
        } else if let Some(target) = self.queued_for.take() {
            self.load_for(target, ctx);
        }
        Ok(())
    }

    fn queue_journal_for(&mut self, target: &JournalTarget) {
        self.queued_for = Some(target.clone());
    }

    fn queue_journal_for_multi(&mut self, request: &MultiTargetRequest) {
        self.queued_multi = Some(request.clone());
    }

    fn commands(&self) -> &'static [CommandDef] {
        JOURNAL_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &self.keymap
    }

    fn dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome {
        match cmd.name {
            "journal.open-picker" => {
                self.open_picker(ctx);
                CommandOutcome::Handled
            }
            "journal.reload" => {
                if let Some(target) = self.target.clone() {
                    self.load_for(target, ctx);
                }
                CommandOutcome::Handled
            }
            "journal.clear" => {
                self.target = None;
                self.entries.clear();
                self.selected = 0;
                self.scroll_offset = 0;
                self.last_error = None;
                CommandOutcome::Handled
            }
            "journal.cursor-up" => {
                self.move_selection(-1);
                CommandOutcome::Handled
            }
            "journal.cursor-down" => {
                self.move_selection(1);
                CommandOutcome::Handled
            }
            "journal.cursor-first" => {
                self.jump_first();
                CommandOutcome::Handled
            }
            "journal.cursor-last" => {
                self.jump_last();
                CommandOutcome::Handled
            }
            "journal.cursor-half-page-down" => {
                self.move_selection(10);
                CommandOutcome::Handled
            }
            "journal.cursor-half-page-up" => {
                self.move_selection(-10);
                CommandOutcome::Handled
            }
            "journal.open-selected" => {
                self.request_open_selected(ctx);
                CommandOutcome::Handled
            }
            "journal.toggle-entry-selection" => {
                self.toggle_entry_selection();
                CommandOutcome::Handled
            }
            "journal.toggle-in-window" => {
                self.toggle_in_window(ctx);
                CommandOutcome::Handled
            }
            "journal.send-to-synth-existing" => {
                self.open_send_to_existing(ctx);
                CommandOutcome::Handled
            }
            "journal.send-to-synth-new" => {
                self.open_send_to_new(ctx);
                CommandOutcome::Handled
            }
            _ => CommandOutcome::NotHandled,
        }
    }

    fn handle_event(&mut self, ev: Event, ctx: &mut TabCtx) -> Result<EventOutcome> {
        let Event::Key(k) = ev else {
            return Ok(EventOutcome::NotHandled);
        };

        // Send-to-synth flow (existing/new picker + prompts) captures
        // keys before the keymap is consulted.
        if self.synth_send.is_some() {
            return Ok(self.handle_synth_send_key(k, ctx));
        }

        // Picker overlay captures the keyboard while open — the picker
        // is tab-resident here (not in `ActiveModal`), so we route raw
        // events to it before consulting the tab keymap.
        if self.picker.is_some() {
            return Ok(self.handle_picker_key(k, ctx));
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

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        match &self.target {
            None => render_empty(frame, area, self.last_error.as_deref()),
            Some(target) => render_loaded(
                frame,
                area,
                target,
                &self.multi_targets,
                &self.entries,
                &self.entry_matched_titles,
                self.selected,
                &self.entry_selected,
                &mut self.scroll_offset,
                self.last_error.as_deref(),
                self.in_window_only,
                self.window.is_some(),
            ),
        }

        if let Some(ref mut picker) = self.picker {
            let popup_area = centered_rect(70, 70, area);
            frame.render_widget(Clear, popup_area);
            picker.render(frame, popup_area);
        }

        if let Some(state) = self.synth_send.as_mut() {
            render_synth_send(frame, area, state);
        }
    }

    fn help_sections(&self) -> Vec<HelpSection> {
        vec![
            HelpSection::new(
                "Source",
                &[
                    ("/", "open the fuzzy note picker"),
                    ("R", "reload the current note's journal"),
                    ("c", "clear back to the picker prompt"),
                ],
            ),
            HelpSection::new(
                "Navigation",
                &[
                    ("↑ / ↓ · j / k", "select prev / next entry"),
                    ("g / G", "first / last entry"),
                    ("Ctrl+D / Ctrl+U", "half-page down / up"),
                ],
            ),
            HelpSection::new(
                "Open",
                &[("Enter", "open selected entry's note in $EDITOR")],
            ),
            HelpSection::new(
                "Synth",
                &[
                    ("Space", "toggle multi-select on the current entry"),
                    ("w", "toggle in-window-only filter (multi-target + window)"),
                    ("s", "pick an existing note to append the scaffold to"),
                    ("Shift+s", "create a new synth note (folder + title)"),
                ],
            ),
        ]
    }
}

fn render_empty(frame: &mut Frame, area: Rect, last_error: Option<&str>) {
    let block = Block::default().borders(Borders::ALL).title(" Journal ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "press `/` to pick a note",
            Style::default().fg(palette::DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Shift+J in the Graph tab on a Note row jumps straight here.",
            Style::default().fg(palette::DIM),
        )),
    ];
    if let Some(err) = last_error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("error: {err}"),
            Style::default().fg(palette::ERROR),
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

#[allow(clippy::too_many_arguments)]
fn render_loaded(
    frame: &mut Frame,
    area: Rect,
    target: &JournalTarget,
    multi_targets: &[JournalTarget],
    entries: &[JournalEntry],
    entry_matched_titles: &[Vec<String>],
    selected: usize,
    entry_selected: &std::collections::HashSet<usize>,
    scroll_offset: &mut usize,
    last_error: Option<&str>,
    in_window_only: bool,
    has_window: bool,
) {
    let title = if multi_targets.len() > 1 {
        let window_suffix = if has_window {
            if in_window_only {
                " · in-window"
            } else {
                " · all-time"
            }
        } else {
            ""
        };
        format!(
            " Journal — {} targets ({} entries){} ",
            multi_targets.len(),
            entries.len(),
            window_suffix
        )
    } else {
        format!(" Journal — {} ({} entries) ", target.label(), entries.len())
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if entries.is_empty() {
        let mut lines = vec![Line::from(Span::styled(
            "no journal entries for this note",
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

    // Each entry contributes: 1 header line + (optional) `matched:` badge
    // + N wrapped body lines + 1 blank separator. We wrap body lines
    // manually (rather than letting ratatui's `Paragraph::wrap` do it) so
    // `entry_starts` stays in sync with the post-wrap visual line count —
    // that's what `scroll((y, 0))` indexes into. Without manual wrap the
    // cursor would drift relative to scroll on entries with long paragraphs.
    let wrap_width = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut entry_starts: Vec<usize> = Vec::with_capacity(entries.len());
    for (i, e) in entries.iter().enumerate() {
        entry_starts.push(lines.len());
        let header_style = if i == selected {
            Style::default()
                .fg(palette::PRIMARY)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default()
                .fg(palette::PRIMARY)
                .add_modifier(Modifier::BOLD)
        };
        let select_marker = if entry_selected.contains(&i) {
            "[*] "
        } else {
            "    "
        };
        lines.push(Line::from(Span::styled(
            format!("{select_marker}{}  {}", e.date, e.source_title),
            header_style,
        )));
        if e.matched.len() > 1 {
            let empty: Vec<String> = Vec::new();
            let titles = entry_matched_titles.get(i).unwrap_or(&empty);
            if !titles.is_empty() {
                let badge = format!("    matched: {}", titles.join(", "));
                lines.push(Line::from(Span::styled(
                    badge,
                    Style::default().fg(palette::DIM),
                )));
            }
        }
        for body_line in e.section_text.lines() {
            for wrapped in wrap_line(body_line, wrap_width) {
                lines.push(Line::from(Span::raw(wrapped)));
            }
        }
        lines.push(Line::from(""));
    }

    // Scroll so the selected entry's header sits within the viewport.
    let view_height = inner.height as usize;
    let selected_start = *entry_starts.get(selected).copied().get_or_insert(0);
    if selected_start < *scroll_offset {
        *scroll_offset = selected_start;
    } else if selected_start >= scroll_offset.saturating_add(view_height) {
        *scroll_offset = selected_start.saturating_sub(view_height.saturating_sub(2));
    }

    frame.render_widget(
        Paragraph::new(lines).scroll((*scroll_offset as u16, 0)),
        inner,
    );

    if let Some(err) = last_error {
        // Overlay a single-line error banner across the bottom of the inner area.
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

/// Word-wrap one logical line to `width` columns, preserving leading
/// whitespace on the first wrapped fragment (so an indented bullet
/// still looks indented). Words longer than `width` are hard-broken on
/// character boundaries; widths are measured in characters (close
/// enough for the paragraph text the journal renders — single-cell
/// ASCII and BMP-letter Unicode). A `width` of 0 returns the original
/// line unchanged to avoid an infinite loop.
fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![line.to_string()];
    }
    if line.is_empty() {
        return vec![String::new()];
    }
    // Preserve any leading indent on the first wrapped fragment.
    let indent_chars: usize = line.chars().take_while(|c| c.is_whitespace()).count();
    let indent: String = line.chars().take(indent_chars).collect();
    let body: String = line.chars().skip(indent_chars).collect();
    if body.is_empty() {
        // Trailing whitespace-only line: keep the chars (chunked so we
        // never exceed width) so spacing in poetry-style content is
        // preserved.
        return chunk_by_chars(line, width);
    }

    let mut out: Vec<String> = Vec::new();
    let mut current = indent.clone();
    let mut current_len = indent_chars;
    for word in body.split_whitespace() {
        let word_len = word.chars().count();
        if word_len > width {
            // Flush whatever's in the current buffer first, then
            // hard-break the long word across full-width chunks.
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
                current_len = 0;
            }
            for chunk in chunk_by_chars(word, width) {
                out.push(chunk);
            }
            continue;
        }
        let needs_space = current_len > 0 && current_len > indent_chars;
        let space_len = if needs_space { 1 } else { 0 };
        if current_len + space_len + word_len > width {
            out.push(std::mem::take(&mut current));
            current.push_str(word);
            current_len = word_len;
        } else {
            if needs_space {
                current.push(' ');
                current_len += 1;
            }
            current.push_str(word);
            current_len += word_len;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Split `s` into chunks of exactly `width` chars (last may be shorter).
fn chunk_by_chars(s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![s.to_string()];
    }
    let chars: Vec<char> = s.chars().collect();
    chars
        .chunks(width)
        .map(|c| c.iter().collect::<String>())
        .collect()
}

/// Insert `ft-synth: true` into the YAML frontmatter of the file at
/// `absolute_path`. If a frontmatter block is already present, the
/// existing `ft-synth: ...` line is replaced (or added if missing); if
/// no frontmatter exists, a fresh `---\nft-synth: true\n---\n\n` block
/// is prepended.
fn mark_note_as_synth(absolute_path: &Path) -> std::io::Result<()> {
    let content = std::fs::read_to_string(absolute_path)?;
    let new_content = upsert_ft_synth_marker(&content);
    if new_content == content {
        return Ok(());
    }
    ft_core::fs::write_atomic(absolute_path, &new_content).map_err(std::io::Error::other)
}

/// Pure transform: ensure the result has `ft-synth: true` in YAML
/// frontmatter. Idempotent.
pub fn upsert_ft_synth_marker(content: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    let has_fm = lines.first() == Some(&"---");
    if !has_fm {
        let mut out = String::from("---\nft-synth: true\n---\n");
        if !content.starts_with('\n') {
            out.push('\n');
        }
        out.push_str(content);
        return out;
    }
    let end_idx = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| **l == "---")
        .map(|(i, _)| i);
    let Some(end_idx) = end_idx else {
        // Unterminated frontmatter — bail and just prepend a fresh block.
        return format!("---\nft-synth: true\n---\n\n{content}");
    };
    let mut new_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let mut found = false;
    for line in new_lines.iter_mut().take(end_idx).skip(1) {
        if line.trim_start().starts_with("ft-synth:") {
            *line = "ft-synth: true".to_string();
            found = true;
            break;
        }
    }
    if !found {
        new_lines.insert(end_idx, "ft-synth: true".to_string());
    }
    new_lines.join("\n")
}

/// Render whichever step of the send-to-synth flow is active.
fn render_synth_send(frame: &mut Frame, area: Rect, state: &mut SynthSendState) {
    match state {
        SynthSendState::PickExisting(picker) => {
            let popup = centered_rect(70, 70, area);
            frame.render_widget(Clear, popup);
            picker.render(frame, popup);
        }
        SynthSendState::PickFolder(picker) => {
            let popup = centered_rect(70, 70, area);
            frame.render_widget(Clear, popup);
            picker.render(frame, popup);
        }
        SynthSendState::NonSynthPrompt { path, focus } => {
            let height = 5.min(area.height);
            let y = area.y + area.height.saturating_sub(height);
            let prompt_area = Rect {
                x: area.x,
                y,
                width: area.width,
                height,
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .title(" This isn't a synth note ")
                .style(Style::default().fg(palette::PRIMARY));
            let inner = block.inner(prompt_area);
            frame.render_widget(Clear, prompt_area);
            frame.render_widget(block, prompt_area);
            let header = format!("{}", path.display());
            let (a_style, m_style) = match focus {
                NonSynthChoice::AppendAnyway => (
                    Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
                    Style::default().fg(palette::DIM),
                ),
                NonSynthChoice::MarkAndAppend => (
                    Style::default().fg(palette::DIM),
                    Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
                ),
            };
            let lines = vec![
                Line::from(Span::styled(header, Style::default().fg(palette::DIM))),
                Line::from(""),
                Line::from(vec![
                    Span::styled(" [a] append anyway ", a_style),
                    Span::raw("  "),
                    Span::styled(" [m] mark and append ", m_style),
                    Span::raw("    [c] cancel"),
                ]),
            ];
            frame.render_widget(Paragraph::new(lines), inner);
        }
        SynthSendState::TitlePrompt { folder, buf, error } => {
            let height = 4.min(area.height);
            let y = area.y + area.height.saturating_sub(height);
            let prompt_area = Rect {
                x: area.x,
                y,
                width: area.width,
                height,
            };
            let folder_disp = if folder.as_os_str().is_empty() {
                ".".to_string()
            } else {
                folder.display().to_string()
            };
            let title =
                format!(" New synth note in {folder_disp}/ — Enter to create, Esc to cancel ");
            let block = Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(Style::default().fg(palette::PRIMARY));
            let inner = block.inner(prompt_area);
            frame.render_widget(Clear, prompt_area);
            frame.render_widget(block, prompt_area);
            let mut lines = vec![Line::from(format!("Title: {}_", buf.text))];
            if let Some(err) = error {
                lines.push(Line::from(Span::styled(
                    format!("error: {err}"),
                    Style::default().fg(palette::ERROR),
                )));
            }
            frame.render_widget(Paragraph::new(lines), inner);
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::wrap_line;

    #[test]
    fn wrap_line_short_fits_on_one_line() {
        assert_eq!(wrap_line("hello world", 40), vec!["hello world"]);
    }

    #[test]
    fn wrap_line_breaks_on_word_boundary() {
        let out = wrap_line("the quick brown fox jumps over the lazy dog", 20);
        assert_eq!(
            out,
            vec!["the quick brown fox", "jumps over the lazy", "dog"]
        );
        for line in &out {
            assert!(line.chars().count() <= 20, "overflow: {line:?}");
        }
    }

    #[test]
    fn wrap_line_preserves_leading_indent_on_first_fragment() {
        // Continuation lines start at column 0, matching how the
        // journal already renders bullet bodies. The indent is kept on
        // the first fragment so the bullet visually leads the wrap.
        let out = wrap_line("  - this is a bullet point that wraps", 20);
        assert_eq!(out[0], "  - this is a bullet");
        assert_eq!(out[1], "point that wraps");
    }

    #[test]
    fn wrap_line_hard_breaks_word_longer_than_width() {
        let out = wrap_line("supercalifragilisticexpialidocious tail", 10);
        // First three chunks are 10-char slices of the long word;
        // remainder + tail wrap accordingly.
        assert_eq!(out[0], "supercalif");
        assert_eq!(out[1], "ragilistic");
        assert_eq!(out[2], "expialidoc");
        assert_eq!(out[3], "ious");
        assert_eq!(out[4], "tail");
    }

    #[test]
    fn wrap_line_empty_input_yields_single_empty_line() {
        assert_eq!(wrap_line("", 20), vec![""]);
    }

    #[test]
    fn wrap_line_width_zero_is_a_no_op() {
        // Defensive: degenerate width should not loop forever.
        assert_eq!(
            wrap_line("anything goes here", 0),
            vec!["anything goes here"]
        );
    }
}

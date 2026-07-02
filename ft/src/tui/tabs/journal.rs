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
use ft_core::graph::{NodeKind, NoteId};
use ft_core::journal::{build_journal, JournalEntry};
use ft_core::link_review::compute_link_review;
use ft_core::synth::scaffold::{apply_synth_scaffold, plan_synth_scaffold};

use crate::tui::command::{Command, CommandDef, CommandOutcome, CommandScope};
use crate::tui::event::Event;
use crate::tui::help::HelpSection;
use crate::tui::keymap::{KeyChord, KeyMap};
use crate::tui::notes_actions::create::enumerate_vault_folders;
use crate::tui::palette;
use crate::tui::tab::{
    AppRequest, AppendOrReplaceMode, EventOutcome, JournalTarget, JournalWindow,
    MultiTargetRequest, Tab, TabCtx, TabKind, ToastStyle,
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
        name: "journal.open-sources-manager",
        description: "Open the Sources Manager modal (add/remove/clear sources)",
        scope: CommandScope::Tab("journal"),
        group: "Sources",
        args_schema: &[],
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
        name: "journal.send-to-synth-new-only",
        description: "Append to an existing note only entries newer than its last synth watermark",
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
        // Sources
        .bind("/", "journal.open-sources-manager")
        .bind("a", "journal.open-sources-manager")
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
        .bind("n", "journal.send-to-synth-new-only")
        .bind("S", "journal.send-to-synth-new")
});

/// Send-to-synth multi-step flow state. Active only when the user has
/// pressed `s` (append-to-existing), `S` (create-new), or `n`
/// (new-only append-to-existing); when `None` the tab handles keys
/// normally.
///
/// Both `s` and `n` converge on `PickExisting`; the `new_only` flag
/// distinguishes them — `n` filters `entries_to_send()` to entries
/// newer than the picked note's last-synth watermark before planning.
/// `S` flows through the folder + title prompts to a create.
pub enum SynthSendState {
    /// `s` / `n` — fuzzy picker over every `.md` in the vault. `new_only`
    /// records which command opened this picker so the on-pick handler
    /// knows whether to apply the watermark filter.
    PickExisting {
        picker: FuzzyPicker<VaultFilePickerSource>,
        new_only: bool,
    },
    /// User picked a real note but its frontmatter lacks
    /// `ft-synth: true`. Inline 3-way prompt: append anyway, mark and
    /// append, or cancel. `new_only` is carried through so the `n` flow
    /// still filters after the mark/append decision.
    NonSynthPrompt {
        path: PathBuf,
        focus: NonSynthChoice,
        new_only: bool,
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
    /// The currently-loaded source set. Empty `Vec` = empty-state.
    /// Every rebuild reads from this slot; cross-tab queues (Review
    /// handoff, Graph jump) mutate it in `on_focus`.
    sources: Vec<JournalTarget>,
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
    /// Queued single-target from the Graph tab's `Shift+J`. Consumed by
    /// `on_focus` and replaces the source set with `vec![target]`.
    queued_for: Option<JournalTarget>,
    /// Queued multi-target request from the Review tab. Consumed by
    /// `on_focus`; replaces the source set with `request.targets`.
    queued_multi: Option<MultiTargetRequest>,
    /// Queued AddSources request from the Graph tab's `Shift+A`.
    /// Consumed by `on_focus` and turned into an
    /// `ActiveModal::JournalAppendOrReplace` prompt.
    queued_add_sources: Option<(Vec<JournalTarget>, AppendOrReplaceMode)>,
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
            sources: Vec::new(),
            window: None,
            in_window_only: false,
            entries: Vec::new(),
            entry_matched_titles: Vec::new(),
            entry_selected: std::collections::HashSet::new(),
            selected: 0,
            scroll_offset: 0,
            queued_for: None,
            queued_multi: None,
            queued_add_sources: None,
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

    /// Rebuild the journal feed from the current `sources` slot.
    /// Handles every state: empty sources (clears entries; no error),
    /// no git repo (banner, no entries), graph build failure (banner),
    /// per-target resolution misses (banner if *all* miss). Re-applies
    /// the in-window filter when applicable. Single seam — all source
    /// mutations funnel through here.
    fn rebuild_journal(&mut self, ctx: &mut TabCtx) {
        // Pre-flight: empty source set is a valid "empty journal"
        // state; entries clear, no error.
        if self.sources.is_empty() {
            self.entries.clear();
            self.entry_matched_titles.clear();
            self.entry_selected.clear();
            self.selected = 0;
            self.scroll_offset = 0;
            self.last_error = None;
            // The in-window filter is meaningless with zero sources.
            self.in_window_only = false;
            return;
        }

        if discover_repo(&ctx.vault.path).is_none() {
            self.last_error = Some(
                "vault is not inside a git repository — journal needs git history".to_string(),
            );
            self.entries.clear();
            self.entry_matched_titles.clear();
            self.entry_selected.clear();
            self.selected = 0;
            self.scroll_offset = 0;
            return;
        }

        // Resolve against the App-owned shared snapshot.
        let Some(snap) = ctx.snapshot.as_ref() else {
            self.last_error =
                Some("graph is still building — press R to retry in a moment".to_string());
            self.entries.clear();
            return;
        };
        let graph = &snap.graph;

        // Resolve every source to a NoteId in the snapshot's graph.
        let mut ids: Vec<NoteId> = Vec::with_capacity(self.sources.len());
        let mut unresolved: Vec<String> = Vec::new();
        for t in &self.sources {
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
                "no Journal sources resolved in current graph: {}",
                unresolved.join(", ")
            ));
            self.entries.clear();
            return;
        }

        if self.cache.is_none() {
            self.cache = Some(BlameCache::load(&ctx.vault.path).unwrap_or_default());
        }
        let cache = self.cache.as_mut().expect("just initialized");

        let report = match build_journal(graph, &ids, ctx.vault, cache) {
            Ok(r) => r,
            Err(e) => {
                self.last_error = Some(format!("build_journal failed: {e}"));
                self.entries.clear();
                return;
            }
        };

        // Best-effort cache save — failures are logged via toast and
        // otherwise non-fatal.
        let _ = cache.save(&ctx.vault.path);

        // Resolve every entry's `matched` NoteIds to display titles
        // while the load-time graph is still in scope. Single-source
        // builds never render the badge so we leave it empty there.
        let entry_matched_titles: Vec<Vec<String>> = if self.sources.len() > 1 {
            report
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
                .collect()
        } else {
            vec![vec![]; report.entries.len()]
        };

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

        self.last_error = None;
        self.entries = report.entries;
        self.entry_matched_titles = entry_matched_titles;
        self.entry_selected.clear();
        self.selected = 0;
        self.scroll_offset = 0;
        // In-window filter only makes sense in multi-target mode with
        // a window attached; otherwise clear it silently.
        if !(self.sources.len() >= 2 && self.window.is_some()) {
            self.in_window_only = false;
        }
        if self.in_window_only {
            self.apply_in_window_filter(ctx);
        }
    }

    /// Recompute and apply the in-window filter against the current
    /// `entries`. Rebuilds from scratch (cheap) so toggling the filter
    /// on/off restores the full list without storing a shadow copy.
    fn refresh_after_filter_toggle(&mut self, ctx: &mut TabCtx) {
        // `rebuild_journal` re-applies the filter at the end if
        // `in_window_only` is still set, so we just rebuild.
        self.rebuild_journal(ctx);
    }

    /// Drop entries whose paragraph lines don't overlap any added line
    /// from `self.window`. No-op when `window` is `None`.
    fn apply_in_window_filter(&mut self, ctx: &mut TabCtx) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let Some(snap) = ctx.snapshot.as_ref() else {
            return;
        };
        let graph = &snap.graph;
        let cfg = ctx.vault.config.config.synth.clone();
        let core_window = window.to_core();
        let review = match compute_link_review(graph, ctx.vault, &core_window, &cfg) {
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
        if self.window.is_none() || self.sources.len() <= 1 {
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
            SynthSendState::PickExisting {
                mut picker,
                new_only,
            } => match picker.handle_key(k) {
                PickerOutcome::Selected(hit) => self.on_existing_picked(ctx, hit.path, new_only),
                PickerOutcome::Cancelled => {}
                PickerOutcome::StillOpen => {
                    self.synth_send = Some(SynthSendState::PickExisting { picker, new_only });
                }
                PickerOutcome::NotHandled => {
                    self.synth_send = Some(SynthSendState::PickExisting { picker, new_only });
                }
            },
            SynthSendState::NonSynthPrompt {
                path,
                focus,
                new_only,
            } => match k.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.commit_non_synth_choice(ctx, &path, focus, new_only),
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    self.commit_non_synth_choice(
                        ctx,
                        &path,
                        NonSynthChoice::AppendAnyway,
                        new_only,
                    );
                }
                KeyCode::Char('m') | KeyCode::Char('M') => {
                    self.commit_non_synth_choice(
                        ctx,
                        &path,
                        NonSynthChoice::MarkAndAppend,
                        new_only,
                    );
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {}
                KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                    let next = match focus {
                        NonSynthChoice::AppendAnyway => NonSynthChoice::MarkAndAppend,
                        NonSynthChoice::MarkAndAppend => NonSynthChoice::AppendAnyway,
                    };
                    self.synth_send = Some(SynthSendState::NonSynthPrompt {
                        path,
                        focus: next,
                        new_only,
                    });
                }
                _ => {
                    self.synth_send = Some(SynthSendState::NonSynthPrompt {
                        path,
                        focus,
                        new_only,
                    });
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
                        self.commit_send(ctx, &target, false, false);
                    }
                }
                // All text edits + cursor moves + readline chords go
                // through the buffer's EDIT_KEYMAP. Any returned
                // outcome (Consumed or NotHandled) re-parks the state.
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

    /// Existing note picked → check its frontmatter and either send
    /// directly (synth-marked) or open the NonSynthPrompt. `new_only`
    /// carries the `n`-command's watermark-filter intent through to
    /// [`commit_send`].
    fn on_existing_picked(&mut self, ctx: &mut TabCtx, path: PathBuf, new_only: bool) {
        let abs = ctx.vault.path.join(&path);
        let is_synth = std::fs::read_to_string(&abs)
            .map(|c| ft_core::synth::callout::is_synth_note(&c))
            .unwrap_or(false);
        if is_synth {
            self.commit_send(ctx, &path, false, new_only);
        } else {
            self.synth_send = Some(SynthSendState::NonSynthPrompt {
                path,
                focus: NonSynthChoice::MarkAndAppend,
                new_only,
            });
        }
    }

    fn commit_non_synth_choice(
        &mut self,
        ctx: &mut TabCtx,
        path: &Path,
        choice: NonSynthChoice,
        new_only: bool,
    ) {
        let mark = matches!(choice, NonSynthChoice::MarkAndAppend);
        self.commit_send(ctx, path, mark, new_only);
    }

    /// `s` — open the existing-note fuzzy picker (append, all entries).
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

    /// `n` — open the existing-note fuzzy picker (append, only entries
    /// newer than the picked note's last-synth watermark). The filter
    /// is applied in [`commit_send`] after the note is picked.
    fn open_send_to_new_only(&mut self, ctx: &TabCtx) {
        if self.entries.is_empty() {
            return;
        }
        let source = VaultFilePickerSource::new(Arc::clone(ctx.vault), Arc::clone(ctx.recents));
        self.synth_send = Some(SynthSendState::PickExisting {
            picker: FuzzyPicker::new(source),
            new_only: true,
        });
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
    /// created — `apply_synth_scaffold` writes the marker fresh). When
    /// `new_only` is set, entries whose `date` is at or before the note's
    /// last-synth watermark are dropped before planning (mirrors `ft synth
    /// grow --new-only`); the watermark is derived from the note's
    /// existing callouts, and an unavailable watermark falls back to
    /// shipping all missing entries with an informational toast.
    fn commit_send(
        &mut self,
        ctx: &mut TabCtx,
        vault_rel_path: &Path,
        mark_synth: bool,
        new_only: bool,
    ) {
        let mut entries = self.entries_to_send();
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

        // `n` (new-only): filter to entries newer than the note's
        // last-synth watermark before planning. The planner's
        // dedup-on-append runs after this, so the two filters compose.
        if new_only {
            let abs = ctx.vault.path.join(vault_rel_path);
            let content = match std::fs::read_to_string(&abs) {
                Ok(c) => c,
                Err(e) => {
                    crate::tui::notes_actions::queue_toast(
                        ctx,
                        &format!("send-to-synth-new-only: could not read note: {e}"),
                        ToastStyle::Error,
                    );
                    return;
                }
            };
            let callouts = ft_core::synth::callout::parse(&content);
            match ft_core::git::RepoMap::discover(&ctx.vault.path) {
                Ok(repo) => {
                    match ft_core::synth::accrete::last_synth_watermark(repo.root(), &callouts) {
                        Ok(Some((_sha, watermark_date))) => {
                            entries.retain(|e| e.date > watermark_date);
                        }
                        Ok(None) => {
                            crate::tui::notes_actions::queue_toast(
                                ctx,
                                "new-only: no last-synth watermark — shipping all missing entries",
                                ToastStyle::Info,
                            );
                        }
                        Err(e) => {
                            crate::tui::notes_actions::queue_toast(
                                ctx,
                                &format!("new-only watermark failed: {e}"),
                                ToastStyle::Error,
                            );
                            return;
                        }
                    }
                }
                Err(e) => {
                    crate::tui::notes_actions::queue_toast(
                        ctx,
                        &format!("new-only: could not find git repo: {e}"),
                        ToastStyle::Error,
                    );
                    return;
                }
            }
            if entries.is_empty() {
                crate::tui::notes_actions::queue_toast(
                    ctx,
                    "new-only: no entries newer than the watermark",
                    ToastStyle::Info,
                );
                return;
            }
        }

        let plan = match plan_synth_scaffold(ctx.vault, vault_rel_path, &entries) {
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

    /// Open the Sources Manager modal pre-landed on its add-source
    /// picker. Reads the shared snapshot so ghost rows reflect the
    /// latest installed vault state.
    fn open_sources_manager(&mut self, ctx: &TabCtx) {
        let Some(snap) = ctx.snapshot.clone() else {
            crate::tui::notes_actions::queue_toast(
                ctx,
                "graph is still building — retry in a moment",
                ToastStyle::Error,
            );
            return;
        };
        let source = crate::tui::widgets::JournalSourcePickerSource::new(
            Arc::clone(ctx.vault),
            Arc::clone(ctx.recents),
            &snap.graph,
        );
        let modal = crate::tui::modal::JournalSourcesModal {
            sources: self.sources.clone(),
            cursor: 0,
            window: self.window.clone(),
            picker: Some(FuzzyPicker::new(source)),
        };
        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
            crate::tui::modal::ActiveModal::JournalSources(modal),
        )));
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

    fn kind(&self) -> TabKind {
        TabKind::Journal
    }

    fn on_focus(&mut self, ctx: &mut TabCtx) -> Result<()> {
        // Priority: multi > single > add_sources. (Commits from the
        // Sources Manager / Append-or-Replace modal arrive via
        // `queue_journal_commit_sources` which rebuilds synchronously
        // and doesn't go through `on_focus`.)
        if let Some(request) = self.queued_multi.take() {
            self.queued_for = None;
            self.queued_add_sources = None;
            self.sources = request.targets;
            self.window = request.window;
            self.in_window_only = false;
            self.rebuild_journal(ctx);
        } else if let Some(target) = self.queued_for.take() {
            self.queued_add_sources = None;
            self.sources = vec![target];
            self.window = None;
            self.in_window_only = false;
            self.rebuild_journal(ctx);
        } else if let Some((targets, default_mode)) = self.queued_add_sources.take() {
            // Raise the Append/Replace prompt; don't mutate sources
            // yet. The modal commits via `JournalCommitSources` which
            // is serviced on the next focus.
            let modal = crate::tui::modal::JournalAppendOrReplaceModal {
                current_sources: self.sources.clone(),
                incoming_targets: targets,
                window: self.window.clone(),
                focus: default_mode,
            };
            *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenModal(Box::new(
                crate::tui::modal::ActiveModal::JournalAppendOrReplace(modal),
            )));
        }
        Ok(())
    }

    fn queue_journal_for(&mut self, target: &JournalTarget) {
        self.queued_for = Some(target.clone());
    }

    fn queue_journal_for_multi(&mut self, request: &MultiTargetRequest) {
        self.queued_multi = Some(request.clone());
    }

    fn queue_journal_add_sources(
        &mut self,
        targets: Vec<JournalTarget>,
        default_mode: AppendOrReplaceMode,
    ) {
        self.queued_add_sources = Some((targets, default_mode));
    }

    fn queue_journal_commit_sources(
        &mut self,
        ctx: &mut TabCtx,
        sources: Vec<JournalTarget>,
        window: Option<JournalWindow>,
    ) {
        self.sources = sources;
        self.window = window;
        self.in_window_only = false;
        self.rebuild_journal(ctx);
    }

    fn commands(&self) -> &'static [CommandDef] {
        JOURNAL_COMMANDS
    }

    fn keymap(&self) -> &KeyMap {
        &self.keymap
    }

    fn dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome {
        match cmd.name {
            "journal.open-sources-manager" => {
                self.open_sources_manager(ctx);
                CommandOutcome::Handled
            }
            "journal.reload" => {
                if !self.sources.is_empty() {
                    self.rebuild_journal(ctx);
                }
                CommandOutcome::Handled
            }
            "journal.clear" => {
                self.sources.clear();
                self.window = None;
                self.in_window_only = false;
                self.entries.clear();
                self.entry_matched_titles.clear();
                self.entry_selected.clear();
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
            "journal.send-to-synth-new-only" => {
                self.open_send_to_new_only(ctx);
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
        // keys before the keymap is consulted. (Source manager is on
        // the App's `ActiveModal` slot — the App's modal-driver
        // intercepts those keys before they reach this method.)
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

    fn render(&mut self, frame: &mut Frame, area: Rect, _ctx: &TabCtx) {
        render_journal(
            frame,
            area,
            &self.sources,
            self.window.as_ref(),
            self.in_window_only,
            &self.entries,
            &self.entry_matched_titles,
            self.selected,
            &self.entry_selected,
            &mut self.scroll_offset,
            self.last_error.as_deref(),
        );

        if let Some(state) = self.synth_send.as_mut() {
            render_synth_send(frame, area, state);
        }
    }

    fn help_sections(&self) -> Vec<HelpSection> {
        vec![
            HelpSection::new(
                "Sources",
                &[
                    ("/", "open the Sources Manager (lands on add-source picker)"),
                    ("a", "open the Sources Manager (alias for /)"),
                    ("R", "reload the journal"),
                    ("c", "clear all sources"),
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
                    (
                        "n",
                        "append only entries newer than the picked note's last synth",
                    ),
                    ("Shift+s", "create a new synth note (folder + title)"),
                ],
            ),
        ]
    }
}

/// Render the always-visible Sources strip (exactly 2 rows). Empty
/// state, single-source, multi-source — all share the same shape so
/// the entry-list scroll math stays stable across state transitions.
fn render_sources_strip(
    frame: &mut Frame,
    area: Rect,
    sources: &[JournalTarget],
    window: Option<&JournalWindow>,
    in_window_only: bool,
) {
    if area.height < 2 {
        return;
    }
    let mut header = format!("Sources ({})", sources.len());
    if let Some(w) = window {
        header.push_str(&format!(" [window: {}]", window_label(w)));
    }
    if in_window_only {
        header.push_str(" [filter: in-window]");
    }
    let header_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let body_area = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            header,
            Style::default()
                .fg(palette::PRIMARY)
                .add_modifier(Modifier::BOLD),
        ))),
        header_area,
    );

    if sources.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "no sources loaded — press / to manage sources",
                Style::default().fg(palette::DIM),
            ))),
            body_area,
        );
        return;
    }

    let labels: Vec<String> = sources.iter().map(|t| t.label()).collect();
    let body_text = truncate_source_list(&labels, body_area.width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            body_text,
            Style::default().fg(palette::DIM),
        ))),
        body_area,
    );
}

/// Join `labels` with ", " up to `width` chars. When the joined string
/// would exceed `width`, truncate after the last fully-fitting label
/// and append `…, +K more` where K is the number of labels elided.
fn truncate_source_list(labels: &[String], width: usize) -> String {
    if labels.is_empty() {
        return String::new();
    }
    if width == 0 {
        return String::new();
    }
    let full = labels.join(", ");
    if full.chars().count() <= width {
        return full;
    }
    // Greedy: pack labels until adding the next one (with separator
    // and suffix budget) would overflow.
    let mut out = String::new();
    let mut shown = 0usize;
    for (i, label) in labels.iter().enumerate() {
        let remaining = labels.len() - i;
        let suffix = format!("…, +{remaining} more");
        let sep = if shown == 0 { "" } else { ", " };
        // Reserve space for the suffix if there's at least one more
        // label past this one (or if appending this one alone would
        // overflow).
        let candidate = format!("{out}{sep}{label}");
        let candidate_with_suffix = format!(
            "{candidate}{}",
            if i + 1 < labels.len() {
                format!(", {suffix}")
            } else {
                String::new()
            }
        );
        if candidate_with_suffix.chars().count() <= width
            || (shown == 0 && i + 1 == labels.len() && candidate.chars().count() <= width)
        {
            out = candidate;
            shown += 1;
        } else {
            break;
        }
    }
    if shown < labels.len() {
        let remaining = labels.len() - shown;
        let sep = if shown == 0 { "" } else { ", " };
        out.push_str(&format!("{sep}…, +{remaining} more"));
        // If even the suffix alone overflows, hard-truncate.
        if out.chars().count() > width {
            out = out.chars().take(width).collect();
        }
    }
    out
}

fn window_label(w: &JournalWindow) -> String {
    match w {
        JournalWindow::Since(d) => {
            let days = d.num_days();
            if days >= 1 {
                format!("since {days}d")
            } else {
                format!("since {}h", d.num_hours())
            }
        }
        JournalWindow::Range { from, to } => format!("range {from}..{to}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_journal(
    frame: &mut Frame,
    area: Rect,
    sources: &[JournalTarget],
    window: Option<&JournalWindow>,
    in_window_only: bool,
    entries: &[JournalEntry],
    entry_matched_titles: &[Vec<String>],
    selected: usize,
    entry_selected: &std::collections::HashSet<usize>,
    scroll_offset: &mut usize,
    last_error: Option<&str>,
) {
    // Tab block: title still carries entry-count for terminal-width
    // compatibility but is no longer the sole signal of what's loaded.
    let title = if sources.is_empty() {
        " Journal ".to_string()
    } else if sources.len() == 1 {
        format!(
            " Journal — {} ({} entries) ",
            sources[0].label(),
            entries.len()
        )
    } else {
        format!(
            " Journal — {} sources ({} entries) ",
            sources.len(),
            entries.len()
        )
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Always-on Sources strip (2 rows) + entry list below.
    if inner.height < 2 {
        return;
    }
    let strip_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 2,
    };
    let body_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };
    render_sources_strip(frame, strip_area, sources, window, in_window_only);

    if sources.is_empty() {
        // Empty-state body: just the error banner if any (the strip
        // already says "no sources loaded — press / …").
        if let Some(err) = last_error {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("error: {err}"),
                    Style::default().fg(palette::ERROR),
                ))),
                body_area,
            );
        }
        return;
    }

    if entries.is_empty() {
        let mut lines = vec![Line::from(Span::styled(
            "no journal entries for these sources",
            Style::default().fg(palette::DIM),
        ))];
        if let Some(err) = last_error {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("error: {err}"),
                Style::default().fg(palette::ERROR),
            )));
        }
        frame.render_widget(Paragraph::new(lines), body_area);
        return;
    }

    // Each entry contributes: 1 header line + (optional) `matched:` badge
    // + N wrapped body lines + 1 blank separator. We wrap body lines
    // manually (rather than letting ratatui's `Paragraph::wrap` do it) so
    // `entry_starts` stays in sync with the post-wrap visual line count —
    // that's what `scroll((y, 0))` indexes into. Without manual wrap the
    // cursor would drift relative to scroll on entries with long paragraphs.
    let inner = body_area;
    let wrap_width = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut entry_starts: Vec<usize> = Vec::with_capacity(entries.len());
    for (i, e) in entries.iter().enumerate() {
        entry_starts.push(lines.len());
        let is_cursor = i == selected;
        let is_multi = entry_selected.contains(&i);
        // Cursor wins the loudest treatment; multi-select gets the gold
        // band; everything else gets the dim full-width separator band.
        let header_style = if is_cursor {
            Style::default()
                .fg(palette::BLACK)
                .bg(palette::PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else if is_multi {
            Style::default()
                .fg(palette::BLACK)
                .bg(palette::SECONDARY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(palette::PRIMARY)
                .bg(palette::ENTRY_HEADER_BG)
                .add_modifier(Modifier::BOLD)
        };
        // A leading glyph keeps multi-select legible even when the
        // cursor's band overrides the gold background.
        let marker = if is_multi { "● " } else { "  " };
        lines.push(Line::from(Span::styled(
            pad_to_width(
                &format!("{marker}{}  {}", e.date, e.source_title),
                wrap_width,
            ),
            header_style,
        )));
        // Always surface which sources each paragraph matched (multi-
        // source only — in single-source mode the match is the lone
        // source and the badge would be pure noise).
        let empty: Vec<String> = Vec::new();
        let titles = entry_matched_titles.get(i).unwrap_or(&empty);
        if !titles.is_empty() {
            let badge = format!("    ↳ matched: {}", titles.join(", "));
            lines.push(Line::from(Span::styled(
                badge,
                Style::default().fg(palette::SECONDARY),
            )));
        }
        for body_line in e.section_text.lines() {
            for wrapped in wrap_line(body_line, wrap_width) {
                lines.push(Line::from(inline_markdown_spans(&wrapped)));
            }
        }
        lines.push(Line::from(""));
    }

    // Scroll so the selected entry shows its header band *and* a peek of
    // its content — never just the band stranded on the bottom row. We
    // keep up to `MIN_VISIBLE` lines of the entry on screen, capped to
    // the entry's own height so short entries don't over-scroll.
    const MIN_VISIBLE: usize = 4;
    let view_height = inner.height as usize;
    let total_lines = lines.len();
    let selected_start = entry_starts.get(selected).copied().unwrap_or(0);
    let selected_end = entry_starts
        .get(selected + 1)
        .copied()
        .unwrap_or(total_lines);
    let want = MIN_VISIBLE.min(selected_end.saturating_sub(selected_start));
    let desired_end = selected_start.saturating_add(want).min(total_lines);
    if selected_start < *scroll_offset {
        // Header above the viewport — pull it to the top.
        *scroll_offset = selected_start;
    } else if desired_end > scroll_offset.saturating_add(view_height) {
        // Wanted context falls below the fold — scroll just enough to
        // reveal it, but never past the header (so tall entries keep
        // their title on screen).
        *scroll_offset = desired_end.saturating_sub(view_height).min(selected_start);
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
/// Build styled spans for one already-wrapped body line, applying
/// minimal inline-markdown styling: `[[wikilinks]]` (gold), `[text](url)`
/// markdown links (orange underlined), `**bold**` (bold), and
/// `` `code` `` (dim). Italic and strikethrough are intentionally
/// skipped — single-asterisk emphasis is ambiguous in prose (often used
/// for multiplication or footnotes).
///
/// Applied AFTER wrap, so a token split across wrap boundaries
/// degrades to plain text on both fragments. Acceptable for a feed of
/// short paragraphs.
fn inline_markdown_spans(line: &str) -> Vec<Span<'static>> {
    let mut out: Vec<Span<'static>> = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    let mut plain_start = 0usize;

    let flush_plain = |out: &mut Vec<Span<'static>>, plain_start: &mut usize, end: usize| {
        if end > *plain_start {
            out.push(Span::raw(line[*plain_start..end].to_string()));
        }
        *plain_start = end;
    };

    while i < bytes.len() {
        // [[wikilink]] or [[wikilink|display]]
        if bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            if let Some(end) = find_balanced(line, i, "[[", "]]") {
                flush_plain(&mut out, &mut plain_start, i);
                out.push(Span::styled(
                    line[i..end].to_string(),
                    Style::default().fg(palette::SECONDARY),
                ));
                i = end;
                plain_start = end;
                continue;
            }
        }
        // [text](url) markdown link — keep it simple: a `[` not followed
        // by `[` that has a matching `](...)`.
        if bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] != b'[' {
            if let Some(end) = find_md_link(line, i) {
                flush_plain(&mut out, &mut plain_start, i);
                out.push(Span::styled(
                    line[i..end].to_string(),
                    Style::default()
                        .fg(palette::PRIMARY)
                        .add_modifier(Modifier::UNDERLINED),
                ));
                i = end;
                plain_start = end;
                continue;
            }
        }
        // **bold**
        if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            if let Some(end) = find_balanced(line, i, "**", "**") {
                flush_plain(&mut out, &mut plain_start, i);
                out.push(Span::styled(
                    line[i..end].to_string(),
                    Style::default().add_modifier(Modifier::BOLD),
                ));
                i = end;
                plain_start = end;
                continue;
            }
        }
        // `inline code`
        if bytes[i] == b'`' {
            if let Some(end) = find_balanced(line, i, "`", "`") {
                flush_plain(&mut out, &mut plain_start, i);
                out.push(Span::styled(
                    line[i..end].to_string(),
                    Style::default().fg(palette::DIM),
                ));
                i = end;
                plain_start = end;
                continue;
            }
        }
        i += 1;
    }
    flush_plain(&mut out, &mut plain_start, bytes.len());
    out
}

/// Find the end (exclusive byte offset) of a balanced `open…close`
/// span starting at `start`. Returns `None` when no closing token is
/// found on the same line.
fn find_balanced(line: &str, start: usize, open: &str, close: &str) -> Option<usize> {
    let after_open = start + open.len();
    if after_open > line.len() {
        return None;
    }
    line[after_open..]
        .find(close)
        .map(|rel| after_open + rel + close.len())
}

/// Find the end of a `[text](url)` markdown link starting at `start`.
/// Returns `None` if either bracket isn't balanced on this line.
fn find_md_link(line: &str, start: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    if start >= bytes.len() || bytes[start] != b'[' {
        return None;
    }
    let close_text = line[start + 1..].find(']')? + start + 1;
    if close_text + 1 >= bytes.len() || bytes[close_text + 1] != b'(' {
        return None;
    }
    let close_url = line[close_text + 2..].find(')')? + close_text + 2;
    Some(close_url + 1)
}

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

/// Pad `s` with trailing spaces to exactly `width` chars so a styled
/// span fills the full row width (giving the header band a solid
/// background). Over-long input is truncated to `width`. A `width` of 0
/// returns the string unchanged.
fn pad_to_width(s: &str, width: usize) -> String {
    if width == 0 {
        return s.to_string();
    }
    let len = s.chars().count();
    if len < width {
        let mut out = s.to_string();
        out.push_str(&" ".repeat(width - len));
        out
    } else if len > width {
        s.chars().take(width).collect()
    } else {
        s.to_string()
    }
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
/// `absolute_path`. Delegates to [`ft_core::synth::callout::upsert_synth_frontmatter`]
/// (the core pure transform) so the marker and the `ft-synth-targets` key
/// compose without clobbering each other. This thin wrapper handles the
/// I/O (read + atomic write).
fn mark_note_as_synth(absolute_path: &Path) -> std::io::Result<()> {
    let content = std::fs::read_to_string(absolute_path)?;
    let new_content = ft_core::synth::callout::upsert_synth_frontmatter(&content, None);
    if new_content == content {
        return Ok(());
    }
    ft_core::fs::write_atomic(absolute_path, &new_content).map_err(std::io::Error::other)
}

// `upsert_ft_synth_marker` (the pure marker-only transform) has moved
// to `ft_core::synth::callout::upsert_synth_frontmatter`, which also
// handles the optional `ft-synth-targets` key. The TUI layer no longer
// keeps its own copy; callers use the core helper directly.
//
// The three unit tests below (`upsert_ft_synth_marker_*`) now exercise
// the core helper to keep coverage of the TUI-layer behavior it backs.

/// Render whichever step of the send-to-synth flow is active.
fn render_synth_send(frame: &mut Frame, area: Rect, state: &mut SynthSendState) {
    match state {
        SynthSendState::PickExisting { picker, .. } => {
            let popup = centered_rect(70, 70, area);
            frame.render_widget(Clear, popup);
            picker.render(frame, popup);
        }
        SynthSendState::PickFolder(picker) => {
            let popup = centered_rect(70, 70, area);
            frame.render_widget(Clear, popup);
            picker.render(frame, popup);
        }
        SynthSendState::NonSynthPrompt { path, focus, .. } => {
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
    use super::{inline_markdown_spans, wrap_line};

    fn rendered_text(spans: &[ratatui::text::Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn inline_markdown_plain_passes_through() {
        let spans = inline_markdown_spans("just some prose, nothing special.");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "just some prose, nothing special.");
    }

    #[test]
    fn inline_markdown_wikilink_is_styled_and_text_preserved() {
        let spans = inline_markdown_spans("see [[Foo]] for context");
        assert_eq!(
            rendered_text(&spans),
            "see [[Foo]] for context",
            "rendered text must round-trip verbatim"
        );
        // The wikilink span exists and is colored.
        assert!(spans
            .iter()
            .any(|s| s.content == "[[Foo]]" && s.style.fg == Some(crate::tui::palette::SECONDARY)));
    }

    #[test]
    fn inline_markdown_bold_and_code_and_md_link() {
        let line = "**urgent**: read `config.toml` and see [docs](https://x.dev)";
        let spans = inline_markdown_spans(line);
        assert_eq!(rendered_text(&spans), line);
        let bold = spans.iter().any(|s| s.content == "**urgent**");
        let code = spans.iter().any(|s| s.content == "`config.toml`");
        let link = spans.iter().any(|s| s.content == "[docs](https://x.dev)");
        assert!(bold, "missing bold span: {spans:?}");
        assert!(code, "missing code span: {spans:?}");
        assert!(link, "missing md-link span: {spans:?}");
    }

    #[test]
    fn inline_markdown_unterminated_token_stays_plain() {
        // No closing `]]` → must not panic and must not eat the rest of the line.
        let spans = inline_markdown_spans("see [[Foo without close");
        assert_eq!(rendered_text(&spans), "see [[Foo without close");
        // Whole line is one plain span (no styled match found).
        assert!(spans
            .iter()
            .all(|s| s.style.fg.is_none() && s.style.add_modifier.is_empty()));
    }

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

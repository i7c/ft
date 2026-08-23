//! Shared send-to-synth state machine for the Gather and Search tabs.
//!
//! Both tabs ship paragraphs into a synth note via the same multi-step
//! flow: `s` picks an existing note (append, with a 3-way prompt when
//! the picked note lacks the `ft.synth.enabled: true` marker), `S`
//! creates a new note (folder picker → title prompt), and `n`
//! appends only entries newer than the note's last-synth watermark
//! (a gather-only refinement; search has no dates). The Gather tab
//! additionally has `o` (load a synth note's targets as the source
//! set + badge context), which the Search tab never opens.
//!
//! A host implements [`SynthSendHost`] to supply the paragraphs to pin
//! when a target is committed (`synth_sources`) and to react to a
//! successful commit (`on_synth_committed`). The flow owns all picker
//! and prompt state, so the tabs' key handlers and renderers stay thin.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use ft_core::synth::scaffold::{apply_synth_scaffold, plan_synth_scaffold};
use ft_core::synth::source::SynthSource;

use crate::tui::notes_actions::create::enumerate_vault_folders;
use crate::tui::palette;
use crate::tui::tab::{AppRequest, EventOutcome, TabCtx, ToastStyle};
use crate::tui::widgets::{
    EditBuffer, FuzzyPicker, PathListPickerSource, PickerOutcome, VaultFilePickerSource,
};

/// User's choice when sending to an existing non-synth note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonSynthChoice {
    /// Append protected sections without touching frontmatter.
    AppendAnyway,
    /// Insert/upgrade frontmatter to include `ft.synth.enabled: true`, then
    /// append.
    MarkAndAppend,
}

/// Multi-step send-to-synth state. Active only while a picker or prompt
/// overlay owns the keyboard; cleared on completion or `Esc`.
pub enum SynthSendState {
    /// `s` / `n` — fuzzy picker over every `.md` in the vault. `new_only`
    /// records which command opened this picker so the on-pick handler
    /// knows whether to apply the watermark filter.
    PickExisting {
        picker: FuzzyPicker<VaultFilePickerSource>,
        new_only: bool,
    },
    /// User picked a real note but its frontmatter lacks
    /// `ft.synth.enabled: true`. Inline 3-way prompt: append anyway, mark and
    /// append, or cancel. `new_only` is carried through so the `n` flow
    /// still filters after the mark/append decision.
    NonSynthPrompt {
        path: PathBuf,
        focus: NonSynthChoice,
        new_only: bool,
    },
    /// `o` — fuzzy picker over the vault's synth notes. Picking one
    /// loads its `ft.synth.targets` as the source set and installs it
    /// as the badge context note (Gather tab only).
    PickContextNote(FuzzyPicker<PathListPickerSource>),
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

/// A tab that hosts a [`SynthSendFlow`].
pub trait SynthSendHost {
    /// The paragraphs to ship to the target note: user-selected entries,
    /// or all candidates when nothing is selected. Called once per
    /// committed target. `new_only` asks the host to drop entries at or
    /// before the target note's last-synth watermark (the gather host
    /// has dated entries and honors it; the search host ignores it).
    fn synth_sources(
        &mut self,
        ctx: &mut TabCtx,
        target: &Path,
        new_only: bool,
    ) -> Vec<SynthSource>;

    /// Called after a successful scaffold write + editor handoff so the
    /// host can update its badge context (Gather tab sets its
    /// context-note here; Recent requests a graph refresh). Default:
    /// no-op.
    fn on_synth_committed(&mut self, _ctx: &mut TabCtx, _target: &Path) {}

    /// `o` picked a context synth note. Default: no-op (only the Gather
    /// tab uses this flow).
    fn on_context_note_picked(&mut self, _ctx: &mut TabCtx, _path: PathBuf) {}
}

/// One tab's send-to-synth state machine.
#[derive(Default)]
pub struct SynthSendFlow {
    state: Option<SynthSendState>,
}

impl SynthSendFlow {
    pub fn new() -> Self {
        Self { state: None }
    }

    /// True while a picker or prompt owns the keyboard.
    pub fn is_active(&self) -> bool {
        self.state.is_some()
    }

    /// `s` — open the existing-note fuzzy picker (append, all entries).
    pub fn open_existing(&mut self, ctx: &TabCtx, new_only: bool) {
        let source = VaultFilePickerSource::with_scan(
            Arc::clone(ctx.vault),
            Arc::clone(ctx.recents),
            ctx.snapshot.as_ref().map(|s| Arc::clone(&s.scan)),
        );
        self.state = Some(SynthSendState::PickExisting {
            picker: FuzzyPicker::new(source),
            new_only,
        });
    }

    /// `S` — open the folder fuzzy picker for the create-new flow.
    pub fn open_new(&mut self, ctx: &TabCtx) {
        let folders = enumerate_vault_folders(ctx.vault);
        let source = PathListPickerSource::new(folders);
        self.state = Some(SynthSendState::PickFolder(FuzzyPicker::new(source)));
    }

    /// `o` — open the synth-note context picker (Gather tab only).
    pub fn open_context_note(&mut self, _ctx: &TabCtx, synth_notes: Vec<PathBuf>) {
        let source = PathListPickerSource::new(synth_notes);
        self.state = Some(SynthSendState::PickContextNote(FuzzyPicker::new(source)));
    }

    /// Drive one key through the state machine. Returns `Consumed` while
    /// a step is active so the tab's keymap is bypassed.
    pub fn handle_key<H: SynthSendHost>(
        &mut self,
        k: KeyEvent,
        ctx: &mut TabCtx,
        host: &mut H,
    ) -> EventOutcome {
        let Some(state) = self.state.take() else {
            return EventOutcome::NotHandled;
        };
        match state {
            SynthSendState::PickExisting {
                mut picker,
                new_only,
            } => match picker.handle_key(k) {
                PickerOutcome::Selected(hit) => {
                    self.on_existing_picked(ctx, host, hit.path, new_only)
                }
                PickerOutcome::Cancelled => {}
                PickerOutcome::StillOpen | PickerOutcome::NotHandled => {
                    self.state = Some(SynthSendState::PickExisting { picker, new_only });
                }
            },
            SynthSendState::NonSynthPrompt {
                path,
                focus,
                new_only,
            } => match k.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.commit_send(
                    ctx,
                    host,
                    &path,
                    focus == NonSynthChoice::MarkAndAppend,
                    new_only,
                ),
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    self.commit_send(ctx, host, &path, false, new_only);
                }
                KeyCode::Char('m') | KeyCode::Char('M') => {
                    self.commit_send(ctx, host, &path, true, new_only);
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {}
                KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                    let next = match focus {
                        NonSynthChoice::AppendAnyway => NonSynthChoice::MarkAndAppend,
                        NonSynthChoice::MarkAndAppend => NonSynthChoice::AppendAnyway,
                    };
                    self.state = Some(SynthSendState::NonSynthPrompt {
                        path,
                        focus: next,
                        new_only,
                    });
                }
                _ => {
                    self.state = Some(SynthSendState::NonSynthPrompt {
                        path,
                        focus,
                        new_only,
                    });
                }
            },
            SynthSendState::PickContextNote(mut picker) => match picker.handle_key(k) {
                PickerOutcome::Selected(path) => host.on_context_note_picked(ctx, path),
                PickerOutcome::Cancelled => {}
                PickerOutcome::StillOpen | PickerOutcome::NotHandled => {
                    self.state = Some(SynthSendState::PickContextNote(picker));
                }
            },
            SynthSendState::PickFolder(mut picker) => match picker.handle_key(k) {
                PickerOutcome::Selected(folder) => {
                    let folder = if folder == Path::new(".") {
                        PathBuf::new()
                    } else {
                        folder
                    };
                    self.state = Some(SynthSendState::TitlePrompt {
                        folder,
                        buf: EditBuffer::default(),
                        error: None,
                    });
                }
                PickerOutcome::Cancelled => {}
                PickerOutcome::StillOpen | PickerOutcome::NotHandled => {
                    self.state = Some(SynthSendState::PickFolder(picker));
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
                        self.state = Some(SynthSendState::TitlePrompt {
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
                        self.commit_send(ctx, host, &target, false, false);
                    }
                }
                // All text edits + cursor moves + readline chords go
                // through the buffer's EDIT_KEYMAP. Any returned
                // outcome (Consumed or NotHandled) re-parks the state.
                _ => {
                    let _ = buf.handle_event(k);
                    self.state = Some(SynthSendState::TitlePrompt {
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
    fn on_existing_picked<H: SynthSendHost>(
        &mut self,
        ctx: &mut TabCtx,
        host: &mut H,
        path: PathBuf,
        new_only: bool,
    ) {
        let abs = ctx.vault.path.join(&path);
        let is_synth = std::fs::read_to_string(&abs)
            .map(|c| ft_core::synth::callout::is_synth_note(&c))
            .unwrap_or(false);
        if is_synth {
            self.commit_send(ctx, host, &path, false, new_only);
        } else {
            self.state = Some(SynthSendState::NonSynthPrompt {
                path,
                focus: NonSynthChoice::MarkAndAppend,
                new_only,
            });
        }
    }

    /// Perform the scaffold write + editor handoff. `mark_synth` ensures
    /// the on-disk file's frontmatter includes `ft.synth.enabled: true`
    /// before the scaffold is applied (no-op when the file already has
    /// the marker or is being created). `new_only` is forwarded to the
    /// host, which applies the watermark filter to its dated entries.
    fn commit_send<H: SynthSendHost>(
        &mut self,
        ctx: &mut TabCtx,
        host: &mut H,
        vault_rel_path: &Path,
        mark_synth: bool,
        new_only: bool,
    ) {
        let sources = host.synth_sources(ctx, vault_rel_path, new_only);
        if sources.is_empty() {
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
                    &format!("could not add ft.synth marker: {e}"),
                    ToastStyle::Error,
                );
                return;
            }
        }

        let plan = match plan_synth_scaffold(ctx.vault, vault_rel_path, &sources) {
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
        host.on_synth_committed(ctx, vault_rel_path);
        *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
            path: written,
            line: 1,
        });
    }

    /// Render whichever step is active.
    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        match state {
            SynthSendState::PickExisting { picker, .. } => {
                let popup = centered_rect(70, 70, area);
                frame.render_widget(Clear, popup);
                picker.render(frame, popup);
            }
            SynthSendState::PickContextNote(picker) | SynthSendState::PickFolder(picker) => {
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
}

/// Ensure a note carries `ft.synth.enabled: true` in its frontmatter
/// (no-op when already marked). Used by the mark-and-append path.
pub fn mark_note_as_synth(absolute_path: &Path) -> std::io::Result<()> {
    let content = std::fs::read_to_string(absolute_path)?;
    let new_content = ft_core::synth::callout::upsert_synth_frontmatter(&content, None);
    if new_content == content {
        return Ok(());
    }
    ft_core::fs::write_atomic(absolute_path, &new_content).map_err(std::io::Error::other)
}

/// A centered popup rectangle (percent of the tab area).
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

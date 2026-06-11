//! Tab-agnostic "create a new note" flow.
//!
//! Five visible steps:
//! 1. (only when entering via `C`) **TemplatePicking** — fuzzy pick a
//!    template under the configured templates dir.
//! 2. **FolderPicking** — fuzzy pick a destination folder under the vault
//!    root.
//! 3. **FilenamePrompt** — single-line edit for the filename.
//! 4. **VarPrompt** — repeated single-line prompt, once per `vars.KEY`
//!    referenced by the template (template path only).
//! 5. (only on collision) **CollisionPrompt** — overwrite / use-existing
//!    / cancel.
//!
//! The `c` entry-point skips steps 1 and 4 (no template, no vars) so the
//! minimal path is FolderPicking → FilenamePrompt → commit.
//!
//! Each step handler returns a [`CreateStep`] describing how the caller's
//! own state should advance:
//!
//! * `Stay` — consumed, no state change.
//! * `Transition(next)` — replace the current `CreateState` with `next`.
//! * `Finished` — the flow ended; the caller drops the slot.
//! * `NotHandled` — the key was not relevant; the caller may try its own
//!   bindings.
//!
//! [`handle_key`] is the public entry point; tabs feed every key event
//! through it while the create flow is active.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ft_core::fs::write_atomic;
use ft_core::notes::template::{render as render_template, TemplateContext};
use ft_core::vault::Vault;
use regex::Regex;

use crate::tui::{
    notes_actions::queue_toast,
    tab::{AppRequest, TabCtx, ToastStyle},
    widgets::{
        edit_keymap::EditOutcome, EditBuffer, FuzzyPicker, PathListPickerSource, PickerOutcome,
    },
};

// ── State ────────────────────────────────────────────────────────────

/// The create flow's state machine. Owned by whichever tab is currently
/// running the flow (Notes tab wraps it in [`NotesState::Creating`];
/// Graph tab stores `Option<CreateState>` directly).
pub enum CreateState {
    TemplatePicking {
        picker: FuzzyPicker<PathListPickerSource>,
        /// When `Some(folder)`, the flow skips `FolderPicking` after the
        /// template is chosen and jumps straight to `FilenamePrompt` with
        /// this folder. Used by tabs that already know the target folder
        /// from the user's selection (Graph tab `C`). `None` preserves
        /// the historical Notes-tab flow (template → folder → filename).
        folder_seed: Option<PathBuf>,
        /// When `Some(filename)`, the flow skips BOTH folder picking AND
        /// filename prompt — after template selection the note is created
        /// at `folder_seed/filename` immediately. Used by the Graph tab
        /// for ghost nodes where the target path is fully known.
        ghost_filename: Option<String>,
    },
    FolderPicking {
        template: Option<TemplatePick>,
        picker: FuzzyPicker<PathListPickerSource>,
    },
    FilenamePrompt {
        template: Option<TemplatePick>,
        folder: PathBuf,
        buf: EditBuffer,
        error: Option<String>,
    },
    VarPrompt {
        template: TemplatePick,
        folder: PathBuf,
        filename: String,
        vars_so_far: BTreeMap<String, String>,
        /// Index into [`TemplatePick::vars_needed`] currently being
        /// prompted. Advances on `Enter`; commit fires when it reaches
        /// the end of the list.
        next_idx: usize,
        buf: EditBuffer,
    },
    CollisionPrompt {
        template: Option<TemplatePick>,
        folder: PathBuf,
        filename: String,
        vars: BTreeMap<String, String>,
        abs_path: PathBuf,
        focus: CollisionChoice,
    },
}

/// A template selected in step 1, cached so we don't re-read the source
/// each step. `vars_needed` is the discovered list of `vars.KEY`
/// references in stable first-appearance order.
#[derive(Debug, Clone)]
pub struct TemplatePick {
    /// Template-dir-relative path, shown as the picker label. The
    /// absolute path is recoverable from `vault.templates_dir().join(rel)`
    /// — we don't cache it because `rel` is the user-facing identity.
    pub rel: PathBuf,
    /// Cached template source — read once, rendered as many times as we
    /// need (typically just once on commit).
    pub source: String,
    /// `vars.KEY` references discovered via regex pass, in first-
    /// appearance order. Empty when the template doesn't prompt.
    pub vars_needed: Vec<String>,
}

/// Which option is focused in the 3-way collision prompt. `←/→` cycle
/// through; `o`/`u`/`c` jump directly; `Enter` commits the focused
/// choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionChoice {
    Overwrite,
    UseExisting,
    Cancel,
}

impl CollisionChoice {
    pub fn prev(self) -> Self {
        match self {
            Self::Overwrite => Self::Cancel,
            Self::UseExisting => Self::Overwrite,
            Self::Cancel => Self::UseExisting,
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Overwrite => Self::UseExisting,
            Self::UseExisting => Self::Cancel,
            Self::Cancel => Self::Overwrite,
        }
    }
}

/// Result of feeding a key event to the create flow. The caller maps
/// these to its own tab-level state transitions.
#[allow(clippy::large_enum_variant)] // single-slot at App level; size doesn't matter
pub enum CreateStep {
    /// Key consumed; no state change.
    Stay,
    /// Key not recognized; the caller may try its own bindings.
    NotHandled,
    /// Replace the current `CreateState` with `next`.
    Transition(CreateState),
    /// The flow has ended (either committed or cancelled). The caller
    /// should drop its create-state slot.
    Finished,
}

// ── Entry points ────────────────────────────────────────────────────

/// Build a `FolderPicking` state. Used directly by the `c` binding
/// (template=None) and by the template picker's `Enter` (template=Some).
pub fn begin_folder_picking(ctx: &TabCtx, template: Option<TemplatePick>) -> CreateState {
    let folders = enumerate_vault_folders(ctx.vault);
    CreateState::FolderPicking {
        template,
        picker: FuzzyPicker::new(PathListPickerSource::new(folders)),
    }
}

/// Build a `TemplatePicking` state. The picker source is seeded with
/// the templates dir's `.md` files, in sorted order. Empty dir is fine
/// — the picker shows no rows and the user can Esc back out.
///
/// `folder_seed`: when `Some`, the flow skips the folder picker after
/// the template is chosen and goes straight to the filename prompt
/// with that folder. Notes-tab callers pass `None`; tabs that derive
/// the folder from selection (Graph tab) pass `Some`.
///
/// `ghost_filename`: when `Some` AND `folder_seed` is also `Some`,
/// the flow skips BOTH folder picker AND filename prompt — after
/// template selection the note is created at
/// `folder_seed/ghost_filename` immediately. Used by the Graph tab
/// for ghost nodes.
pub fn begin_template_picking(
    ctx: &TabCtx,
    folder_seed: Option<PathBuf>,
    ghost_filename: Option<String>,
) -> CreateState {
    let templates = enumerate_templates(ctx.vault);
    CreateState::TemplatePicking {
        picker: FuzzyPicker::new(PathListPickerSource::new(templates)),
        folder_seed,
        ghost_filename,
    }
}

/// Build a `FilenamePrompt` directly, skipping the folder picker.
/// Used by tabs that already know the target folder from the user's
/// current selection (Graph tab seeds folder from the selected row).
pub fn begin_filename_prompt(folder: PathBuf, template: Option<TemplatePick>) -> CreateState {
    CreateState::FilenamePrompt {
        template,
        folder,
        buf: EditBuffer::default(),
        error: None,
    }
}

// ── Vault enumeration ───────────────────────────────────────────────

/// Walk the vault root and return every directory under it as a
/// vault-relative path, sorted alphabetically with the vault root
/// itself (empty path, displayed as ".") first.
///
/// Skips dotfiles (`.obsidian`, `.git`, `.ft`), `attachments/`, and the
/// configured templates dir.
pub fn enumerate_vault_folders(vault: &Vault) -> Vec<PathBuf> {
    use std::fs;
    let templates_abs = vault.templates_dir();
    let mut out: Vec<PathBuf> = vec![PathBuf::from(".")];

    fn walk(dir: &Path, root: &Path, templates_abs: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') {
                continue;
            }
            if name_str == "attachments" {
                continue;
            }
            if path == *templates_abs {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|_| path.clone());
            out.push(rel);
            walk(&path, root, templates_abs, out);
        }
    }

    walk(&vault.path, &vault.path, &templates_abs, &mut out);
    out.sort_by_key(|p| p.display().to_string());
    out
}

/// List `.md` files at the top level of the configured templates dir.
/// Returns templates-dir-relative paths. Missing dir → empty vec.
pub fn enumerate_templates(vault: &Vault) -> Vec<PathBuf> {
    let dir = vault.templates_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "md"))
        .map(|p| p.strip_prefix(&dir).map(|r| r.to_path_buf()).unwrap_or(p))
        .collect();
    out.sort_by_key(|p| p.display().to_string());
    out
}

/// Discover `vars.KEY` references in `source`, in first-appearance
/// order. Catches `{{ vars.foo }}` and any `{{ vars.foo | ... }}` chain;
/// bracket-lookup (`vars["foo"]`) is not supported (we don't use it in
/// any hand-ported template).
pub(crate) fn discover_template_vars(source: &str) -> Vec<String> {
    let re = Regex::new(r"\{\{\s*vars\.([a-zA-Z_][a-zA-Z0-9_]*)").unwrap();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut ordered: Vec<String> = Vec::new();
    for cap in re.captures_iter(source) {
        let name = cap[1].to_string();
        if seen.insert(name.clone()) {
            ordered.push(name);
        }
    }
    ordered
}

// ── Dispatcher + step handlers ──────────────────────────────────────

/// Feed a key event to the active `CreateState`. The caller maps the
/// returned [`CreateStep`] onto its own tab-level state transitions.
pub fn handle_key(state: &mut CreateState, k: KeyEvent, ctx: &TabCtx) -> CreateStep {
    match state {
        CreateState::TemplatePicking {
            picker,
            folder_seed,
            ghost_filename,
        } => handle_template_picker_key(k, picker, folder_seed, ghost_filename, ctx),
        CreateState::FolderPicking { template, picker } => {
            handle_folder_picker_key(k, template, picker, ctx)
        }
        CreateState::FilenamePrompt {
            template,
            folder,
            buf,
            error,
        } => handle_filename_key(k, template, folder, buf, error, ctx),
        CreateState::VarPrompt {
            template,
            folder,
            filename,
            vars_so_far,
            next_idx,
            buf,
        } => handle_var_key(
            k,
            template,
            folder,
            filename,
            vars_so_far,
            next_idx,
            buf,
            ctx,
        ),
        CreateState::CollisionPrompt {
            template,
            folder,
            filename,
            vars,
            abs_path,
            focus,
        } => handle_collision_key(k, template, folder, filename, vars, abs_path, focus, ctx),
    }
}

fn handle_template_picker_key(
    k: KeyEvent,
    picker: &mut FuzzyPicker<PathListPickerSource>,
    folder_seed: &mut Option<PathBuf>,
    ghost_filename: &mut Option<String>,
    ctx: &TabCtx,
) -> CreateStep {
    match picker.handle_key(k) {
        PickerOutcome::Selected(rel) => {
            let abs = ctx.vault.templates_dir().join(&rel);
            let source = match std::fs::read_to_string(&abs) {
                Ok(s) => s,
                Err(e) => {
                    queue_toast(
                        ctx,
                        &format!("could not read template: {e}"),
                        ToastStyle::Error,
                    );
                    return CreateStep::Finished;
                }
            };
            let vars_needed = discover_template_vars(&source);
            let template = TemplatePick {
                rel,
                source,
                vars_needed,
            };
            // Ghost target: folder_seed + ghost_filename are both set →
            // commit immediately at the exact path, skipping folder
            // picker and filename prompt.
            if ghost_filename.is_some() {
                if let (Some(folder), Some(filename)) = (folder_seed.take(), ghost_filename.take())
                {
                    let abs_path = ctx.vault.path.join(&folder).join(&filename);
                    commit_create(
                        ctx,
                        Some(&template),
                        &folder,
                        &filename,
                        &BTreeMap::new(),
                        &abs_path,
                    );
                    return CreateStep::Finished;
                }
            }
            // Tabs that pre-seeded the folder (Graph tab `C`) skip the
            // folder picker entirely and go straight to filename; the
            // historical Notes-tab flow (folder_seed=None) keeps the
            // folder picker as step 2.
            if let Some(folder) = folder_seed.take() {
                CreateStep::Transition(begin_filename_prompt(folder, Some(template)))
            } else {
                CreateStep::Transition(begin_folder_picking(ctx, Some(template)))
            }
        }
        PickerOutcome::Cancelled => CreateStep::Finished,
        PickerOutcome::StillOpen => CreateStep::Stay,
        PickerOutcome::NotHandled => CreateStep::NotHandled,
    }
}

fn handle_folder_picker_key(
    k: KeyEvent,
    template: &mut Option<TemplatePick>,
    picker: &mut FuzzyPicker<PathListPickerSource>,
    ctx: &TabCtx,
) -> CreateStep {
    match picker.handle_key(k) {
        PickerOutcome::Selected(folder) => {
            let folder = if folder == Path::new(".") {
                PathBuf::new()
            } else {
                folder
            };
            CreateStep::Transition(CreateState::FilenamePrompt {
                template: template.take(),
                folder,
                buf: EditBuffer::default(),
                error: None,
            })
        }
        PickerOutcome::Cancelled => {
            // Esc: back to template picker if we came from `C`, else finish.
            if template.take().is_some() {
                CreateStep::Transition(begin_template_picking(ctx, None, None))
            } else {
                CreateStep::Finished
            }
        }
        PickerOutcome::StillOpen => CreateStep::Stay,
        PickerOutcome::NotHandled => CreateStep::NotHandled,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_filename_key(
    k: KeyEvent,
    template: &mut Option<TemplatePick>,
    folder: &mut PathBuf,
    buf: &mut EditBuffer,
    error: &mut Option<String>,
    ctx: &TabCtx,
) -> CreateStep {
    match k.code {
        KeyCode::Esc => CreateStep::Transition(begin_folder_picking(ctx, template.take())),
        KeyCode::Enter => {
            let trimmed = buf.text.trim();
            if trimmed.is_empty() {
                *error = Some("filename is required".into());
                return CreateStep::Stay;
            }
            if trimmed.contains('/') || trimmed.contains('\\') {
                *error = Some("filename can't contain path separators".into());
                return CreateStep::Stay;
            }
            let filename = if trimmed.ends_with(".md") {
                trimmed.to_string()
            } else {
                format!("{trimmed}.md")
            };
            let abs_path = ctx.vault.path.join(folder.as_path()).join(&filename);

            if abs_path.exists() {
                return CreateStep::Transition(CreateState::CollisionPrompt {
                    template: template.take(),
                    folder: std::mem::take(folder),
                    filename,
                    vars: BTreeMap::new(),
                    abs_path,
                    focus: CollisionChoice::Overwrite,
                });
            }

            match template.take() {
                Some(tpl) if !tpl.vars_needed.is_empty() => {
                    CreateStep::Transition(CreateState::VarPrompt {
                        template: tpl,
                        folder: std::mem::take(folder),
                        filename,
                        vars_so_far: BTreeMap::new(),
                        next_idx: 0,
                        buf: EditBuffer::default(),
                    })
                }
                tpl => {
                    commit_create(
                        ctx,
                        tpl.as_ref(),
                        folder.as_path(),
                        &filename,
                        &BTreeMap::new(),
                        &abs_path,
                    );
                    CreateStep::Finished
                }
            }
        }
        _ => match buf.handle_event(k) {
            EditOutcome::Consumed => {
                *error = None;
                CreateStep::Stay
            }
            EditOutcome::NotHandled => CreateStep::NotHandled,
        },
    }
}

#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
fn handle_var_key(
    k: KeyEvent,
    template: &mut TemplatePick,
    folder: &mut PathBuf,
    filename: &mut String,
    vars_so_far: &mut BTreeMap<String, String>,
    next_idx: &mut usize,
    buf: &mut EditBuffer,
    ctx: &TabCtx,
) -> CreateStep {
    match k.code {
        KeyCode::Esc => CreateStep::Finished,
        KeyCode::Enter => {
            let key_name = template
                .vars_needed
                .get(*next_idx)
                .cloned()
                .unwrap_or_default();
            // Empty values are allowed — strict-undefined still rejects
            // keys missing from the map, so we always insert.
            vars_so_far.insert(key_name, buf.text.clone());
            *next_idx += 1;
            if *next_idx >= template.vars_needed.len() {
                let abs_path = ctx.vault.path.join(folder.as_path()).join(&*filename);
                commit_create(
                    ctx,
                    Some(template),
                    folder.as_path(),
                    filename,
                    vars_so_far,
                    &abs_path,
                );
                CreateStep::Finished
            } else {
                buf.text.clear();
                buf.cursor = 0;
                CreateStep::Stay
            }
        }
        _ => match buf.handle_event(k) {
            EditOutcome::Consumed => CreateStep::Stay,
            EditOutcome::NotHandled => CreateStep::NotHandled,
        },
    }
}

#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
fn handle_collision_key(
    k: KeyEvent,
    template: &mut Option<TemplatePick>,
    folder: &mut PathBuf,
    filename: &mut String,
    vars: &mut BTreeMap<String, String>,
    abs_path: &mut PathBuf,
    focus: &mut CollisionChoice,
    ctx: &TabCtx,
) -> CreateStep {
    match (k.code, k.modifiers) {
        (KeyCode::Char('o'), KeyModifiers::NONE) | (KeyCode::Char('O'), _) => {
            commit_with_choice(
                ctx,
                CollisionChoice::Overwrite,
                template.as_ref(),
                folder.as_path(),
                filename,
                vars,
                abs_path,
            );
            CreateStep::Finished
        }
        (KeyCode::Char('u'), KeyModifiers::NONE) | (KeyCode::Char('U'), _) => {
            commit_with_choice(
                ctx,
                CollisionChoice::UseExisting,
                template.as_ref(),
                folder.as_path(),
                filename,
                vars,
                abs_path,
            );
            CreateStep::Finished
        }
        (KeyCode::Char('c'), KeyModifiers::NONE) | (KeyCode::Char('C'), _) | (KeyCode::Esc, _) => {
            CreateStep::Transition(CreateState::FilenamePrompt {
                template: template.take(),
                folder: std::mem::take(folder),
                buf: EditBuffer::from(filename),
                error: None,
            })
        }
        (KeyCode::Enter, _) => {
            if *focus == CollisionChoice::Cancel {
                return CreateStep::Transition(CreateState::FilenamePrompt {
                    template: template.take(),
                    folder: std::mem::take(folder),
                    buf: EditBuffer::from(filename),
                    error: None,
                });
            }
            commit_with_choice(
                ctx,
                *focus,
                template.as_ref(),
                folder.as_path(),
                filename,
                vars,
                abs_path,
            );
            CreateStep::Finished
        }
        (KeyCode::Left, _) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
            *focus = focus.prev();
            CreateStep::Stay
        }
        (KeyCode::Right, _) | (KeyCode::Char('l'), KeyModifiers::NONE) => {
            *focus = focus.next();
            CreateStep::Stay
        }
        _ => CreateStep::NotHandled,
    }
}

// ── Commit helpers ──────────────────────────────────────────────────

/// Dispatch the collision prompt's three choices.
#[allow(clippy::too_many_arguments)]
fn commit_with_choice(
    ctx: &TabCtx,
    choice: CollisionChoice,
    template: Option<&TemplatePick>,
    folder: &Path,
    filename: &str,
    vars: &BTreeMap<String, String>,
    abs_path: &Path,
) {
    match choice {
        CollisionChoice::Overwrite => {
            commit_create(ctx, template, folder, filename, vars, abs_path);
        }
        CollisionChoice::UseExisting => {
            *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
                path: abs_path.to_path_buf(),
                line: 1,
            });
        }
        CollisionChoice::Cancel => {}
    }
}

/// Render the template (or fall back to `# <title>\n` for blank), write
/// atomically, queue an `OpenInEditor` request. Any failure → error
/// toast.
fn commit_create(
    ctx: &TabCtx,
    template: Option<&TemplatePick>,
    _folder: &Path,
    filename: &str,
    vars: &BTreeMap<String, String>,
    abs_path: &Path,
) {
    let title = abs_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let content = match template {
        None => format!("# {title}\n"),
        Some(tpl) => {
            let tctx = build_template_context(title.clone(), ctx.today, vars.clone());
            match render_template(&tpl.source, &tctx) {
                Ok(s) => s,
                Err(e) => {
                    queue_toast(
                        ctx,
                        &format!("template render failed: {e}"),
                        ToastStyle::Error,
                    );
                    return;
                }
            }
        }
    };
    if let Some(parent) = abs_path.parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                queue_toast(ctx, &format!("mkdir failed: {e}"), ToastStyle::Error);
                return;
            }
        }
    }
    if let Err(e) = write_atomic(abs_path, &content) {
        queue_toast(ctx, &format!("write failed: {e}"), ToastStyle::Error);
        return;
    }
    let rel = abs_path
        .strip_prefix(&ctx.vault.path)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| abs_path.to_path_buf());
    ctx.recents.record_open(&rel);
    *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
        path: abs_path.to_path_buf(),
        line: 1,
    });
    let _ = filename; // (kept in signature for future toast wording)
}

/// Build a [`TemplateContext`] honoring `FT_TODAY` for `now` (pinned to
/// `00:00:00` so test renders are deterministic) and falling back to
/// local wall-clock time otherwise.
///
/// `pub(crate)` because the section-move new-target sub-flow in
/// `tabs::notes` reuses it; eventually that flow will move into a
/// sibling `notes_actions` module and this can shrink to module-private.
pub(crate) fn build_template_context(
    title: String,
    today: NaiveDate,
    vars: BTreeMap<String, String>,
) -> TemplateContext {
    let now: NaiveDateTime = std::env::var("FT_TODAY")
        .ok()
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .map(|d| d.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap()))
        .unwrap_or_else(|| Local::now().naive_local());
    let mut ctx = TemplateContext::new(title, today, now);
    ctx.vars = vars;
    ctx
}

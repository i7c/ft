//! High-level task mutation primitives. Each entry point reads a file,
//! computes the new content, and writes atomically via `crate::fs::write_atomic`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use thiserror::Error;

use super::{
    format::{ParseContext, TaskFormat},
    recurrence::{self, RecurrenceError},
    Priority, Status, Task,
};
use crate::fs::write_atomic;

#[derive(Debug, Error)]
pub enum CreateError {
    #[error("could not read {}: {source}", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "duplicate task already exists at {}:{} (use --force to insert anyway)",
        .path.display(),
        .line
    )]
    Duplicate { path: PathBuf, line: usize },
    #[error("invalid --at-line {line}: file has only {file_lines} lines")]
    LineOutOfRange { line: usize, file_lines: usize },
    #[error("write failed: {source}")]
    Write {
        #[from]
        source: crate::error::Error,
    },
}

/// Where to insert a new task within the target file.
#[derive(Debug, Clone)]
pub enum Position {
    /// Append at the end of the file (creating it if missing).
    Append,
    /// Insert at the end of the section under the given heading. The heading
    /// match is exact on the heading text (without `#` markers). If the
    /// heading is missing, it is created at the end of the file.
    UnderHeading(String),
    /// Insert at this 1-indexed line, pushing existing content down.
    AtLine(usize),
    /// Insert as a subtask (indented child) of the task at this 1-indexed
    /// line. The new line lands at the end of the parent's existing indented
    /// block and is indented to match the parent's current children — or one
    /// step (two spaces) deeper if the parent has none yet.
    Subtask { parent_line: usize },
}

/// Pick the [`Position`] for a new task when the caller has no explicit
/// position to apply. Precedence: the target note's `ft.tasks.section`
/// frontmatter, then `config_default_section`, else [`Position::Append`].
///
/// `target_path` is read to inspect frontmatter; a read error (e.g. the
/// daily note doesn't exist yet) is treated as "no frontmatter" so the
/// config default still applies. A resolved section becomes
/// [`Position::UnderHeading`], which creates the heading at file end if the
/// note lacks it.
pub fn auto_position(target_path: &Path, config_default_section: Option<&str>) -> Position {
    let from_frontmatter = std::fs::read_to_string(target_path)
        .ok()
        .and_then(|content| crate::frontmatter::ft_tasks_section(&content));
    match from_frontmatter.or_else(|| config_default_section.map(str::to_string)) {
        Some(section) => Position::UnderHeading(section),
        None => Position::Append,
    }
}

/// User-provided fields for a new task. `description` should contain only
/// the user's free text — `tags` are appended automatically.
#[derive(Debug, Clone, Default)]
pub struct CreateInput {
    pub description: String,
    pub status: Status,
    pub priority: Option<Priority>,
    pub tags: Vec<String>,
    pub created: Option<NaiveDate>,
    pub start: Option<NaiveDate>,
    pub scheduled: Option<NaiveDate>,
    pub due: Option<NaiveDate>,
    pub recurrence: Option<String>,
    pub id: Option<String>,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CreateOptions {
    pub position: Position,
    /// If true, skip the duplicate check.
    pub force: bool,
}

#[derive(Debug)]
pub struct CreateOutcome {
    /// 1-indexed line number where the new task ended up.
    pub line: usize,
    /// The serialized task line (without trailing newline).
    pub serialized: String,
}

/// Build a `Task` value suitable for serialization from user input.
///
/// Public so callers can preview what `create_task` would write before
/// committing to disk (the TUI quickline uses this to render a live
/// emoji-format preview as the user types).
pub fn build_task(input: &CreateInput) -> Task {
    let mut description = input.description.trim_end().to_string();
    for tag in &input.tags {
        let bare = tag.trim_start_matches('#');
        let needle = format!("#{bare}");
        if !description.split_whitespace().any(|w| w == needle) {
            if !description.is_empty() {
                description.push(' ');
            }
            description.push_str(&needle);
        }
    }

    let tags = super::emoji::extract_tags(&description);

    Task {
        description,
        status: input.status,
        priority: input.priority,
        tags,
        created: input.created,
        start: input.start,
        scheduled: input.scheduled,
        due: input.due,
        recurrence: input.recurrence.clone(),
        id: input.id.clone(),
        depends_on: input.depends_on.clone(),
        ..Default::default()
    }
}

/// Create a new task in `target_path`. The path must be absolute (the binary
/// resolves it against the vault root before calling). `format` is the
/// vault's wire format — callers with a `Vault` at hand should pass
/// `vault.task_format()`.
pub fn create_task(
    target_path: &Path,
    format: &dyn TaskFormat,
    input: CreateInput,
    opts: CreateOptions,
) -> Result<CreateOutcome, CreateError> {
    let task = build_task(&input);
    let existing = read_or_empty(target_path)?;

    // Subtask placement reads the file to derive the child's indentation and
    // concrete insertion line, then proceeds as an ordinary `AtLine` splice.
    let (serialized, position) = match &opts.position {
        Position::Subtask { parent_line } => {
            let (indent, line) = subtask_placement(&existing, *parent_line)?;
            let serialized = format!("{indent}{}", format.serialize_line(&task));
            (serialized, Position::AtLine(line))
        }
        other => (format.serialize_line(&task), other.clone()),
    };

    if !opts.force {
        if let Some(line) = find_duplicate(&existing, &task, format) {
            return Err(CreateError::Duplicate {
                path: target_path.to_path_buf(),
                line,
            });
        }
    }

    let (new_content, line) = splice(&existing, &serialized, &position)?;

    write_atomic(target_path, &new_content)?;
    Ok(CreateOutcome { line, serialized })
}

fn read_or_empty(path: &Path) -> Result<String, CreateError> {
    crate::markdown::lines::read_or_empty(path).map_err(|source| CreateError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// Returns the 1-indexed line number of any existing task whose description,
/// due, scheduled, and start dates all match `task`. The status is ignored
/// (a done duplicate is still a duplicate).
fn find_duplicate(content: &str, task: &Task, format: &dyn TaskFormat) -> Option<usize> {
    for (idx, line) in content.lines().enumerate() {
        let ctx = ParseContext {
            source_file: PathBuf::new(),
            source_line: idx + 1,
        };
        if let Some(existing) = format.parse_line(line, ctx) {
            if existing.description == task.description
                && existing.due == task.due
                && existing.scheduled == task.scheduled
                && existing.start == task.start
            {
                return Some(idx + 1);
            }
        }
    }
    None
}

/// Insert `line` into `content` according to `pos`. Returns the new content
/// (always ending in `\n`) and the 1-indexed line number where `line` ended up.
fn splice(content: &str, line: &str, pos: &Position) -> Result<(String, usize), CreateError> {
    use crate::markdown::lines as md_lines;
    let mut lines = md_lines::split(content);

    let inserted_at_idx = match pos {
        Position::Append => {
            lines.push(line.to_string());
            lines.len() - 1
        }
        Position::AtLine(n) => {
            let n = *n;
            if n == 0 || n > lines.len() + 1 {
                return Err(CreateError::LineOutOfRange {
                    line: n,
                    file_lines: lines.len(),
                });
            }
            lines.insert(n - 1, line.to_string());
            n - 1
        }
        Position::UnderHeading(heading) => match md_lines::find_heading(&lines, heading) {
            Some((heading_idx, level)) => {
                let insert_at = md_lines::section_end(&lines, heading_idx, level);
                lines.insert(insert_at, line.to_string());
                insert_at
            }
            None => {
                if !lines.is_empty() && !lines.last().unwrap().is_empty() {
                    lines.push(String::new());
                }
                lines.push(format!("## {heading}"));
                lines.push(line.to_string());
                lines.len() - 1
            }
        },
        // `create_task` resolves `Subtask` to an `AtLine` before splicing.
        Position::Subtask { .. } => unreachable!("subtask placement resolved before splice"),
    };

    Ok((md_lines::join_with_newline(&lines), inserted_at_idx + 1))
}

// ── line guard ───────────────────────────────────────────────────────────────

/// Compare the task the caller expects at a line against the task actually
/// parsed there. Both sides are canonicalized through `format.serialize_line`
/// so context fields (source path/line, indent, parent) don't participate —
/// only the wire content does. Returns `(expected, found)` serialized forms
/// on mismatch so error variants can show the user both.
///
/// This is the defense against stale line numbers: callers hold `(path,
/// line)` from a scan, and the file may have changed since (another `ft`
/// mutation, Obsidian, a git pull). Without the guard, a shifted file makes
/// line-addressed mutations silently hit whatever task now occupies the line.
pub(crate) fn check_expected(
    format: &dyn TaskFormat,
    found: &Task,
    expected: Option<&Task>,
) -> Result<(), (String, String)> {
    if let Some(exp) = expected {
        let expected_ser = format.serialize_line(exp);
        let found_ser = format.serialize_line(found);
        if expected_ser != found_ser {
            return Err((expected_ser, found_ser));
        }
    }
    Ok(())
}

// ── complete_task ────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum CompleteError {
    #[error("could not read {}: {source}", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("line {line} not found in {} ({file_lines} lines)", .path.display())]
    LineMissing {
        path: PathBuf,
        line: usize,
        file_lines: usize,
    },
    #[error("line {line} in {} is not a task", .path.display())]
    NotATask { path: PathBuf, line: usize },
    #[error(
        "task at {}:{} changed on disk — expected `{expected}`, found `{found}` (rescan and retry)",
        .path.display(),
        .line
    )]
    LineChanged {
        path: PathBuf,
        line: usize,
        expected: String,
        found: String,
    },
    #[error("task at {}:{} is already done (on {})", .path.display(), .line, .done)]
    AlreadyDone {
        path: PathBuf,
        line: usize,
        done: NaiveDate,
    },
    #[error(transparent)]
    Recurrence(#[from] RecurrenceError),
    #[error("write failed: {source}")]
    Write {
        #[from]
        source: crate::error::Error,
    },
}

#[derive(Debug, Clone)]
pub struct CompleteOptions {
    /// Date to record as the done date.
    pub on: NaiveDate,
}

#[derive(Debug)]
pub struct CompleteOutcome {
    /// 1-indexed line of the now-done task in the rewritten file.
    pub completed_line: usize,
    /// Serialized form of the completed task line.
    pub completed_serialized: String,
    /// If the task was recurring, the new instance's 1-indexed line and
    /// serialized form.
    pub next_instance: Option<NextInstance>,
}

#[derive(Debug)]
pub struct NextInstance {
    pub line: usize,
    pub serialized: String,
}

/// Mark the task at `target_path:line` complete. If the task is recurring,
/// the next instance is inserted *above* the now-completed line (matching
/// plugin behavior).
///
/// `expected` is the task the caller believes lives at `line` (from a scan).
/// When provided and the line's current content doesn't match it, the call
/// fails with [`CompleteError::LineChanged`] instead of completing whatever
/// shifted into that slot. Pass `None` only when no scanned task is at hand.
pub fn complete_task(
    target_path: &Path,
    line: usize,
    format: &dyn TaskFormat,
    expected: Option<&Task>,
    opts: CompleteOptions,
) -> Result<CompleteOutcome, CompleteError> {
    let content = std::fs::read_to_string(target_path).map_err(|e| CompleteError::Read {
        path: target_path.to_path_buf(),
        source: e,
    })?;

    let mut lines = crate::markdown::lines::split(&content);

    if line == 0 || line > lines.len() {
        return Err(CompleteError::LineMissing {
            path: target_path.to_path_buf(),
            line,
            file_lines: lines.len(),
        });
    }

    let idx = line - 1;
    let original = &lines[idx];
    let ctx = ParseContext {
        source_file: PathBuf::new(),
        source_line: line,
    };
    let task = format
        .parse_line(original, ctx)
        .ok_or_else(|| CompleteError::NotATask {
            path: target_path.to_path_buf(),
            line,
        })?;

    check_expected(format, &task, expected).map_err(|(expected, found)| {
        CompleteError::LineChanged {
            path: target_path.to_path_buf(),
            line,
            expected,
            found,
        }
    })?;

    if task.status == Status::Done {
        if let Some(done) = task.done {
            return Err(CompleteError::AlreadyDone {
                path: target_path.to_path_buf(),
                line,
                done,
            });
        }
    }

    let next_task = if let Some(rule_str) = task.recurrence.as_deref() {
        let rule = recurrence::parse_rule(rule_str)?;
        let next = recurrence::next_dates(&rule, &task)?;
        let mut t = task.clone();
        t.status = Status::Open;
        t.start = next.start;
        t.scheduled = next.scheduled;
        t.due = next.due;
        t.done = None;
        t.cancelled = None;
        Some(t)
    } else {
        None
    };

    let mut completed = task;
    completed.status = Status::Done;
    completed.done = Some(opts.on);
    let completed_line = format.serialize_line(&completed);

    let mut next_instance: Option<NextInstance> = None;
    if let Some(t) = next_task {
        let serialized = format.serialize_line(&t);
        // Insert the new instance above the completed line.
        lines.insert(idx, serialized.clone());
        next_instance = Some(NextInstance {
            // The just-inserted line takes over the original `line` slot.
            line,
            serialized,
        });
        // The completed task is now at idx+1 (1-indexed: line+1).
        lines[idx + 1] = completed_line.clone();
    } else {
        lines[idx] = completed_line.clone();
    }

    let joined = crate::markdown::lines::join_with_newline(&lines);
    write_atomic(target_path, &joined)?;

    let completed_line_no = if next_instance.is_some() {
        line + 1
    } else {
        line
    };

    Ok(CompleteOutcome {
        completed_line: completed_line_no,
        completed_serialized: completed_line,
        next_instance,
    })
}

// ── update_task_line / cancel_task ───────────────────────────────────────────

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("could not read {}: {source}", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("line {line} not found in {} ({file_lines} lines)", .path.display())]
    LineMissing {
        path: PathBuf,
        line: usize,
        file_lines: usize,
    },
    #[error("line {line} in {} is not a task", .path.display())]
    NotATask { path: PathBuf, line: usize },
    #[error(
        "task at {}:{} changed on disk — expected `{expected}`, found `{found}` (rescan and retry)",
        .path.display(),
        .line
    )]
    LineChanged {
        path: PathBuf,
        line: usize,
        expected: String,
        found: String,
    },
    #[error("write failed: {source}")]
    Write {
        #[from]
        source: crate::error::Error,
    },
}

/// Apply `mutate` to the task at `target_path:line`, then serialize the
/// result back into the file via `write_atomic`. Returns the post-mutation
/// task with `source_file` / `source_line` filled in. Used by both the CLI
/// and the TUI for quick-key edits (date nudges, priority cycle, cancel).
///
/// `expected` guards against stale line numbers — see [`complete_task`].
///
/// Does not handle recurrence — for completing recurring tasks, use
/// [`complete_task`] which inserts the next instance.
pub fn update_task_line<F>(
    target_path: &Path,
    line: usize,
    format: &dyn TaskFormat,
    expected: Option<&Task>,
    mutate: F,
) -> Result<Task, UpdateError>
where
    F: FnOnce(&mut Task),
{
    let content = std::fs::read_to_string(target_path).map_err(|e| UpdateError::Read {
        path: target_path.to_path_buf(),
        source: e,
    })?;

    let mut lines = crate::markdown::lines::split(&content);

    if line == 0 || line > lines.len() {
        return Err(UpdateError::LineMissing {
            path: target_path.to_path_buf(),
            line,
            file_lines: lines.len(),
        });
    }

    let idx = line - 1;
    let ctx = ParseContext {
        source_file: target_path.to_path_buf(),
        source_line: line,
    };
    let mut task = format
        .parse_line(&lines[idx], ctx)
        .ok_or_else(|| UpdateError::NotATask {
            path: target_path.to_path_buf(),
            line,
        })?;

    check_expected(format, &task, expected).map_err(|(expected, found)| {
        UpdateError::LineChanged {
            path: target_path.to_path_buf(),
            line,
            expected,
            found,
        }
    })?;

    mutate(&mut task);

    let serialized = format.serialize_line(&task);
    lines[idx] = serialized;

    let joined = crate::markdown::lines::join_with_newline(&lines);
    write_atomic(target_path, &joined)?;

    Ok(task)
}

/// A standalone `#tag` word: starts with `#` and the remainder is non-empty
/// and entirely `alphanumeric | _ | - | /`. Matches the char set accepted by
/// [`super::emoji::extract_tags`] so what retag sees as "a tag" is exactly
/// what the system parsed into `Task.tags`.
///
/// Public so the TUI retag picker can reuse the same definition.
pub fn is_tag_word(w: &str) -> bool {
    let Some(rest) = w.strip_prefix('#') else {
        return false;
    };
    !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'))
}

/// Rewrite the task's description so every inline `#tag` word whose bare
/// form is in `list` is removed, then `#<tag>` is appended (when `tag` is
/// non-empty). Tags not in `list` and all non-tag description text are
/// preserved verbatim. `tag == ""` clears every list-member tag without
/// appending a replacement.
///
/// `expected` guards against stale line numbers — see [`complete_task`].
///
/// Tags are a derived index rebuilt from the description on parse; the
/// serializer emits `description` verbatim, so a tag edit MUST rewrite the
/// description rather than `Task.tags` (the latter is a silent no-op on
/// disk — see `emoji::serialize_line`).
pub fn retag_task(
    target_path: &Path,
    line: usize,
    format: &dyn TaskFormat,
    expected: Option<&Task>,
    list: &[String],
    tag: &str,
) -> Result<Task, UpdateError> {
    let list_set: std::collections::HashSet<&str> = list.iter().map(|s| s.as_str()).collect();
    update_task_line(target_path, line, format, expected, |t| {
        let mut kept: Vec<&str> = Vec::new();
        for word in t.description.split_whitespace() {
            // Drop list-member tags; keep everything else (non-tags and
            // tags outside the configured switchable set).
            let is_list_tag = is_tag_word(word)
                && word
                    .strip_prefix('#')
                    .is_some_and(|bare| list_set.contains(bare));
            if !is_list_tag {
                kept.push(word);
            }
        }
        let mut out = kept.join(" ");
        let bare = tag.trim_start_matches('#');
        if !bare.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push('#');
            out.push_str(bare);
        }
        t.description = out;
    })
}

#[derive(Debug, Error)]
pub enum CancelError {
    #[error(transparent)]
    Update(#[from] UpdateError),
    #[error("task at {}:{} is already cancelled (on {})", .path.display(), .line, .cancelled)]
    AlreadyCancelled {
        path: PathBuf,
        line: usize,
        cancelled: NaiveDate,
    },
}

/// Mark the task at `target_path:line` cancelled, recording `on` as the
/// cancellation date. Cancelled tasks do not recur (so unlike
/// [`complete_task`] no next-instance is generated).
///
/// `expected` guards against stale line numbers — see [`complete_task`].
pub fn cancel_task(
    target_path: &Path,
    line: usize,
    format: &dyn TaskFormat,
    expected: Option<&Task>,
    on: NaiveDate,
) -> Result<Task, CancelError> {
    // Snapshot the pre-state so we can detect "already cancelled" without
    // racing the write inside the mutate closure.
    let pre = update_task_line(target_path, line, format, expected, |_| {})?;
    if pre.status == Status::Cancelled {
        if let Some(c) = pre.cancelled {
            return Err(CancelError::AlreadyCancelled {
                path: target_path.to_path_buf(),
                line,
                cancelled: c,
            });
        }
    }

    // Guard the second write against `pre` (not the caller's `expected`):
    // the no-op pass above rewrote the line in canonical form, and `pre`
    // is exactly what it left there.
    let task = update_task_line(target_path, line, format, Some(&pre), |t| {
        t.status = Status::Cancelled;
        t.cancelled = Some(on);
    })?;
    Ok(task)
}

// ── move_tasks ───────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum MoveError {
    #[error("could not read {}: {source}", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("line {line} not found in {} ({file_lines} lines)", .path.display())]
    LineMissing {
        path: PathBuf,
        line: usize,
        file_lines: usize,
    },
    #[error("line {line} in {} is not a task", .path.display())]
    NotATask { path: PathBuf, line: usize },
    #[error(
        "task at {}:{} changed on disk — expected `{expected}`, found `{found}` (rescan and retry)",
        .path.display(),
        .line
    )]
    LineChanged {
        path: PathBuf,
        line: usize,
        expected: String,
        found: String,
    },
    #[error(
        "target task at {}:{} changed on disk — expected `{expected}`, found `{found}` (rescan and retry)",
        .path.display(),
        .line
    )]
    TargetChanged {
        path: PathBuf,
        line: usize,
        expected: String,
        found: String,
    },
    #[error("can't move a task under itself or one of its subtasks ({}:{})", .path.display(), .line)]
    IntoOwnSubtree { path: PathBuf, line: usize },
    #[error("task at {}:{} has no previous sibling to indent under", .path.display(), .line)]
    NoPreviousSibling { path: PathBuf, line: usize },
    #[error("task at {}:{} is already top-level", .path.display(), .line)]
    AlreadyTopLevel { path: PathBuf, line: usize },
    #[error("write failed: {source}")]
    Write {
        #[from]
        source: crate::error::Error,
    },
}

/// How moved blocks land relative to an [`MoveTarget::UnderTask`] target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Become the target's last children.
    Child,
    /// Become the target's next siblings, after its whole subtree.
    SiblingAfter,
}

/// Where to insert moved tasks in the target file.
#[derive(Debug, Clone)]
pub enum MoveTarget {
    /// Append to the file (creating it if missing).
    Append(PathBuf),
    /// Append to the section under the given heading; create the heading at
    /// file end if missing.
    UnderHeading(PathBuf, String),
    /// Nest under (or right after) the task at `line` of `path`. Moved
    /// blocks are re-indented to the new depth. `expected` guards the
    /// target line like [`MoveSource::expected`] guards sources.
    UnderTask {
        path: PathBuf,
        line: usize,
        expected: Option<Box<Task>>,
        placement: Placement,
    },
}

impl MoveTarget {
    pub fn path(&self) -> &Path {
        match self {
            MoveTarget::Append(p)
            | MoveTarget::UnderHeading(p, _)
            | MoveTarget::UnderTask { path: p, .. } => p,
        }
    }
}

/// A single source identifier: absolute path + 1-indexed source line of the
/// task to move. Children of the task come along automatically.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MoveSource {
    pub path: PathBuf,
    pub line: usize,
    /// The task the caller believes lives at `line` (from a scan). When set,
    /// `plan_move` fails with [`MoveError::LineChanged`] if the line's
    /// current content doesn't match — see [`complete_task`].
    pub expected: Option<Task>,
}

/// Per-file before/after content. The CLI uses these to render diffs and to
/// apply atomic writes.
#[derive(Debug, Clone)]
pub struct FileEdit {
    pub path: PathBuf,
    pub original: String,
    pub new: String,
}

#[derive(Debug, Clone, Default)]
pub struct MovePlan {
    /// File edits keyed by absolute path. Includes both source files (lines
    /// removed) and the target file (lines inserted). A no-op produces an
    /// edit whose `original == new`.
    pub edits: Vec<FileEdit>,
    /// Per source, the start/end (1-indexed inclusive) of the moved block.
    pub blocks: Vec<MovedBlock>,
}

#[derive(Debug, Clone)]
pub struct MovedBlock {
    pub source: MoveSource,
    pub end_line: usize,
    pub task_description: String,
    /// Target path + 1-indexed line of the block's head after the move.
    pub dest: Option<(PathBuf, usize)>,
}

/// Build a [`MovePlan`] for moving `sources` to `target`. Only reads files;
/// never writes. Wikilink rewriting on cross-folder moves is deferred to
/// plan 003 — see TODO in the body.
pub fn plan_move(
    sources: &[MoveSource],
    target: &MoveTarget,
    format: &dyn TaskFormat,
) -> Result<MovePlan, MoveError> {
    // TODO(plan-003): rewrite [[wikilinks]] when the target file is in a
    // different folder than the source. Today we move the bytes verbatim and
    // rely on Obsidian's own rename-aware index for cross-folder cases.

    // Group sources by file so we read each only once.
    let mut by_file: BTreeMap<PathBuf, Vec<(usize, Option<&Task>)>> = BTreeMap::new();
    for s in sources {
        by_file
            .entry(s.path.clone())
            .or_default()
            .push((s.line, s.expected.as_ref()));
    }

    let target_path = target.path().to_path_buf();
    let target_original = read_or_empty_move(&target_path)?;
    let mut target_lines: Vec<String> = crate::markdown::lines::split(&target_original);
    let anchor = match target {
        MoveTarget::UnderTask {
            line,
            expected,
            placement,
            ..
        } => Some(resolve_anchor(
            &target_path,
            &target_lines,
            *line,
            expected.as_deref(),
            *placement,
            format,
        )?),
        _ => None,
    };
    // Target-file lines removed at or before the anchor's block end; shifts
    // the insertion point when the target file is also a source.
    let mut removed_before_anchor_end = 0usize;

    // Ranges to remove from each source file plus the lines that are being
    // hoisted out (so we can splice them at the target). For source==target,
    // we collect removals and the insertion happens after removals are
    // applied; insertion lines reference the *original* indices via the
    // collected `block_lines`.
    let mut source_edits: BTreeMap<PathBuf, FileEditWork> = BTreeMap::new();
    let mut blocks: Vec<MovedBlock> = Vec::new();
    let mut moved_lines: Vec<String> = Vec::new();
    let mut block_offsets: Vec<usize> = Vec::new();

    for (path, mut lines_to_move) in by_file {
        let original = read_or_empty_move(&path)?;
        let raw_lines: Vec<String> = crate::markdown::lines::split(&original);

        // Sort + dedupe + drop descendants of any other in-list line.
        lines_to_move.sort_unstable_by_key(|(line, _)| *line);
        lines_to_move.dedup_by_key(|(line, _)| *line);

        let mut ranges: Vec<(usize, usize, String, usize)> = Vec::new(); // (start, end, description, head_indent)
        for (line, expected) in &lines_to_move {
            let idx = line.checked_sub(1).ok_or_else(|| MoveError::LineMissing {
                path: path.clone(),
                line: *line,
                file_lines: raw_lines.len(),
            })?;
            if idx >= raw_lines.len() {
                return Err(MoveError::LineMissing {
                    path: path.clone(),
                    line: *line,
                    file_lines: raw_lines.len(),
                });
            }
            let raw = &raw_lines[idx];
            let ctx = ParseContext {
                source_file: PathBuf::new(),
                source_line: *line,
            };
            let task = format
                .parse_line(raw, ctx)
                .ok_or_else(|| MoveError::NotATask {
                    path: path.clone(),
                    line: *line,
                })?;
            check_expected(format, &task, *expected).map_err(|(expected, found)| {
                MoveError::LineChanged {
                    path: path.clone(),
                    line: *line,
                    expected,
                    found,
                }
            })?;
            let end = block_end(&raw_lines, idx, task.indent_level);
            ranges.push((idx, end, task.description.clone(), task.indent_level));
        }

        // If a range fully contains another, drop the contained one — its
        // content is already part of the parent block. Process ranges in
        // (start asc, len desc) order and drop later ranges whose start is
        // covered by a kept range.
        ranges.sort_by(|a, b| a.0.cmp(&b.0).then((b.1 - b.0).cmp(&(a.1 - a.0))));
        let mut kept: Vec<(usize, usize, String, usize)> = Vec::new();
        for r in ranges {
            if kept.iter().any(|k| r.0 >= k.0 && r.1 <= k.1) {
                continue;
            }
            kept.push(r);
        }

        if let (Some(a), true) = (&anchor, path == target_path) {
            if kept.iter().any(|(s, e, _, _)| (*s..=*e).contains(&a.idx)) {
                return Err(MoveError::IntoOwnSubtree {
                    path: target_path.clone(),
                    line: a.idx + 1,
                });
            }
            removed_before_anchor_end = kept
                .iter()
                .filter(|(_, e, _, _)| *e <= a.end)
                .map(|(s, e, _, _)| e - s + 1)
                .sum();
        }

        let src_unit = anchor.as_ref().map(|_| detect_indent_unit(&raw_lines));

        // Append moved lines (re-indented under a task anchor, otherwise
        // normalized to column 0) into `moved_lines`.
        for (start, end, description, head_indent) in &kept {
            blocks.push(MovedBlock {
                source: MoveSource {
                    path: path.clone(),
                    line: start + 1,
                    expected: None,
                },
                end_line: end + 1,
                task_description: description.clone(),
                dest: None,
            });
            block_offsets.push(moved_lines.len());
            let block = &raw_lines[*start..=*end];
            match (&anchor, src_unit) {
                (Some(a), Some(src)) => {
                    moved_lines.extend(reindent_block(block, *head_indent, src, a.unit, &a.head_ws))
                }
                _ => moved_lines.extend(block.iter().map(|raw| strip_indent(raw, *head_indent))),
            }
        }

        // Build the post-removal content for this file. Walk lines, skipping
        // any index that falls within a kept range.
        let mut new_lines: Vec<String> = Vec::with_capacity(raw_lines.len());
        let mut i = 0;
        while i < raw_lines.len() {
            if let Some((_, end, _, _)) = kept.iter().find(|(s, _, _, _)| *s == i) {
                i = *end + 1;
                continue;
            }
            new_lines.push(raw_lines[i].clone());
            i += 1;
        }

        source_edits.insert(
            path.clone(),
            FileEditWork {
                original,
                new_lines,
            },
        );
    }

    // Apply the insertion to the target file. If the target file is also in
    // `source_edits`, merge by working off the post-removal lines.
    let target_work = source_edits.remove(&target_path);
    if let Some(w) = target_work {
        target_lines = w.new_lines;
    }

    let anchor_insert = anchor
        .as_ref()
        .map(|a| a.end + 1 - removed_before_anchor_end);
    let (final_target, insert_idx) =
        splice_into_target(target_lines, &moved_lines, target, anchor_insert);
    for (block, offset) in blocks.iter_mut().zip(&block_offsets) {
        block.dest = Some((target_path.clone(), insert_idx + offset + 1));
    }

    // Build edits.
    let mut edits: Vec<FileEdit> = Vec::new();
    for (path, w) in source_edits {
        let new_content = crate::markdown::lines::join_with_newline(&w.new_lines);
        edits.push(FileEdit {
            path,
            original: w.original,
            new: new_content,
        });
    }
    edits.push(FileEdit {
        path: target_path,
        original: target_original,
        new: final_target,
    });

    Ok(MovePlan { edits, blocks })
}

/// A validated [`MoveTarget::UnderTask`] target, resolved against the
/// target file's original lines.
struct TaskAnchor {
    /// 0-indexed line of the target task.
    idx: usize,
    /// 0-indexed last line of the target task's block.
    end: usize,
    /// Leading whitespace the moved block heads take.
    head_ws: String,
    unit: IndentUnit,
}

fn resolve_anchor(
    path: &Path,
    lines: &[String],
    line: usize,
    expected: Option<&Task>,
    placement: Placement,
    format: &dyn TaskFormat,
) -> Result<TaskAnchor, MoveError> {
    let idx = line
        .checked_sub(1)
        .filter(|&i| i < lines.len())
        .ok_or_else(|| MoveError::LineMissing {
            path: path.to_path_buf(),
            line,
            file_lines: lines.len(),
        })?;
    let ctx = ParseContext {
        source_file: PathBuf::new(),
        source_line: line,
    };
    let task = format
        .parse_line(&lines[idx], ctx)
        .ok_or_else(|| MoveError::NotATask {
            path: path.to_path_buf(),
            line,
        })?;
    check_expected(format, &task, expected).map_err(|(expected, found)| {
        MoveError::TargetChanged {
            path: path.to_path_buf(),
            line,
            expected,
            found,
        }
    })?;
    let end = block_end(lines, idx, task.indent_level);
    let unit = detect_indent_unit(lines);
    let own_ws = leading_ws(&lines[idx]);
    let head_ws = match placement {
        Placement::SiblingAfter => own_ws.to_string(),
        Placement::Child if end > idx => leading_ws(&lines[idx + 1]).to_string(),
        Placement::Child => format!("{own_ws}{}", unit.as_str()),
    };
    Ok(TaskAnchor {
        idx,
        end,
        head_ws,
        unit,
    })
}

/// Re-indent a moved block so its head sits at `new_head_ws`. Nested lines
/// keep their depth relative to the head: verbatim when both files share an
/// indent unit, otherwise re-expressed in `dst` units.
fn reindent_block(
    block: &[String],
    head_indent: usize,
    src: IndentUnit,
    dst: IndentUnit,
    new_head_ws: &str,
) -> Vec<String> {
    block
        .iter()
        .map(|line| {
            let ws = leading_ws(line);
            let content = &line[ws.len()..];
            let rel = ws.get(head_indent.min(ws.len())..).unwrap_or("");
            let rel = if src == dst {
                rel.to_string()
            } else {
                let (depth, rest) = src.split_depth(rel);
                format!("{}{rest}", dst.as_str().repeat(depth))
            };
            format!("{new_head_ws}{rel}{content}")
        })
        .collect()
}

/// One nesting level of a markdown list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentUnit {
    Tab,
    Spaces(usize),
}

impl IndentUnit {
    pub fn as_str(self) -> String {
        match self {
            IndentUnit::Tab => "\t".to_string(),
            IndentUnit::Spaces(n) => " ".repeat(n),
        }
    }

    /// Split leading whitespace into whole units of `self` plus the rest.
    fn split_depth(self, ws: &str) -> (usize, &str) {
        match self {
            IndentUnit::Tab => {
                let n = ws.bytes().take_while(|b| *b == b'\t').count();
                (n, &ws[n..])
            }
            IndentUnit::Spaces(width) => {
                let n = ws.bytes().take_while(|b| *b == b' ').count();
                let depth = n / width.max(1);
                (depth, &ws[depth * width.max(1)..])
            }
        }
    }
}

/// Infer a file's list indent unit: a tab if the first nested list item is
/// tab-indented, otherwise the smallest positive space step between a list
/// item and the item it nests under. Two spaces when nothing is nested.
pub fn detect_indent_unit(lines: &[String]) -> IndentUnit {
    let mut stack: Vec<&str> = Vec::new();
    let mut min_step: Option<usize> = None;
    for line in lines {
        let ws = leading_ws(line);
        let rest = &line[ws.len()..];
        if rest.is_empty() {
            continue;
        }
        if !is_list_item(rest) {
            if ws.is_empty() {
                stack.clear();
            }
            continue;
        }
        while stack.last().is_some_and(|top| top.len() >= ws.len()) {
            stack.pop();
        }
        if let Some(parent) = stack.last() {
            let step = &ws[parent.len().min(ws.len())..];
            if step.contains('\t') {
                if min_step.is_none() {
                    return IndentUnit::Tab;
                }
            } else {
                let n = ws.len() - parent.len();
                min_step = Some(min_step.map_or(n, |m| m.min(n)));
            }
        }
        stack.push(ws);
    }
    IndentUnit::Spaces(min_step.unwrap_or(2))
}

fn is_list_item(rest: &str) -> bool {
    if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        return true;
    }
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && (rest[digits..].starts_with(". ") || rest[digits..].starts_with(") "))
}

/// Plan an indent: the task becomes the last child of its previous sibling
/// (the task whose block ends right above it, at the same depth).
pub fn plan_indent(source: &MoveSource, format: &dyn TaskFormat) -> Result<MovePlan, MoveError> {
    let (tasks, lines, i) = locate_in_file(source, format)?;
    let me = &tasks[i];
    let idx = me.source_line - 1;
    let prev = tasks[..i].iter().rev().find(|t| {
        t.parent == me.parent
            && t.indent_level == me.indent_level
            && block_end(&lines, t.source_line - 1, t.indent_level) + 1 == idx
    });
    let Some(prev) = prev else {
        return Err(MoveError::NoPreviousSibling {
            path: source.path.clone(),
            line: source.line,
        });
    };
    let target = MoveTarget::UnderTask {
        path: source.path.clone(),
        line: prev.source_line,
        expected: Some(Box::new(prev.clone())),
        placement: Placement::Child,
    };
    plan_move(std::slice::from_ref(source), &target, format)
}

/// Plan an outdent: the task becomes its parent's next sibling, placed after
/// the parent's whole block. Later siblings stay with the old parent.
pub fn plan_outdent(source: &MoveSource, format: &dyn TaskFormat) -> Result<MovePlan, MoveError> {
    let (tasks, _, i) = locate_in_file(source, format)?;
    let Some(parent_line) = tasks[i].parent else {
        return Err(MoveError::AlreadyTopLevel {
            path: source.path.clone(),
            line: source.line,
        });
    };
    let parent = tasks
        .iter()
        .find(|t| t.source_line == parent_line)
        .expect("resolve_hierarchy parents point at tasks in the same file");
    let target = MoveTarget::UnderTask {
        path: source.path.clone(),
        line: parent_line,
        expected: Some(Box::new(parent.clone())),
        placement: Placement::SiblingAfter,
    };
    plan_move(std::slice::from_ref(source), &target, format)
}

/// Parse every task in `source.path` with hierarchy resolved, and return the
/// index of the (guard-checked) source task.
fn locate_in_file(
    source: &MoveSource,
    format: &dyn TaskFormat,
) -> Result<(Vec<Task>, Vec<String>, usize), MoveError> {
    let content = read_or_empty_move(&source.path)?;
    let lines = crate::markdown::lines::split(&content);
    let mut tasks: Vec<Task> = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, l)| {
            format.parse_line(
                l,
                ParseContext {
                    source_file: source.path.clone(),
                    source_line: idx + 1,
                },
            )
        })
        .collect();
    super::hierarchy::resolve_hierarchy(&mut tasks);
    if source.line == 0 || source.line > lines.len() {
        return Err(MoveError::LineMissing {
            path: source.path.clone(),
            line: source.line,
            file_lines: lines.len(),
        });
    }
    let i = tasks
        .iter()
        .position(|t| t.source_line == source.line)
        .ok_or_else(|| MoveError::NotATask {
            path: source.path.clone(),
            line: source.line,
        })?;
    check_expected(format, &tasks[i], source.expected.as_ref()).map_err(|(expected, found)| {
        MoveError::LineChanged {
            path: source.path.clone(),
            line: source.line,
            expected,
            found,
        }
    })?;
    Ok((tasks, lines, i))
}

/// Apply a [`MovePlan`] by writing each affected file atomically.
pub fn apply_move_plan(plan: &MovePlan) -> Result<(), MoveError> {
    for edit in &plan.edits {
        if edit.original == edit.new {
            continue;
        }
        write_atomic(&edit.path, &edit.new)?;
    }
    Ok(())
}

struct FileEditWork {
    original: String,
    new_lines: Vec<String>,
}

fn read_or_empty_move(path: &Path) -> Result<String, MoveError> {
    crate::markdown::lines::read_or_empty(path).map_err(|source| MoveError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// Resolve indentation and insertion line for a new subtask of the task at
/// `parent_line` (1-indexed). Returns the leading-whitespace string the child
/// line should carry and the 1-indexed line to splice it at (the end of the
/// parent's existing indented block). The child matches its siblings' indent
/// verbatim — tabs included — or sits one detected indent unit past the
/// parent if it has no children yet.
fn subtask_placement(content: &str, parent_line: usize) -> Result<(String, usize), CreateError> {
    use crate::markdown::lines as md_lines;
    let lines = md_lines::split(content);
    let idx = parent_line
        .checked_sub(1)
        .filter(|&i| i < lines.len())
        .ok_or(CreateError::LineOutOfRange {
            line: parent_line,
            file_lines: lines.len(),
        })?;

    let parent_indent = leading_ws(&lines[idx]).len();
    let end = block_end(&lines, idx, parent_indent);

    let indent = if end > idx {
        // Match the first child's leading whitespace exactly.
        leading_ws(&lines[idx + 1]).to_string()
    } else {
        format!(
            "{}{}",
            leading_ws(&lines[idx]),
            detect_indent_unit(&lines).as_str()
        )
    };

    // Splice after the block's last line: 0-indexed `end + 1` ⇒ `AtLine(end + 2)`.
    Ok((indent, end + 2))
}

/// Leading whitespace (spaces/tabs) prefix of `line`.
pub(crate) fn leading_ws(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// Find the last line of the block whose head is `start_idx` with indent
/// `head_indent`. The block extends through every following line whose first
/// non-whitespace column is greater than `head_indent`. Blank lines never
/// belong to the block — they belong to the surrounding context.
fn block_end(lines: &[String], start_idx: usize, head_indent: usize) -> usize {
    let mut end = start_idx;
    for (i, l) in lines.iter().enumerate().skip(start_idx + 1) {
        let trimmed = l.trim_start();
        if trimmed.is_empty() {
            break;
        }
        let indent = l.len() - trimmed.len();
        if indent <= head_indent {
            break;
        }
        end = i;
    }
    end
}

/// Strip up to `head_indent` leading whitespace bytes from `line`. We use byte
/// slicing because indents are spaces / tabs (single-byte in ASCII).
fn strip_indent(line: &str, head_indent: usize) -> String {
    let trimmed_prefix_len = line
        .as_bytes()
        .iter()
        .take(head_indent)
        .take_while(|b| **b == b' ' || **b == b'\t')
        .count();
    line[trimmed_prefix_len..].to_string()
}

/// Splice `moved_lines` into the target's lines. Returns the new content and
/// the 0-indexed line where the first moved line landed. `anchor_insert` is
/// the pre-resolved insertion index for [`MoveTarget::UnderTask`].
fn splice_into_target(
    mut lines: Vec<String>,
    moved_lines: &[String],
    target: &MoveTarget,
    anchor_insert: Option<usize>,
) -> (String, usize) {
    let insert_at = match target {
        MoveTarget::Append(_) => lines.len(),
        MoveTarget::UnderTask { .. } => anchor_insert.expect("UnderTask anchor resolved"),
        MoveTarget::UnderHeading(_, heading) => {
            match crate::markdown::lines::find_heading(&lines, heading) {
                Some((heading_idx, level)) => {
                    crate::markdown::lines::section_end(&lines, heading_idx, level)
                }
                None if moved_lines.is_empty() => lines.len(),
                None => {
                    if !lines.is_empty() && !lines.last().unwrap().is_empty() {
                        lines.push(String::new());
                    }
                    lines.push(format!("## {heading}"));
                    lines.len()
                }
            }
        }
    };
    lines.splice(insert_at..insert_at, moved_lines.iter().cloned());
    (crate::markdown::lines::join_with_newline(&lines), insert_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::emoji::EmojiFormat;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn input(desc: &str) -> CreateInput {
        CreateInput {
            description: desc.into(),
            ..Default::default()
        }
    }

    #[test]
    fn build_task_appends_tags() {
        let mut i = input("Buy milk");
        i.tags = vec!["work".into(), "#urgent".into()];
        let t = build_task(&i);
        assert_eq!(t.description, "Buy milk #work #urgent");
        assert_eq!(t.tags, vec!["work", "urgent"]);
    }

    #[test]
    fn build_task_does_not_double_existing_tag() {
        let mut i = input("Buy milk #work");
        i.tags = vec!["work".into()];
        let t = build_task(&i);
        assert_eq!(t.description, "Buy milk #work");
        assert_eq!(t.tags, vec!["work"]);
    }

    #[test]
    fn append_to_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("daily.md");
        let mut i = input("Buy milk");
        i.due = Some(d(2026, 5, 10));
        let outcome = create_task(
            &p,
            &EmojiFormat,
            i,
            CreateOptions {
                position: Position::Append,
                force: false,
            },
        )
        .unwrap();
        assert_eq!(outcome.line, 1);
        let content = std::fs::read_to_string(&p).unwrap();
        assert_eq!(content, "- [ ] Buy milk 📅 2026-05-10\n");
    }

    #[test]
    fn auto_position_frontmatter_wins_over_config() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("note.md");
        std::fs::write(&p, "---\nft:\n  tasks:\n    section: Inbox\n---\n# Note\n").unwrap();
        match auto_position(&p, Some("Tasks")) {
            Position::UnderHeading(h) => assert_eq!(h, "Inbox"),
            other => panic!("expected UnderHeading(Inbox), got {other:?}"),
        }
    }

    #[test]
    fn auto_position_falls_back_to_config() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("note.md");
        std::fs::write(&p, "# Note\n").unwrap();
        match auto_position(&p, Some("Tasks")) {
            Position::UnderHeading(h) => assert_eq!(h, "Tasks"),
            other => panic!("expected UnderHeading(Tasks), got {other:?}"),
        }
    }

    #[test]
    fn auto_position_missing_file_uses_config() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("does-not-exist.md");
        match auto_position(&p, Some("Tasks")) {
            Position::UnderHeading(h) => assert_eq!(h, "Tasks"),
            other => panic!("expected UnderHeading(Tasks), got {other:?}"),
        }
    }

    #[test]
    fn auto_position_no_section_appends() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("note.md");
        std::fs::write(&p, "# Note\n").unwrap();
        assert!(matches!(auto_position(&p, None), Position::Append));
    }

    #[test]
    fn auto_position_creates_missing_heading_on_create() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("daily.md");
        std::fs::write(&p, "# 2026-06-24\n\nNotes.\n").unwrap();
        let position = auto_position(&p, Some("Tasks"));
        create_task(
            &p,
            &EmojiFormat,
            input("Buy milk"),
            CreateOptions {
                position,
                force: false,
            },
        )
        .unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        assert_eq!(
            content,
            "# 2026-06-24\n\nNotes.\n\n## Tasks\n- [ ] Buy milk\n"
        );
    }

    #[test]
    fn append_to_existing_file_preserves_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "# Notes\n\nSome prose.\n").unwrap();
        let outcome = create_task(
            &p,
            &EmojiFormat,
            input("Buy milk"),
            CreateOptions {
                position: Position::Append,
                force: false,
            },
        )
        .unwrap();
        assert_eq!(outcome.line, 4);
        let content = std::fs::read_to_string(&p).unwrap();
        assert_eq!(content, "# Notes\n\nSome prose.\n- [ ] Buy milk\n");
    }

    #[test]
    fn at_line_inserts_in_middle() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "line1\nline2\nline3\n").unwrap();
        let outcome = create_task(
            &p,
            &EmojiFormat,
            input("Buy milk"),
            CreateOptions {
                position: Position::AtLine(2),
                force: false,
            },
        )
        .unwrap();
        assert_eq!(outcome.line, 2);
        let content = std::fs::read_to_string(&p).unwrap();
        assert_eq!(content, "line1\n- [ ] Buy milk\nline2\nline3\n");
    }

    fn create_subtask(p: &Path, desc: &str, parent_line: usize) -> CreateOutcome {
        create_task(
            p,
            &EmojiFormat,
            input(desc),
            CreateOptions {
                position: Position::Subtask { parent_line },
                force: false,
            },
        )
        .unwrap()
    }

    #[test]
    fn subtask_into_childless_parent_indents_two_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.md");
        std::fs::write(&p, "- [ ] parent\n- [ ] other\n").unwrap();
        let outcome = create_subtask(&p, "child", 1);
        assert_eq!(outcome.line, 2);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "- [ ] parent\n  - [ ] child\n- [ ] other\n"
        );
    }

    #[test]
    fn subtask_appends_after_existing_children_matching_indent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.md");
        std::fs::write(&p, "- [ ] parent\n  - [ ] a\n  - [ ] b\n- [ ] other\n").unwrap();
        let outcome = create_subtask(&p, "child", 1);
        assert_eq!(outcome.line, 4);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "- [ ] parent\n  - [ ] a\n  - [ ] b\n  - [ ] child\n- [ ] other\n"
        );
    }

    #[test]
    fn subtask_matches_four_space_children() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.md");
        std::fs::write(&p, "- [ ] parent\n    - [ ] a\n").unwrap();
        let outcome = create_subtask(&p, "child", 1);
        assert_eq!(outcome.line, 3);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "- [ ] parent\n    - [ ] a\n    - [ ] child\n"
        );
    }

    #[test]
    fn subtask_of_a_nested_parent_goes_one_level_deeper() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.md");
        std::fs::write(&p, "- [ ] top\n  - [ ] parent\n    - [ ] gc\n").unwrap();
        // Parent is itself a subtask (line 2); its new child matches `gc`.
        let outcome = create_subtask(&p, "child", 2);
        assert_eq!(outcome.line, 4);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "- [ ] top\n  - [ ] parent\n    - [ ] gc\n    - [ ] child\n"
        );
    }

    #[test]
    fn subtask_parent_out_of_range_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.md");
        std::fs::write(&p, "- [ ] parent\n").unwrap();
        let err = create_task(
            &p,
            &EmojiFormat,
            input("child"),
            CreateOptions {
                position: Position::Subtask { parent_line: 9 },
                force: false,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CreateError::LineOutOfRange { .. }));
    }

    #[test]
    fn at_line_zero_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "line1\n").unwrap();
        let err = create_task(
            &p,
            &EmojiFormat,
            input("X"),
            CreateOptions {
                position: Position::AtLine(0),
                force: false,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CreateError::LineOutOfRange { .. }));
    }

    #[test]
    fn under_heading_existing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "## Tasks\n- [ ] existing\n\n## Other\nstuff\n").unwrap();
        let outcome = create_task(
            &p,
            &EmojiFormat,
            input("Buy milk"),
            CreateOptions {
                position: Position::UnderHeading("Tasks".into()),
                force: false,
            },
        )
        .unwrap();
        // Inserted right after "- [ ] existing" — before the blank line, so
        // it visually belongs to the Tasks section.
        let content = std::fs::read_to_string(&p).unwrap();
        assert_eq!(
            content,
            "## Tasks\n- [ ] existing\n- [ ] Buy milk\n\n## Other\nstuff\n"
        );
        assert_eq!(outcome.line, 3);
    }

    #[test]
    fn under_heading_missing_creates_heading() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "# Notes\n\nProse.\n").unwrap();
        create_task(
            &p,
            &EmojiFormat,
            input("Buy milk"),
            CreateOptions {
                position: Position::UnderHeading("Tasks".into()),
                force: false,
            },
        )
        .unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        assert_eq!(content, "# Notes\n\nProse.\n\n## Tasks\n- [ ] Buy milk\n");
    }

    #[test]
    fn under_heading_at_top_of_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        create_task(
            &p,
            &EmojiFormat,
            input("Buy milk"),
            CreateOptions {
                position: Position::UnderHeading("Tasks".into()),
                force: false,
            },
        )
        .unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        assert_eq!(content, "## Tasks\n- [ ] Buy milk\n");
    }

    #[test]
    fn duplicate_refused_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "- [ ] Buy milk 📅 2026-05-10\n").unwrap();
        let mut i = input("Buy milk");
        i.due = Some(d(2026, 5, 10));
        let err = create_task(
            &p,
            &EmojiFormat,
            i,
            CreateOptions {
                position: Position::Append,
                force: false,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CreateError::Duplicate { .. }));
    }

    #[test]
    fn duplicate_inserted_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "- [ ] Buy milk 📅 2026-05-10\n").unwrap();
        let mut i = input("Buy milk");
        i.due = Some(d(2026, 5, 10));
        create_task(
            &p,
            &EmojiFormat,
            i,
            CreateOptions {
                position: Position::Append,
                force: true,
            },
        )
        .unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        let count = content.matches("- [ ] Buy milk 📅 2026-05-10").count();
        assert_eq!(count, 2);
    }

    #[test]
    fn duplicate_check_distinguishes_dates() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "- [ ] Buy milk 📅 2026-05-10\n").unwrap();
        // Different due date → not a duplicate.
        let mut i = input("Buy milk");
        i.due = Some(d(2026, 5, 11));
        create_task(
            &p,
            &EmojiFormat,
            i,
            CreateOptions {
                position: Position::Append,
                force: false,
            },
        )
        .unwrap();
    }

    #[test]
    fn parse_heading_levels() {
        use crate::markdown::lines::parse_heading;
        assert_eq!(parse_heading("# Top"), Some((1, "Top")));
        assert_eq!(parse_heading("### Three"), Some((3, "Three")));
        assert_eq!(parse_heading("###### Six"), Some((6, "Six")));
        assert_eq!(parse_heading("####### Seven"), None);
        assert_eq!(parse_heading("not a heading"), None);
        assert_eq!(parse_heading("#NoSpace"), None);
    }

    // ── complete_task ─────────────────────────────────────────────────────────

    #[test]
    fn complete_simple_task_marks_done_with_date() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "# Notes\n- [ ] Buy milk 📅 2026-05-10\n").unwrap();
        let outcome = complete_task(
            &p,
            2,
            &EmojiFormat,
            None,
            CompleteOptions { on: d(2026, 5, 10) },
        )
        .unwrap();
        assert_eq!(outcome.completed_line, 2);
        assert!(outcome.next_instance.is_none());
        let content = std::fs::read_to_string(&p).unwrap();
        assert_eq!(
            content,
            "# Notes\n- [x] Buy milk 📅 2026-05-10 ✅ 2026-05-10\n"
        );
    }

    #[test]
    fn complete_already_done_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "- [x] task ✅ 2026-05-09\n").unwrap();
        let err = complete_task(
            &p,
            1,
            &EmojiFormat,
            None,
            CompleteOptions { on: d(2026, 5, 10) },
        )
        .unwrap_err();
        assert!(matches!(err, CompleteError::AlreadyDone { .. }), "{err:?}");
    }

    #[test]
    fn complete_non_task_line_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "# Heading\nProse\n").unwrap();
        let err = complete_task(
            &p,
            1,
            &EmojiFormat,
            None,
            CompleteOptions { on: d(2026, 5, 10) },
        )
        .unwrap_err();
        assert!(matches!(err, CompleteError::NotATask { .. }), "{err:?}");
    }

    #[test]
    fn complete_line_out_of_range_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "- [ ] x\n").unwrap();
        let err = complete_task(
            &p,
            5,
            &EmojiFormat,
            None,
            CompleteOptions { on: d(2026, 5, 10) },
        )
        .unwrap_err();
        assert!(matches!(err, CompleteError::LineMissing { .. }), "{err:?}");
    }

    #[test]
    fn complete_recurring_task_inserts_next_instance_above() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(
            &p,
            "- [ ] Pay tax 🔁 every month on the 18th 📅 2026-05-18\n",
        )
        .unwrap();
        let outcome = complete_task(
            &p,
            1,
            &EmojiFormat,
            None,
            CompleteOptions { on: d(2026, 5, 18) },
        )
        .unwrap();
        // The next instance lives where the original task was; the completed
        // task moved down one line.
        assert_eq!(outcome.completed_line, 2);
        let next = outcome.next_instance.expect("recurrence creates next");
        assert_eq!(next.line, 1);

        let content = std::fs::read_to_string(&p).unwrap();
        assert_eq!(
            content,
            "- [ ] Pay tax 🔁 every month on the 18th 📅 2026-06-18\n\
             - [x] Pay tax 🔁 every month on the 18th 📅 2026-05-18 ✅ 2026-05-18\n"
        );
    }

    #[test]
    fn complete_recurring_weekly_shifts_all_dates() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(
            &p,
            "- [ ] Standup 🔁 every week ⏳ 2026-05-08 📅 2026-05-10\n",
        )
        .unwrap();
        complete_task(
            &p,
            1,
            &EmojiFormat,
            None,
            CompleteOptions { on: d(2026, 5, 10) },
        )
        .unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        // delta = +7 days, so scheduled and due both shift by 7.
        assert_eq!(
            content,
            "- [ ] Standup 🔁 every week ⏳ 2026-05-15 📅 2026-05-17\n\
             - [x] Standup 🔁 every week ⏳ 2026-05-08 📅 2026-05-10 ✅ 2026-05-10\n"
        );
    }

    #[test]
    fn complete_recurring_with_unsupported_pattern_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "- [ ] Yearly thing 🔁 every year 📅 2026-05-10\n").unwrap();
        let err = complete_task(
            &p,
            1,
            &EmojiFormat,
            None,
            CompleteOptions { on: d(2026, 5, 10) },
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                CompleteError::Recurrence(RecurrenceError::Unsupported { .. })
            ),
            "{err:?}"
        );
        // File must be untouched.
        let content = std::fs::read_to_string(&p).unwrap();
        assert_eq!(content, "- [ ] Yearly thing 🔁 every year 📅 2026-05-10\n");
    }

    #[test]
    fn complete_recurring_with_no_anchor_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "- [ ] No-anchor 🔁 every day\n").unwrap();
        let err = complete_task(
            &p,
            1,
            &EmojiFormat,
            None,
            CompleteOptions { on: d(2026, 5, 10) },
        )
        .unwrap_err();
        assert!(
            matches!(err, CompleteError::Recurrence(RecurrenceError::NoAnchor)),
            "{err:?}"
        );
    }

    #[test]
    fn complete_preserves_indentation_and_other_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(
            &p,
            "## Tasks\n- [ ] parent\n  - [ ] child to complete\n  - [ ] sibling\n",
        )
        .unwrap();
        complete_task(
            &p,
            3,
            &EmojiFormat,
            None,
            CompleteOptions { on: d(2026, 5, 10) },
        )
        .unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        assert_eq!(
            content,
            "## Tasks\n- [ ] parent\n  - [x] child to complete ✅ 2026-05-10\n  - [ ] sibling\n"
        );
    }

    // ── move_tasks ────────────────────────────────────────────────────────────

    fn write(dir: &tempfile::TempDir, rel: &str, content: &str) -> PathBuf {
        let p = dir.path().join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn move_single_task_to_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(
            &dir,
            "inbox.md",
            "- [ ] keep\n- [ ] move me 📅 2026-05-10\n",
        );
        let target = dir.path().join("done.md");

        let plan = plan_move(
            &[MoveSource {
                path: src.clone(),
                line: 2,
                expected: None,
            }],
            &MoveTarget::Append(target.clone()),
            &EmojiFormat,
        )
        .unwrap();

        apply_move_plan(&plan).unwrap();

        assert_eq!(std::fs::read_to_string(&src).unwrap(), "- [ ] keep\n");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "- [ ] move me 📅 2026-05-10\n"
        );
        assert_eq!(plan.blocks.len(), 1);
        assert_eq!(plan.blocks[0].task_description, "move me");
    }

    #[test]
    fn move_task_with_subtasks_takes_them_along() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(
            &dir,
            "src.md",
            "- [ ] keep top\n\
             - [ ] parent\n  - [ ] child A\n  - [ ] child B\n\
             - [ ] keep bottom\n",
        );
        let target = dir.path().join("dst.md");

        let plan = plan_move(
            &[MoveSource {
                path: src.clone(),
                line: 2,
                expected: None,
            }],
            &MoveTarget::Append(target.clone()),
            &EmojiFormat,
        )
        .unwrap();

        apply_move_plan(&plan).unwrap();

        assert_eq!(
            std::fs::read_to_string(&src).unwrap(),
            "- [ ] keep top\n- [ ] keep bottom\n"
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "- [ ] parent\n  - [ ] child A\n  - [ ] child B\n"
        );
    }

    #[test]
    fn move_indented_task_normalizes_indentation() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(
            &dir,
            "src.md",
            "- [ ] outer\n  - [ ] inner\n    - [ ] grandchild\n",
        );
        let target = dir.path().join("dst.md");

        // Move just the inner task (line 2). Its grandchild comes along.
        let plan = plan_move(
            &[MoveSource {
                path: src.clone(),
                line: 2,
                expected: None,
            }],
            &MoveTarget::Append(target.clone()),
            &EmojiFormat,
        )
        .unwrap();
        apply_move_plan(&plan).unwrap();

        assert_eq!(std::fs::read_to_string(&src).unwrap(), "- [ ] outer\n");
        // Inner block had a 2-space head indent; normalized to 0, the
        // grandchild's relative indent (4 → 2) is preserved.
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "- [ ] inner\n  - [ ] grandchild\n"
        );
    }

    #[test]
    fn move_to_under_heading_creates_heading_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "src.md", "- [ ] move me\n");
        let target = write(&dir, "dst.md", "# Existing\n\nSome prose.\n");

        let plan = plan_move(
            &[MoveSource {
                path: src.clone(),
                line: 1,
                expected: None,
            }],
            &MoveTarget::UnderHeading(target.clone(), "Triage".into()),
            &EmojiFormat,
        )
        .unwrap();
        apply_move_plan(&plan).unwrap();

        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "# Existing\n\nSome prose.\n\n## Triage\n- [ ] move me\n"
        );
    }

    #[test]
    fn move_to_existing_heading_appends_to_section() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "src.md", "- [ ] new entry\n");
        let target = write(
            &dir,
            "dst.md",
            "## Triage\n- [ ] existing\n\n## Other\nstuff\n",
        );

        let plan = plan_move(
            &[MoveSource {
                path: src.clone(),
                line: 1,
                expected: None,
            }],
            &MoveTarget::UnderHeading(target.clone(), "Triage".into()),
            &EmojiFormat,
        )
        .unwrap();
        apply_move_plan(&plan).unwrap();

        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "## Triage\n- [ ] existing\n- [ ] new entry\n\n## Other\nstuff\n"
        );
    }

    #[test]
    fn move_drops_descendant_when_parent_also_in_list() {
        // If the user query matches both the parent and a child, the child
        // is subsumed by the parent's block — no double move.
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "src.md", "- [ ] parent\n  - [ ] child\n");
        let target = dir.path().join("dst.md");

        let plan = plan_move(
            &[
                MoveSource {
                    path: src.clone(),
                    line: 1,
                    expected: None,
                },
                MoveSource {
                    path: src.clone(),
                    line: 2,
                    expected: None,
                },
            ],
            &MoveTarget::Append(target.clone()),
            &EmojiFormat,
        )
        .unwrap();

        apply_move_plan(&plan).unwrap();
        assert_eq!(plan.blocks.len(), 1);
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "- [ ] parent\n  - [ ] child\n"
        );
        assert_eq!(std::fs::read_to_string(&src).unwrap(), "");
    }

    #[test]
    fn move_bulk_from_multiple_files_into_one_target() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.md", "- [ ] A1\n- [ ] A2\n");
        let b = write(&dir, "b.md", "# B\n- [ ] B1\n");
        let target = dir.path().join("triage.md");

        let plan = plan_move(
            &[
                MoveSource {
                    path: a.clone(),
                    line: 1,
                    expected: None,
                },
                MoveSource {
                    path: b.clone(),
                    line: 2,
                    expected: None,
                },
            ],
            &MoveTarget::Append(target.clone()),
            &EmojiFormat,
        )
        .unwrap();
        apply_move_plan(&plan).unwrap();

        assert_eq!(std::fs::read_to_string(&a).unwrap(), "- [ ] A2\n");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "# B\n");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "- [ ] A1\n- [ ] B1\n"
        );
    }

    #[test]
    fn move_within_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "f.md", "## Inbox\n- [ ] move\n\n## Done\n");
        let plan = plan_move(
            &[MoveSource {
                path: p.clone(),
                line: 2,
                expected: None,
            }],
            &MoveTarget::UnderHeading(p.clone(), "Done".into()),
            &EmojiFormat,
        )
        .unwrap();
        apply_move_plan(&plan).unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "## Inbox\n\n## Done\n- [ ] move\n"
        );
    }

    #[test]
    fn move_non_task_line_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "f.md", "## Heading\n");
        let err = plan_move(
            &[MoveSource {
                path: p.clone(),
                line: 1,
                expected: None,
            }],
            &MoveTarget::Append(dir.path().join("out.md")),
            &EmojiFormat,
        )
        .unwrap_err();
        assert!(matches!(err, MoveError::NotATask { .. }), "{err:?}");
    }

    #[test]
    fn move_line_out_of_range_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "f.md", "- [ ] only\n");
        let err = plan_move(
            &[MoveSource {
                path: p.clone(),
                line: 5,
                expected: None,
            }],
            &MoveTarget::Append(dir.path().join("out.md")),
            &EmojiFormat,
        )
        .unwrap_err();
        assert!(matches!(err, MoveError::LineMissing { .. }), "{err:?}");
    }

    #[test]
    fn plan_move_empty_input_yields_target_only_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let target = write(&dir, "dst.md", "## Heading\n");
        let plan = plan_move(&[], &MoveTarget::Append(target.clone()), &EmojiFormat).unwrap();
        // The target edit should be a no-op (original == new).
        assert_eq!(plan.edits.len(), 1);
        assert_eq!(plan.edits[0].original, plan.edits[0].new);
    }

    // ── expected-line guard ───────────────────────────────────────────────────

    /// Parse `line` as the caller (a scan) would see it.
    fn parsed(line: &str) -> Task {
        EmojiFormat
            .parse_line(
                line,
                ParseContext {
                    source_file: PathBuf::from("some/note.md"),
                    source_line: 42,
                },
            )
            .unwrap()
    }

    #[test]
    fn complete_with_matching_expected_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("n.md");
        std::fs::write(&p, "- [ ] Buy milk 📅 2026-05-10\n").unwrap();
        let expected = parsed("- [ ] Buy milk 📅 2026-05-10");
        complete_task(
            &p,
            1,
            &EmojiFormat,
            Some(&expected),
            CompleteOptions { on: d(2026, 5, 10) },
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "- [x] Buy milk 📅 2026-05-10 ✅ 2026-05-10\n"
        );
    }

    #[test]
    fn complete_with_stale_expected_errors_and_leaves_file_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("n.md");
        // The caller scanned when "Buy milk" was at line 1, but the file
        // shifted: a different task now sits there.
        std::fs::write(&p, "- [ ] Call mom\n- [ ] Buy milk 📅 2026-05-10\n").unwrap();
        let expected = parsed("- [ ] Buy milk 📅 2026-05-10");
        let err = complete_task(
            &p,
            1,
            &EmojiFormat,
            Some(&expected),
            CompleteOptions { on: d(2026, 5, 10) },
        )
        .unwrap_err();
        assert!(matches!(err, CompleteError::LineChanged { .. }), "{err:?}");
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "- [ ] Call mom\n- [ ] Buy milk 📅 2026-05-10\n"
        );
    }

    #[test]
    fn guard_catches_recurring_next_instance_at_stale_line() {
        // The flagship race: completing a recurring task inserts the next
        // instance at the completed line's old slot. A second caller holding
        // the pre-complete scan would re-complete "the same line" — which now
        // holds the *new* instance (same description, shifted dates). The
        // guard must catch this; a description-only comparison would not.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("n.md");
        let original = "- [ ] Pay tax 🔁 every month on the 18th 📅 2026-05-18";
        std::fs::write(&p, format!("{original}\n")).unwrap();
        let stale_expected = parsed(original);

        complete_task(
            &p,
            1,
            &EmojiFormat,
            Some(&stale_expected),
            CompleteOptions { on: d(2026, 5, 18) },
        )
        .unwrap();

        let err = complete_task(
            &p,
            1,
            &EmojiFormat,
            Some(&stale_expected),
            CompleteOptions { on: d(2026, 5, 18) },
        )
        .unwrap_err();
        assert!(matches!(err, CompleteError::LineChanged { .. }), "{err:?}");
    }

    #[test]
    fn guard_ignores_context_fields() {
        // Two scans of the same content differ only in source_file /
        // source_line / parent — the guard compares wire content, so a
        // context mismatch must not trip it.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("n.md");
        std::fs::write(&p, "- [ ] task ⏫\n").unwrap();
        let mut expected = parsed("- [ ] task ⏫");
        expected.source_file = PathBuf::from("elsewhere.md");
        expected.source_line = 999;
        expected.parent = Some(3);
        complete_task(
            &p,
            1,
            &EmojiFormat,
            Some(&expected),
            CompleteOptions { on: d(2026, 5, 10) },
        )
        .unwrap();
    }

    #[test]
    fn update_with_stale_expected_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("n.md");
        std::fs::write(&p, "- [ ] other task\n").unwrap();
        let expected = parsed("- [ ] Buy milk");
        let err = update_task_line(&p, 1, &EmojiFormat, Some(&expected), |t| {
            t.priority = Some(Priority::High);
        })
        .unwrap_err();
        assert!(matches!(err, UpdateError::LineChanged { .. }), "{err:?}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "- [ ] other task\n");
    }

    #[test]
    fn cancel_with_matching_expected_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("n.md");
        std::fs::write(&p, "- [ ] drop this\n").unwrap();
        let expected = parsed("- [ ] drop this");
        let t = cancel_task(&p, 1, &EmojiFormat, Some(&expected), d(2026, 5, 10)).unwrap();
        assert_eq!(t.status, Status::Cancelled);
    }

    #[test]
    fn plan_move_with_stale_expected_errors() {
        let dir = tempfile::tempdir().unwrap();
        let src = write(&dir, "src.md", "- [ ] other task\n");
        let err = plan_move(
            &[MoveSource {
                path: src.clone(),
                line: 1,
                expected: Some(parsed("- [ ] move me")),
            }],
            &MoveTarget::Append(dir.path().join("dst.md")),
            &EmojiFormat,
        )
        .unwrap_err();
        assert!(matches!(err, MoveError::LineChanged { .. }), "{err:?}");
        assert_eq!(std::fs::read_to_string(&src).unwrap(), "- [ ] other task\n");
    }

    // ── retag_task ───────────────────────────────────────────────────

    fn retag_round_trip(initial: &str, list: &[&str], tag: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("n.md");
        std::fs::write(&p, format!("{initial}\n")).unwrap();
        let expected = parsed(initial);
        let list_owned: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        retag_task(&p, 1, &EmojiFormat, Some(&expected), &list_owned, tag).unwrap();
        std::fs::read_to_string(&p).unwrap().trim_end().to_string()
    }

    #[test]
    fn retag_swaps_prior_list_tag_preserves_others() {
        // list = {wait, computer, physical}; #computer is a member, #finance is not.
        let out = retag_round_trip(
            "- [ ] Pay invoice #finance #computer",
            &["wait", "computer", "physical"],
            "wait",
        );
        assert_eq!(out, "- [ ] Pay invoice #finance #wait");
    }

    #[test]
    fn retag_appends_when_no_list_tag_present() {
        let out = retag_round_trip(
            "- [ ] Email Sarah #finance",
            &["wait", "computer", "physical"],
            "computer",
        );
        assert_eq!(out, "- [ ] Email Sarah #finance #computer");
    }

    #[test]
    fn retag_preserves_glued_or_nonmember_tags() {
        // `#computer-thing` is a single tag word whose bare form is
        // `computer-thing` — not `computer`, so it is not matched/removed.
        let out = retag_round_trip("- [ ] fix #computer-thing", &["computer"], "wait");
        assert_eq!(out, "- [ ] fix #computer-thing #wait");
    }

    #[test]
    fn retag_empty_tag_clears_list_members_only() {
        // `tag == ""` clears every list-member tag and appends nothing.
        let out = retag_round_trip(
            "- [ ] Pay invoice #finance #computer",
            &["wait", "computer", "physical"],
            "",
        );
        assert_eq!(out, "- [ ] Pay invoice #finance");
    }

    #[test]
    fn retag_non_list_tag_preserved_when_swapping() {
        // #finance stays; #wait (a list member) is replaced by #computer.
        let out = retag_round_trip(
            "- [ ] Email #finance #wait",
            &["wait", "computer", "physical"],
            "computer",
        );
        assert_eq!(out, "- [ ] Email #finance #computer");
    }

    #[test]
    fn retag_strips_leading_hash_from_tag_argument() {
        // The picker passes a bare name, but a stray leading `#` is tolerated.
        let out = retag_round_trip(
            "- [ ] thing #computer",
            &["wait", "computer", "physical"],
            "#wait",
        );
        assert_eq!(out, "- [ ] thing #wait");
    }

    #[test]
    fn retag_with_stale_expected_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("n.md");
        std::fs::write(&p, "- [ ] other task\n").unwrap();
        let expected = parsed("- [ ] Pay invoice #computer");
        let list = vec!["wait".to_string(), "computer".to_string()];
        let err = retag_task(&p, 1, &EmojiFormat, Some(&expected), &list, "wait").unwrap_err();
        assert!(matches!(err, UpdateError::LineChanged { .. }), "{err:?}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "- [ ] other task\n");
    }

    // ── reparent (UnderTask / indent / outdent) ──────────────────────

    fn lines_of(s: &str) -> Vec<String> {
        crate::markdown::lines::split(s)
    }

    #[test]
    fn detect_indent_unit_variants() {
        assert_eq!(
            detect_indent_unit(&lines_of("- a\n  - b\n    - c\n")),
            IndentUnit::Spaces(2)
        );
        assert_eq!(
            detect_indent_unit(&lines_of("- a\n    - b\n")),
            IndentUnit::Spaces(4)
        );
        assert_eq!(
            detect_indent_unit(&lines_of("- a\n\t- b\n")),
            IndentUnit::Tab
        );
        assert_eq!(
            detect_indent_unit(&lines_of("- a\n- b\n")),
            IndentUnit::Spaces(2)
        );
        assert_eq!(
            detect_indent_unit(&lines_of("1. a\n   1. b\n")),
            IndentUnit::Spaces(3)
        );
        // Indented prose and new top-level paragraphs don't count as nesting.
        assert_eq!(
            detect_indent_unit(&lines_of(
                "- a\n      continuation\npara\n    - b\n- c\n    - d\n"
            )),
            IndentUnit::Spaces(4)
        );
    }

    #[test]
    fn subtask_into_childless_parent_follows_tab_unit() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.md");
        std::fs::write(&p, "- [ ] other\n\t- [ ] nested\n- [ ] parent\n").unwrap();
        create_subtask(&p, "child", 3);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "- [ ] other\n\t- [ ] nested\n- [ ] parent\n\t- [ ] child\n"
        );
    }

    fn under(path: &Path, line: usize, placement: Placement) -> MoveTarget {
        MoveTarget::UnderTask {
            path: path.to_path_buf(),
            line,
            expected: None,
            placement,
        }
    }

    fn src(path: &Path, line: usize) -> MoveSource {
        MoveSource {
            path: path.to_path_buf(),
            line,
            expected: None,
        }
    }

    fn run_move(sources: &[MoveSource], target: &MoveTarget) -> MovePlan {
        let plan = plan_move(sources, target, &EmojiFormat).unwrap();
        apply_move_plan(&plan).unwrap();
        plan
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    #[test]
    fn under_task_same_file_source_above_target() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.md", "- [ ] A\n  - [ ] a1\n- [ ] B\n");
        let plan = run_move(&[src(&a, 2)], &under(&a, 3, Placement::Child));
        assert_eq!(read(&a), "- [ ] A\n- [ ] B\n  - [ ] a1\n");
        assert_eq!(plan.blocks[0].dest, Some((a.clone(), 3)));
    }

    #[test]
    fn under_task_same_file_block_above_target() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.md", "- [ ] X\n  - [ ] x1\n- [ ] T\n");
        let plan = run_move(&[src(&a, 1)], &under(&a, 3, Placement::Child));
        assert_eq!(read(&a), "- [ ] T\n  - [ ] X\n    - [ ] x1\n");
        assert_eq!(plan.blocks[0].dest, Some((a.clone(), 2)));
    }

    #[test]
    fn under_task_same_file_source_below_target_with_children() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.md", "- [ ] T\n  - [ ] t1\n- [ ] B\n- [ ] C\n");
        run_move(&[src(&a, 4)], &under(&a, 1, Placement::Child));
        assert_eq!(read(&a), "- [ ] T\n  - [ ] t1\n  - [ ] C\n- [ ] B\n");
    }

    #[test]
    fn sibling_after_skips_target_subtree() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.md", "- [ ] A\n  - [ ] a1\n- [ ] B\n- [ ] C\n");
        run_move(&[src(&a, 4)], &under(&a, 1, Placement::SiblingAfter));
        assert_eq!(read(&a), "- [ ] A\n  - [ ] a1\n- [ ] C\n- [ ] B\n");
    }

    #[test]
    fn sibling_after_nested_target_takes_its_indent() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.md", "- [ ] P\n  - [ ] p1\n- [ ] Q\n  - [ ] q1\n");
        run_move(&[src(&a, 3)], &under(&a, 2, Placement::SiblingAfter));
        assert_eq!(read(&a), "- [ ] P\n  - [ ] p1\n  - [ ] Q\n    - [ ] q1\n");
    }

    #[test]
    fn under_task_cross_file_matches_existing_children_and_converts_unit() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = write(
            &dir,
            "inbox.md",
            "- [ ] keep\n- [ ] call vendor\n  - [ ] find number\n    note line\n",
        );
        let proj = write(
            &dir,
            "proj.md",
            "- [ ] Vendor onboarding\n    - [ ] sign NDA\n- [ ] other\n",
        );
        let plan = run_move(&[src(&inbox, 2)], &under(&proj, 1, Placement::Child));
        assert_eq!(read(&inbox), "- [ ] keep\n");
        assert_eq!(
            read(&proj),
            "- [ ] Vendor onboarding\n    - [ ] sign NDA\n    - [ ] call vendor\n        - [ ] find number\n            note line\n- [ ] other\n"
        );
        assert_eq!(plan.blocks[0].dest, Some((proj.clone(), 3)));
    }

    #[test]
    fn under_task_tabs_to_spaces_childless_parent() {
        let dir = tempfile::tempdir().unwrap();
        let s = write(&dir, "s.md", "- [ ] X\n\t- [ ] x1\n");
        let t = write(&dir, "t.md", "- [ ] P\n- [ ] Q\n    - [ ] q1\n");
        run_move(&[src(&s, 1)], &under(&t, 1, Placement::Child));
        assert_eq!(read(&s), "");
        assert_eq!(
            read(&t),
            "- [ ] P\n    - [ ] X\n        - [ ] x1\n- [ ] Q\n    - [ ] q1\n"
        );
    }

    #[test]
    fn under_task_same_unit_keeps_relative_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let s = write(
            &dir,
            "s.md",
            "  - [ ] X\n    - [ ] x1\n      odd 2-space note\n",
        );
        let t = write(&dir, "t.md", "- [ ] P\n");
        run_move(&[src(&s, 1)], &under(&t, 1, Placement::Child));
        assert_eq!(
            read(&t),
            "- [ ] P\n  - [ ] X\n    - [ ] x1\n      odd 2-space note\n"
        );
    }

    #[test]
    fn under_task_multiple_sources_ancestor_and_descendant() {
        let dir = tempfile::tempdir().unwrap();
        let s = write(&dir, "s.md", "- [ ] A\n  - [ ] a1\n- [ ] B\n");
        let t = write(&dir, "t.md", "- [ ] P\n");
        let plan = run_move(
            &[src(&s, 2), src(&s, 1), src(&s, 3)],
            &under(&t, 1, Placement::Child),
        );
        assert_eq!(read(&s), "");
        assert_eq!(read(&t), "- [ ] P\n  - [ ] A\n    - [ ] a1\n  - [ ] B\n");
        let dests: Vec<usize> = plan
            .blocks
            .iter()
            .map(|b| b.dest.as_ref().unwrap().1)
            .collect();
        assert_eq!(dests, vec![2, 4]);
    }

    #[test]
    fn under_own_descendant_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.md", "- [ ] A\n  - [ ] a1\n");
        for target_line in [1, 2] {
            let err = plan_move(
                &[src(&a, 1)],
                &under(&a, target_line, Placement::Child),
                &EmojiFormat,
            )
            .unwrap_err();
            assert!(matches!(err, MoveError::IntoOwnSubtree { .. }), "{err:?}");
        }
        assert_eq!(read(&a), "- [ ] A\n  - [ ] a1\n");
    }

    #[test]
    fn under_task_stale_or_non_task_target_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.md", "- [ ] A\n# heading\n- [ ] B\n");
        let stale = MoveTarget::UnderTask {
            path: a.clone(),
            line: 3,
            expected: Some(Box::new(parsed("- [ ] something else"))),
            placement: Placement::Child,
        };
        let err = plan_move(&[src(&a, 1)], &stale, &EmojiFormat).unwrap_err();
        assert!(matches!(err, MoveError::TargetChanged { .. }), "{err:?}");
        let err =
            plan_move(&[src(&a, 1)], &under(&a, 2, Placement::Child), &EmojiFormat).unwrap_err();
        assert!(matches!(err, MoveError::NotATask { .. }), "{err:?}");
        let err =
            plan_move(&[src(&a, 1)], &under(&a, 9, Placement::Child), &EmojiFormat).unwrap_err();
        assert!(matches!(err, MoveError::LineMissing { .. }), "{err:?}");
    }

    #[test]
    fn append_and_heading_targets_report_dest() {
        let dir = tempfile::tempdir().unwrap();
        let s = write(&dir, "s.md", "- [ ] one\n- [ ] two\n");
        let t = write(&dir, "t.md", "intro\n");
        let plan = run_move(&[src(&s, 2)], &MoveTarget::Append(t.clone()));
        assert_eq!(plan.blocks[0].dest, Some((t.clone(), 2)));
        let plan = run_move(
            &[src(&s, 1)],
            &MoveTarget::UnderHeading(t.clone(), "Inbox".into()),
        );
        assert_eq!(read(&t), "intro\n- [ ] two\n\n## Inbox\n- [ ] one\n");
        assert_eq!(plan.blocks[0].dest, Some((t.clone(), 5)));
    }

    fn indent(p: &Path, line: usize) -> Result<MovePlan, MoveError> {
        plan_indent(&src(p, line), &EmojiFormat)
    }

    fn outdent(p: &Path, line: usize) -> Result<MovePlan, MoveError> {
        plan_outdent(&src(p, line), &EmojiFormat)
    }

    #[test]
    fn indent_under_previous_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.md", "- [ ] A\n- [ ] B\n");
        apply_move_plan(&indent(&a, 2).unwrap()).unwrap();
        assert_eq!(read(&a), "- [ ] A\n  - [ ] B\n");
    }

    #[test]
    fn indent_under_sibling_with_children_goes_last() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(
            &dir,
            "a.md",
            "- [ ] A\n    - [ ] a1\n- [ ] B\n    - [ ] b1\n",
        );
        apply_move_plan(&indent(&a, 3).unwrap()).unwrap();
        assert_eq!(
            read(&a),
            "- [ ] A\n    - [ ] a1\n    - [ ] B\n        - [ ] b1\n"
        );
    }

    #[test]
    fn indent_without_adjacent_sibling_errors() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(
            &dir,
            "a.md",
            "- [ ] P\n  - [ ] c1\n- [ ] A\n\n## H\n- [ ] B\n",
        );
        for line in [1, 2, 6] {
            let err = indent(&a, line).unwrap_err();
            assert!(
                matches!(err, MoveError::NoPreviousSibling { .. }),
                "line {line}: {err:?}"
            );
        }
    }

    #[test]
    fn outdent_places_after_parent_block_leaving_later_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(
            &dir,
            "a.md",
            "- [ ] P\n  - [ ] c1\n    - [ ] g\n  - [ ] c2\n- [ ] Q\n",
        );
        let plan = outdent(&a, 2).unwrap();
        apply_move_plan(&plan).unwrap();
        assert_eq!(
            read(&a),
            "- [ ] P\n  - [ ] c2\n- [ ] c1\n  - [ ] g\n- [ ] Q\n"
        );
        assert_eq!(plan.blocks[0].dest, Some((a.clone(), 3)));
    }

    #[test]
    fn outdent_top_level_errors() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.md", "- [ ] A\n");
        let err = outdent(&a, 1).unwrap_err();
        assert!(matches!(err, MoveError::AlreadyTopLevel { .. }), "{err:?}");
    }

    #[test]
    fn indent_stale_source_errors() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.md", "- [ ] A\n- [ ] B\n");
        let s = MoveSource {
            path: a.clone(),
            line: 2,
            expected: Some(parsed("- [ ] not B")),
        };
        let err = plan_indent(&s, &EmojiFormat).unwrap_err();
        assert!(matches!(err, MoveError::LineChanged { .. }), "{err:?}");
    }

    mod reparent_props {
        use super::*;
        use proptest::prelude::*;

        /// Small task forests as (depth) sequences: each line's depth is at
        /// most one deeper than the previous line's.
        fn forest() -> impl Strategy<Value = Vec<usize>> {
            prop::collection::vec(0usize..3, 2..8).prop_map(|raw| {
                let mut out = Vec::with_capacity(raw.len());
                let mut prev = 0usize;
                for (i, d) in raw.into_iter().enumerate() {
                    let d = if i == 0 { 0 } else { d.min(prev + 1) };
                    out.push(d);
                    prev = d;
                }
                out
            })
        }

        proptest! {
            #[test]
            fn under_task_moves_preserve_lines_and_set_parent(
                depths in forest(),
                s_pick in 0usize..8,
                t_pick in 0usize..8,
                sibling in any::<bool>(),
            ) {
                let content: String = depths
                    .iter()
                    .enumerate()
                    .map(|(i, d)| format!("{}- [ ] t{i}\n", "  ".repeat(*d)))
                    .collect();
                let dir = tempfile::tempdir().unwrap();
                let p = dir.path().join("f.md");
                std::fs::write(&p, &content).unwrap();
                let s_line = s_pick % depths.len() + 1;
                let t_line = t_pick % depths.len() + 1;
                let placement = if sibling { Placement::SiblingAfter } else { Placement::Child };
                let target = MoveTarget::UnderTask { path: p.clone(), line: t_line, expected: None, placement };
                let s = MoveSource { path: p.clone(), line: s_line, expected: None };
                let Ok(plan) = plan_move(&[s], &target, &EmojiFormat) else { return Ok(()); };
                let new = &plan.edits.last().unwrap().new;

                let mut before: Vec<&str> = content.lines().map(str::trim).collect();
                let mut after: Vec<&str> = new.lines().map(str::trim).collect();
                before.sort_unstable();
                after.sort_unstable();
                prop_assert_eq!(before, after);

                let mut tasks: Vec<Task> = new
                    .lines()
                    .enumerate()
                    .filter_map(|(i, l)| EmojiFormat.parse_line(l, ParseContext { source_file: p.clone(), source_line: i + 1 }))
                    .collect();
                crate::task::hierarchy::resolve_hierarchy(&mut tasks);
                let by_desc = |d: &str| tasks.iter().find(|t| t.description == d).unwrap().clone();
                let moved = by_desc(&format!("t{}", s_line - 1));
                let tgt = by_desc(&format!("t{}", t_line - 1));
                let expected_parent = match placement {
                    Placement::Child => Some(tgt.source_line),
                    Placement::SiblingAfter => tgt.parent,
                };
                prop_assert_eq!(moved.parent, expected_parent);
            }
        }
    }
}

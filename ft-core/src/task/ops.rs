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
fn check_expected(
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
    #[error("write failed: {source}")]
    Write {
        #[from]
        source: crate::error::Error,
    },
}

/// Where to insert moved tasks in the target file.
#[derive(Debug, Clone)]
pub enum MoveTarget {
    /// Append to the file (creating it if missing).
    Append(PathBuf),
    /// Append to the section under the given heading; create the heading at
    /// file end if missing.
    UnderHeading(PathBuf, String),
}

impl MoveTarget {
    pub fn path(&self) -> &Path {
        match self {
            MoveTarget::Append(p) | MoveTarget::UnderHeading(p, _) => p,
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

    // Ranges to remove from each source file plus the lines that are being
    // hoisted out (so we can splice them at the target). For source==target,
    // we collect removals and the insertion happens after removals are
    // applied; insertion lines reference the *original* indices via the
    // collected `block_lines`.
    let mut source_edits: BTreeMap<PathBuf, FileEditWork> = BTreeMap::new();
    let mut blocks: Vec<MovedBlock> = Vec::new();
    let mut moved_lines: Vec<String> = Vec::new();

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

        // Append moved lines (with normalized indent) into `moved_lines`.
        for (start, end, description, head_indent) in &kept {
            blocks.push(MovedBlock {
                source: MoveSource {
                    path: path.clone(),
                    line: start + 1,
                    expected: None,
                },
                end_line: end + 1,
                task_description: description.clone(),
            });
            for raw in &raw_lines[*start..=*end] {
                moved_lines.push(strip_indent(raw, *head_indent));
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

    let final_target = splice_into_target(target_lines, &moved_lines, target);

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
/// verbatim — tabs included — or sits two spaces past the parent if it has no
/// children yet.
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
        format!("{}  ", leading_ws(&lines[idx]))
    };

    // Splice after the block's last line: 0-indexed `end + 1` ⇒ `AtLine(end + 2)`.
    Ok((indent, end + 2))
}

/// Leading whitespace (spaces/tabs) prefix of `line`.
fn leading_ws(line: &str) -> &str {
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

fn splice_into_target(
    mut lines: Vec<String>,
    moved_lines: &[String],
    target: &MoveTarget,
) -> String {
    if moved_lines.is_empty() {
        return crate::markdown::lines::join_with_newline(&lines);
    }
    match target {
        MoveTarget::Append(_) => {
            for ml in moved_lines {
                lines.push(ml.clone());
            }
        }
        MoveTarget::UnderHeading(_, heading) => {
            match crate::markdown::lines::find_heading(&lines, heading) {
                Some((heading_idx, level)) => {
                    let insert_at = crate::markdown::lines::section_end(&lines, heading_idx, level);
                    for (offset, ml) in moved_lines.iter().enumerate() {
                        lines.insert(insert_at + offset, ml.clone());
                    }
                }
                None => {
                    if !lines.is_empty() && !lines.last().unwrap().is_empty() {
                        lines.push(String::new());
                    }
                    lines.push(format!("## {heading}"));
                    for ml in moved_lines {
                        lines.push(ml.clone());
                    }
                }
            }
        }
    }
    crate::markdown::lines::join_with_newline(&lines)
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
}

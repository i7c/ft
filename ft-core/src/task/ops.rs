//! High-level task mutation primitives. Each entry point reads a file,
//! computes the new content, and writes atomically via `crate::fs::write_atomic`.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use thiserror::Error;

use super::{
    emoji::EmojiFormat,
    format::{ParseContext, TaskFormat},
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
fn build_task(input: &CreateInput) -> Task {
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
        done: None,
        cancelled: None,
        recurrence: input.recurrence.clone(),
        id: input.id.clone(),
        depends_on: input.depends_on.clone(),
        on_completion: None,
        block_link: None,
        raw_trailing: None,
        source_file: PathBuf::new(),
        source_line: 0,
        indent_level: 0,
        parent: None,
    }
}

/// Create a new task in `target_path`. The path must be absolute (the binary
/// resolves it against the vault root before calling).
pub fn create_task(
    target_path: &Path,
    input: CreateInput,
    opts: CreateOptions,
) -> Result<CreateOutcome, CreateError> {
    let task = build_task(&input);
    let serialized = EmojiFormat.serialize_line(&task);

    let existing = read_or_empty(target_path)?;

    if !opts.force {
        if let Some(line) = find_duplicate(&existing, &task) {
            return Err(CreateError::Duplicate {
                path: target_path.to_path_buf(),
                line,
            });
        }
    }

    let (new_content, line) = splice(&existing, &serialized, &opts.position)?;

    write_atomic(target_path, &new_content)?;
    Ok(CreateOutcome { line, serialized })
}

fn read_or_empty(path: &Path) -> Result<String, CreateError> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(CreateError::Read {
            path: path.to_path_buf(),
            source: e,
        }),
    }
}

/// Returns the 1-indexed line number of any existing task whose description,
/// due, scheduled, and start dates all match `task`. The status is ignored
/// (a done duplicate is still a duplicate).
fn find_duplicate(content: &str, task: &Task) -> Option<usize> {
    for (idx, line) in content.lines().enumerate() {
        let ctx = ParseContext {
            source_file: PathBuf::new(),
            source_line: idx + 1,
        };
        if let Some(existing) = EmojiFormat.parse_line(line, ctx) {
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
    let mut lines: Vec<String> = if content.is_empty() {
        Vec::new()
    } else {
        content
            .split_inclusive('\n')
            .map(|s| s.trim_end_matches('\n').trim_end_matches('\r').to_string())
            .collect()
    };

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
        Position::UnderHeading(heading) => match find_heading(&lines, heading) {
            Some((heading_idx, level)) => {
                let insert_at = section_end(&lines, heading_idx, level);
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
    };

    let mut joined = lines.join("\n");
    joined.push('\n');
    Ok((joined, inserted_at_idx + 1))
}

/// Find a heading by exact text match. Returns `(index, level)` where
/// `level` is the number of leading `#` characters.
fn find_heading(lines: &[String], target: &str) -> Option<(usize, usize)> {
    for (i, l) in lines.iter().enumerate() {
        if let Some((level, text)) = parse_heading(l) {
            if text == target {
                return Some((i, level));
            }
        }
    }
    None
}

fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.bytes().take_while(|b| *b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let after = &trimmed[hashes..];
    let after = after.strip_prefix(' ')?;
    Some((hashes, after.trim_end()))
}

/// Find the index *just after* the last line of the section opened by
/// `heading_idx` at `level`. The section ends at the next heading whose
/// level is `<= level`, or at the end of the file. Trailing blank lines
/// belong to the *next* section, not this one — we insert before them so
/// the heading visually owns its tasks.
fn section_end(lines: &[String], heading_idx: usize, level: usize) -> usize {
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(heading_idx + 1) {
        if let Some((lvl, _)) = parse_heading(l) {
            if lvl <= level {
                end = i;
                break;
            }
        }
    }
    // Walk back over trailing blank lines — but never cross the heading itself.
    while end > heading_idx + 1 && lines[end - 1].is_empty() {
        end -= 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn append_to_existing_file_preserves_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "# Notes\n\nSome prose.\n").unwrap();
        let outcome = create_task(
            &p,
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

    #[test]
    fn at_line_zero_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes.md");
        std::fs::write(&p, "line1\n").unwrap();
        let err = create_task(
            &p,
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
        assert_eq!(parse_heading("# Top"), Some((1, "Top")));
        assert_eq!(parse_heading("### Three"), Some((3, "Three")));
        assert_eq!(parse_heading("###### Six"), Some((6, "Six")));
        assert_eq!(parse_heading("####### Seven"), None);
        assert_eq!(parse_heading("not a heading"), None);
        assert_eq!(parse_heading("#NoSpace"), None);
    }
}

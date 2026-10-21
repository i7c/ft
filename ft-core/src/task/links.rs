//! Logical (cross-file) task parents expressed as wikilinks: a block
//! reference to a task (`[[note#^id]]`, `[[#^id]]`) or a link to an anchor
//! note (`[[note]]` whose frontmatter names its anchor task via
//! `ft.tasks.anchor`). Resolved once per scan into [`Task::parent_link`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::format::{ParseContext, TaskFormat};
use super::ops::{self, FileEdit, MoveSource};
use super::{Task, TaskKey};
use crate::fs::write_atomic;
use crate::graph::resolve::NoteLookup;
use crate::graph::resolve::{resolve_wiki, Resolution, ScanNoteIndex};
use crate::graph::LinkForm;
use crate::scan::Scan;

/// A task's parent link: the first qualifying wikilink on its line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParentLink {
    /// The link's raw text, e.g. `[[proj#^vo1]]`.
    pub raw: String,
    /// Raw texts of later qualifying links on the same line (ignored).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<String>,
    pub state: ParentLinkState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParentLinkState {
    /// Points at this task; it is the effective parent.
    Resolved(TaskKey),
    /// The link names a parent that doesn't exist (missing note, missing
    /// block id, anchor note without its anchor task).
    Dangling,
    /// The task is physically nested; indentation wins over the link.
    Ignored,
    /// Following the link would make the task its own ancestor.
    Cycle,
}

impl ParentLinkState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ParentLinkState::Resolved(_) => "resolved",
            ParentLinkState::Dangling => "dangling",
            ParentLinkState::Ignored => "ignored",
            ParentLinkState::Cycle => "cycle",
        }
    }
}

/// What a block id in a note marks.
#[derive(Clone, Copy)]
enum Block {
    Task(usize),
    Other,
}

/// How one link on a task line classifies.
enum Candidate {
    Resolved(usize),
    Dangling,
}

/// Vault-wide lookups for classifying parent-link candidates, built from a
/// scan without touching the filesystem. Everything is keyed by file index
/// (into `scan.files`) and task index (into `scan.tasks`) — hashing paths
/// per task would dominate the pass.
pub(crate) struct ParentIndex {
    pub(crate) notes: ScanNoteIndex,
    /// Per task: index of its file in `scan.files` (`usize::MAX` if none).
    task_file: Vec<usize>,
    /// (file idx, line) → task idx.
    task_at: HashMap<(usize, usize), usize>,
    /// (file idx, block id) → what it marks. First occurrence wins.
    blocks: HashMap<(usize, String), Block>,
    /// file idx → the anchor task (`None` when `ft.tasks.anchor` is set but
    /// no task carries that 🆔).
    anchors: HashMap<usize, Option<usize>>,
}

impl ParentIndex {
    pub(crate) fn build(scan: &Scan) -> Self {
        let notes = ScanNoteIndex::new(scan.files.iter().map(|f| f.rel.as_path()));
        let file_of: HashMap<&std::ffi::OsStr, usize> = scan
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| (f.rel.as_os_str(), i))
            .collect();
        let task_file: Vec<usize> = scan
            .tasks
            .iter()
            .map(|t| {
                file_of
                    .get(t.source_file.as_os_str())
                    .copied()
                    .unwrap_or(usize::MAX)
            })
            .collect();
        let task_at: HashMap<(usize, usize), usize> = scan
            .tasks
            .iter()
            .enumerate()
            .map(|(ti, t)| ((task_file[ti], t.source_line), ti))
            .collect();

        let mut blocks = HashMap::new();
        let mut anchors = HashMap::new();
        for (fi, f) in scan.files.iter().enumerate() {
            for (id, line) in &f.block_ids {
                let block = match task_at.get(&(fi, *line)) {
                    Some(&ti) if scan.tasks[ti].block_link.as_deref() == Some(id) => {
                        Block::Task(ti)
                    }
                    _ => Block::Other,
                };
                blocks.entry((fi, id.clone())).or_insert(block);
            }
            if let Some(anchor) = f
                .frontmatter
                .as_deref()
                .and_then(crate::frontmatter::ft_tasks_anchor_in)
            {
                let task = (0..scan.tasks.len()).find(|&ti| {
                    task_file[ti] == fi && scan.tasks[ti].id.as_deref() == Some(anchor.as_str())
                });
                anchors.insert(fi, task);
            }
        }
        Self {
            notes,
            task_file,
            task_at,
            blocks,
            anchors,
        }
    }

    /// Classify a wikilink written in file `from` (index into the scan's
    /// files). `None` means it is an ordinary link, not a parent candidate.
    fn classify(&self, from: usize, target: &str, anchor: Option<&str>) -> Option<Candidate> {
        let file = if target.trim().is_empty() {
            Some(from)
        } else {
            match resolve_wiki(target, &self.notes) {
                Resolution::Resolved(i) => Some(i),
                _ => None,
            }
        };
        match anchor {
            Some(a) => {
                let id = a.strip_prefix('^')?;
                let Some(file) = file else {
                    return Some(Candidate::Dangling);
                };
                match self.blocks.get(&(file, id.to_string())) {
                    Some(Block::Task(t)) => Some(Candidate::Resolved(*t)),
                    Some(Block::Other) => None,
                    None => Some(Candidate::Dangling),
                }
            }
            None => match self.anchors.get(&file?)? {
                Some(t) => Some(Candidate::Resolved(*t)),
                None => Some(Candidate::Dangling),
            },
        }
    }
}

/// Resolve every task's parent link in place. Runs once at the end of
/// [`crate::scan::scan_vault`]; linear in tasks plus task-line links, no I/O.
pub fn resolve_parent_links(scan: &mut Scan) {
    let index = ParentIndex::build(scan);
    let mut links_by_line: HashMap<(usize, usize), Vec<&crate::graph::parser::RawLink>> =
        HashMap::new();
    for (fi, f) in scan.files.iter().enumerate() {
        for l in f.links.iter().chain(&f.self_block_refs) {
            if l.form == LinkForm::WikiLink {
                links_by_line.entry((fi, l.line)).or_default().push(l);
            }
        }
    }
    for list in links_by_line.values_mut() {
        list.sort_by_key(|l| l.byte_range.start);
    }

    let n = scan.tasks.len();
    let mut resolved: Vec<Option<ParentLink>> = Vec::with_capacity(n);
    // Effective parent per task, by index: physical first, then the link.
    let mut parent: Vec<Option<usize>> = Vec::with_capacity(n);
    for (ti, task) in scan.tasks.iter().enumerate() {
        let fi = index.task_file[ti];
        let physical = task
            .parent
            .and_then(|line| index.task_at.get(&(fi, line)).copied());
        let mut link_parent = None;
        let link = links_by_line
            .get(&(fi, task.source_line))
            .and_then(|links| {
                let mut first: Option<(String, Candidate)> = None;
                let mut extra = Vec::new();
                for l in links {
                    let Some(c) = index.classify(fi, &l.target_text, l.anchor.as_deref()) else {
                        continue;
                    };
                    if first.is_none() {
                        first = Some((l.raw_text.clone(), c));
                    } else {
                        extra.push(l.raw_text.clone());
                    }
                }
                first.map(|(raw, c)| {
                    let state = match (task.parent, c) {
                        (Some(_), _) => ParentLinkState::Ignored,
                        (None, Candidate::Resolved(t)) => {
                            link_parent = Some(t);
                            let p = &scan.tasks[t];
                            ParentLinkState::Resolved((p.source_file.clone(), p.source_line))
                        }
                        (None, Candidate::Dangling) => ParentLinkState::Dangling,
                    };
                    ParentLink { raw, extra, state }
                })
            });
        resolved.push(link);
        parent.push(physical.or(link_parent));
    }
    for (task, link) in scan.tasks.iter_mut().zip(resolved) {
        task.parent_link = link;
    }
    break_cycles(&mut scan.tasks, &parent);
}

/// Mark one link per effective-parent cycle as [`ParentLinkState::Cycle`]:
/// the link-parented member with the greatest key. `parent` holds each
/// task's effective parent index.
fn break_cycles(tasks: &mut [Task], parent: &[Option<usize>]) {
    // 0 = unvisited, 1 = on the current walk, 2 = done.
    let mut color = vec![0u8; tasks.len()];
    let mut path = Vec::new();
    for start in 0..tasks.len() {
        if color[start] != 0 {
            continue;
        }
        path.clear();
        let mut cur = Some(start);
        while let Some(i) = cur {
            match color[i] {
                0 => {
                    color[i] = 1;
                    path.push(i);
                    cur = parent[i];
                }
                1 => {
                    let pos = path.iter().position(|&p| p == i).expect("on current walk");
                    let breaker = path[pos..]
                        .iter()
                        .copied()
                        .filter(|&m| tasks[m].parent.is_none())
                        .max_by(|&a, &b| {
                            (&tasks[a].source_file, tasks[a].source_line)
                                .cmp(&(&tasks[b].source_file, tasks[b].source_line))
                        })
                        .expect("a cycle contains at least one link edge");
                    if let Some(link) = tasks[breaker].parent_link.as_mut() {
                        link.state = ParentLinkState::Cycle;
                    }
                    break;
                }
                _ => break,
            }
        }
        for &i in &path {
            color[i] = 2;
        }
    }
}

// ── writing links ───────────────────────────────────────────────────────────

/// What a generated parent link points at.
#[derive(Debug, Clone, Copy)]
pub enum LinkAnchor<'a> {
    /// A task's block id: `[[note#^id]]`.
    Block(&'a str),
    /// An anchor note as a whole: `[[note]]`.
    Note,
}

/// The wikilink text for a parent link from a task in `from_rel` to a target
/// in `to_rel` (both vault-relative): `[[#^id]]` within one file, otherwise
/// the note's title when it is unique in the vault, else its path without
/// `.md`.
pub fn link_text<L: NoteLookup>(
    from_rel: &Path,
    to_rel: &Path,
    anchor: LinkAnchor<'_>,
    notes: &L,
) -> String {
    let same_file = from_rel == to_rel;
    let note = if same_file && matches!(anchor, LinkAnchor::Block(_)) {
        String::new()
    } else {
        let title = crate::graph::title_of(to_rel);
        if notes.by_title(&title).len() == 1 {
            title
        } else {
            to_rel
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/")
        }
    };
    match anchor {
        LinkAnchor::Block(id) => format!("[[{note}#^{id}]]"),
        LinkAnchor::Note => format!("[[{note}]]"),
    }
}

/// A fresh 6-character base-36 block id, unique among `existing`.
pub(crate) fn new_block_id(
    path: &Path,
    line: usize,
    description: &str,
    existing: &HashSet<String>,
) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    block_id_from_seed(path, line, description, nanos, existing)
}

fn block_id_from_seed(
    path: &Path,
    line: usize,
    description: &str,
    nanos: u128,
    existing: &HashSet<String>,
) -> String {
    use std::hash::{Hash, Hasher};
    (0u64..)
        .map(|counter| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (path, line, description, nanos, counter).hash(&mut h);
            let mut n = h.finish();
            (0..6)
                .map(|_| {
                    let d = (n % 36) as u32;
                    n /= 36;
                    char::from_digit(d, 36).expect("base-36 digit")
                })
                .collect::<String>()
        })
        .find(|id| !existing.contains(id))
        .expect("an unused id exists")
}

/// Remove `raw` (a link's text) from `line` along with one adjacent space.
/// Returns `None` when `raw` isn't on the line.
pub(crate) fn remove_link_text(line: &str, raw: &str) -> Option<String> {
    let at = line.find(raw)?;
    let (mut start, mut end) = (at, at + raw.len());
    if line[..start].ends_with(' ') {
        start -= 1;
    } else if line[end..].starts_with(' ') {
        end += 1;
    }
    Some(format!("{}{}", &line[..start], &line[end..]))
}

/// Insert ` {text}` right after the task's description on `line` (before
/// any emoji fields and block id), leaving every other byte untouched.
fn append_to_description(line: &str, text: &str, format: &dyn TaskFormat) -> Option<String> {
    let task = format.parse_line(line, ParseContext::default())?;
    let ws = ops::leading_ws(line).len();
    let start = ws + line[ws..].find("] ")? + 2;
    let rest = &line[start..];
    if !rest.starts_with(&task.description) {
        return None;
    }
    let at = start + task.description.len();
    Some(format!("{} {text}{}", &line[..at], &line[at..]))
}

#[derive(Debug, Error)]
pub enum LinkError {
    #[error("could not read {}: {source}", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("line {line} not found in {}", .path.display())]
    LineMissing { path: PathBuf, line: usize },
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
    #[error("can't link a task under its own subtask ({}:{})", .path.display(), .line)]
    WouldCycle { path: PathBuf, line: usize },
    #[error("{description} is nested under another task — outdent it first")]
    PhysicallyNested {
        path: PathBuf,
        line: usize,
        description: String,
    },
    #[error("task at {}:{} has no parent link", .path.display(), .line)]
    NoParentLink { path: PathBuf, line: usize },
    #[error("write failed: {source}")]
    Write {
        #[from]
        source: crate::error::Error,
    },
}

/// Vault context for planning link edits: the vault root (paths in
/// [`MoveSource`]s are absolute), a note index for link text, and the
/// scanned tasks for ancestor walks.
pub struct LinkCtx<'a> {
    pub root: PathBuf,
    pub notes: ScanNoteIndex,
    pub tasks: &'a [Task],
}

impl<'a> LinkCtx<'a> {
    pub fn new(root: &Path, scan: &'a Scan) -> Self {
        Self {
            root: root.to_path_buf(),
            notes: ScanNoteIndex::new(scan.files.iter().map(|f| f.rel.as_path())),
            tasks: &scan.tasks,
        }
    }

    fn rel(&self, abs: &Path) -> PathBuf {
        abs.strip_prefix(&self.root).unwrap_or(abs).to_path_buf()
    }
}

/// A child whose line a [`LinkPlan`] rewrites.
#[derive(Debug, Clone)]
pub struct LinkedChild {
    pub description: String,
    /// Absolute path and 1-indexed line (lines don't shift).
    pub path: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct LinkPlan {
    pub edits: Vec<FileEdit>,
    pub children: Vec<LinkedChild>,
    /// Absolute path + line of the parent (`None` for unlink plans).
    pub parent: Option<(PathBuf, usize)>,
}

/// Write every changed file in `plan` atomically.
pub fn apply_link_plan(plan: &LinkPlan) -> Result<(), LinkError> {
    for edit in &plan.edits {
        if edit.original != edit.new {
            write_atomic(&edit.path, &edit.new)?;
        }
    }
    Ok(())
}

/// File contents being edited by a plan, read once per file.
struct Files(BTreeMap<PathBuf, (String, Vec<String>)>);

impl Files {
    fn lines(&mut self, path: &Path) -> Result<&mut Vec<String>, LinkError> {
        if !self.0.contains_key(path) {
            let content =
                crate::markdown::lines::read_or_empty(path).map_err(|source| LinkError::Read {
                    path: path.to_path_buf(),
                    source,
                })?;
            let lines = crate::markdown::lines::split(&content);
            self.0.insert(path.to_path_buf(), (content, lines));
        }
        Ok(&mut self.0.get_mut(path).expect("just inserted").1)
    }

    fn into_edits(self) -> Vec<FileEdit> {
        self.0
            .into_iter()
            .map(|(path, (original, lines))| FileEdit {
                path,
                original,
                new: crate::markdown::lines::join_with_newline(&lines),
            })
            .collect()
    }
}

/// Parse the task at `line` of `lines`, checking it against `expected`.
fn guarded_task(
    path: &Path,
    lines: &[String],
    line: usize,
    expected: Option<&Task>,
    format: &dyn TaskFormat,
    is_target: bool,
) -> Result<Task, LinkError> {
    let raw = line
        .checked_sub(1)
        .and_then(|i| lines.get(i))
        .ok_or_else(|| LinkError::LineMissing {
            path: path.to_path_buf(),
            line,
        })?;
    let task = format
        .parse_line(raw, ParseContext::default())
        .ok_or_else(|| LinkError::NotATask {
            path: path.to_path_buf(),
            line,
        })?;
    ops::check_expected(format, &task, expected).map_err(|(expected, found)| {
        let path = path.to_path_buf();
        if is_target {
            LinkError::TargetChanged {
                path,
                line,
                expected,
                found,
            }
        } else {
            LinkError::LineChanged {
                path,
                line,
                expected,
                found,
            }
        }
    })?;
    Ok(task)
}

/// Whether the task at `line` is indented under another task in `lines`.
fn physically_nested(lines: &[String], line: usize, format: &dyn TaskFormat) -> bool {
    let mut tasks: Vec<Task> = lines
        .iter()
        .take(line)
        .enumerate()
        .filter_map(|(i, l)| {
            format.parse_line(
                l,
                ParseContext {
                    source_file: PathBuf::new(),
                    source_line: i + 1,
                },
            )
        })
        .collect();
    super::hierarchy::resolve_hierarchy(&mut tasks);
    tasks
        .last()
        .is_some_and(|t| t.source_line == line && t.parent.is_some())
}

/// Plan linking `children` under `parent`: give the parent a block id if it
/// needs one (anchor tasks of anchor notes are linked as `[[note]]`
/// instead), and append the link to each child's description, replacing
/// its previous parent link. Lines never move.
pub fn plan_link(
    children: &[MoveSource],
    parent: &MoveSource,
    ctx: &LinkCtx<'_>,
    format: &dyn TaskFormat,
) -> Result<LinkPlan, LinkError> {
    let mut files = Files(BTreeMap::new());
    let parent_rel = ctx.rel(&parent.path);

    let parent_lines = files.lines(&parent.path)?;
    let parent_task = guarded_task(
        &parent.path,
        parent_lines,
        parent.line,
        parent.expected.as_ref(),
        format,
        true,
    )?;
    let parent_content = crate::markdown::lines::join_with_newline(parent_lines);
    let is_anchor_task = parent_task.id.is_some()
        && crate::frontmatter::ft_tasks_anchor(&parent_content) == parent_task.id;

    let block_id = if is_anchor_task {
        None
    } else if let Some(id) = parent_task.block_link.clone() {
        Some(id)
    } else {
        let existing: HashSet<String> = parent_lines
            .iter()
            .filter_map(|l| crate::scan::trailing_block_id(l).map(str::to_string))
            .collect();
        let id = new_block_id(
            &parent_rel,
            parent.line,
            &parent_task.description,
            &existing,
        );
        let l = &mut parent_lines[parent.line - 1];
        *l = format!("{} ^{id}", l.trim_end());
        Some(id)
    };

    // The parent and its effective ancestors can't become children.
    let by_key: HashMap<TaskKey, &Task> = ctx
        .tasks
        .iter()
        .map(|t| ((t.source_file.clone(), t.source_line), t))
        .collect();
    let mut forbidden: HashSet<TaskKey> = HashSet::new();
    let mut cur = Some((parent_rel.clone(), parent.line));
    while let Some(k) = cur {
        if !forbidden.insert(k.clone()) {
            break;
        }
        cur = by_key.get(&k).and_then(|t| super::hierarchy::parent_key(t));
    }

    let mut linked = Vec::new();
    for child in children {
        let child_rel = ctx.rel(&child.path);
        if forbidden.contains(&(child_rel.clone(), child.line)) {
            return Err(LinkError::WouldCycle {
                path: child.path.clone(),
                line: child.line,
            });
        }
        let lines = files.lines(&child.path)?;
        let task = guarded_task(
            &child.path,
            lines,
            child.line,
            child.expected.as_ref(),
            format,
            false,
        )?;
        if physically_nested(lines, child.line, format) {
            return Err(LinkError::PhysicallyNested {
                path: child.path.clone(),
                line: child.line,
                description: task.description,
            });
        }
        let anchor = match &block_id {
            Some(id) => LinkAnchor::Block(id),
            None => LinkAnchor::Note,
        };
        let text = link_text(&child_rel, &parent_rel, anchor, &ctx.notes);
        let mut line = lines[child.line - 1].clone();
        if let Some(raw) = child
            .expected
            .as_ref()
            .and_then(|t| t.parent_link.as_ref())
            .map(|l| l.raw.as_str())
        {
            line = remove_link_text(&line, raw).unwrap_or(line);
        }
        let new_line =
            append_to_description(&line, &text, format).ok_or_else(|| LinkError::NotATask {
                path: child.path.clone(),
                line: child.line,
            })?;
        lines[child.line - 1] = new_line;
        linked.push(LinkedChild {
            description: task.description,
            path: child.path.clone(),
            line: child.line,
        });
    }

    Ok(LinkPlan {
        edits: files.into_edits(),
        children: linked,
        parent: Some((parent.path.clone(), parent.line)),
    })
}

/// Plan removing `task`'s parent link (as recorded in its scanned
/// `expected` task). The parent's block id stays.
pub fn plan_unlink(task: &MoveSource, format: &dyn TaskFormat) -> Result<LinkPlan, LinkError> {
    let no_link = || LinkError::NoParentLink {
        path: task.path.clone(),
        line: task.line,
    };
    let raw = task
        .expected
        .as_ref()
        .and_then(|t| t.parent_link.as_ref())
        .map(|l| l.raw.clone())
        .ok_or_else(no_link)?;
    let mut files = Files(BTreeMap::new());
    let lines = files.lines(&task.path)?;
    let parsed = guarded_task(
        &task.path,
        lines,
        task.line,
        task.expected.as_ref(),
        format,
        false,
    )?;
    let new_line = remove_link_text(&lines[task.line - 1], &raw).ok_or_else(no_link)?;
    lines[task.line - 1] = new_line;
    Ok(LinkPlan {
        edits: files.into_edits(),
        children: vec![LinkedChild {
            description: parsed.description,
            path: task.path.clone(),
            line: task.line,
        }],
        parent: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn scan(files: &[(&str, &str)]) -> (tempfile::TempDir, Scan) {
        let dir = tempfile::tempdir().unwrap();
        for (rel, body) in files {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        let s = crate::scan::scan_vault(dir.path(), &[]);
        (dir, s)
    }

    fn task<'a>(s: &'a Scan, rel: &str, line: usize) -> &'a Task {
        s.tasks
            .iter()
            .find(|t| t.source_file == Path::new(rel) && t.source_line == line)
            .unwrap_or_else(|| panic!("no task at {rel}:{line}"))
    }

    fn state(s: &Scan, rel: &str, line: usize) -> Option<ParentLinkState> {
        task(s, rel, line)
            .parent_link
            .as_ref()
            .map(|l| l.state.clone())
    }

    fn resolved(rel: &str, line: usize) -> Option<ParentLinkState> {
        Some(ParentLinkState::Resolved((PathBuf::from(rel), line)))
    }

    #[test]
    fn block_reference_across_files() {
        let (_d, s) = scan(&[
            ("proj.md", "- [ ] Vendor onboarding ^vo1\n"),
            (
                "journal/2026-10-21.md",
                "# Day\n- [ ] reproduce on staging [[proj#^vo1]]\n",
            ),
        ]);
        assert_eq!(
            state(&s, "journal/2026-10-21.md", 2),
            resolved("proj.md", 1)
        );
        let link = task(&s, "journal/2026-10-21.md", 2)
            .parent_link
            .clone()
            .unwrap();
        assert_eq!(link.raw, "[[proj#^vo1]]");
    }

    #[test]
    fn same_file_block_reference_and_embed() {
        let (_d, s) = scan(&[(
            "a.md",
            "- [ ] A ^a1\n\ntext\n\n- [ ] B [[#^a1]]\n- [ ] C ![[a#^a1]]\n",
        )]);
        assert_eq!(state(&s, "a.md", 5), resolved("a.md", 1));
        assert_eq!(state(&s, "a.md", 6), resolved("a.md", 1));
    }

    #[test]
    fn anchor_note() {
        let (_d, s) = scan(&[
            (
                "jira/ABC-123.md",
                "---\nft:\n  tasks:\n    anchor: ABC-123\n---\n- [ ] Fix login 🆔 ABC-123\n",
            ),
            (
                "j.md",
                "- [ ] write test [[ABC-123]]\n- [ ] alias [[ABC-123|the bug]]\n",
            ),
        ]);
        assert_eq!(state(&s, "j.md", 1), resolved("jira/ABC-123.md", 6));
        assert_eq!(state(&s, "j.md", 2), resolved("jira/ABC-123.md", 6));
    }

    #[test]
    fn non_candidates() {
        let (_d, s) = scan(&[
            ("Claude.md", "some note\n"),
            ("notes.md", "a paragraph ^p1\n## Head\n"),
            (
                "j.md",
                "- [ ] plain [[Claude]]\n- [ ] para [[notes#^p1]]\n- [ ] heading [[notes#Head]]\n- [ ] ghost [[Nope]]\n- [ ] md [x](notes.md)\n",
            ),
        ]);
        for line in 1..=5 {
            assert_eq!(state(&s, "j.md", line), None, "line {line}");
        }
    }

    #[test]
    fn dangling_variants() {
        let (_d, s) = scan(&[
            ("proj.md", "- [ ] P ^p1\n"),
            ("anchor.md", "---\nft:\n  tasks:\n    anchor: X-1\n---\n- [ ] no id here\n"),
            (
                "j.md",
                "- [ ] missing id [[proj#^gone]]\n- [ ] missing note [[ghost#^p1]]\n- [ ] anchor without task [[anchor]]\n",
            ),
        ]);
        for line in 1..=3 {
            assert_eq!(
                state(&s, "j.md", line),
                Some(ParentLinkState::Dangling),
                "line {line}"
            );
        }
    }

    #[test]
    fn first_candidate_wins_rest_are_extra() {
        let (_d, s) = scan(&[
            ("a.md", "- [ ] A ^x\n"),
            ("b.md", "- [ ] B ^y\n"),
            ("j.md", "- [ ] two [[Claude]] [[a#^x]] and [[b#^y]]\n"),
        ]);
        let link = task(&s, "j.md", 1).parent_link.clone().unwrap();
        assert_eq!(link.state, resolved("a.md", 1).unwrap());
        assert_eq!(link.extra, vec!["[[b#^y]]".to_string()]);
    }

    #[test]
    fn physically_nested_link_is_ignored() {
        let (_d, s) = scan(&[
            ("proj.md", "- [ ] P ^p1\n"),
            ("j.md", "- [ ] parent\n  - [ ] child [[proj#^p1]]\n"),
        ]);
        assert_eq!(state(&s, "j.md", 2), Some(ParentLinkState::Ignored));
        let child = task(&s, "j.md", 2);
        assert_eq!(
            super::super::hierarchy::parent_key(child),
            Some((PathBuf::from("j.md"), 1))
        );
    }

    #[test]
    fn two_cycle_breaks_at_greatest_key() {
        let (_d, s) = scan(&[
            ("a.md", "- [ ] A [[b#^b1]] ^a1\n"),
            ("b.md", "- [ ] B [[a#^a1]] ^b1\n"),
        ]);
        assert_eq!(state(&s, "a.md", 1), resolved("b.md", 1));
        assert_eq!(state(&s, "b.md", 1), Some(ParentLinkState::Cycle));
    }

    #[test]
    fn cycle_through_physical_edge_breaks_on_a_link() {
        // c.md:2 is physically under c.md:1 and links nothing; c.md:1 links
        // a.md:1, which links back to c.md:2 → cycle a → c2 → c1 → a.
        let (_d, s) = scan(&[
            ("a.md", "- [ ] A [[c#^c2]] ^a1\n"),
            ("c.md", "- [ ] C1 [[a#^a1]]\n  - [ ] C2 ^c2\n"),
        ]);
        // Greatest link-parented key among {a.md:1, c.md:1} is c.md:1.
        assert_eq!(state(&s, "c.md", 1), Some(ParentLinkState::Cycle));
        assert_eq!(state(&s, "a.md", 1), resolved("c.md", 2));
    }

    #[test]
    fn self_link_is_a_cycle() {
        let (_d, s) = scan(&[("a.md", "- [ ] A [[#^a1]] ^a1\n")]);
        assert_eq!(state(&s, "a.md", 1), Some(ParentLinkState::Cycle));
    }

    #[test]
    fn three_cycle_leaves_two_links_resolved() {
        let (_d, s) = scan(&[
            ("a.md", "- [ ] A [[b#^b1]] ^a1\n"),
            ("b.md", "- [ ] B [[c#^c1]] ^b1\n"),
            ("c.md", "- [ ] C [[a#^a1]] ^c1\n"),
        ]);
        assert_eq!(state(&s, "a.md", 1), resolved("b.md", 1));
        assert_eq!(state(&s, "b.md", 1), resolved("c.md", 1));
        assert_eq!(state(&s, "c.md", 1), Some(ParentLinkState::Cycle));
    }

    #[test]
    fn duplicate_block_id_first_occurrence_wins() {
        let (_d, s) = scan(&[
            ("p.md", "- [ ] first ^d\n- [ ] second ^d\n"),
            ("j.md", "- [ ] x [[p#^d]]\n"),
        ]);
        assert_eq!(state(&s, "j.md", 1), resolved("p.md", 1));
    }

    fn key(rel: &str, line: usize) -> TaskKey {
        (PathBuf::from(rel), line)
    }

    #[test]
    fn forest_nests_linked_children_physical_first() {
        use crate::task::hierarchy::{expand_forest, expand_forest_visible};
        let (_d, s) = scan(&[
            (
                "j.md",
                "- [ ] linked child [[proj#^p1]]
- [ ] unrelated
",
            ),
            (
                "proj.md",
                "- [ ] Parent ^p1
  - [ ] physical child
",
            ),
        ]);
        let rows = expand_forest(&s.tasks, &[key("proj.md", 1), key("j.md", 1)]);
        let shape: Vec<(String, usize)> = rows
            .iter()
            .map(|r| (s.tasks[r.idx].description.clone(), r.depth))
            .collect();
        assert_eq!(
            shape,
            vec![
                ("Parent".to_string(), 0),
                ("physical child".to_string(), 1),
                ("linked child [[proj#^p1]]".to_string(), 1),
            ]
        );
        // Child unmatched by the query still shows under its parent; the
        // linked child is not a second root when matched alone with it.
        let expanded = [key("proj.md", 1)].into_iter().collect();
        let vis = expand_forest_visible(&s.tasks, &[key("proj.md", 1)], &expanded);
        assert_eq!(vis.len(), 3);
        assert!(vis[0].has_children && vis[0].expanded);
    }

    #[test]
    fn graph_has_cross_file_subtask_edges_only_for_resolved_links() {
        use crate::graph::{EdgeKind, Graph};
        let (_d, s) = scan(&[
            (
                "proj.md",
                "- [ ] Parent ^p1
",
            ),
            (
                "j.md",
                "- [ ] linked [[proj#^p1]]
- [ ] dangling [[proj#^nope]]
- [ ] nest
  - [ ] ignored [[proj#^p1]]
",
            ),
        ]);
        let g = Graph::build(&s).unwrap();
        let parent = g.task_by_loc(Path::new("proj.md"), 1).unwrap();
        let kids: Vec<_> = g
            .outgoing(parent)
            .filter(|(_, e)| matches!(e, EdgeKind::Subtask))
            .map(|(id, _)| id)
            .collect();
        assert_eq!(kids, vec![g.task_by_loc(Path::new("j.md"), 1).unwrap()]);
        // The linked child still hangs off its own note.
        let note = g.note_by_path(Path::new("j.md")).unwrap();
        assert!(g
            .outgoing(note)
            .any(|(id, e)| matches!(e, EdgeKind::HasTask)
                && Some(id) == g.task_by_loc(Path::new("j.md"), 1)));
    }

    fn query(s: &Scan, dsl: &str) -> Vec<String> {
        use crate::graph::query::{parse_with, Profile};
        let g = crate::graph::Graph::build(s).unwrap();
        let q = parse_with(
            dsl,
            Profile::Tasks,
            chrono::NaiveDate::from_ymd_opt(2026, 10, 21).unwrap(),
        )
        .unwrap_or_else(|e| panic!("{dsl}: {e}"));
        let keys = crate::task::resolve::by_query(&g, &q);
        let mut out: Vec<String> = s
            .tasks
            .iter()
            .filter(|t| keys.contains(&(t.source_file.clone(), t.source_line)))
            .map(|t| t.description.split(" [[").next().unwrap().to_string())
            .collect();
        out.sort();
        out
    }

    #[test]
    fn hierarchy_dsl_attributes() {
        let (_d, s) = scan(&[
            (
                "proj.md",
                "- [x] Done parent ^dp
- [ ] Open parent ^op
  - [ ] physical kid
",
            ),
            (
                "j.md",
                "- [ ] orphan [[proj#^dp]]
- [ ] linked [[proj#^op]]
- [ ] broken [[proj#^zz]]
- [ ] top
",
            ),
        ]);
        assert_eq!(
            query(&s, "status = Open and parent_status in {Done, Cancelled}"),
            ["orphan"]
        );
        assert_eq!(
            query(&s, crate::query::preset::builtin("orphaned").unwrap()),
            ["orphan"]
        );
        assert_eq!(query(&s, "children > 0"), ["Done parent", "Open parent"]);
        assert_eq!(query(&s, "children = 2"), ["Open parent"]);
        assert_eq!(
            query(&s, "parent includes \"Open\""),
            ["linked", "physical kid"]
        );
        assert_eq!(query(&s, "parent_link = dangling"), ["broken"]);
        assert_eq!(
            query(&s, crate::query::preset::builtin("broken-links").unwrap()),
            ["broken"]
        );
        assert_eq!(
            query(&s, "parent is null and status = Open"),
            ["Open parent", "broken", "top"]
        );
        assert_eq!(
            query(&s, "parent_link is null and status = Open"),
            ["Open parent", "physical kid", "top"]
        );
    }

    #[test]
    fn hierarchy_attrs_rejected_on_edges() {
        use crate::graph::query::{parse_with, Profile};
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        let src = "node where kind = Task; expand where edge.children > 0;";
        assert!(parse_with(src, Profile::Default, today).is_err());
    }

    // ── plan_link / plan_unlink ──────────────────────────────────────

    use crate::task::emoji::EmojiFormat;

    fn src_of(dir: &Path, s: &Scan, rel: &str, line: usize) -> MoveSource {
        MoveSource {
            path: dir.join(rel),
            line,
            expected: Some(task(s, rel, line).clone()),
        }
    }

    fn read(dir: &Path, rel: &str) -> String {
        std::fs::read_to_string(dir.join(rel)).unwrap()
    }

    fn link(
        dir: &Path,
        s: &Scan,
        children: &[(&str, usize)],
        parent: (&str, usize),
    ) -> Result<LinkPlan, LinkError> {
        let ctx = LinkCtx::new(dir, s);
        let kids: Vec<MoveSource> = children
            .iter()
            .map(|(r, l)| src_of(dir, s, r, *l))
            .collect();
        let plan = plan_link(
            &kids,
            &src_of(dir, s, parent.0, parent.1),
            &ctx,
            &EmojiFormat,
        )?;
        apply_link_plan(&plan)?;
        Ok(plan)
    }

    fn block_of(line: &str) -> String {
        crate::scan::trailing_block_id(line).unwrap().to_string()
    }

    #[test]
    fn link_journal_task_under_project_task() {
        let (d, s) = scan(&[
            ("proj.md", "- [ ] Vendor onboarding\n"),
            (
                "journal/2026-10-21.md",
                "# Day\n\n- [ ] reproduce 📅 2026-10-23\n",
            ),
        ]);
        let plan = link(
            d.path(),
            &s,
            &[("journal/2026-10-21.md", 3)],
            ("proj.md", 1),
        )
        .unwrap();
        let proj = read(d.path(), "proj.md");
        let id = block_of(proj.lines().next().unwrap());
        assert_eq!(id.len(), 6);
        assert_eq!(
            read(d.path(), "journal/2026-10-21.md"),
            format!("# Day\n\n- [ ] reproduce [[proj#^{id}]] 📅 2026-10-23\n")
        );
        assert_eq!(plan.children.len(), 1);
        // A rescan resolves the new link.
        let s2 = crate::scan::scan_vault(d.path(), &[]);
        assert_eq!(
            state(&s2, "journal/2026-10-21.md", 3),
            resolved("proj.md", 1)
        );
    }

    #[test]
    fn link_reuses_existing_block_id_and_same_file_form() {
        let (d, s) = scan(&[("a.md", "- [ ] A ^a1\n- [ ] B\n- [ ] C\n")]);
        link(d.path(), &s, &[("a.md", 2), ("a.md", 3)], ("a.md", 1)).unwrap();
        assert_eq!(
            read(d.path(), "a.md"),
            "- [ ] A ^a1\n- [ ] B [[#^a1]]\n- [ ] C [[#^a1]]\n"
        );
    }

    #[test]
    fn relink_replaces_old_link() {
        let (d, s) = scan(&[
            ("old.md", "- [ ] Old ^o1\n"),
            ("new.md", "- [ ] New ^n1\n"),
            ("j.md", "- [ ] kid [[old#^o1]] see [[Claude]] ⏫\n"),
        ]);
        link(d.path(), &s, &[("j.md", 1)], ("new.md", 1)).unwrap();
        assert_eq!(
            read(d.path(), "j.md"),
            "- [ ] kid see [[Claude]] [[new#^n1]] ⏫\n"
        );
    }

    #[test]
    fn ambiguous_title_uses_path_form() {
        let (d, s) = scan(&[
            ("a/proj.md", "- [ ] A ^x\n"),
            ("b/proj.md", "- [ ] B ^y\n"),
            ("j.md", "- [ ] kid\n"),
        ]);
        link(d.path(), &s, &[("j.md", 1)], ("b/proj.md", 1)).unwrap();
        assert_eq!(read(d.path(), "j.md"), "- [ ] kid [[b/proj#^y]]\n");
    }

    #[test]
    fn anchor_task_gets_note_link_and_no_block_id() {
        let anchor = "---\nft:\n  tasks:\n    anchor: ABC-123\n---\n- [ ] Fix login 🆔 ABC-123\n";
        let (d, s) = scan(&[("jira/ABC-123.md", anchor), ("j.md", "- [ ] write test\n")]);
        link(d.path(), &s, &[("j.md", 1)], ("jira/ABC-123.md", 6)).unwrap();
        assert_eq!(read(d.path(), "jira/ABC-123.md"), anchor);
        assert_eq!(read(d.path(), "j.md"), "- [ ] write test [[ABC-123]]\n");
    }

    #[test]
    fn link_refuses_cycles_and_nested_children() {
        let (d, s) = scan(&[
            ("p.md", "- [ ] P ^p1\n"),
            ("c.md", "- [ ] C [[p#^p1]] ^c1\n- [ ] Q\n  - [ ] nested\n"),
        ]);
        let err = link(d.path(), &s, &[("p.md", 1)], ("c.md", 1)).unwrap_err();
        assert!(matches!(err, LinkError::WouldCycle { .. }), "{err:?}");
        let err = link(d.path(), &s, &[("p.md", 1)], ("p.md", 1)).unwrap_err();
        assert!(matches!(err, LinkError::WouldCycle { .. }), "{err:?}");
        let err = link(d.path(), &s, &[("c.md", 3)], ("p.md", 1)).unwrap_err();
        assert!(matches!(err, LinkError::PhysicallyNested { .. }), "{err:?}");
        assert_eq!(
            read(d.path(), "c.md"),
            "- [ ] C [[p#^p1]] ^c1\n- [ ] Q\n  - [ ] nested\n"
        );
    }

    #[test]
    fn link_guards_stale_lines() {
        let (d, s) = scan(&[("p.md", "- [ ] P\n"), ("j.md", "- [ ] kid\n")]);
        std::fs::write(d.path().join("p.md"), "- [ ] P edited\n").unwrap();
        let err = link(d.path(), &s, &[("j.md", 1)], ("p.md", 1)).unwrap_err();
        assert!(matches!(err, LinkError::TargetChanged { .. }), "{err:?}");
        std::fs::write(d.path().join("p.md"), "- [ ] P\n").unwrap();
        std::fs::write(d.path().join("j.md"), "- [ ] kid edited\n").unwrap();
        let err = link(d.path(), &s, &[("j.md", 1)], ("p.md", 1)).unwrap_err();
        assert!(matches!(err, LinkError::LineChanged { .. }), "{err:?}");
    }

    #[test]
    fn unlink_removes_link_and_keeps_block_id() {
        let (d, s) = scan(&[
            ("proj.md", "- [ ] P ^vo1\n"),
            (
                "j.md",
                "- [ ] reproduce [[proj#^vo1]] 📅 2026-10-23\n- [ ] plain\n",
            ),
        ]);
        let plan = plan_unlink(&src_of(d.path(), &s, "j.md", 1), &EmojiFormat).unwrap();
        apply_link_plan(&plan).unwrap();
        assert_eq!(
            read(d.path(), "j.md"),
            "- [ ] reproduce 📅 2026-10-23\n- [ ] plain\n"
        );
        assert_eq!(read(d.path(), "proj.md"), "- [ ] P ^vo1\n");
        let err = plan_unlink(&src_of(d.path(), &s, "j.md", 2), &EmojiFormat).unwrap_err();
        assert!(matches!(err, LinkError::NoParentLink { .. }), "{err:?}");
    }

    #[test]
    fn block_ids_avoid_collisions() {
        let p = Path::new("p.md");
        let first = block_id_from_seed(p, 1, "x", 42, &HashSet::new());
        let taken: HashSet<String> = [first.clone()].into_iter().collect();
        let second = block_id_from_seed(p, 1, "x", 42, &taken);
        assert_ne!(first, second);
        assert!(second.len() == 6 && second.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    /// Gated perf budget: the post-pass stays a small fraction of a scan.
    /// Run with `FT_PERF_TESTS=1 cargo test --release …` — unoptimized
    /// builds skew the serial pass against the parallel scan.
    #[test]
    fn post_pass_is_cheap_relative_to_scan() {
        if std::env::var("FT_PERF_TESTS").as_deref() != Ok("1") || cfg!(debug_assertions) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        for i in 0..2000usize {
            let parent = (i + 1) % 2000;
            let body = format!(
                "# Note {i}\n\nSome prose [[note-{parent}]].\n\n- [ ] task a{i} ^a{i}\n  - [ ] sub\n- [ ] linked [[note-{parent}#^a{parent}]]\n- [ ] plain [[note-{parent}]]\n"
            );
            std::fs::write(dir.path().join(format!("note-{i}.md")), body).unwrap();
        }
        let start = std::time::Instant::now();
        let mut s = crate::scan::scan_vault(dir.path(), &[]);
        let scan_time = start.elapsed();
        let start = std::time::Instant::now();
        resolve_parent_links(&mut s);
        let pass_time = start.elapsed();
        eprintln!("post-pass {pass_time:?} vs scan {scan_time:?}");
        assert!(
            pass_time * 10 < scan_time,
            "post-pass {pass_time:?} vs scan {scan_time:?}"
        );
        assert!(s
            .tasks
            .iter()
            .filter(|t| t.description.starts_with("linked"))
            .all(|t| matches!(
                t.parent_link.as_ref().map(|l| &l.state),
                Some(ParentLinkState::Resolved(_))
            )));
    }
}

//! Logical (cross-file) task parents: a task names any number of parents
//! by their `🆔` in a `⤴ id,id` field ([`Task::parent_ids`]), resolved once
//! per scan through a vault-wide id index into [`Task::parent_refs`].
//! Also home to block-reference hygiene on moves ([`plan_retarget`]).

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

/// One logical parent reference of a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParentRef {
    /// The referenced `🆔`.
    pub id: String,
    pub origin: RefOrigin,
    pub state: RefState,
}

/// Where a reference was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefOrigin {
    /// The task line's `⤴` field.
    Line,
    /// The anchor note's `ft.tasks.parent` (sync-owned).
    Frontmatter,
}

impl RefOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            RefOrigin::Line => "line",
            RefOrigin::Frontmatter => "frontmatter",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefState {
    /// Exactly one task carries the id; it is an effective parent.
    Resolved(TaskKey),
    /// No task carries the id.
    Dangling,
    /// More than one task carries the id.
    Ambiguous,
    /// Following the reference would make the task its own ancestor.
    Cycle,
}

impl RefState {
    pub fn kind(&self) -> RefStateKind {
        match self {
            RefState::Resolved(_) => RefStateKind::Resolved,
            RefState::Dangling => RefStateKind::Dangling,
            RefState::Ambiguous => RefStateKind::Ambiguous,
            RefState::Cycle => RefStateKind::Cycle,
        }
    }

    pub fn as_str(&self) -> &'static str {
        self.kind().as_str()
    }

    pub fn resolved(&self) -> Option<&TaskKey> {
        match self {
            RefState::Resolved(k) => Some(k),
            _ => None,
        }
    }
}

/// [`RefState`] without its payload — what graph nodes carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefStateKind {
    Resolved,
    Dangling,
    Ambiguous,
    Cycle,
}

impl RefStateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RefStateKind::Resolved => "resolved",
            RefStateKind::Dangling => "dangling",
            RefStateKind::Ambiguous => "ambiguous",
            RefStateKind::Cycle => "cycle",
        }
    }
}

/// The set of [`RefStateKind`]s among a task's references, in one byte —
/// what graph nodes carry (`NodeKind` stays within the size lint).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RefKinds(u8);

impl RefKinds {
    const ALL: [RefStateKind; 4] = [
        RefStateKind::Resolved,
        RefStateKind::Dangling,
        RefStateKind::Ambiguous,
        RefStateKind::Cycle,
    ];

    pub fn insert(&mut self, k: RefStateKind) {
        self.0 |= 1 << k as u8;
    }

    pub fn contains(self, k: RefStateKind) -> bool {
        self.0 & (1 << k as u8) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = RefStateKind> {
        Self::ALL.into_iter().filter(move |&k| self.contains(k))
    }
}

impl FromIterator<RefStateKind> for RefKinds {
    fn from_iter<I: IntoIterator<Item = RefStateKind>>(iter: I) -> Self {
        let mut set = RefKinds::default();
        for k in iter {
            set.insert(k);
        }
        set
    }
}

/// What a `🆔` names across the vault.
#[derive(Clone, Copy)]
enum IdHit {
    One(usize),
    Many,
}

fn id_index(tasks: &[Task]) -> HashMap<&str, IdHit> {
    let mut ids: HashMap<&str, IdHit> = HashMap::new();
    for (ti, t) in tasks.iter().enumerate() {
        if let Some(id) = t.id.as_deref() {
            ids.entry(id)
                .and_modify(|h| *h = IdHit::Many)
                .or_insert(IdHit::One(ti));
        }
    }
    ids
}

/// Resolve every task's parent references in place. Runs once at the end
/// of [`crate::scan::scan_vault`]; linear in tasks plus references, no I/O.
pub fn resolve_parent_refs(scan: &mut Scan) {
    let n = scan.tasks.len();
    // Everything is keyed by file index and task index — hashing paths per
    // task would dominate the pass.
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
    let task_at: HashMap<(usize, usize), usize> = (0..n)
        .map(|ti| ((task_file[ti], scan.tasks[ti].source_line), ti))
        .collect();
    let ids = id_index(&scan.tasks);

    // Anchor task idx → its note's `ft.tasks.parent` id.
    let mut fm_parent: HashMap<usize, String> = HashMap::new();
    for (fi, f) in scan.files.iter().enumerate() {
        let Some(fm) = f.frontmatter.as_deref() else {
            continue;
        };
        let (Some(anchor), Some(parent)) = (
            crate::frontmatter::ft_tasks_anchor_in(fm),
            crate::frontmatter::ft_tasks_parent_in(fm),
        ) else {
            continue;
        };
        if let Some(&IdHit::One(ti)) = ids.get(anchor.as_str()) {
            if task_file[ti] == fi {
                fm_parent.insert(ti, parent);
            }
        }
    }

    let key = |ti: usize| -> TaskKey {
        let t = &scan.tasks[ti];
        (t.source_file.clone(), t.source_line)
    };
    let mut refs: Vec<Vec<ParentRef>> = Vec::with_capacity(n);
    // Effective parents per task, by index: physical first, then references.
    let mut parents: Vec<Vec<usize>> = Vec::with_capacity(n);
    // Per task: whether its first parent is the physical one.
    let mut physical: Vec<Option<usize>> = Vec::with_capacity(n);
    for (ti, task) in scan.tasks.iter().enumerate() {
        let phys = task
            .parent
            .and_then(|line| task_at.get(&(task_file[ti], line)).copied());
        let mut out: Vec<ParentRef> = Vec::new();
        let mut ps: Vec<usize> = phys.into_iter().collect();
        let line_ids = task.parent_ids.iter().map(|id| (id, RefOrigin::Line));
        let fm_id = fm_parent.get(&ti).map(|id| (id, RefOrigin::Frontmatter));
        for (id, origin) in line_ids.chain(fm_id) {
            if out.iter().any(|r| &r.id == id) {
                continue;
            }
            let state = match ids.get(id.as_str()) {
                Some(IdHit::One(p)) if *p == ti => RefState::Cycle,
                Some(IdHit::One(p)) => {
                    if !ps.contains(p) {
                        ps.push(*p);
                    }
                    RefState::Resolved(key(*p))
                }
                Some(IdHit::Many) => RefState::Ambiguous,
                None => RefState::Dangling,
            };
            out.push(ParentRef {
                id: id.clone(),
                origin,
                state,
            });
        }
        refs.push(out);
        parents.push(ps);
        physical.push(phys);
    }
    break_cycles(&scan.tasks, &mut refs, &mut parents, &physical);
    for (task, r) in scan.tasks.iter_mut().zip(refs) {
        task.parent_refs = r;
    }
}

/// Mark references as [`RefState::Cycle`] until the effective-parent graph
/// is acyclic: per cycle found, the logical edge held by the member with the
/// greatest key. Physical edges alone can't cycle, so one always exists.
fn break_cycles(
    tasks: &[Task],
    refs: &mut [Vec<ParentRef>],
    parents: &mut [Vec<usize>],
    physical: &[Option<usize>],
) {
    while let Some(cycle) = find_cycle(parents) {
        // `cycle[i]`'s parent on the cycle is `cycle[i + 1]` (wrapping).
        let (m, p) = (0..cycle.len())
            .map(|i| (cycle[i], cycle[(i + 1) % cycle.len()]))
            .filter(|&(m, p)| physical[m] != Some(p))
            .max_by(|&(a, _), &(b, _)| {
                (&tasks[a].source_file, tasks[a].source_line)
                    .cmp(&(&tasks[b].source_file, tasks[b].source_line))
            })
            .expect("a cycle contains at least one logical edge");
        parents[m].retain(|&x| x != p);
        let pk = (tasks[p].source_file.clone(), tasks[p].source_line);
        for r in &mut refs[m] {
            if r.state.resolved() == Some(&pk) {
                r.state = RefState::Cycle;
            }
        }
    }
}

/// One cycle in the child → parents graph, as a path whose last element's
/// parent is the first; `None` when acyclic. Iterative three-colour DFS.
fn find_cycle(parents: &[Vec<usize>]) -> Option<Vec<usize>> {
    // 0 = unvisited, 1 = on the stack, 2 = done.
    let mut color = vec![0u8; parents.len()];
    for start in 0..parents.len() {
        if color[start] != 0 {
            continue;
        }
        // (node, next parent position to explore)
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        color[start] = 1;
        while let Some(top) = stack.last_mut() {
            let node = top.0;
            if let Some(&p) = parents[node].get(top.1) {
                top.1 += 1;
                match color[p] {
                    0 => {
                        color[p] = 1;
                        stack.push((p, 0));
                    }
                    1 => {
                        let from = stack.iter().position(|&(x, _)| x == p).expect("on stack");
                        return Some(stack[from..].iter().map(|&(x, _)| x).collect());
                    }
                    _ => {}
                }
            } else {
                color[node] = 2;
                stack.pop();
            }
        }
    }
    None
}

// ── writing references ──────────────────────────────────────────────────────

/// The wikilink text for a block reference from a note in `from_rel` to
/// block `id` in `to_rel` (both vault-relative): `[[#^id]]` within one
/// file, otherwise the note's title when it is unique in the vault, else
/// its path without `.md`.
pub fn link_text<L: NoteLookup>(from_rel: &Path, to_rel: &Path, id: &str, notes: &L) -> String {
    format!("[[{}#^{id}]]", note_part(from_rel, to_rel, notes))
}

/// The target part of a block reference (before `#`) per [`link_text`].
fn note_part<L: NoteLookup>(from_rel: &Path, to_rel: &Path, notes: &L) -> String {
    if from_rel == to_rel {
        return String::new();
    }
    let title = crate::graph::title_of(to_rel);
    if notes.by_title(&title).len() == 1 {
        title
    } else {
        to_rel
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/")
    }
}

/// A fresh 6-character base-36 task id, absent from `taken`.
pub fn new_task_id(path: &Path, line: usize, description: &str, taken: &HashSet<String>) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    id_from_seed(path, line, description, nanos, taken)
}

fn id_from_seed(
    path: &Path,
    line: usize,
    description: &str,
    nanos: u128,
    taken: &HashSet<String>,
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
        .find(|id| !taken.contains(id))
        .expect("an unused id exists")
}

/// `line` with its task rewritten by `edit` through the format, keeping the
/// line's own leading whitespace (tabs stay tabs). `None` if not a task.
pub fn rewrite_task_line(
    line: &str,
    format: &dyn TaskFormat,
    edit: impl FnOnce(&mut Task),
) -> Option<String> {
    let mut task = format.parse_line(line, ParseContext::default())?;
    edit(&mut task);
    task.indent_level = 0;
    Some(format!(
        "{}{}",
        ops::leading_ws(line),
        format.serialize_line(&task)
    ))
}

/// `line` without `id` in its `⤴` field; `None` when the id isn't listed.
pub fn remove_parent_id(line: &str, id: &str, format: &dyn TaskFormat) -> Option<String> {
    let task = format.parse_line(line, ParseContext::default())?;
    if !task.parent_ids.iter().any(|p| p == id) {
        return None;
    }
    rewrite_task_line(line, format, |t| t.parent_ids.retain(|p| p != id))
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
    #[error("{description} is already nested under it")]
    AlreadyPhysicalParent {
        path: PathBuf,
        line: usize,
        description: String,
    },
    #[error("task at {}:{} has no parent link", .path.display(), .line)]
    NoParentLink { path: PathBuf, line: usize },
    #[error("task at {}:{} has several parents — choose one (or all)", .path.display(), .line)]
    NeedsChoice {
        path: PathBuf,
        line: usize,
        refs: Vec<ParentRef>,
    },
    #[error("task at {}:{} is not linked to that parent", .path.display(), .line)]
    NotLinkedTo { path: PathBuf, line: usize },
    #[error("parent comes from the note's ft.tasks.parent — edit the issue in its source")]
    ParentFromFrontmatter { path: PathBuf, line: usize },
    #[error("write failed: {0}")]
    Write(#[from] crate::error::Error),
}

/// Vault context for planning link edits: the vault root (paths in
/// [`MoveSource`]s are absolute), a note index for block-reference text,
/// and the scanned tasks for ancestor walks and id uniqueness.
pub struct LinkCtx<'a> {
    pub root: PathBuf,
    pub notes: ScanNoteIndex,
    pub tasks: &'a [Task],
    pub files: &'a [crate::scan::ParsedFile],
}

impl<'a> LinkCtx<'a> {
    pub fn new(root: &Path, scan: &'a Scan) -> Self {
        Self {
            root: root.to_path_buf(),
            notes: ScanNoteIndex::new(scan.files.iter().map(|f| f.rel.as_path())),
            tasks: &scan.tasks,
            files: &scan.files,
        }
    }

    /// This context, with `rel` (a note about to be created) known to the
    /// note index so links to it get the short title form.
    pub fn including(mut self, rel: &Path) -> Self {
        let rels: Vec<&Path> = self
            .files
            .iter()
            .map(|f| f.rel.as_path())
            .chain(std::iter::once(rel))
            .collect();
        self.notes = ScanNoteIndex::new(rels);
        self
    }

    /// The vault-relative file a wikilink written in `from` points at.
    fn target_of(&self, from: &Path, target: &str) -> Option<PathBuf> {
        if target.trim().is_empty() {
            return Some(from.to_path_buf());
        }
        match resolve_wiki(target, &self.notes) {
            Resolution::Resolved(i) => self.notes.path_of(i).map(Path::to_path_buf),
            _ => None,
        }
    }

    fn rel(&self, abs: &Path) -> PathBuf {
        abs.strip_prefix(&self.root).unwrap_or(abs).to_path_buf()
    }

    /// Every `🆔` in the vault — new ids must avoid these.
    pub fn taken_ids(&self) -> HashSet<String> {
        self.tasks.iter().filter_map(|t| t.id.clone()).collect()
    }
}

/// A child whose line a [`LinkPlan`] rewrites (or, `unchanged`, would have).
#[derive(Debug, Clone)]
pub struct LinkedChild {
    pub description: String,
    /// Absolute path and 1-indexed line (lines don't shift).
    pub path: PathBuf,
    pub line: usize,
    /// Already linked to the parent; nothing written for it.
    pub unchanged: bool,
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
    fn new() -> Self {
        Files(BTreeMap::new())
    }

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

/// The 1-indexed line of the task's physical parent in `lines`, if nested.
fn physical_parent_line(lines: &[String], line: usize, format: &dyn TaskFormat) -> Option<usize> {
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
        .filter(|t| t.source_line == line)
        .and_then(|t| t.parent)
}

/// Validate `parent` and return its `🆔`, generating one (unique against
/// `taken`, which gains it) and writing it onto the parent line in `files`
/// when it has none.
fn prepare_parent(
    files: &mut Files,
    parent: &MoveSource,
    ctx: &LinkCtx<'_>,
    format: &dyn TaskFormat,
    taken: &mut HashSet<String>,
) -> Result<String, LinkError> {
    let lines = files.lines(&parent.path)?;
    let task = guarded_task(
        &parent.path,
        lines,
        parent.line,
        parent.expected.as_ref(),
        format,
        true,
    )?;
    if let Some(id) = task.id {
        return Ok(id);
    }
    let id = new_task_id(
        &ctx.rel(&parent.path),
        parent.line,
        &task.description,
        taken,
    );
    taken.insert(id.clone());
    let l = &mut lines[parent.line - 1];
    *l = rewrite_task_line(l, format, |t| t.id = Some(id.clone())).ok_or_else(|| {
        LinkError::NotATask {
            path: parent.path.clone(),
            line: parent.line,
        }
    })?;
    Ok(id)
}

/// For a task about to be created: the `🆔`s of `parents` (in order) to put
/// in its `⤴` field, plus a plan with the parent-side edits (new ids), if
/// any. Apply the plan before creating the task.
pub fn plan_parents_for_new_task(
    parents: &[MoveSource],
    ctx: &LinkCtx<'_>,
    format: &dyn TaskFormat,
) -> Result<(Vec<String>, LinkPlan), LinkError> {
    let mut files = Files::new();
    let mut taken = ctx.taken_ids();
    let mut ids = Vec::new();
    for parent in parents {
        let id = prepare_parent(&mut files, parent, ctx, format, &mut taken)?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let plan = LinkPlan {
        edits: files.into_edits(),
        children: Vec::new(),
        parent: parents.first().map(|p| (p.path.clone(), p.line)),
    };
    Ok((ids, plan))
}

/// Plan linking `children` under `parent`: give the parent a `🆔` if it has
/// none, and add it to each child's `⤴` field, keeping the child's other
/// parents. Children already listing it are reported `unchanged`. Lines
/// never move.
pub fn plan_link(
    children: &[MoveSource],
    parent: &MoveSource,
    ctx: &LinkCtx<'_>,
    format: &dyn TaskFormat,
) -> Result<LinkPlan, LinkError> {
    let mut files = Files::new();
    let parent_rel = ctx.rel(&parent.path);
    let mut taken = ctx.taken_ids();
    let parent_id = prepare_parent(&mut files, parent, ctx, format, &mut taken)?;

    // The parent and its effective ancestors can't become children.
    let by_key: HashMap<TaskKey, &Task> = ctx
        .tasks
        .iter()
        .map(|t| ((t.source_file.clone(), t.source_line), t))
        .collect();
    let mut forbidden: HashSet<TaskKey> = HashSet::new();
    let mut todo = vec![(parent_rel.clone(), parent.line)];
    while let Some(k) = todo.pop() {
        if forbidden.insert(k.clone()) {
            if let Some(t) = by_key.get(&k) {
                todo.extend(super::hierarchy::parent_keys(t));
            }
        }
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
        if child.path == parent.path
            && physical_parent_line(lines, child.line, format) == Some(parent.line)
        {
            return Err(LinkError::AlreadyPhysicalParent {
                path: child.path.clone(),
                line: child.line,
                description: task.description,
            });
        }
        let unchanged = task.parent_ids.contains(&parent_id);
        if !unchanged {
            let l = &mut lines[child.line - 1];
            *l = rewrite_task_line(l, format, |t| t.parent_ids.push(parent_id.clone()))
                .ok_or_else(|| LinkError::NotATask {
                    path: child.path.clone(),
                    line: child.line,
                })?;
        }
        linked.push(LinkedChild {
            description: task.description,
            path: child.path.clone(),
            line: child.line,
            unchanged,
        });
    }

    Ok(LinkPlan {
        edits: files.into_edits(),
        children: linked,
        parent: Some((parent.path.clone(), parent.line)),
    })
}

/// Extend a move plan so block references survive cross-file moves:
/// links anywhere that pointed at a block id carried by a moved line now
/// name the target file; same-file references (`[[#^id]]`) inside moved
/// blocks whose target stayed behind become explicit; explicit references
/// inside moved blocks that now point into their own file collapse to
/// `[[#^id]]`. Works on the plan's post-move content. Returns the number of
/// links rewritten.
pub fn plan_retarget(plan: &mut ops::MovePlan, ctx: &LinkCtx<'_>) -> Result<usize, LinkError> {
    // (source rel, id) → target rel, for every block id on a moved line.
    let mut moved_ids: HashMap<(PathBuf, String), PathBuf> = HashMap::new();
    // Moved-in line ranges: (target rel, first line, last line, source rel).
    let mut moved_in: Vec<(PathBuf, usize, usize, PathBuf)> = Vec::new();
    for block in &plan.blocks {
        let Some((dest_abs, dest_line)) = &block.dest else {
            continue;
        };
        let (src, dst) = (ctx.rel(&block.source.path), ctx.rel(dest_abs));
        if src == dst {
            continue;
        }
        let len = block.end_line - block.source.line + 1;
        moved_in.push((dst.clone(), *dest_line, dest_line + len - 1, src.clone()));
        let Some(original) = plan
            .edits
            .iter()
            .find(|e| e.path == block.source.path)
            .map(|e| e.original.as_str())
        else {
            continue;
        };
        for l in original.lines().skip(block.source.line - 1).take(len) {
            if let Some(id) = crate::scan::trailing_block_id(l) {
                moved_ids.insert((src.clone(), id.to_string()), dst.clone());
            }
        }
    }
    if moved_in.is_empty() {
        return Ok(0);
    }

    // Files to revisit: those with a scanned link at a moved id, plus every
    // file the plan touches (moved blocks carry their own links along).
    let mut candidates: Vec<PathBuf> = plan.edits.iter().map(|e| ctx.rel(&e.path)).collect();
    for f in ctx.files {
        let hits = f.links.iter().chain(&f.self_block_refs).any(|l| {
            l.anchor
                .as_deref()
                .and_then(|a| a.strip_prefix('^'))
                .is_some_and(|id| {
                    ctx.target_of(&f.rel, &l.target_text)
                        .is_some_and(|t| moved_ids.contains_key(&(t, id.to_string())))
                })
        });
        if hits && !candidates.contains(&f.rel) {
            candidates.push(f.rel.clone());
        }
    }

    let mut rewritten = 0;
    for rel in candidates {
        let abs = ctx.root.join(&rel);
        let idx = match plan.edits.iter().position(|e| e.path == abs) {
            Some(i) => i,
            None => {
                let original = crate::markdown::lines::read_or_empty(&abs).map_err(|source| {
                    LinkError::Read {
                        path: abs.clone(),
                        source,
                    }
                })?;
                plan.edits.push(FileEdit {
                    path: abs.clone(),
                    new: original.clone(),
                    original,
                });
                plan.edits.len() - 1
            }
        };
        let content = plan.edits[idx].new.clone();
        let (links, self_refs) = crate::graph::parser::extract_links_and_self_block_refs(&content);
        let mut replacements: Vec<(std::ops::Range<usize>, String)> = Vec::new();
        for l in links.iter().chain(&self_refs) {
            if l.form != LinkForm::WikiLink {
                continue;
            }
            let Some(id) = l.anchor.as_deref().and_then(|a| a.strip_prefix('^')) else {
                continue;
            };
            let moved_from = moved_in
                .iter()
                .find(|(dst, first, last, _)| *dst == rel && (*first..=*last).contains(&l.line))
                .map(|(_, _, _, src)| src);
            let resolved = ctx.target_of(&rel, &l.target_text);
            let new_target = match (moved_from, resolved) {
                // A same-file reference that travelled with its line: it
                // still means the source file unless its block moved too.
                (Some(src), _) if l.target_text.trim().is_empty() => {
                    if moved_ids.get(&(src.clone(), id.to_string())) == Some(&rel) {
                        continue;
                    }
                    src.clone()
                }
                (_, Some(t)) => match moved_ids.get(&(t.clone(), id.to_string())) {
                    Some(dst) => dst.clone(),
                    None if moved_from.is_some()
                        && t == rel
                        && !l.target_text.trim().is_empty() =>
                    {
                        t
                    }
                    None => continue,
                },
                _ => continue,
            };
            let note = note_part(&rel, &new_target, &ctx.notes);
            let alias = l
                .display
                .as_deref()
                .map(|d| format!("|{d}"))
                .unwrap_or_default();
            let bang = if l.is_embed { "!" } else { "" };
            let text = format!("{bang}[[{note}#^{id}{alias}]]");
            if text != l.raw_text {
                replacements.push((l.byte_range.clone(), text));
            }
        }
        replacements.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
        let mut new = content;
        for (range, text) in &replacements {
            new.replace_range(range.clone(), text);
        }
        rewritten += replacements.len();
        plan.edits[idx].new = new;
    }
    Ok(rewritten)
}

/// Which of a task's line references [`plan_unlink`] removes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unlink {
    /// The single line reference; fails with `NeedsChoice` when there are
    /// several.
    Only,
    /// Every line reference resolving to this task.
    Parent(TaskKey),
    /// The reference to this id (also for unresolved ones).
    Id(String),
    /// Every line reference.
    All,
}

/// Plan removing parent references from `task`'s `⤴` field, as selected by
/// `which` against its scanned `expected` task. The parents' `🆔`s stay.
pub fn plan_unlink(
    task: &MoveSource,
    which: &Unlink,
    format: &dyn TaskFormat,
) -> Result<LinkPlan, LinkError> {
    let (path, line) = (task.path.clone(), task.line);
    let mut files = Files::new();
    let lines = files.lines(&task.path)?;
    let current = guarded_task(
        &task.path,
        lines,
        task.line,
        task.expected.as_ref(),
        format,
        false,
    )?;
    let refs: &[ParentRef] = task
        .expected
        .as_ref()
        .map_or(&[], |t| t.parent_refs.as_slice());
    let fm_has = |pred: &dyn Fn(&ParentRef) -> bool| {
        refs.iter()
            .any(|r| r.origin == RefOrigin::Frontmatter && pred(r))
    };
    let mut line_ids: Vec<&String> = Vec::new();
    for id in &current.parent_ids {
        if !line_ids.contains(&id) {
            line_ids.push(id);
        }
    }

    let remove: Vec<String> = match which {
        Unlink::Only | Unlink::All if line_ids.is_empty() => {
            return Err(if fm_has(&|_| true) {
                LinkError::ParentFromFrontmatter { path, line }
            } else {
                LinkError::NoParentLink { path, line }
            });
        }
        Unlink::Only if line_ids.len() > 1 => {
            let refs = line_ids
                .iter()
                .map(|id| {
                    refs.iter()
                        .find(|r| &&r.id == id && r.origin == RefOrigin::Line)
                        .cloned()
                        .unwrap_or_else(|| ParentRef {
                            id: (*id).clone(),
                            origin: RefOrigin::Line,
                            state: RefState::Dangling,
                        })
                })
                .collect();
            return Err(LinkError::NeedsChoice { path, line, refs });
        }
        Unlink::Only | Unlink::All => line_ids.into_iter().cloned().collect(),
        Unlink::Parent(key) => {
            let ids: Vec<String> = refs
                .iter()
                .filter(|r| r.origin == RefOrigin::Line && r.state.resolved() == Some(key))
                .map(|r| r.id.clone())
                .filter(|id| line_ids.contains(&id))
                .collect();
            if ids.is_empty() {
                return Err(if fm_has(&|r| r.state.resolved() == Some(key)) {
                    LinkError::ParentFromFrontmatter { path, line }
                } else {
                    LinkError::NotLinkedTo { path, line }
                });
            }
            ids
        }
        Unlink::Id(id) => {
            if !line_ids.contains(&id) {
                return Err(if fm_has(&|r| &r.id == id) {
                    LinkError::ParentFromFrontmatter { path, line }
                } else {
                    LinkError::NotLinkedTo { path, line }
                });
            }
            vec![id.clone()]
        }
    };

    let l = &mut lines[task.line - 1];
    *l = rewrite_task_line(l, format, |t| t.parent_ids.retain(|p| !remove.contains(p)))
        .ok_or_else(|| LinkError::NotATask {
            path: path.clone(),
            line,
        })?;
    Ok(LinkPlan {
        edits: files.into_edits(),
        children: vec![LinkedChild {
            description: current.description,
            path,
            line,
            unchanged: false,
        }],
        parent: None,
    })
}

/// A structural problem found by [`check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    Dangling,
    Ambiguous,
    Cycle,
    /// A line reference naming the physical parent, or an id listed twice.
    Redundant,
    /// A task carrying a `🆔` already used by an earlier task.
    DuplicateId,
}

impl IssueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IssueKind::Dangling => "dangling",
            IssueKind::Ambiguous => "ambiguous",
            IssueKind::Cycle => "cycle",
            IssueKind::Redundant => "redundant",
            IssueKind::DuplicateId => "duplicate-id",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub kind: IssueKind,
    /// Index into the scanned tasks.
    pub task: usize,
    /// The id involved.
    pub id: String,
}

/// Every parent-reference problem in a resolved scan, in `(path, line)`
/// order of the task concerned.
pub fn check(tasks: &[Task]) -> Vec<Issue> {
    let mut order: Vec<usize> = (0..tasks.len()).collect();
    order.sort_by(|&a, &b| {
        (&tasks[a].source_file, tasks[a].source_line)
            .cmp(&(&tasks[b].source_file, tasks[b].source_line))
    });
    let mut issues = Vec::new();
    let mut seen_ids: HashSet<&str> = HashSet::new();
    for ti in order {
        let t = &tasks[ti];
        let issue = |kind, id: &str| Issue {
            kind,
            task: ti,
            id: id.to_string(),
        };
        if let Some(id) = t.id.as_deref() {
            if !seen_ids.insert(id) {
                issues.push(issue(IssueKind::DuplicateId, id));
            }
        }
        let physical = t.parent.map(|line| (t.source_file.clone(), line));
        for r in &t.parent_refs {
            let kind = match &r.state {
                RefState::Dangling => Some(IssueKind::Dangling),
                RefState::Ambiguous => Some(IssueKind::Ambiguous),
                RefState::Cycle => Some(IssueKind::Cycle),
                RefState::Resolved(k)
                    if r.origin == RefOrigin::Line && physical.as_ref() == Some(k) =>
                {
                    Some(IssueKind::Redundant)
                }
                RefState::Resolved(_) => None,
            };
            if let Some(kind) = kind {
                issues.push(issue(kind, &r.id));
            }
        }
        let mut listed: HashSet<&str> = HashSet::new();
        for id in &t.parent_ids {
            if !listed.insert(id) {
                issues.push(issue(IssueKind::Redundant, id));
            }
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::emoji::EmojiFormat;
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

    fn states(s: &Scan, rel: &str, line: usize) -> Vec<RefState> {
        task(s, rel, line)
            .parent_refs
            .iter()
            .map(|r| r.state.clone())
            .collect()
    }

    fn resolved(rel: &str, line: usize) -> RefState {
        RefState::Resolved((PathBuf::from(rel), line))
    }

    fn key(rel: &str, line: usize) -> TaskKey {
        (PathBuf::from(rel), line)
    }

    // ── resolution ───────────────────────────────────────────────────

    #[test]
    fn local_and_remote_parents_in_one_field() {
        let (_d, s) = scan(&[
            ("proj.md", "- [ ] Vendor onboarding 🆔 k3x9qa\n"),
            (
                "jira/ABC-123.md",
                "---\nft:\n  tasks:\n    anchor: ABC-123\n---\n- [ ] Fix login 🆔 ABC-123\n",
            ),
            ("j.md", "# Day\n- [ ] reproduce ⤴ k3x9qa,ABC-123\n"),
        ]);
        assert_eq!(
            states(&s, "j.md", 2),
            [resolved("proj.md", 1), resolved("jira/ABC-123.md", 6)]
        );
        let t = task(&s, "j.md", 2);
        assert!(t.parent_refs.iter().all(|r| r.origin == RefOrigin::Line));
        assert_eq!(
            crate::task::hierarchy::parent_keys(t),
            [key("proj.md", 1), key("jira/ABC-123.md", 6)]
        );
    }

    #[test]
    fn wikilinks_are_ordinary_links() {
        let (_d, s) = scan(&[
            ("proj.md", "- [ ] Vendor ^vo1 \n"),
            ("j.md", "- [ ] t [[proj#^vo1]] [[proj]]\n"),
        ]);
        assert!(task(&s, "j.md", 1).parent_refs.is_empty());
    }

    #[test]
    fn dangling_ambiguous_and_duplicate_ids_collapse() {
        let (_d, s) = scan(&[
            ("a.md", "- [ ] A 🆔 dup\n- [ ] B 🆔 dup\n"),
            ("j.md", "- [ ] t ⤴ dup,gone,gone\n"),
        ]);
        assert_eq!(
            states(&s, "j.md", 1),
            [RefState::Ambiguous, RefState::Dangling]
        );
        assert!(crate::task::hierarchy::parent_keys(task(&s, "j.md", 1)).is_empty());
    }

    #[test]
    fn physical_and_logical_parents_combine() {
        let (_d, s) = scan(&[
            ("proj.md", "- [ ] Vendor 🆔 k3x9qa\n"),
            (
                "g.md",
                "- [ ] Ship Q4 🆔 p2m7rd\n  - [ ] kid ⤴ k3x9qa,p2m7rd\n",
            ),
        ]);
        let t = task(&s, "g.md", 2);
        assert_eq!(
            crate::task::hierarchy::parent_keys(t),
            [key("g.md", 1), key("proj.md", 1)]
        );
        // Naming the physical parent again stays resolved but is redundant.
        let issues = check(&s.tasks);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, IssueKind::Redundant);
        assert_eq!(issues[0].id, "p2m7rd");
    }

    #[test]
    fn two_cycle_breaks_at_greatest_key() {
        let (_d, s) = scan(&[
            ("a.md", "- [ ] A 🆔 aaaaaa ⤴ bbbbbb\n"),
            ("b.md", "- [ ] B 🆔 bbbbbb ⤴ aaaaaa\n"),
        ]);
        assert_eq!(states(&s, "a.md", 1), [resolved("b.md", 1)]);
        assert_eq!(states(&s, "b.md", 1), [RefState::Cycle]);
    }

    #[test]
    fn cycle_through_physical_edge_breaks_on_a_reference() {
        // p.md:2 is physically under p.md:1, which references p.md:2.
        let (_d, s) = scan(&[("p.md", "- [ ] P ⤴ cccccc\n  - [ ] C 🆔 cccccc\n")]);
        assert_eq!(states(&s, "p.md", 1), [RefState::Cycle]);
    }

    #[test]
    fn self_reference_and_three_cycle() {
        let (_d, s) = scan(&[
            ("s.md", "- [ ] S 🆔 ssssss ⤴ ssssss\n"),
            ("a.md", "- [ ] A 🆔 a1 ⤴ c1\n"),
            ("b.md", "- [ ] B 🆔 b1 ⤴ a1\n"),
            ("c.md", "- [ ] C 🆔 c1 ⤴ b1\n"),
        ]);
        assert_eq!(states(&s, "s.md", 1), [RefState::Cycle]);
        assert_eq!(states(&s, "a.md", 1), [resolved("c.md", 1)]);
        assert_eq!(states(&s, "b.md", 1), [resolved("a.md", 1)]);
        assert_eq!(states(&s, "c.md", 1), [RefState::Cycle]);
    }

    #[test]
    fn two_cycles_through_one_task_are_both_broken() {
        let (_d, s) = scan(&[
            ("a.md", "- [ ] A 🆔 a1 ⤴ b1,c1\n"),
            ("b.md", "- [ ] B 🆔 b1 ⤴ a1\n"),
            ("c.md", "- [ ] C 🆔 c1 ⤴ a1\n"),
        ]);
        assert_eq!(
            states(&s, "a.md", 1),
            [resolved("b.md", 1), resolved("c.md", 1)]
        );
        assert_eq!(states(&s, "b.md", 1), [RefState::Cycle]);
        assert_eq!(states(&s, "c.md", 1), [RefState::Cycle]);
    }

    #[test]
    fn deleted_parent_dangles() {
        let (d, _) = scan(&[
            ("proj.md", "- [ ] Vendor 🆔 k3x9qa\n"),
            ("j.md", "- [ ] t ⤴ k3x9qa\n"),
        ]);
        std::fs::write(d.path().join("proj.md"), "").unwrap();
        let s = crate::scan::scan_vault(d.path(), &[]);
        assert_eq!(states(&s, "j.md", 1), [RefState::Dangling]);
    }

    // ── DAG forest + graph ───────────────────────────────────────────

    fn shape(s: &Scan, rows: &[crate::task::hierarchy::ForestRow]) -> Vec<(String, usize)> {
        rows.iter()
            .map(|r| (s.tasks[r.idx].description.clone(), r.depth))
            .collect()
    }

    fn two_parents() -> (tempfile::TempDir, Scan) {
        scan(&[
            (
                "proj.md",
                "- [ ] Vendor 🆔 k3x9qa\n  - [ ] physical child\n",
            ),
            ("goals.md", "- [ ] Ship Q4 🆔 p2m7rd\n"),
            (
                "j.md",
                "- [ ] journal task ⤴ k3x9qa,p2m7rd\n  - [ ] grandkid\n",
            ),
        ])
    }

    #[test]
    fn forest_repeats_a_task_under_every_parent() {
        use crate::task::hierarchy::expand_forest;
        let (_d, s) = two_parents();
        let matched = [key("proj.md", 1), key("goals.md", 1), key("j.md", 1)];
        let rows = expand_forest(&s.tasks, &matched);
        let sh = |n: &str, d| (n.to_string(), d);
        assert_eq!(
            shape(&s, &rows),
            [
                sh("Vendor", 0),
                sh("physical child", 1),
                sh("journal task", 1),
                sh("grandkid", 2),
                sh("Ship Q4", 0),
                sh("journal task", 1),
                sh("grandkid", 2),
            ]
        );
        let jt: Vec<_> = rows
            .iter()
            .filter(|r| s.tasks[r.idx].description == "journal task")
            .map(|r| r.via.map(|v| s.tasks[v].description.clone()))
            .collect();
        assert_eq!(jt, [Some("Vendor".into()), Some("Ship Q4".into())]);
    }

    #[test]
    fn forest_with_one_parent_in_view() {
        use crate::task::hierarchy::expand_forest;
        let (_d, s) = two_parents();
        let rows = expand_forest(&s.tasks, &[key("j.md", 1), key("proj.md", 1)]);
        let names: Vec<_> = shape(&s, &rows).into_iter().map(|(n, _)| n).collect();
        assert_eq!(
            names,
            ["Vendor", "physical child", "journal task", "grandkid"]
        );
        // Matched alone, it is a root.
        let rows = expand_forest(&s.tasks, &[key("j.md", 1)]);
        assert_eq!(rows[0].via, None);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn visible_forest_expands_every_occurrence() {
        use crate::task::hierarchy::expand_forest_visible;
        let (_d, s) = two_parents();
        let expanded = [key("proj.md", 1), key("goals.md", 1)]
            .into_iter()
            .collect();
        let rows = expand_forest_visible(
            &s.tasks,
            &[key("proj.md", 1), key("goals.md", 1)],
            &expanded,
        );
        let jt: Vec<_> = rows
            .iter()
            .filter(|r| s.tasks[r.idx].description == "journal task")
            .collect();
        assert_eq!(jt.len(), 2);
        assert!(jt.iter().all(|r| r.has_children && !r.expanded));
    }

    #[test]
    fn graph_has_one_subtask_edge_per_parent() {
        use crate::graph::{EdgeKind, Graph};
        let (_d, s) = scan(&[
            ("proj.md", "- [ ] Vendor 🆔 k3x9qa\n"),
            (
                "goals.md",
                "- [ ] Ship Q4 🆔 p2m7rd\n  - [ ] kid ⤴ p2m7rd,k3x9qa,nope\n",
            ),
        ]);
        let g = Graph::build(&s).unwrap();
        let kid = g.task_by_loc(Path::new("goals.md"), 2).unwrap();
        let parents: Vec<_> = g
            .incoming(kid)
            .filter(|(_, e)| matches!(e, EdgeKind::Subtask))
            .map(|(id, _)| id)
            .collect();
        assert_eq!(parents.len(), 2);
        for (rel, line) in [("proj.md", 1), ("goals.md", 1)] {
            assert!(parents.contains(&g.task_by_loc(Path::new(rel), line).unwrap()));
        }
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
            .map(|t| t.description.clone())
            .collect();
        out.sort();
        out
    }

    #[test]
    fn hierarchy_dsl_attributes() {
        let (_d, s) = scan(&[
            (
                "proj.md",
                "- [x] Done parent 🆔 dp\n- [ ] Open parent 🆔 op\n  - [ ] physical kid\n",
            ),
            (
                "j.md",
                "- [ ] orphan ⤴ dp\n- [ ] half ⤴ dp,op\n- [ ] broken ⤴ zz\n- [ ] top\n",
            ),
        ]);
        let preset = |n| crate::query::preset::builtin(n).unwrap();
        assert_eq!(query(&s, preset("orphaned")), ["orphan"]);
        assert_eq!(query(&s, "parent_status = Done"), ["half", "orphan"]);
        assert_eq!(
            query(&s, "parent includes \"Open\""),
            ["half", "physical kid"]
        );
        assert_eq!(query(&s, "parents = 2"), ["half"]);
        assert_eq!(query(&s, "open_parents = 1"), ["half", "physical kid"]);
        assert_eq!(query(&s, "children = 2"), ["Done parent", "Open parent"]);
        assert_eq!(query(&s, "id = op"), ["Open parent"]);
        assert_eq!(query(&s, "parent_ref = dangling"), ["broken"]);
        assert_eq!(query(&s, preset("broken-links")), ["broken"]);
        assert_eq!(
            query(&s, "parent is null and status = Open"),
            ["Open parent", "broken", "top"]
        );
        assert_eq!(
            query(&s, "parent_ref is null and status = Open"),
            ["Open parent", "physical kid", "top"]
        );
    }

    #[test]
    fn hierarchy_attrs_rejected_on_edges() {
        use crate::graph::query::{parse_with, Profile};
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        for a in ["children", "parents", "parent_ref", "id"] {
            let src = format!("node where kind = Task; expand where edge.{a} > 0;");
            assert!(parse_with(&src, Profile::Default, today).is_err(), "{a}");
        }
        assert!(parse_with("parent_link = resolved", Profile::Tasks, today).is_err());
    }

    // ── plan_link / plan_unlink ──────────────────────────────────────

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

    fn id_of(line: &str) -> String {
        line.split("🆔 ")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .trim()
            .to_string()
    }

    #[test]
    fn link_journal_task_under_project_task() {
        let (d, s) = scan(&[
            ("proj.md", "- [ ] Vendor onboarding\n"),
            (
                "journal/2026-10-21.md",
                "# Day\n\t- [ ] reproduce 📅 2026-10-23\n",
            ),
        ]);
        let plan = link(
            d.path(),
            &s,
            &[("journal/2026-10-21.md", 2)],
            ("proj.md", 1),
        )
        .unwrap();
        assert_eq!(plan.children.len(), 1);
        assert!(!plan.children[0].unchanged);
        let proj = read(d.path(), "proj.md");
        let id = id_of(&proj);
        assert_eq!(id.len(), 6);
        assert!(id
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()));
        assert_eq!(proj, format!("- [ ] Vendor onboarding 🆔 {id}\n"));
        assert_eq!(
            read(d.path(), "journal/2026-10-21.md"),
            format!("# Day\n\t- [ ] reproduce ⤴ {id} 📅 2026-10-23\n")
        );
    }

    #[test]
    fn link_adds_a_second_parent_and_is_idempotent() {
        let (d, s) = scan(&[
            ("proj.md", "- [ ] Vendor 🆔 k3x9qa\n- [ ] Fix 🆔 ABC-123\n"),
            ("j.md", "- [ ] t ⤴ k3x9qa\n"),
        ]);
        link(d.path(), &s, &[("j.md", 1)], ("proj.md", 2)).unwrap();
        assert_eq!(read(d.path(), "j.md"), "- [ ] t ⤴ k3x9qa,ABC-123\n");
        let s = crate::scan::scan_vault(d.path(), &[]);
        let plan = link(d.path(), &s, &[("j.md", 1)], ("proj.md", 1)).unwrap();
        assert!(plan.children[0].unchanged);
        assert!(plan.edits.iter().all(|e| e.original == e.new));
    }

    #[test]
    fn link_refuses_cycles_and_redundant_physical_parent() {
        let (d, s) = scan(&[
            ("a.md", "- [ ] A 🆔 a1\n  - [ ] A kid\n"),
            ("b.md", "- [ ] B ⤴ a1\n"),
        ]);
        let err = link(d.path(), &s, &[("a.md", 1)], ("b.md", 1)).unwrap_err();
        assert!(matches!(err, LinkError::WouldCycle { .. }), "{err:?}");
        let err = link(d.path(), &s, &[("a.md", 1)], ("a.md", 2)).unwrap_err();
        assert!(matches!(err, LinkError::WouldCycle { .. }), "{err:?}");
        let err = link(d.path(), &s, &[("a.md", 2)], ("a.md", 1)).unwrap_err();
        assert!(
            matches!(err, LinkError::AlreadyPhysicalParent { .. }),
            "{err:?}"
        );
        // A nested task can still gain a logical parent elsewhere.
        link(d.path(), &s, &[("a.md", 2)], ("b.md", 1)).unwrap();
        assert!(read(d.path(), "a.md").contains("  - [ ] A kid ⤴ "));
    }

    #[test]
    fn link_guards_stale_lines() {
        let (d, s) = scan(&[("p.md", "- [ ] P\n"), ("c.md", "- [ ] C\n")]);
        std::fs::write(d.path().join("c.md"), "- [ ] changed\n").unwrap();
        let err = link(d.path(), &s, &[("c.md", 1)], ("p.md", 1)).unwrap_err();
        assert!(matches!(err, LinkError::LineChanged { .. }), "{err:?}");
    }

    #[test]
    fn ids_avoid_vault_wide_collisions() {
        let mut taken = HashSet::new();
        let first = id_from_seed(Path::new("a.md"), 1, "x", 7, &taken);
        taken.insert(first.clone());
        let second = id_from_seed(Path::new("a.md"), 1, "x", 7, &taken);
        assert_ne!(first, second);
        assert_eq!(second.len(), 6);
    }

    #[test]
    fn parents_for_new_task_reuse_and_assign_ids() {
        let (d, s) = scan(&[("p.md", "- [ ] Ticket 🆔 ABC-123\n- [ ] Goal\n")]);
        let ctx = LinkCtx::new(d.path(), &s);
        let (ids, plan) = plan_parents_for_new_task(
            &[
                src_of(d.path(), &s, "p.md", 1),
                src_of(d.path(), &s, "p.md", 2),
            ],
            &ctx,
            &EmojiFormat,
        )
        .unwrap();
        apply_link_plan(&plan).unwrap();
        assert_eq!(ids[0], "ABC-123");
        assert_eq!(
            read(d.path(), "p.md"),
            format!("- [ ] Ticket 🆔 ABC-123\n- [ ] Goal 🆔 {}\n", ids[1])
        );
    }

    fn unlink(d: &Path, s: &Scan, rel: &str, line: usize, which: Unlink) -> Result<(), LinkError> {
        let plan = plan_unlink(&src_of(d, s, rel, line), &which, &EmojiFormat)?;
        apply_link_plan(&plan)
    }

    #[test]
    fn unlink_variants() {
        let (d, s) = scan(&[
            ("p.md", "- [ ] V 🆔 k3x9qa\n- [ ] F 🆔 ABC-123\n"),
            ("j.md", "- [ ] two ⤴ k3x9qa,ABC-123 📅 2026-10-23\n- [ ] one ⤴ k3x9qa\n- [ ] none\n- [ ] gone ⤴ zz,k3x9qa\n"),
        ]);
        match unlink(d.path(), &s, "j.md", 1, Unlink::Only).unwrap_err() {
            LinkError::NeedsChoice { refs, .. } => {
                assert_eq!(refs.len(), 2);
                assert_eq!(refs[1].state, resolved("p.md", 2));
            }
            e => panic!("{e:?}"),
        }
        unlink(d.path(), &s, "j.md", 1, Unlink::Parent(key("p.md", 2))).unwrap();
        unlink(d.path(), &s, "j.md", 2, Unlink::Only).unwrap();
        let err = unlink(d.path(), &s, "j.md", 3, Unlink::Only).unwrap_err();
        assert!(matches!(err, LinkError::NoParentLink { .. }), "{err:?}");
        let err = unlink(d.path(), &s, "j.md", 4, Unlink::Parent(key("p.md", 2))).unwrap_err();
        assert!(matches!(err, LinkError::NotLinkedTo { .. }), "{err:?}");
        unlink(d.path(), &s, "j.md", 4, Unlink::Id("zz".into())).unwrap();
        assert_eq!(
            read(d.path(), "j.md"),
            "- [ ] two ⤴ k3x9qa 📅 2026-10-23\n- [ ] one\n- [ ] none\n- [ ] gone ⤴ k3x9qa\n"
        );
        let s = crate::scan::scan_vault(d.path(), &[]);
        unlink(d.path(), &s, "j.md", 4, Unlink::All).unwrap();
        assert!(read(d.path(), "j.md").ends_with("- [ ] gone\n"));
        assert_eq!(
            read(d.path(), "p.md"),
            "- [ ] V 🆔 k3x9qa\n- [ ] F 🆔 ABC-123\n"
        );
    }

    #[test]
    fn check_reports_every_kind() {
        let (_d, s) = scan(&[
            (
                "a.md",
                "- [ ] A 🆔 k3x9qa\n- [ ] P 🆔 pp\n  - [ ] red ⤴ pp\n",
            ),
            (
                "b.md",
                "- [ ] copy 🆔 k3x9qa\n- [ ] amb ⤴ k3x9qa\n- [ ] dang ⤴ nope,nope\n",
            ),
            ("c.md", "- [ ] S 🆔 ss ⤴ ss\n"),
        ]);
        let got: Vec<(&str, String, usize, String)> = check(&s.tasks)
            .into_iter()
            .map(|i| {
                let t = &s.tasks[i.task];
                (
                    i.kind.as_str(),
                    t.source_file.display().to_string(),
                    t.source_line,
                    i.id,
                )
            })
            .collect();
        let row = |k, f: &str, l, id: &str| (k, f.to_string(), l, id.to_string());
        assert_eq!(
            got,
            [
                row("redundant", "a.md", 3, "pp"),
                row("duplicate-id", "b.md", 1, "k3x9qa"),
                row("ambiguous", "b.md", 2, "k3x9qa"),
                row("dangling", "b.md", 3, "nope"),
                row("redundant", "b.md", 3, "nope"),
                row("cycle", "c.md", 1, "ss"),
            ]
        );
    }

    // ── move integration ─────────────────────────────────────────────

    use crate::task::ops::{apply_move_plan, plan_indent, plan_move, MoveTarget, Placement};

    #[test]
    fn nesting_strips_only_the_new_parents_id() {
        let (d, s) = scan(&[
            ("proj.md", "- [ ] Vendor 🆔 k3x9qa\n- [ ] Other\n"),
            (
                "j.md",
                "- [ ] reproduce ⤴ k3x9qa,ABC-123 📅 2026-10-23\n  - [ ] kid ⤴ k3x9qa\n",
            ),
        ]);
        let under = |line| MoveTarget::UnderTask {
            path: d.path().join("proj.md"),
            line,
            expected: None,
            placement: Placement::Child,
        };
        let plan = plan_move(&[src_of(d.path(), &s, "j.md", 1)], &under(1), &EmojiFormat).unwrap();
        apply_move_plan(&plan).unwrap();
        assert_eq!(
            read(d.path(), "proj.md"),
            "- [ ] Vendor 🆔 k3x9qa\n  - [ ] reproduce ⤴ ABC-123 📅 2026-10-23\n    - [ ] kid ⤴ k3x9qa\n- [ ] Other\n"
        );
    }

    #[test]
    fn nesting_under_unrelated_task_keeps_references() {
        let (d, s) = scan(&[
            ("proj.md", "- [ ] Other\n"),
            ("j.md", "- [ ] reproduce ⤴ k3x9qa\n"),
        ]);
        let target = MoveTarget::UnderTask {
            path: d.path().join("proj.md"),
            line: 1,
            expected: None,
            placement: Placement::Child,
        };
        let plan = plan_move(&[src_of(d.path(), &s, "j.md", 1)], &target, &EmojiFormat).unwrap();
        apply_move_plan(&plan).unwrap();
        assert_eq!(
            read(d.path(), "proj.md"),
            "- [ ] Other\n  - [ ] reproduce ⤴ k3x9qa\n"
        );
    }

    #[test]
    fn move_to_file_keeps_references() {
        let (d, s) = scan(&[("j.md", "- [ ] reproduce ⤴ k3x9qa\n")]);
        let plan = plan_move(
            &[src_of(d.path(), &s, "j.md", 1)],
            &MoveTarget::Append(d.path().join("inbox.md")),
            &EmojiFormat,
        )
        .unwrap();
        apply_move_plan(&plan).unwrap();
        assert_eq!(read(d.path(), "inbox.md"), "- [ ] reproduce ⤴ k3x9qa\n");
    }

    #[test]
    fn indent_strips_the_new_parents_id() {
        let (d, s) = scan(&[("j.md", "- [ ] A 🆔 a1\n- [ ] B ⤴ a1,zz\n")]);
        let plan = plan_indent(&src_of(d.path(), &s, "j.md", 2), &EmojiFormat).unwrap();
        apply_move_plan(&plan).unwrap();
        assert_eq!(read(d.path(), "j.md"), "- [ ] A 🆔 a1\n  - [ ] B ⤴ zz\n");
    }

    fn move_retarget(
        d: &Path,
        s: &Scan,
        source: (&str, usize),
        target: MoveTarget,
    ) -> (usize, ops::MovePlan) {
        let mut plan =
            plan_move(&[src_of(d, s, source.0, source.1)], &target, &EmojiFormat).unwrap();
        let n = plan_retarget(&mut plan, &LinkCtx::new(d, s)).unwrap();
        apply_move_plan(&plan).unwrap();
        (n, plan)
    }

    #[test]
    fn retarget_links_when_parent_moves_cross_file() {
        let (d, s) = scan(&[
            (
                "proj.md",
                "- [ ] Vendor ^vo1\n- [ ] stays\n- [ ] local ref [[#^vo1]]\n",
            ),
            (
                "journal.md",
                "- [ ] reproduce [[proj#^vo1]]\n- [ ] embed ![[proj#^vo1|the parent]]\n",
            ),
            ("archive/2026.md", "# Archive\n"),
        ]);
        let (n, _) = move_retarget(
            d.path(),
            &s,
            ("proj.md", 1),
            MoveTarget::Append(d.path().join("archive/2026.md")),
        );
        assert_eq!(n, 3);
        assert_eq!(
            read(d.path(), "journal.md"),
            "- [ ] reproduce [[2026#^vo1]]\n- [ ] embed ![[2026#^vo1|the parent]]\n"
        );
        assert_eq!(
            read(d.path(), "proj.md"),
            "- [ ] stays\n- [ ] local ref [[2026#^vo1]]\n"
        );
    }

    #[test]
    fn retarget_links_inside_moved_block() {
        let (d, s) = scan(&[
            ("a.md", "- [ ] Root ^r1\n- [ ] block [[#^r1]]\n  - [ ] inner ^i1\n  - [ ] ref inner [[#^i1]]\n  - [ ] ref b [[b#^b1]]\n"),
            ("b.md", "- [ ] B ^b1\n"),
        ]);
        let target = MoveTarget::UnderTask {
            path: d.path().join("b.md"),
            line: 1,
            expected: None,
            placement: Placement::Child,
        };
        let (n, _) = move_retarget(d.path(), &s, ("a.md", 2), target);
        // [[#^r1]] → explicit a; [[b#^b1]] collapses; [[#^i1]] travelled
        // with its block and stays.
        assert_eq!(
            read(d.path(), "b.md"),
            "- [ ] B ^b1\n  - [ ] block [[a#^r1]]\n    - [ ] inner ^i1\n    - [ ] ref inner [[#^i1]]\n    - [ ] ref b [[#^b1]]\n"
        );
        assert_eq!(n, 2);
    }

    #[test]
    fn retarget_same_file_moves_are_noops() {
        let (d, s) = scan(&[("a.md", "- [ ] P ^p\n- [ ] Q\n- [ ] r [[#^p]]\n")]);
        let target = MoveTarget::UnderTask {
            path: d.path().join("a.md"),
            line: 2,
            expected: None,
            placement: Placement::Child,
        };
        let (n, _) = move_retarget(d.path(), &s, ("a.md", 1), target);
        assert_eq!(n, 0);
        assert_eq!(
            read(d.path(), "a.md"),
            "- [ ] Q\n  - [ ] P ^p\n- [ ] r [[#^p]]\n"
        );
    }

    #[test]
    fn retarget_explicit_same_file_ref_left_behind_becomes_explicit() {
        let (d, s) = scan(&[("a.md", "- [ ] stay ^s1\n- [ ] go see [[#^s1]]\n")]);
        let (n, _) = move_retarget(
            d.path(),
            &s,
            ("a.md", 2),
            MoveTarget::Append(d.path().join("z.md")),
        );
        assert_eq!(n, 1);
        assert_eq!(read(d.path(), "z.md"), "- [ ] go see [[a#^s1]]\n");
    }

    // ── frontmatter parents (anchor notes) ───────────────────────────

    fn anchor_note_md(key: &str, parent: Option<&str>) -> String {
        let p = parent
            .map(|p| format!("    parent: {p}\n"))
            .unwrap_or_default();
        format!("---\nft:\n  tasks:\n    anchor: {key}\n{p}---\n- [ ] {key} title 🆔 {key}\n")
    }

    #[test]
    fn anchor_note_frontmatter_parent_adds_to_line_refs() {
        let (_d, s) = scan(&[
            ("jira/EPIC-7.md", &anchor_note_md("EPIC-7", None)),
            ("goals.md", "- [ ] Q4 🆔 p2m7rd\n"),
            (
                "jira/ABC-123.md",
                "---\nft:\n  tasks:\n    anchor: ABC-123\n    parent: EPIC-7\n---\n- [ ] t ⤴ p2m7rd 🆔 ABC-123\n",
            ),
            ("jira/X-1.md", &anchor_note_md("X-1", Some("\"[[EPIC-7]]\""))),
        ]);
        let refs = &task(&s, "jira/ABC-123.md", 7).parent_refs;
        assert_eq!(refs.len(), 2);
        assert_eq!(
            (refs[0].origin, refs[0].state.clone()),
            (RefOrigin::Line, resolved("goals.md", 1))
        );
        assert_eq!(
            (refs[1].origin, refs[1].state.clone()),
            (RefOrigin::Frontmatter, resolved("jira/EPIC-7.md", 6))
        );
        // An old wikilink value is just an unknown id.
        assert_eq!(states(&s, "jira/X-1.md", 7), [RefState::Dangling]);
    }

    #[test]
    fn frontmatter_parent_cycle_and_unlink_refusal() {
        let (d, s) = scan(&[
            ("jira/A-1.md", &anchor_note_md("A-1", Some("B-1"))),
            ("jira/B-1.md", &anchor_note_md("B-1", Some("A-1"))),
        ]);
        assert_eq!(states(&s, "jira/A-1.md", 7), [resolved("jira/B-1.md", 7)]);
        assert_eq!(states(&s, "jira/B-1.md", 7), [RefState::Cycle]);
        let err = unlink(d.path(), &s, "jira/A-1.md", 7, Unlink::Only).unwrap_err();
        assert!(
            matches!(err, LinkError::ParentFromFrontmatter { .. }),
            "{err:?}"
        );
        let err = unlink(
            d.path(),
            &s,
            "jira/A-1.md",
            7,
            Unlink::Parent(key("jira/B-1.md", 7)),
        )
        .unwrap_err();
        assert!(
            matches!(err, LinkError::ParentFromFrontmatter { .. }),
            "{err:?}"
        );
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
            let (p, q) = ((i + 1) % 2000, (i + 7) % 2000);
            let body = format!(
                "# Note {i}\n\nSome prose [[note-{p}]].\n\n- [ ] task a{i} 🆔 a{i}\n  - [ ] sub\n- [ ] linked ⤴ a{p},a{q}\n- [ ] plain [[note-{p}]]\n"
            );
            std::fs::write(dir.path().join(format!("note-{i}.md")), body).unwrap();
        }
        let start = std::time::Instant::now();
        let mut s = crate::scan::scan_vault(dir.path(), &[]);
        let scan_time = start.elapsed();
        let start = std::time::Instant::now();
        resolve_parent_refs(&mut s);
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
            .all(|t| t.parent_refs.len() == 2
                && t.parent_refs.iter().all(|r| r.state.resolved().is_some())));
    }
}

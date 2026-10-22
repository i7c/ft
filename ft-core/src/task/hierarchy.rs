use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::Task;

/// Stable identity of a task across a scan: its source file plus 1-indexed
/// line. The same key the graph uses for its task nodes.
pub type TaskKey = (PathBuf, usize);

/// The effective parents of `task`: its physical (indentation) parent
/// first, when it has one, then every task its resolved parent references
/// point at, in order, without duplicates.
pub fn parent_keys(task: &Task) -> Vec<TaskKey> {
    let mut keys: Vec<TaskKey> = task
        .parent
        .map(|line| (task.source_file.clone(), line))
        .into_iter()
        .collect();
    for r in &task.parent_refs {
        if let Some(k) = r.state.resolved() {
            if !keys.contains(k) {
                keys.push(k.clone());
            }
        }
    }
    keys
}

/// Forest expansion stops after this many rows: a task shows under every
/// parent, so subtrees repeat, and a pathological DAG could blow up.
pub const MAX_FOREST_ROWS: usize = 50_000;

/// A node in a depth-annotated task forest: an index into the caller's task
/// slice, the node's depth (0 = root), whether it has any subtasks in the
/// underlying vault (for a collapse/expand affordance), and the parent this
/// occurrence hangs under (`None` for roots).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForestRow {
    pub idx: usize,
    pub depth: usize,
    pub has_children: bool,
    pub via: Option<usize>,
}

/// Expand an ordered set of matched tasks into a depth-annotated forest:
/// every matched task **plus all of its transitive subtasks**, even subtasks
/// that aren't themselves matched. The result is in pre-order — each task is
/// immediately followed by its subtree.
///
/// `all` is the full task slice (one scan); `matched` lists the keys to root
/// the forest on, already in the caller's desired root order. The hierarchy
/// is a DAG: a task shows under **every** effective parent present in the
/// output (subtrees repeat), and a matched task is a root only when none of
/// its parents is present. A task already on the current root-to-row path
/// is not emitted again on it, and emission stops at [`MAX_FOREST_ROWS`].
pub fn expand_forest(all: &[Task], matched: &[TaskKey]) -> Vec<ForestRow> {
    let index = key_index(all);
    let children = children_map(all, &index);
    let display = display_set(matched, &index, &children);

    let mut rows = Vec::with_capacity(display.len());
    let mut path = Vec::new();
    for i in roots(all, matched, &index, &display) {
        emit_subtree(i, None, &children, &display, &mut path, &mut rows);
    }
    rows
}

/// Pre-order walk emitting `idx` then its in-display children.
fn emit_subtree(
    idx: usize,
    via: Option<usize>,
    children: &HashMap<usize, Vec<usize>>,
    display: &HashSet<usize>,
    path: &mut Vec<usize>,
    rows: &mut Vec<ForestRow>,
) {
    if path.contains(&idx) || rows.len() >= MAX_FOREST_ROWS {
        return;
    }
    let kids = children.get(&idx);
    rows.push(ForestRow {
        idx,
        depth: path.len(),
        has_children: kids.is_some_and(|k| !k.is_empty()),
        via,
    });
    if let Some(kids) = kids {
        path.push(idx);
        for &child in kids {
            if display.contains(&child) {
                emit_subtree(child, Some(idx), children, display, path, rows);
            }
        }
        path.pop();
    }
}

/// A node in a depth-annotated task forest that also reports its live
/// expansion state — for TUI list views that expand/collapse subtrees in
/// place (e.g. the Tasks tab). `expanded` is true only when the node both
/// has children and its key is in the caller's `expanded` set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRow {
    pub idx: usize,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub via: Option<usize>,
}

/// Like [`expand_forest`] but respects a live expand/collapse state: a
/// task's subtasks are emitted only if the task's key is in `expanded`
/// (expanding a task expands every occurrence of it). Unexpanded parents
/// emit as a leaf row (their `has_children` flag still signals the
/// affordance). Roots and repeats follow [`expand_forest`].
pub fn expand_forest_visible(
    all: &[Task],
    matched: &[TaskKey],
    expanded: &HashSet<TaskKey>,
) -> Vec<VisibleRow> {
    let index = key_index(all);
    let children = children_map(all, &index);
    let display = display_set(matched, &index, &children);

    let mut rows = Vec::with_capacity(display.len());
    let mut path = Vec::new();
    let ctx = VisCtx {
        all,
        children: &children,
        display: &display,
        expanded,
    };
    for i in roots(all, matched, &index, &display) {
        emit_visible(i, None, &ctx, &mut path, &mut rows);
    }
    rows
}

/// Shared context for [`emit_visible`], kept in a struct to stay under
/// clippy's argument-count limit and avoid re-threading each ref.
struct VisCtx<'a> {
    all: &'a [Task],
    children: &'a HashMap<usize, Vec<usize>>,
    display: &'a HashSet<usize>,
    expanded: &'a HashSet<TaskKey>,
}

/// Pre-order walk for [`expand_forest_visible`]: emit `idx`, then recurse
/// into its in-display children only when `idx` is expanded.
fn emit_visible(
    idx: usize,
    via: Option<usize>,
    ctx: &VisCtx,
    path: &mut Vec<usize>,
    rows: &mut Vec<VisibleRow>,
) {
    if path.contains(&idx) || rows.len() >= MAX_FOREST_ROWS {
        return;
    }
    let kids = ctx.children.get(&idx);
    let has_children = kids.is_some_and(|k| !k.is_empty());
    let key = (ctx.all[idx].source_file.clone(), ctx.all[idx].source_line);
    let is_expanded = has_children && ctx.expanded.contains(&key);
    rows.push(VisibleRow {
        idx,
        depth: path.len(),
        has_children,
        expanded: is_expanded,
        via,
    });
    if let (true, Some(kids)) = (is_expanded, kids) {
        path.push(idx);
        for &child in kids {
            if ctx.display.contains(&child) {
                emit_visible(child, Some(idx), ctx, path, rows);
            }
        }
        path.pop();
    }
}

/// Matched tasks plus all of their transitive descendants.
fn display_set(
    matched: &[TaskKey],
    index: &HashMap<TaskKey, usize>,
    children: &HashMap<usize, Vec<usize>>,
) -> HashSet<usize> {
    let mut display = HashSet::new();
    for key in matched {
        if let Some(&i) = index.get(key) {
            collect_subtree(i, children, &mut display);
        }
    }
    display
}

/// Matched tasks (in matched order, once each) none of whose effective
/// parents is in the display set — the others nest under those parents.
fn roots(
    all: &[Task],
    matched: &[TaskKey],
    index: &HashMap<TaskKey, usize>,
    display: &HashSet<usize>,
) -> Vec<usize> {
    let mut seen = HashSet::new();
    matched
        .iter()
        .filter_map(|k| index.get(k).copied())
        .filter(|&i| seen.insert(i))
        .filter(|&i| !parent_idxs(all, index, i).any(|p| display.contains(&p)))
        .collect()
}

/// Collect `idx` and all of its transitive descendants into `out`.
fn collect_subtree(idx: usize, children: &HashMap<usize, Vec<usize>>, out: &mut HashSet<usize>) {
    let mut todo = vec![idx];
    while let Some(i) = todo.pop() {
        if out.insert(i) {
            if let Some(kids) = children.get(&i) {
                todo.extend(kids);
            }
        }
    }
}

/// Map every task key to its index in `all`.
fn key_index(all: &[Task]) -> HashMap<TaskKey, usize> {
    all.iter()
        .enumerate()
        .map(|(i, t)| ((t.source_file.clone(), t.source_line), i))
        .collect()
}

/// Indices of the effective parents of `all[idx]`.
fn parent_idxs<'a>(
    all: &'a [Task],
    index: &'a HashMap<TaskKey, usize>,
    idx: usize,
) -> impl Iterator<Item = usize> + 'a {
    parent_keys(&all[idx])
        .into_iter()
        .filter_map(|k| index.get(&k).copied())
}

/// Map each parent task index to its direct subtasks' indices (physical
/// children in source order, then linked children). A task with several
/// parents is listed under each. Indices point into `all`. Useful for lazy,
/// one-level-at-a-time expansion (e.g. the TUI tree). Built from resolved
/// hierarchy, so the scan's post-pass must already have run.
pub fn child_index_map(all: &[Task]) -> HashMap<usize, Vec<usize>> {
    children_map(all, &key_index(all))
}

fn children_map(all: &[Task], index: &HashMap<TaskKey, usize>) -> HashMap<usize, Vec<usize>> {
    let mut map: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..all.len() {
        for p in parent_idxs(all, index, i) {
            map.entry(p).or_default().push(i);
        }
    }
    // Don't trust scan order: physical children first in source order, then
    // linked children (from other files or not nested) by `(path, line)`.
    for (&p, kids) in map.iter_mut() {
        let parent = &all[p];
        let linked = |t: &Task| {
            !(t.source_file == parent.source_file && t.parent == Some(parent.source_line))
        };
        kids.sort_by(|&a, &b| {
            let (ta, tb) = (&all[a], &all[b]);
            (linked(ta), &ta.source_file, ta.source_line).cmp(&(
                linked(tb),
                &tb.source_file,
                tb.source_line,
            ))
        });
    }
    map
}

/// Resolve `parent` pointers for a slice of tasks from the same file.
///
/// Tasks must be ordered by `source_line` (ascending). A task becomes the
/// child of the nearest preceding task whose `indent_level` is strictly
/// smaller. After this call every task's `parent` field is either `None`
/// (top-level) or the `source_line` of its direct parent.
pub fn resolve_hierarchy(tasks: &mut [Task]) {
    for i in 1..tasks.len() {
        let current_indent = tasks[i].indent_level;
        // Walk backwards looking for the nearest ancestor.
        for j in (0..i).rev() {
            if tasks[j].indent_level < current_indent {
                let parent_line = tasks[j].source_line;
                tasks[i].parent = Some(parent_line);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{
        emoji::EmojiFormat,
        format::{ParseContext, TaskFormat},
    };
    use std::path::PathBuf;

    fn parse_tasks(lines: &[&str]) -> Vec<Task> {
        let path = PathBuf::from("test.md");
        lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| {
                EmojiFormat.parse_line(
                    line,
                    ParseContext {
                        source_file: path.clone(),
                        source_line: i + 1,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn no_children_when_all_same_indent() {
        let mut tasks = parse_tasks(&["- [ ] task A", "- [ ] task B", "- [ ] task C"]);
        resolve_hierarchy(&mut tasks);
        for t in &tasks {
            assert!(t.parent.is_none(), "flat tasks should have no parent");
        }
    }

    #[test]
    fn single_level_children() {
        let mut tasks = parse_tasks(&["- [ ] parent", "  - [ ] child A", "  - [ ] child B"]);
        resolve_hierarchy(&mut tasks);
        assert!(tasks[0].parent.is_none());
        // source_line of parent is 1 (1-indexed)
        assert_eq!(tasks[1].parent, Some(1));
        assert_eq!(tasks[2].parent, Some(1));
    }

    #[test]
    fn two_level_nesting() {
        let mut tasks = parse_tasks(&["- [ ] grandparent", "  - [ ] parent", "    - [ ] child"]);
        resolve_hierarchy(&mut tasks);
        assert!(tasks[0].parent.is_none());
        assert_eq!(tasks[1].parent, Some(1)); // parent's parent = grandparent (line 1)
        assert_eq!(tasks[2].parent, Some(2)); // child's parent = parent (line 2)
    }

    #[test]
    fn three_level_nesting() {
        let mut tasks = parse_tasks(&[
            "- [ ] L0",
            "  - [ ] L1",
            "    - [ ] L2a",
            "    - [ ] L2b",
            "  - [ ] L1b",
        ]);
        resolve_hierarchy(&mut tasks);
        assert!(tasks[0].parent.is_none()); // L0: no parent
        assert_eq!(tasks[1].parent, Some(1)); // L1 → L0 (line 1)
        assert_eq!(tasks[2].parent, Some(2)); // L2a → L1 (line 2)
        assert_eq!(tasks[3].parent, Some(2)); // L2b → L1 (line 2)
        assert_eq!(tasks[4].parent, Some(1)); // L1b → L0 (line 1)
    }

    #[test]
    fn mixed_statuses_in_hierarchy() {
        let mut tasks = parse_tasks(&[
            "- [ ] open parent",
            "  - [x] done child ✅ 2026-05-01",
            "  - [-] cancelled child ❌ 2026-05-02",
        ]);
        resolve_hierarchy(&mut tasks);
        assert_eq!(tasks[1].parent, Some(1));
        assert_eq!(tasks[2].parent, Some(1));
    }

    // --- forest expansion --------------------------------------------------

    fn resolved(lines: &[&str]) -> Vec<Task> {
        let mut tasks = parse_tasks(lines);
        resolve_hierarchy(&mut tasks);
        tasks
    }

    fn key(line: usize) -> TaskKey {
        (PathBuf::from("test.md"), line)
    }

    /// Compact `(line, depth)` view of a forest for easy assertions.
    fn shape(all: &[Task], rows: &[ForestRow]) -> Vec<(usize, usize)> {
        rows.iter()
            .map(|r| (all[r.idx].source_line, r.depth))
            .collect()
    }

    #[test]
    fn forest_pulls_unmatched_subtasks_along() {
        // Only the parent matches; its whole subtree rides along, recursively.
        let all = resolved(&[
            "- [ ] house",
            "  - [ ] walls",
            "    - [ ] bricks",
            "  - [ ] pipes",
        ]);
        let rows = expand_forest(&all, &[key(1)]);
        assert_eq!(shape(&all, &rows), vec![(1, 0), (2, 1), (3, 2), (4, 1)]);
        // The parent reports having children; a leaf doesn't.
        assert!(rows[0].has_children);
        assert!(!rows[2].has_children);
    }

    #[test]
    fn forest_dedupes_matched_descendant_under_parent() {
        // Both parent and a child match: the child nests once, no duplicate
        // top-level row.
        let all = resolved(&["- [ ] house", "  - [ ] walls", "  - [ ] pipes"]);
        let rows = expand_forest(&all, &[key(1), key(2)]);
        assert_eq!(shape(&all, &rows), vec![(1, 0), (2, 1), (3, 1)]);
    }

    #[test]
    fn forest_matched_child_without_parent_is_a_root() {
        // Parent isn't matched and has no matched ancestor pulling it in, so
        // the matched child becomes a de-indented root.
        let all = resolved(&["- [ ] house", "  - [ ] walls", "    - [ ] bricks"]);
        let rows = expand_forest(&all, &[key(2)]);
        assert_eq!(shape(&all, &rows), vec![(2, 0), (3, 1)]);
    }

    #[test]
    fn forest_roots_follow_matched_order() {
        // Two independent top-level matches keep the caller's root ordering.
        let all = resolved(&["- [ ] alpha", "- [ ] beta"]);
        let rows = expand_forest(&all, &[key(2), key(1)]);
        assert_eq!(shape(&all, &rows), vec![(2, 0), (1, 0)]);
    }

    #[test]
    fn child_index_map_lists_direct_children_in_order() {
        let all = resolved(&[
            "- [ ] house",
            "  - [ ] walls",
            "    - [ ] bricks",
            "  - [ ] pipes",
        ]);
        let map = child_index_map(&all);
        assert_eq!(map.get(&0), Some(&vec![1, 3])); // house → walls, pipes
        assert_eq!(map.get(&1), Some(&vec![2])); // walls → bricks
        assert_eq!(map.get(&2), None); // bricks is a leaf
    }

    /// A matched subtask whose parent is also matched appears once, nested
    /// — not also as a depth-0 root. (graph-task-interaction §D7.)
    #[test]
    fn visible_forest_dedups_matched_subtask() {
        let all = resolved(&["- [ ] parent", "  - [ ] child"]);
        let both = vec![key(1), key(2)];
        let expanded = HashSet::from([key(1)]); // expand the parent
        let rows = expand_forest_visible(&all, &both, &expanded);
        // Exactly two rows: parent at depth 0 (expanded), child nested at 1.
        assert_eq!(
            rows.len(),
            2,
            "matched child must not also appear as a root"
        );
        assert_eq!(rows[0].idx, 0);
        assert_eq!(rows[0].depth, 0);
        assert!(rows[0].expanded);
        assert_eq!(rows[1].idx, 1);
        assert_eq!(rows[1].depth, 1);
        assert!(!rows[1].expanded);
    }

    /// Collapsed parent emits as a leaf (has_children true, expanded false);
    /// its subtasks are not emitted until it is expanded.
    #[test]
    fn visible_forest_collapsed_parent_hides_children() {
        let all = resolved(&["- [ ] parent", "  - [ ] child"]);
        let both = vec![key(1), key(2)];
        let expanded = HashSet::new(); // collapsed
        let rows = expand_forest_visible(&all, &both, &expanded);
        assert_eq!(rows.len(), 1, "collapsed parent should hide its child");
        assert!(rows[0].has_children);
        assert!(!rows[0].expanded);
    }

    /// A matched subtask whose parent is NOT matched still renders as a root.
    #[test]
    fn visible_forest_matched_orphan_child_is_root() {
        let all = resolved(&["- [ ] parent", "  - [ ] child"]);
        let child_only = vec![key(2)];
        let rows = expand_forest_visible(&all, &child_only, &HashSet::new());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].idx, 1);
        assert_eq!(rows[0].depth, 0, "un-matched parent → child is a root");
    }
}

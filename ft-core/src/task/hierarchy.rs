use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::Task;

/// Stable identity of a task across a scan: its source file plus 1-indexed
/// line. The same key the graph uses for its task nodes.
pub type TaskKey = (PathBuf, usize);

/// A node in a depth-annotated task forest: an index into the caller's task
/// slice, the node's depth (0 = root), and whether it has any subtasks in the
/// underlying vault (for a collapse/expand affordance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForestRow {
    pub idx: usize,
    pub depth: usize,
    pub has_children: bool,
}

/// Expand an ordered set of matched tasks into a depth-annotated forest:
/// every matched task **plus all of its transitive subtasks**, even subtasks
/// that aren't themselves matched. The result is in pre-order — each task is
/// immediately followed by its subtree.
///
/// `all` is the full task slice (one scan); `matched` lists the keys to root
/// the forest on, already in the caller's desired root order. A matched task
/// whose parent is also present (matched, or pulled in as a subtask) nests
/// under that parent rather than becoming a second root, so nothing appears
/// twice. Siblings within a subtree follow source order.
pub fn expand_forest(all: &[Task], matched: &[TaskKey]) -> Vec<ForestRow> {
    let index = key_index(all);
    let children = children_map(all, &index);

    // Display set = matched ∪ all descendants of matched.
    let mut display: HashSet<usize> = HashSet::new();
    for key in matched {
        if let Some(&i) = index.get(key) {
            collect_subtree(i, &children, &mut display);
        }
    }

    // Roots: matched tasks whose parent is not itself in the display set.
    // (Descendants always have a matched ancestor, so they're never roots.)
    let mut rows = Vec::with_capacity(display.len());
    let mut seen: HashSet<usize> = HashSet::new();
    for key in matched {
        let Some(&i) = index.get(key) else { continue };
        if parent_idx(all, &index, i).is_some_and(|p| display.contains(&p)) {
            continue; // nests under a parent we'll emit
        }
        emit_subtree(i, 0, &children, &display, &mut seen, &mut rows);
    }
    rows
}

/// Pre-order walk emitting `idx` then its in-display children (source order).
fn emit_subtree(
    idx: usize,
    depth: usize,
    children: &HashMap<usize, Vec<usize>>,
    display: &HashSet<usize>,
    seen: &mut HashSet<usize>,
    rows: &mut Vec<ForestRow>,
) {
    if !seen.insert(idx) {
        return;
    }
    let kids = children.get(&idx);
    rows.push(ForestRow {
        idx,
        depth,
        has_children: kids.is_some_and(|k| !k.is_empty()),
    });
    if let Some(kids) = kids {
        for &child in kids {
            if display.contains(&child) {
                emit_subtree(child, depth + 1, children, display, seen, rows);
            }
        }
    }
}

/// Collect `idx` and all of its transitive descendants into `out`.
fn collect_subtree(idx: usize, children: &HashMap<usize, Vec<usize>>, out: &mut HashSet<usize>) {
    if !out.insert(idx) {
        return;
    }
    if let Some(kids) = children.get(&idx) {
        for &child in kids {
            collect_subtree(child, children, out);
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

/// Index of the direct parent task of `all[idx]`, if it has one.
fn parent_idx(all: &[Task], index: &HashMap<TaskKey, usize>, idx: usize) -> Option<usize> {
    let t = &all[idx];
    let pline = t.parent?;
    index.get(&(t.source_file.clone(), pline)).copied()
}

/// Map each parent task index to its direct subtasks' indices, in source
/// order. Indices point into `all`. Useful for lazy, one-level-at-a-time
/// expansion (e.g. the TUI tree). Built from `parent` pointers, so hierarchy
/// must already be resolved.
pub fn child_index_map(all: &[Task]) -> HashMap<usize, Vec<usize>> {
    children_map(all, &key_index(all))
}

/// Map each parent task index to its direct children's indices, in source
/// order. Built from `parent` pointers, so hierarchy must already be resolved.
fn children_map(all: &[Task], index: &HashMap<TaskKey, usize>) -> HashMap<usize, Vec<usize>> {
    let mut map: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, t) in all.iter().enumerate() {
        if let Some(pline) = t.parent {
            if let Some(&p) = index.get(&(t.source_file.clone(), pline)) {
                map.entry(p).or_default().push(i);
            }
        }
    }
    // Don't trust scan order: sort each sibling list by source line so the
    // forest reads top-to-bottom regardless of how the scan was assembled.
    for kids in map.values_mut() {
        kids.sort_by_key(|&i| all[i].source_line);
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
}

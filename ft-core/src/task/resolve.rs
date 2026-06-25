//! Resolve tasks by graph query (shared by CLI bulk verbs and the TUI).
//!
//! The pattern — parse a DSL string (or use a pre-parsed `GraphQuery`),
//! `select` NoteIds from the graph, map each `NodeKind::Task` node back to
//! its `(source_file, source_line)` key — is duplicated across `ft tasks
//! list`, `ft tasks move --query`, the Tasks-tab `recompute_matches`, and
//! (via this change) `ft tasks complete --query` / `cancel` / `edit`. This
//! module is the single home for it.
//!
//! AND-composition: when multiple queries are passed, a task must match
//! every one (each query produces a set of `NoteId`s; the result is their
//! intersection, projected to task keys).

use std::collections::HashSet;

use crate::graph::{query::GraphQuery, Graph, NodeKind};

pub use crate::task::hierarchy::TaskKey;

/// Resolve the set of task keys matched by a single graph query.
///
/// Each `NodeKind::Task` node in `q.select(graph)` is projected to its
/// `(source_file, source_line)` key. Non-task nodes are ignored.
pub fn by_query(graph: &Graph, q: &GraphQuery) -> HashSet<TaskKey> {
    q.select(graph)
        .into_iter()
        .filter_map(|id| match graph.node(id) {
            NodeKind::Task(td) => Some((td.source_file.clone(), td.source_line)),
            _ => None,
        })
        .collect()
}

/// AND-compose multiple queries: a task must match every query. Returns
/// the intersected key set. An empty `queries` slice returns an empty set
/// (callers that want "all tasks when no query" should handle that branch
/// themselves, as `run_list` does).
pub fn by_queries<'a>(
    graph: &Graph,
    queries: impl IntoIterator<Item = &'a GraphQuery>,
) -> HashSet<TaskKey> {
    let mut acc: Option<HashSet<TaskKey>> = None;
    for q in queries {
        let keys = by_query(graph, q);
        acc = Some(match acc {
            None => keys,
            Some(prev) => prev.intersection(&keys).cloned().collect(),
        });
    }
    acc.unwrap_or_default()
}

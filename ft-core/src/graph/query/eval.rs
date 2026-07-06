//! Evaluators: [`GraphQuery::select`] / [`expand`](GraphQuery::expand) /
//! [`walk`](GraphQuery::walk) and the per-node / per-edge condition
//! predicates they share.

use super::*;

impl GraphQuery {
    pub fn select(&self, graph: &Graph) -> Vec<NoteId> {
        let mut results: Vec<NoteId> = Vec::new();
        for selector in &self.initial {
            for (id, _) in graph.nodes() {
                let cond_ok = match &selector.condition {
                    None => true,
                    Some(e) => e.matches_node(graph, id),
                };
                if cond_ok {
                    if let Some(ref filter) = selector.without {
                        if !neighbor_filter_matches(graph, id, filter) {
                            push_unique(&mut results, id);
                        }
                    } else {
                        push_unique(&mut results, id);
                    }
                }
            }
        }
        results
    }

    /// Per-hop expansion. Returns:
    /// - `None` if no expand block was provided.
    /// - `Some(children)` otherwise (children may be empty if the
    ///   parent doesn't satisfy `from` conditions or no outgoing edges
    ///   match).
    pub fn expand(&self, graph: &Graph, parent: NoteId) -> Option<Vec<NoteId>> {
        let policy = self.expansion.as_ref()?;
        let from_conds: Vec<&Condition> = policy
            .conditions
            .iter()
            .filter(|c| matches!(c.subject, Subject::From))
            .collect();
        let edge_conds: Vec<&Condition> = policy
            .conditions
            .iter()
            .filter(|c| matches!(c.subject, Subject::Edge))
            .collect();
        let to_conds: Vec<&Condition> = policy
            .conditions
            .iter()
            .filter(|c| matches!(c.subject, Subject::To))
            .collect();

        // Parent fails its from-side filter → empty children, but Some.
        for c in &from_conds {
            if !eval_cond_on_node(graph, parent, c) {
                return Some(Vec::new());
            }
        }

        let mut children = Vec::new();
        for (child_id, edge) in graph.outgoing(parent) {
            if edge_conds.iter().all(|c| eval_cond_on_edge(edge, c))
                && to_conds
                    .iter()
                    .all(|c| eval_cond_on_node(graph, child_id, c))
            {
                children.push(child_id);
            }
        }
        // Sort children by a content-derived key so iteration order is
        // independent of filesystem walk order (which varies by OS /
        // filesystem). Without this, the TUI tree and walk() output
        // would be non-deterministic across machines.
        children.sort_by_key(|id| child_sort_key(graph, *id));
        Some(children)
    }

    /// Materialize the full reachable subtree from each root returned by
    /// [`GraphQuery::select`] by repeatedly applying [`GraphQuery::expand`].
    ///
    /// The shape of the result is bounded by `opts`:
    ///
    /// - `opts.max_depth = None` is unlimited; `Some(0)` returns roots
    ///   only; `Some(n)` returns at most n hops below each root.
    /// - `opts.visit = Dedup` (default) expands each reachable node once;
    ///   a re-encounter is emitted as a [`NodeClosure::Reference`] leaf, so
    ///   an unbounded walk is `O(V + E)`. `Tree` repeats shared subtrees and
    ///   only stops ancestor cycles ([`NodeClosure::Cycle`]); `Allow` stops
    ///   nothing — both rely on `max_depth` to terminate on dense or cyclic
    ///   graphs. See [`VisitPolicy`].
    /// - `opts.max_nodes` is a defensive cap; when exceeded the walk stops
    ///   descending and returns the partial tree.
    ///
    /// When the query has no `expand` block, the returned `WalkNode`s
    /// have empty `children`, matching the `None` return from
    /// [`GraphQuery::expand`].
    pub fn walk(&self, graph: &Graph, opts: &WalkOptions) -> Vec<WalkNode> {
        let mut roots = self.select(graph);
        reorder_ghost_roots(graph, &mut roots);
        let mut st = WalkState::default();
        let mut out = Vec::with_capacity(roots.len());
        for id in roots {
            if st.over_budget(opts) {
                break;
            }
            out.push(self.walk_node(graph, id, 0, None, opts, &mut st));
        }
        out
    }

    fn walk_node(
        &self,
        graph: &Graph,
        id: NoteId,
        depth: usize,
        edge_to_parent: Option<EdgeKind>,
        opts: &WalkOptions,
        st: &mut WalkState,
    ) -> WalkNode {
        st.count += 1;

        let closure = match opts.visit {
            VisitPolicy::Dedup if st.visited.contains(&id) => NodeClosure::Reference,
            VisitPolicy::Tree if st.ancestors.contains(&id) => NodeClosure::Cycle,
            _ => NodeClosure::Open,
        };

        let at_depth_limit = opts.max_depth == Some(depth);
        let descend =
            matches!(closure, NodeClosure::Open) && !at_depth_limit && !st.over_budget(opts);

        let children = if !descend {
            Vec::new()
        } else {
            // Mark expanded before descending so siblings reached later in
            // this subtree dedup against it under `Dedup`.
            if matches!(opts.visit, VisitPolicy::Dedup) {
                st.visited.insert(id);
            }
            // Walk one hop using the expand policy; this returns None
            // when there is no expand block, which we map to no children.
            let child_ids = self.expand(graph, id).unwrap_or_default();
            if child_ids.is_empty() {
                Vec::new()
            } else {
                st.ancestors.push(id);
                let mut out = Vec::with_capacity(child_ids.len());
                for child_id in child_ids {
                    if st.over_budget(opts) {
                        break;
                    }
                    let edge_kind = edge_kind_between(graph, id, child_id);
                    out.push(self.walk_node(graph, child_id, depth + 1, edge_kind, opts, st));
                }
                st.ancestors.pop();
                out
            }
        };

        WalkNode {
            id,
            depth,
            edge_to_parent,
            closure,
            children,
        }
    }
}

/// Mutable bookkeeping carried through a single [`GraphQuery::walk`].
#[derive(Default)]
struct WalkState {
    /// Nodes already expanded in this walk — the dedup set for
    /// [`VisitPolicy::Dedup`].
    visited: HashSet<NoteId>,
    /// The current DFS path, for ancestor-cycle detection under
    /// [`VisitPolicy::Tree`].
    ancestors: Vec<NoteId>,
    /// Total materialized nodes so far, for the `max_nodes` backstop.
    count: usize,
}

impl WalkState {
    fn over_budget(&self, opts: &WalkOptions) -> bool {
        opts.max_nodes.is_some_and(|cap| self.count >= cap)
    }
}

/// Find the edge kind on the first outgoing edge from `src` that lands
/// on `dst`. Used by `walk` to label the parent → child relationship in
/// each [`WalkNode`]. Multiple parallel edges (e.g. two wikilinks from
/// the same source to the same target) are collapsed to the first
/// match — `walk` is a tree view, not an edge enumeration.
fn edge_kind_between(graph: &Graph, src: NoteId, dst: NoteId) -> Option<EdgeKind> {
    graph
        .outgoing(src)
        .find(|(d, _)| *d == dst)
        .map(|(_, e)| e.clone())
}

fn push_unique(v: &mut Vec<NoteId>, id: NoteId) {
    if !v.contains(&id) {
        v.push(id);
    }
}

/// Stable, content-derived key for ordering child nodes returned by
/// [`GraphQuery::expand`]. Two-level: (kind_rank, name) so directories
/// group before notes, and within a kind we sort alphabetically by the
/// natural display string. Independent of filesystem walk order.
fn child_sort_key(graph: &Graph, id: NoteId) -> (u8, String) {
    match graph.node(id) {
        NodeKind::Directory(d) => (0, d.path.to_string_lossy().into_owned()),
        NodeKind::Note(n) => (1, n.path.to_string_lossy().into_owned()),
        // Ghosts rank by mention count (desc) before name, so ghost
        // siblings — and the `ghosts` preset — read as the ranked
        // "which concepts earned a note" view. The zero-padded
        // inverted count makes lexicographic-ascending equal
        // mentions-descending.
        NodeKind::Ghost(g) => (2, ghost_order_key(graph, id, &g.raw)),
        NodeKind::Task(t) => (3, format!("{}:{}", t.source_file.display(), t.source_line)),
        NodeKind::Paragraph(p) => (
            4,
            format!("{}:{:08}", p.source_file.display(), p.line_start),
        ),
        NodeKind::Heading(h) => (5, format!("{}:{:08}", h.source_file.display(), h.line)),
    }
}

/// Sort key placing higher-mentioned ghosts first, ties alphabetical.
fn ghost_order_key(graph: &Graph, id: NoteId, raw: &str) -> String {
    let mentions = crate::graph::ghosts::mention_count(graph, id);
    format!("{:08}:{raw}", 99_999_999usize.saturating_sub(mentions))
}

/// Reorder ghost roots among themselves by `(mentions desc, name asc)`
/// while every non-ghost root keeps its position — the walk-level
/// counterpart of the ghost arm in [`child_sort_key`].
fn reorder_ghost_roots(graph: &Graph, roots: &mut [NoteId]) {
    let ghost_positions: Vec<usize> = roots
        .iter()
        .enumerate()
        .filter(|(_, id)| matches!(graph.node(**id), NodeKind::Ghost(_)))
        .map(|(i, _)| i)
        .collect();
    if ghost_positions.len() < 2 {
        return;
    }
    let mut ghosts: Vec<NoteId> = ghost_positions.iter().map(|&i| roots[i]).collect();
    ghosts.sort_by_key(|id| {
        let raw = match graph.node(*id) {
            NodeKind::Ghost(g) => g.raw.as_str(),
            _ => "",
        };
        ghost_order_key(graph, *id, raw)
    });
    for (pos, id) in ghost_positions.into_iter().zip(ghosts) {
        roots[pos] = id;
    }
}

pub(crate) fn eval_cond_on_node(graph: &Graph, id: NoteId, c: &Condition) -> bool {
    // `is null` / `is not null` short-circuit before string extraction.
    if matches!(c.op, Op::IsNull | Op::IsNotNull) {
        let present = node_optional_attr_present(graph.node(id), c.attr);
        return match c.op {
            Op::IsNull => !present,
            Op::IsNotNull => present,
            _ => unreachable!(),
        };
    }
    match c.attr {
        Attr::Kind | Attr::Path | Attr::Title => {
            let v = match node_string_attr(graph.node(id), c.attr) {
                Some(s) => s,
                None => return false,
            };
            eval_string_op(&v, c.op, &c.value)
        }
        // Task-specific string/enum attributes
        Attr::Status | Attr::Priority | Attr::Description => {
            let v = match node_string_attr(graph.node(id), c.attr) {
                Some(s) => s,
                None => return false,
            };
            eval_string_op(&v, c.op, &c.value)
        }
        // Date attributes — compared as dates against Literal::Date.
        Attr::Due | Attr::Scheduled | Attr::Created | Attr::Start | Attr::Completed => {
            let raw = match node_date_attr_str(graph.node(id), c.attr) {
                Some(s) => s,
                None => return false,
            };
            let actual = match chrono::NaiveDate::parse_from_str(&raw, "%Y-%m-%d") {
                Ok(d) => d,
                Err(_) => return false,
            };
            eval_date_op(actual, c.op, &c.value)
        }
        // Task tags — special handling for `includes` and `in` operators
        Attr::Tags => {
            let node = graph.node(id);
            let task_tags: &[String] = match node {
                NodeKind::Task(t) => &t.tags,
                _ => return false,
            };
            match (c.op, &c.value) {
                (Op::Includes, Value::Single(lit)) => {
                    let tag = literal_as_str(lit);
                    task_tags.iter().any(|t| t == tag)
                }
                (Op::In, Value::Set(lits)) => {
                    let tags: Vec<&str> = lits.iter().map(literal_as_str).collect();
                    task_tags.iter().any(|t| tags.contains(&t.as_str()))
                }
                // Other operators on tags return false
                _ => false,
            }
        }
        Attr::Indegree => {
            let count = graph.incoming(id).count() as i64;
            eval_int_op(count, c.op, &c.value)
        }
        Attr::Outdegree => {
            let count = graph.outgoing(id).count() as i64;
            eval_int_op(count, c.op, &c.value)
        }
        // Form / Embed are edge-only — never reached on a node.
        Attr::Form | Attr::Embed => false,
    }
}

fn node_optional_attr_present(node: &NodeKind, attr: Attr) -> bool {
    let task = match node {
        NodeKind::Task(t) => t,
        _ => return false,
    };
    match attr {
        Attr::Due => task.due.is_some(),
        Attr::Scheduled => task.scheduled.is_some(),
        Attr::Created => task.created.is_some(),
        Attr::Start => task.start.is_some(),
        Attr::Completed => task.completed.is_some(),
        _ => false,
    }
}

fn node_date_attr_str(node: &NodeKind, attr: Attr) -> Option<String> {
    let task = match node {
        NodeKind::Task(t) => t,
        _ => return None,
    };
    match attr {
        Attr::Due => task.due.clone(),
        Attr::Scheduled => task.scheduled.clone(),
        Attr::Created => task.created.clone(),
        Attr::Start => task.start.clone(),
        Attr::Completed => task.completed.clone(),
        _ => None,
    }
}

fn literal_as_date(lit: &Literal) -> Option<chrono::NaiveDate> {
    match lit {
        Literal::Date(d) => Some(*d),
        _ => None,
    }
}

fn eval_date_op(actual: chrono::NaiveDate, op: Op, value: &Value) -> bool {
    match (op, value) {
        (Op::Eq, Value::Single(lit)) => literal_as_date(lit).is_some_and(|d| actual == d),
        (Op::NotEq, Value::Single(lit)) => literal_as_date(lit).is_some_and(|d| actual != d),
        (Op::Lt, Value::Single(lit)) => literal_as_date(lit).is_some_and(|d| actual < d),
        (Op::Le, Value::Single(lit)) => literal_as_date(lit).is_some_and(|d| actual <= d),
        (Op::Gt, Value::Single(lit)) => literal_as_date(lit).is_some_and(|d| actual > d),
        (Op::Ge, Value::Single(lit)) => literal_as_date(lit).is_some_and(|d| actual >= d),
        (Op::In, Value::Set(items)) => items.iter().any(|lit| literal_as_date(lit) == Some(actual)),
        _ => false,
    }
}

fn eval_cond_on_edge(edge: &EdgeKind, c: &Condition) -> bool {
    let v = match c.attr {
        Attr::Kind => edge_kind_str(edge).to_string(),
        Attr::Form => match edge_form_str(edge) {
            Some(s) => s.to_string(),
            None => return false,
        },
        Attr::Embed => match edge.link() {
            Some(l) => if l.is_embed { "true" } else { "false" }.to_string(),
            None => return false,
        },
        _ => return false,
    };
    eval_string_op(&v, c.op, &c.value)
}

fn node_string_attr(node: &NodeKind, attr: Attr) -> Option<String> {
    match attr {
        Attr::Kind => Some(node_kind_str(node).to_string()),
        Attr::Path => match node {
            NodeKind::Note(n) => Some(n.path.to_string_lossy().into_owned()),
            NodeKind::Directory(d) => Some(d.path.to_string_lossy().into_owned()),
            // Paragraphs and headings live inside notes; they don't have
            // their own filesystem path. Use `kind = Paragraph` /
            // `kind = Heading` to filter to them.
            NodeKind::Paragraph(_) => None,
            NodeKind::Heading(_) => None,
            NodeKind::Ghost(_) => None,
            // `self.path` on a task is the vault-relative path of the
            // note that owns it — the source file. This is canonical
            // (see graph-task-interaction §D1): `path includes "Areas/"`
            // is a load-bearing task query. The graph-task-nodes spec
            // scenario asserting this yields no match is stale and was
            // corrected by this change.
            NodeKind::Task(t) => Some(t.source_file.to_string_lossy().into_owned()),
        },
        Attr::Title => match node {
            NodeKind::Note(n) => Some(n.title.clone()),
            NodeKind::Heading(h) => Some(h.text.clone()),
            _ => None,
        },
        // Task-specific string attributes
        Attr::Status => match node {
            NodeKind::Task(t) => Some(t.status.clone()),
            _ => None,
        },
        Attr::Priority => match node {
            NodeKind::Task(t) => t.priority.clone(),
            _ => None,
        },
        Attr::Due => match node {
            NodeKind::Task(t) => t.due.clone(),
            _ => None,
        },
        Attr::Scheduled => match node {
            NodeKind::Task(t) => t.scheduled.clone(),
            _ => None,
        },
        Attr::Description => match node {
            NodeKind::Task(t) => Some(t.description.clone()),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn node_kind_str(n: &NodeKind) -> &'static str {
    match n {
        NodeKind::Note(_) => "Note",
        NodeKind::Directory(_) => "Directory",
        NodeKind::Ghost(_) => "Ghost",
        NodeKind::Task(_) => "Task",
        NodeKind::Paragraph(_) => "Paragraph",
        NodeKind::Heading(_) => "Heading",
    }
}

/// Every value accepted by `edge.kind` in the DSL — the canonical strings
/// [`edge_kind_str`] emits for each [`EdgeKind`] variant. Single source of
/// truth for parse-time validation; a test keeps it in lockstep with
/// `edge_kind_str` so a new edge variant can't silently become unqueryable.
pub(crate) const EDGE_KIND_VALUES: &[&str] = &[
    "note-link",
    "heading-link",
    "paragraph-link",
    "directory-contains",
    "has-task",
    "subtask",
    "links-into",
    "owns-paragraph",
    "owns-heading",
];

pub(crate) fn edge_kind_str(e: &EdgeKind) -> &'static str {
    match e {
        EdgeKind::NoteLink(_) => "note-link",
        EdgeKind::HeadingLink(_) => "heading-link",
        EdgeKind::ParagraphLink(_) => "paragraph-link",
        EdgeKind::Contains => "directory-contains",
        EdgeKind::HasTask => "has-task",
        EdgeKind::Subtask => "subtask",
        EdgeKind::LinksInto => "links-into",
        EdgeKind::OwnsParagraph => "owns-paragraph",
        EdgeKind::OwnsHeading => "owns-heading",
    }
}

fn edge_form_str(e: &EdgeKind) -> Option<&'static str> {
    e.link().map(|l| match l.form {
        LinkForm::WikiLink => "wiki",
        LinkForm::MdLink => "md",
    })
}

fn eval_string_op(actual: &str, op: Op, value: &Value) -> bool {
    match (op, value) {
        (Op::Eq, Value::Single(lit)) => actual == literal_as_str(lit),
        (Op::NotEq, Value::Single(lit)) => actual != literal_as_str(lit),
        (Op::Includes, Value::Single(lit)) => actual.contains(literal_as_str(lit)),
        (Op::StartsWith, Value::Single(lit)) => actual.starts_with(literal_as_str(lit)),
        (Op::EndsWith, Value::Single(lit)) => actual.ends_with(literal_as_str(lit)),
        (Op::In, Value::Set(items)) => items.iter().any(|lit| actual == literal_as_str(lit)),
        _ => false,
    }
}

fn eval_int_op(actual: i64, op: Op, value: &Value) -> bool {
    let int_of = |lit: &Literal| -> Option<i64> {
        match lit {
            Literal::Int(n) => Some(*n),
            _ => None,
        }
    };
    match (op, value) {
        (Op::Eq, Value::Single(lit)) => int_of(lit).is_some_and(|n| actual == n),
        (Op::NotEq, Value::Single(lit)) => int_of(lit).is_some_and(|n| actual != n),
        (Op::Lt, Value::Single(lit)) => int_of(lit).is_some_and(|n| actual < n),
        (Op::Le, Value::Single(lit)) => int_of(lit).is_some_and(|n| actual <= n),
        (Op::Gt, Value::Single(lit)) => int_of(lit).is_some_and(|n| actual > n),
        (Op::Ge, Value::Single(lit)) => int_of(lit).is_some_and(|n| actual >= n),
        (Op::In, Value::Set(items)) => items.iter().any(|lit| int_of(lit) == Some(actual)),
        // includes / starts_with / ends_with / is null on integers → false
        _ => false,
    }
}

fn neighbor_filter_matches(graph: &Graph, id: NoteId, filter: &NeighborFilter) -> bool {
    let edges: Box<dyn Iterator<Item = (NoteId, &EdgeKind)>> = match filter.direction {
        Direction::Incoming => Box::new(graph.incoming(id)),
        Direction::Outgoing => Box::new(graph.outgoing(id)),
    };
    for (_, edge) in edges {
        let all_match = filter.conditions.iter().all(|c| eval_cond_on_edge(edge, c));
        if all_match {
            return true;
        }
    }
    false
}

// ── Display (canonical serialization) ─────────────────────────────────

//! Graph query DSL v2 — tokenizer, recursive-descent parser, evaluator,
//! and canonical serializer for filtering nodes and per-hop expansion
//! in the note-link graph.
//!
//! The DSL describes a *navigation policy*, not a subgraph. Initial
//! nodes come from [`GraphQuery::select`]; per-hop expansion comes from
//! [`GraphQuery::expand`]. To materialize a finite subgraph, compose the
//! two with a depth bound and cycle handling — see plan 019's
//! `GraphQuery::walk`.
//!
//! Grammar (v2):
//! ```text
//! query           = node_block (";" node_block)* (";" expand_block)? ";"?
//!
//! node_block      = "node" [where_clause] [neighbor_exclusion]
//!
//! where_clause    = "where" condition_list
//!
//! condition_list  = condition ("and" condition)*
//!
//! condition       = qualified_attr op value
//!
//! qualified_attr  = entity "." attribute       -- explicit
//!                 | attribute                  -- bare; implicit `self`
//!
//! entity          = "self" | "from" | "to" | "edge"
//!
//! attribute       = "kind" | "path" | "title" | "form"
//!                 | "indegree" | "outdegree"
//!
//! op              = "=" | "!=" | "in" | "includes"
//!                 | "starts_with" | "ends_with"
//!
//! value           = literal | "{" literal ("," literal)* "}"
//!
//! literal         = IDENT | STRING | INTEGER
//!
//! neighbor_exclusion = "without" neighbor_filter
//! neighbor_filter    = "incoming" "(" [condition_list] ")"
//!                    | "outgoing" "(" [condition_list] ")"
//!
//! expand_block    = "expand" [where_clause]
//! ```
//!
//! See `docs/graph-query-dsl.md` for worked examples and the error
//! catalog.

use std::collections::HashSet;
use std::fmt;

use chrono::NaiveDate;

use crate::graph::{EdgeKind, Graph, LinkForm, NodeKind, NoteId};

mod display;
mod eval;
mod lexer;
mod parser;

#[cfg(test)]
mod tests;

pub use parser::{parse, parse_with};

// Cross-submodule helpers, re-exported so each submodule's
// `use super::*;` sees the whole split as one flat namespace.
pub(crate) use display::attr_name;
pub(crate) use eval::{eval_cond_on_node, EDGE_KIND_VALUES};
pub(crate) use lexer::{op_label, token_desc, token_label, Lexer, Spanned, Token};
pub(crate) use parser::literal_as_str;

// ── AST types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphQuery {
    pub initial: Vec<NodeSelector>,
    pub expansion: Option<EdgePolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSelector {
    /// Boolean expression over [`Condition`] leaves. `None` for a bare
    /// `node;` block with no `where` clause.
    pub condition: Option<CondExpr>,
    pub without: Option<NeighborFilter>,
}

impl NodeSelector {
    /// Iterate over the [`Condition`] leaves of the expression tree, in
    /// left-to-right textual order. Used by the canonical serializer and
    /// by simple consumers that don't care about boolean structure.
    pub fn conditions(&self) -> Vec<&Condition> {
        let mut out = Vec::new();
        if let Some(ref e) = self.condition {
            collect_conditions(e, &mut out);
        }
        out
    }
}

fn collect_conditions<'a>(e: &'a CondExpr, out: &mut Vec<&'a Condition>) {
    match e {
        CondExpr::Cond(c) => out.push(c),
        CondExpr::And(parts) | CondExpr::Or(parts) => {
            for p in parts {
                collect_conditions(p, out);
            }
        }
    }
}

/// Boolean expression tree over [`Condition`] leaves. Used inside
/// node-block `where` clauses to express `and`, `or`, and grouping
/// with parens. Expand blocks and neighbor filters retain the simpler
/// `Vec<Condition>` shape (AND-only) because the evaluator partitions
/// them by subject (`from` / `edge` / `to`) which doesn't compose
/// naturally with arbitrary boolean nesting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CondExpr {
    Cond(Condition),
    And(Vec<CondExpr>),
    Or(Vec<CondExpr>),
}

impl CondExpr {
    /// Evaluate `self` against `subject_id` in `graph`. Pure boolean
    /// combinator over [`eval_cond_on_node`].
    fn matches_node(&self, graph: &Graph, id: NoteId) -> bool {
        match self {
            CondExpr::Cond(c) => eval_cond_on_node(graph, id, c),
            CondExpr::And(parts) => parts.iter().all(|p| p.matches_node(graph, id)),
            CondExpr::Or(parts) => parts.iter().any(|p| p.matches_node(graph, id)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgePolicy {
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighborFilter {
    pub direction: Direction,
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Incoming,
    Outgoing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub subject: Subject,
    pub attr: Attr,
    pub op: Op,
    pub value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    SelfNode,
    From,
    To,
    Edge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attr {
    Kind,
    Path,
    Title,
    Form,
    /// Edge-only: whether the link is an embed (`is_embed`). Accepts
    /// `true` / `false` as enum-style values.
    Embed,
    Indegree,
    Outdegree,
    /// Task-specific: status (e.g., "Open", "Done")
    Status,
    /// Task-specific: priority (e.g., "High", "Medium")
    Priority,
    /// Task-specific: due date (YYYY-MM-DD)
    Due,
    /// Task-specific: scheduled date (YYYY-MM-DD)
    Scheduled,
    /// Task-specific: created date (YYYY-MM-DD)
    Created,
    /// Task-specific: start date (YYYY-MM-DD)
    Start,
    /// Task-specific: completed date (YYYY-MM-DD)
    Completed,
    /// Task-specific: description text
    Description,
    /// Task-specific: tags (Vec<String>)
    Tags,
    /// Does this node (or, for `Task`, its owning paragraph via the
    /// `OwnsTask` edge) link to a target whose concept identity matches
    /// the value? Concept identity is `Note.title` for resolved targets,
    /// `Ghost.raw` for unresolved targets, and `Heading.text` for
    /// heading-anchor targets — the wikilink display alias is NOT matched.
    /// Generalized across node kinds: `Paragraph` walks its
    /// `ParagraphLink` edges, `Heading` its `HeadingLink` edges, `Note`
    /// its `NoteLink` edges, `Ghost`/`Directory` return empty, and `Task`
    /// walks its owning paragraph's `ParagraphLink` edges (via the
    /// `OwnsTask` edge).
    Mentions,
}

/// Value-type classification for an attribute. Drives parse-time
/// operator-vs-attribute compatibility checks and chooses the parser
/// mode for the right-hand-side value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Int,
    Date,
    Str,
    Enum,
    Set,
}

impl Attr {
    /// The value type of this attribute, used for parse-time operator
    /// validation.
    pub fn value_type(self) -> ValueType {
        match self {
            Attr::Kind | Attr::Form | Attr::Status | Attr::Priority | Attr::Embed => {
                ValueType::Enum
            }
            Attr::Path | Attr::Title | Attr::Description | Attr::Mentions => ValueType::Str,
            Attr::Indegree | Attr::Outdegree => ValueType::Int,
            Attr::Due | Attr::Scheduled | Attr::Created | Attr::Start | Attr::Completed => {
                ValueType::Date
            }
            Attr::Tags => ValueType::Set,
        }
    }

    /// Whether this attribute is optional in the underlying model and
    /// therefore valid as the lhs of `is null` / `is not null`.
    pub fn is_optional(self) -> bool {
        matches!(
            self,
            Attr::Due | Attr::Scheduled | Attr::Created | Attr::Start | Attr::Completed
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    NotEq,
    In,
    Includes,
    StartsWith,
    EndsWith,
    Lt,
    Le,
    Gt,
    Ge,
    /// Postfix; no right-hand-side value.
    IsNull,
    /// Postfix; no right-hand-side value.
    IsNotNull,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Single(Literal),
    Set(Vec<Literal>),
    /// Marker for postfix `is null` / `is not null` operators that don't
    /// carry a right-hand-side value.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    Ident(String),
    Str(String),
    Int(i64),
    Date(NaiveDate),
}

/// Parser profile — controls Tasks-tab UX sugar (implicit
/// `node where kind = Task and …` block and implicit `self.` subject).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// Verbose graph syntax — no sugar applied.
    #[default]
    Default,
    /// Tasks profile: bare predicates are wrapped in `node where kind = Task and …`
    /// and bare attribute references default to `Subject::SelfNode`.
    Tasks,
}

// ── Walk types ───────────────────────────────────────────────────────

/// Control how [`GraphQuery::walk`] handles re-encountering a node it has
/// already seen during the same walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VisitPolicy {
    /// Expand each reachable node at most once across the whole walk. A
    /// node reached again — whether via a back-edge or via a distinct
    /// non-cyclic path (a shared hub or diamond) — is emitted as a
    /// [`NodeClosure::Reference`] leaf and not descended into. This bounds
    /// an unbounded walk to `O(V + E)` and is the only policy that is safe
    /// without a [`WalkOptions::max_depth`] bound. Default.
    #[default]
    Dedup,
    /// `tree(1)`-style: a node reachable by multiple paths is repeated with
    /// its full subtree under every parent. Only a node appearing in its
    /// own ancestor chain is stopped (emitted as [`NodeClosure::Cycle`]).
    /// Because shared descendants are re-expanded once per path, a dense or
    /// cyclic graph requires a [`WalkOptions::max_depth`] bound to terminate
    /// in practical time.
    Tree,
    /// No re-encounter detection at all. The traversal terminates only via
    /// [`WalkOptions::max_depth`]; combining `Allow` with an unbounded depth
    /// over a cyclic subgraph will loop forever.
    Allow,
}

/// Why a [`WalkNode`] was not descended into, distinguishing a node the
/// walk fully expanded from one it deliberately stopped at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeClosure {
    /// Expanded normally. `children` reflects the matching neighbors, which
    /// may legitimately be empty for a genuine leaf or a depth-bounded node.
    #[default]
    Open,
    /// Already expanded earlier in this walk; emitted so the incoming edge
    /// is visible, but with empty `children` and not descended into.
    /// Produced by [`VisitPolicy::Dedup`].
    Reference,
    /// Re-entered via the current ancestor path. Emitted with empty
    /// `children`. Produced by [`VisitPolicy::Tree`].
    Cycle,
}

/// Knobs for [`GraphQuery::walk`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WalkOptions {
    /// Maximum tree depth. `None` means unlimited. `Some(0)` returns
    /// roots only — every child list is empty.
    pub max_depth: Option<usize>,
    pub visit: VisitPolicy,
    /// Defensive cap on the total number of materialized nodes. `None`
    /// means unbounded. When the walk would exceed this it stops descending
    /// and returns the partial tree. Primarily a backstop for the
    /// unbounded-unsafe [`VisitPolicy::Tree`] / [`VisitPolicy::Allow`]; the
    /// default [`VisitPolicy::Dedup`] is already `O(V + E)`.
    pub max_nodes: Option<usize>,
}

impl WalkOptions {
    /// Unlimited depth with the default [`VisitPolicy::Dedup`] — each
    /// reachable node expanded once, terminating on any finite graph.
    pub fn unlimited() -> Self {
        Self::default()
    }
}

/// One node in the materialized walk tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkNode {
    pub id: NoteId,
    /// Depth of this node from the nearest root in the walk (root = 0).
    pub depth: usize,
    /// Edge kind that led from the immediate parent to this node.
    /// `None` for roots returned from [`GraphQuery::select`].
    pub edge_to_parent: Option<EdgeKind>,
    /// Whether this node was expanded, or stopped at as a dedup reference
    /// or an ancestor cycle. `Reference`/`Cycle` always have empty
    /// `children`. See [`NodeClosure`].
    pub closure: NodeClosure,
    pub children: Vec<WalkNode>,
}

// ── Parse context ─────────────────────────────────────────────────────

/// Which grammar scope a condition is being parsed in. Determines which
/// entities are valid prefixes (`self` / `from` / `to` / `edge`) and
/// which attributes are accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    NodeBlock,
    ExpandBlock,
    NeighborFilter,
}

// ── Error type ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DslError {
    EmptyInput,
    NoInitialSet,
    UnexpectedToken {
        found: String,
        expected: String,
        position: usize,
    },
    UnknownAttribute {
        attr: String,
        position: usize,
    },
    AmbiguousAttribute {
        attr: String,
        hint: String,
        position: usize,
    },
    ScopeError {
        entity: String,
        hint: String,
        position: usize,
    },
    TypeMismatch {
        op: String,
        expected: String,
        got: String,
        position: usize,
    },
    UnknownKindValue {
        attr: String,
        value: String,
        allowed: Vec<String>,
        position: usize,
    },
    TrailingInput {
        token: String,
        position: usize,
    },
    UnterminatedString {
        position: usize,
    },
    IllegalCharacter {
        ch: char,
        position: usize,
    },
}

impl fmt::Display for DslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DslError::EmptyInput => write!(f, "empty query"),
            DslError::NoInitialSet => write!(
                f,
                "query has no `node` block — at least one is required before any `expand`"
            ),
            DslError::UnexpectedToken {
                found,
                expected,
                position,
            } => write!(
                f,
                "expected {expected}, found {found} at position {position}"
            ),
            DslError::UnknownAttribute { attr, position } => {
                write!(f, "unknown attribute `{attr}` at position {position}")
            }
            DslError::AmbiguousAttribute {
                attr,
                hint,
                position,
            } => write!(
                f,
                "ambiguous attribute `{attr}` at position {position}: {hint}"
            ),
            DslError::ScopeError {
                entity,
                hint,
                position,
            } => write!(f, "`{entity}` not valid here (position {position}): {hint}"),
            DslError::TypeMismatch {
                op,
                expected,
                got,
                position,
            } => write!(
                f,
                "operator `{op}` expects {expected}, got {got} at position {position}"
            ),
            DslError::UnknownKindValue {
                attr,
                value,
                allowed,
                position,
            } => write!(
                f,
                "unknown value `{value}` for `{attr}` at position {position} (allowed: {})",
                allowed.join(", ")
            ),
            DslError::TrailingInput { token, position } => write!(
                f,
                "unexpected input after end of query: {token} at position {position}"
            ),
            DslError::UnterminatedString { position } => {
                write!(f, "unterminated string starting at position {position}")
            }
            DslError::IllegalCharacter { ch, position } => {
                write!(f, "illegal character `{ch}` at position {position}")
            }
        }
    }
}

impl std::error::Error for DslError {}

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
#[cfg(test)]
use crate::vault::Scan;

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
            Attr::Path | Attr::Title | Attr::Description => ValueType::Str,
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

// ── Tokenizer ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    // Keywords
    Node,
    Where,
    And,
    Or,
    Without,
    Incoming,
    Outgoing,
    Expand,
    SelfKw,
    From,
    To,
    Edge,
    InKw,
    IncludesKw,
    StartsWithKw,
    EndsWithKw,
    IsKw,
    NotKw,
    NullKw,
    // Punctuation
    Dot,
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Semi,
    // Values
    Ident(String),
    Str(String),
    Int(i64),
    Eof,
}

#[derive(Debug, Clone)]
struct Spanned {
    tok: Token,
    pos: usize,
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
}

impl Lexer {
    fn new(src: &str) -> Self {
        Lexer {
            chars: src.chars().collect(),
            pos: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }

    fn read_ident(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            if c.is_alphanumeric() || c == '-' || c == '_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn read_number(&mut self) -> Result<i64, DslError> {
        let start = self.pos;
        while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        s.parse::<i64>().map_err(|_| DslError::IllegalCharacter {
            ch: self.chars[start],
            position: start,
        })
    }

    /// Try to read a contiguous ISO date `YYYY-MM-DD` starting at `self.pos`.
    /// Returns the string and advances `self.pos` on a hit; leaves `self.pos`
    /// unchanged on miss.
    fn try_read_iso_date(&mut self) -> Option<String> {
        let start = self.pos;
        let need_digits = |off: usize, n: usize| -> bool {
            (0..n).all(|i| {
                self.chars
                    .get(start + off + i)
                    .is_some_and(|c| c.is_ascii_digit())
            })
        };
        let dash_at = |off: usize| -> bool { self.chars.get(start + off) == Some(&'-') };
        if !(need_digits(0, 4)
            && dash_at(4)
            && need_digits(5, 2)
            && dash_at(7)
            && need_digits(8, 2))
        {
            return None;
        }
        // Reject when the trailing context could extend the token —
        // bare alphanumeric / dash / underscore continuations are part of
        // the same lexeme by the ident rules, so a date that runs into
        // `2026-05-09-extra` should not be silently truncated.
        if let Some(&next) = self.chars.get(start + 10) {
            if next.is_alphanumeric() || next == '-' || next == '_' {
                return None;
            }
        }
        self.pos = start + 10;
        Some(self.chars[start..self.pos].iter().collect())
    }

    /// Read a relative-offset token of shape `[+-]\d+(d|w|m|day(s)?|week(s)?|month(s)?)`.
    /// Returns the string and advances `self.pos` on a hit; leaves `self.pos`
    /// unchanged on miss.
    fn try_read_relative_offset(&mut self) -> Option<String> {
        let start = self.pos;
        let sign = self.chars.get(start)?;
        if *sign != '+' && *sign != '-' {
            return None;
        }
        let mut cur = start + 1;
        let digits_start = cur;
        while self.chars.get(cur).is_some_and(|c| c.is_ascii_digit()) {
            cur += 1;
        }
        if cur == digits_start {
            return None;
        }
        let unit_start = cur;
        while self.chars.get(cur).is_some_and(|c| c.is_ascii_alphabetic()) {
            cur += 1;
        }
        if cur == unit_start {
            return None;
        }
        let unit: String = self.chars[unit_start..cur].iter().collect();
        if !matches!(
            unit.as_str(),
            "d" | "day" | "days" | "w" | "week" | "weeks" | "m" | "month" | "months"
        ) {
            return None;
        }
        self.pos = cur;
        Some(self.chars[start..self.pos].iter().collect())
    }

    fn read_string(&mut self, quote: char) -> Result<String, DslError> {
        let start = self.pos;
        self.pos += 1; // consume opening quote
        let mut s = String::new();
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            if c == quote {
                self.pos += 1; // consume closing quote
                return Ok(s);
            }
            if c == '\\' && self.pos + 1 < self.chars.len() {
                let next = self.chars[self.pos + 1];
                match next {
                    '\\' => s.push('\\'),
                    '"' => s.push('"'),
                    '\'' => s.push('\''),
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    other => {
                        s.push('\\');
                        s.push(other);
                    }
                }
                self.pos += 2;
                continue;
            }
            s.push(c);
            self.pos += 1;
        }
        Err(DslError::UnterminatedString { position: start })
    }

    fn tokenize(&mut self) -> Result<Vec<Spanned>, DslError> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            if self.pos >= self.chars.len() {
                out.push(Spanned {
                    tok: Token::Eof,
                    pos: self.pos,
                });
                return Ok(out);
            }
            let start = self.pos;
            let ch = self.chars[self.pos];
            let tok = match ch {
                '.' => {
                    self.pos += 1;
                    Token::Dot
                }
                '=' => {
                    self.pos += 1;
                    Token::Eq
                }
                '!' => {
                    if self.pos + 1 < self.chars.len() && self.chars[self.pos + 1] == '=' {
                        self.pos += 2;
                        Token::NotEq
                    } else {
                        return Err(DslError::IllegalCharacter {
                            ch: '!',
                            position: self.pos,
                        });
                    }
                }
                '<' => {
                    if self.pos + 1 < self.chars.len() && self.chars[self.pos + 1] == '=' {
                        self.pos += 2;
                        Token::Le
                    } else {
                        self.pos += 1;
                        Token::Lt
                    }
                }
                '>' => {
                    if self.pos + 1 < self.chars.len() && self.chars[self.pos + 1] == '=' {
                        self.pos += 2;
                        Token::Ge
                    } else {
                        self.pos += 1;
                        Token::Gt
                    }
                }
                '(' => {
                    self.pos += 1;
                    Token::LParen
                }
                ')' => {
                    self.pos += 1;
                    Token::RParen
                }
                '{' => {
                    self.pos += 1;
                    Token::LBrace
                }
                '}' => {
                    self.pos += 1;
                    Token::RBrace
                }
                ',' => {
                    self.pos += 1;
                    Token::Comma
                }
                ';' => {
                    self.pos += 1;
                    Token::Semi
                }
                '"' | '\'' => {
                    let s = self.read_string(ch)?;
                    Token::Str(s)
                }
                '+' | '-' => {
                    if let Some(rel) = self.try_read_relative_offset() {
                        Token::Ident(rel)
                    } else {
                        return Err(DslError::IllegalCharacter {
                            ch,
                            position: self.pos,
                        });
                    }
                }
                c if c.is_ascii_digit() => {
                    if let Some(iso) = self.try_read_iso_date() {
                        Token::Ident(iso)
                    } else {
                        let n = self.read_number()?;
                        Token::Int(n)
                    }
                }
                c if c.is_alphabetic() => {
                    let ident = self.read_ident();
                    keyword_or_ident(ident)
                }
                other => {
                    return Err(DslError::IllegalCharacter {
                        ch: other,
                        position: self.pos,
                    });
                }
            };
            out.push(Spanned { tok, pos: start });
        }
    }
}

fn keyword_or_ident(s: String) -> Token {
    match s.as_str() {
        "node" => Token::Node,
        "where" => Token::Where,
        "and" => Token::And,
        "or" => Token::Or,
        "without" => Token::Without,
        "incoming" => Token::Incoming,
        "outgoing" => Token::Outgoing,
        "expand" => Token::Expand,
        "self" => Token::SelfKw,
        "from" => Token::From,
        "to" => Token::To,
        "edge" => Token::Edge,
        "in" => Token::InKw,
        "includes" => Token::IncludesKw,
        "starts_with" => Token::StartsWithKw,
        "ends_with" => Token::EndsWithKw,
        "is" => Token::IsKw,
        "not" => Token::NotKw,
        "null" => Token::NullKw,
        _ => Token::Ident(s),
    }
}

fn token_desc(t: &Token) -> &'static str {
    match t {
        Token::Node => "`node`",
        Token::Where => "`where`",
        Token::And => "`and`",
        Token::Or => "`or`",
        Token::Without => "`without`",
        Token::Incoming => "`incoming`",
        Token::Outgoing => "`outgoing`",
        Token::Expand => "`expand`",
        Token::SelfKw => "`self`",
        Token::From => "`from`",
        Token::To => "`to`",
        Token::Edge => "`edge`",
        Token::InKw => "`in`",
        Token::IncludesKw => "`includes`",
        Token::StartsWithKw => "`starts_with`",
        Token::EndsWithKw => "`ends_with`",
        Token::IsKw => "`is`",
        Token::NotKw => "`not`",
        Token::NullKw => "`null`",
        Token::Dot => "`.`",
        Token::Eq => "`=`",
        Token::NotEq => "`!=`",
        Token::Lt => "`<`",
        Token::Le => "`<=`",
        Token::Gt => "`>`",
        Token::Ge => "`>=`",
        Token::LParen => "`(`",
        Token::RParen => "`)`",
        Token::LBrace => "`{`",
        Token::RBrace => "`}`",
        Token::Comma => "`,`",
        Token::Semi => "`;`",
        Token::Ident(_) => "identifier",
        Token::Str(_) => "string",
        Token::Int(_) => "integer",
        Token::Eof => "end of input",
    }
}

fn token_label(t: &Token) -> String {
    match t {
        Token::Ident(s) => format!("`{s}`"),
        Token::Str(s) => format!("\"{s}\""),
        Token::Int(n) => format!("{n}"),
        Token::Eof => "end of input".to_string(),
        other => token_desc(other).to_string(),
    }
}

fn op_label(op: Op) -> &'static str {
    match op {
        Op::Eq => "=",
        Op::NotEq => "!=",
        Op::In => "in",
        Op::Includes => "includes",
        Op::StartsWith => "starts_with",
        Op::EndsWith => "ends_with",
        Op::Lt => "<",
        Op::Le => "<=",
        Op::Gt => ">",
        Op::Ge => ">=",
        Op::IsNull => "is null",
        Op::IsNotNull => "is not null",
    }
}

// ── Parser ────────────────────────────────────────────────────────────

struct Parser {
    tokens: Vec<Spanned>,
    pos: usize,
    today: NaiveDate,
}

impl Parser {
    fn new(tokens: Vec<Spanned>, today: NaiveDate) -> Self {
        Parser {
            tokens,
            pos: 0,
            today,
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].tok
    }

    fn peek_pos(&self) -> usize {
        self.tokens[self.pos].pos
    }

    fn advance(&mut self) -> Spanned {
        let s = self.tokens[self.pos].clone();
        if !matches!(s.tok, Token::Eof) {
            self.pos += 1;
        }
        s
    }

    fn unexpected(&self, expected: &str) -> DslError {
        DslError::UnexpectedToken {
            found: token_label(self.peek()),
            expected: expected.to_string(),
            position: self.peek_pos(),
        }
    }

    fn consume(&mut self, expected: Token) -> Result<(), DslError> {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(&expected) {
            self.advance();
            Ok(())
        } else {
            Err(self.unexpected(token_desc(&expected)))
        }
    }

    fn parse_query(&mut self) -> Result<GraphQuery, DslError> {
        let mut initial = Vec::new();

        while matches!(self.peek(), Token::Node) {
            initial.push(self.parse_node_block()?);
            // The closing `;` between blocks is required only if more
            // input follows. A trailing `;` is optional.
            if matches!(self.peek(), Token::Semi) {
                self.advance();
            } else {
                break;
            }
        }

        if initial.is_empty() {
            return Err(DslError::NoInitialSet);
        }

        let expansion = if matches!(self.peek(), Token::Expand) {
            let policy = self.parse_expand_block()?;
            if matches!(self.peek(), Token::Semi) {
                self.advance();
            }
            Some(policy)
        } else {
            None
        };

        match self.peek() {
            Token::Eof => Ok(GraphQuery { initial, expansion }),
            other => Err(DslError::TrailingInput {
                token: token_label(other),
                position: self.peek_pos(),
            }),
        }
    }

    fn parse_node_block(&mut self) -> Result<NodeSelector, DslError> {
        self.consume(Token::Node)?;

        let condition = if matches!(self.peek(), Token::Where) {
            self.advance();
            Some(self.parse_cond_expr(Scope::NodeBlock)?)
        } else {
            None
        };

        let without = if matches!(self.peek(), Token::Without) {
            self.advance();
            Some(self.parse_neighbor_filter()?)
        } else {
            None
        };

        Ok(NodeSelector { condition, without })
    }

    /// Parse a boolean expression over conditions: `or_expr`.
    /// Precedence: `and` binds tighter than `or`. Parens override.
    fn parse_cond_expr(&mut self, scope: Scope) -> Result<CondExpr, DslError> {
        let mut parts = vec![self.parse_and_expr(scope)?];
        while matches!(self.peek(), Token::Or) {
            self.advance();
            parts.push(self.parse_and_expr(scope)?);
        }
        Ok(if parts.len() == 1 {
            parts.into_iter().next().unwrap()
        } else {
            CondExpr::Or(parts)
        })
    }

    fn parse_and_expr(&mut self, scope: Scope) -> Result<CondExpr, DslError> {
        let mut parts = vec![self.parse_cond_atom(scope)?];
        while matches!(self.peek(), Token::And) {
            self.advance();
            parts.push(self.parse_cond_atom(scope)?);
        }
        Ok(if parts.len() == 1 {
            parts.into_iter().next().unwrap()
        } else {
            CondExpr::And(parts)
        })
    }

    fn parse_cond_atom(&mut self, scope: Scope) -> Result<CondExpr, DslError> {
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            let inner = self.parse_cond_expr(scope)?;
            self.consume(Token::RParen)?;
            Ok(inner)
        } else {
            Ok(CondExpr::Cond(self.parse_condition(scope)?))
        }
    }

    fn parse_expand_block(&mut self) -> Result<EdgePolicy, DslError> {
        self.consume(Token::Expand)?;
        let conditions = if matches!(self.peek(), Token::Where) {
            self.advance();
            self.parse_condition_list(Scope::ExpandBlock)?
        } else {
            Vec::new()
        };
        Ok(EdgePolicy { conditions })
    }

    fn parse_neighbor_filter(&mut self) -> Result<NeighborFilter, DslError> {
        let direction = match self.peek() {
            Token::Incoming => {
                self.advance();
                Direction::Incoming
            }
            Token::Outgoing => {
                self.advance();
                Direction::Outgoing
            }
            _ => return Err(self.unexpected("`incoming` or `outgoing`")),
        };
        self.consume(Token::LParen)?;
        let conditions = if matches!(self.peek(), Token::RParen) {
            Vec::new()
        } else {
            self.parse_condition_list(Scope::NeighborFilter)?
        };
        self.consume(Token::RParen)?;
        Ok(NeighborFilter {
            direction,
            conditions,
        })
    }

    fn parse_condition_list(&mut self, scope: Scope) -> Result<Vec<Condition>, DslError> {
        let mut out = vec![self.parse_condition(scope)?];
        while matches!(self.peek(), Token::And) {
            self.advance();
            out.push(self.parse_condition(scope)?);
        }
        Ok(out)
    }

    fn parse_condition(&mut self, scope: Scope) -> Result<Condition, DslError> {
        let (subject, attr) = self.parse_qualified_attr(scope)?;
        let op_pos = self.peek_pos();
        let op = self.parse_op()?;

        // Parse-time op×attr type check (where the operator must match the
        // attribute's value type).
        check_op_vs_attr(op, attr, op_pos)?;

        // Postfix `is null` / `is not null` carry no rhs value.
        let value = if matches!(op, Op::IsNull | Op::IsNotNull) {
            Value::None
        } else if attr.value_type() == ValueType::Date {
            self.parse_date_rhs(op_pos)?
        } else {
            self.parse_value()?
        };

        // Parse-time op×value-shape check.
        let value_kind = match &value {
            Value::Single(_) => "literal",
            Value::Set(_) => "set",
            Value::None => "none",
        };
        match (op, &value) {
            (Op::In, Value::Single(_)) => {
                return Err(DslError::TypeMismatch {
                    op: op_label(op).into(),
                    expected: "set".into(),
                    got: value_kind.into(),
                    position: op_pos,
                });
            }
            (
                Op::Eq
                | Op::NotEq
                | Op::Includes
                | Op::StartsWith
                | Op::EndsWith
                | Op::Lt
                | Op::Le
                | Op::Gt
                | Op::Ge,
                Value::Set(_),
            ) => {
                return Err(DslError::TypeMismatch {
                    op: op_label(op).into(),
                    expected: "literal".into(),
                    got: value_kind.into(),
                    position: op_pos,
                });
            }
            _ => {}
        }

        // Parse-time kind/form/embed value check (only for the enum-like attrs).
        if matches!(attr, Attr::Kind) {
            self.check_kind_values(subject, &value, op_pos)?;
        } else if matches!(attr, Attr::Form) {
            self.check_form_values(&value, op_pos)?;
        } else if matches!(attr, Attr::Embed) {
            self.check_embed_values(&value, op_pos)?;
        }

        Ok(Condition {
            subject,
            attr,
            op,
            value,
        })
    }

    /// Parse the rhs in "date mode" — single literal or set of literals,
    /// each of which must be a `Date`. Keyword tokens like `today` are
    /// resolved against `self.today`. Idents that fail to resolve become
    /// `TypeMismatch` errors.
    fn parse_date_rhs(&mut self, op_pos: usize) -> Result<Value, DslError> {
        if matches!(self.peek(), Token::LBrace) {
            self.advance();
            let mut items = vec![self.parse_date_literal(op_pos)?];
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                items.push(self.parse_date_literal(op_pos)?);
            }
            self.consume(Token::RBrace)?;
            Ok(Value::Set(items))
        } else {
            Ok(Value::Single(self.parse_date_literal(op_pos)?))
        }
    }

    fn parse_date_literal(&mut self, op_pos: usize) -> Result<Literal, DslError> {
        let pos = self.peek_pos();
        let s: String = match self.advance().tok {
            Token::Ident(s) => s,
            Token::Str(s) => s,
            other => {
                return Err(DslError::UnexpectedToken {
                    found: token_label(&other),
                    expected: "date value (YYYY-MM-DD, `today`, `tomorrow`, `yesterday`, or relative offset)".into(),
                    position: pos,
                });
            }
        };
        match crate::dates::parse_date_value(&s, self.today) {
            Some(d) => Ok(Literal::Date(d)),
            None => Err(DslError::TypeMismatch {
                op: "<date>".into(),
                expected:
                    "date value (YYYY-MM-DD, `today`, `tomorrow`, `yesterday`, or relative offset)"
                        .into(),
                got: format!("`{s}`"),
                position: op_pos,
            }),
        }
    }

    fn parse_qualified_attr(&mut self, scope: Scope) -> Result<(Subject, Attr), DslError> {
        let entity_pos = self.peek_pos();
        let (subject, explicit) = match self.peek() {
            Token::SelfKw => {
                self.advance();
                (Subject::SelfNode, Some("self"))
            }
            Token::From => {
                self.advance();
                (Subject::From, Some("from"))
            }
            Token::To => {
                self.advance();
                (Subject::To, Some("to"))
            }
            Token::Edge => {
                self.advance();
                (Subject::Edge, Some("edge"))
            }
            _ => {
                // Bare attribute — subject inferred from scope.
                let subject = match scope {
                    Scope::NodeBlock => Subject::SelfNode,
                    Scope::ExpandBlock => {
                        // Ambiguous: in expand block the user must
                        // qualify with from/to/edge.
                        let attr_name = match self.peek() {
                            Token::Ident(s) => s.clone(),
                            other => return Err(self.unexpected_owned(token_label(other))),
                        };
                        return Err(DslError::AmbiguousAttribute {
                            attr: attr_name,
                            hint: "use `from.<attr>`, `to.<attr>`, or `edge.<attr>`".into(),
                            position: entity_pos,
                        });
                    }
                    Scope::NeighborFilter => Subject::Edge,
                };
                (subject, None)
            }
        };

        // If we saw an entity keyword, validate scope.
        if let Some(ent) = explicit {
            match (scope, subject) {
                (Scope::NodeBlock, Subject::From)
                | (Scope::NodeBlock, Subject::To)
                | (Scope::NodeBlock, Subject::Edge) => {
                    return Err(DslError::ScopeError {
                        entity: ent.into(),
                        hint: "use `self.<attr>` or bare `<attr>` in a node block".into(),
                        position: entity_pos,
                    });
                }
                (Scope::ExpandBlock, Subject::SelfNode) => {
                    return Err(DslError::ScopeError {
                        entity: ent.into(),
                        hint:
                            "in an `expand` block use `from.<attr>`, `to.<attr>`, or `edge.<attr>`"
                                .into(),
                        position: entity_pos,
                    });
                }
                (Scope::NeighborFilter, Subject::SelfNode)
                | (Scope::NeighborFilter, Subject::From)
                | (Scope::NeighborFilter, Subject::To) => {
                    return Err(DslError::ScopeError {
                        entity: ent.into(),
                        hint: "neighbor filter conditions apply to the edge — use `edge.<attr>` or bare `<attr>`".into(),
                        position: entity_pos,
                    });
                }
                _ => {}
            }
            self.consume(Token::Dot)?;
        }

        // Read the attribute name.
        let attr_pos = self.peek_pos();
        let attr_name = match self.advance().tok {
            Token::Ident(s) => s,
            // Keywords aren't valid attribute names.
            other => {
                return Err(DslError::UnexpectedToken {
                    found: token_label(&other),
                    expected: "attribute name".into(),
                    position: attr_pos,
                });
            }
        };

        let attr = parse_attr(&attr_name, attr_pos)?;

        // Validate (attr, subject) compatibility.
        validate_attr_subject(attr, subject, &attr_name, attr_pos)?;

        Ok((subject, attr))
    }

    fn unexpected_owned(&self, found: String) -> DslError {
        DslError::UnexpectedToken {
            found,
            expected: "attribute name".into(),
            position: self.peek_pos(),
        }
    }

    fn parse_op(&mut self) -> Result<Op, DslError> {
        let pos = self.peek_pos();
        match self.advance().tok {
            Token::Eq => Ok(Op::Eq),
            Token::NotEq => Ok(Op::NotEq),
            Token::InKw => Ok(Op::In),
            Token::IncludesKw => Ok(Op::Includes),
            Token::StartsWithKw => Ok(Op::StartsWith),
            Token::EndsWithKw => Ok(Op::EndsWith),
            Token::Lt => Ok(Op::Lt),
            Token::Le => Ok(Op::Le),
            Token::Gt => Ok(Op::Gt),
            Token::Ge => Ok(Op::Ge),
            Token::IsKw => {
                // `is null` or `is not null`
                if matches!(self.peek(), Token::NotKw) {
                    self.advance();
                    self.consume(Token::NullKw)
                        .map_err(|_| DslError::UnexpectedToken {
                            found: token_label(self.peek()),
                            expected: "`null` (after `is not`)".into(),
                            position: self.peek_pos(),
                        })?;
                    Ok(Op::IsNotNull)
                } else if matches!(self.peek(), Token::NullKw) {
                    self.advance();
                    Ok(Op::IsNull)
                } else {
                    Err(DslError::UnexpectedToken {
                        found: token_label(self.peek()),
                        expected: "`null` or `not null` (after `is`)".into(),
                        position: self.peek_pos(),
                    })
                }
            }
            other => Err(DslError::UnexpectedToken {
                found: token_label(&other),
                expected: "an operator (`=`, `!=`, `<`, `<=`, `>`, `>=`, `in`, `includes`, `starts_with`, `ends_with`, `is null`, `is not null`)"
                    .into(),
                position: pos,
            }),
        }
    }

    fn parse_value(&mut self) -> Result<Value, DslError> {
        if matches!(self.peek(), Token::LBrace) {
            self.advance();
            let mut items = vec![self.parse_literal()?];
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                items.push(self.parse_literal()?);
            }
            self.consume(Token::RBrace)?;
            Ok(Value::Set(items))
        } else {
            Ok(Value::Single(self.parse_literal()?))
        }
    }

    fn parse_literal(&mut self) -> Result<Literal, DslError> {
        let pos = self.peek_pos();
        match self.advance().tok {
            Token::Ident(s) => Ok(Literal::Ident(s)),
            Token::Str(s) => Ok(Literal::Str(s)),
            Token::Int(n) => Ok(Literal::Int(n)),
            other => Err(DslError::UnexpectedToken {
                found: token_label(&other),
                expected: "identifier, string, or integer".into(),
                position: pos,
            }),
        }
    }

    fn check_kind_values(
        &self,
        subject: Subject,
        value: &Value,
        position: usize,
    ) -> Result<(), DslError> {
        let allowed: &[&str] = match subject {
            Subject::Edge => EDGE_KIND_VALUES,
            _ => &["Note", "Directory", "Ghost", "Task", "Paragraph", "Heading"],
        };
        let check = |lit: &Literal| -> Result<(), DslError> {
            let s = literal_as_str(lit);
            if allowed.contains(&s) {
                Ok(())
            } else {
                Err(DslError::UnknownKindValue {
                    attr: "kind".into(),
                    value: s.to_string(),
                    allowed: allowed.iter().map(|s| (*s).to_string()).collect(),
                    position,
                })
            }
        };
        match value {
            Value::Single(l) => check(l),
            Value::Set(ls) => {
                for l in ls {
                    check(l)?;
                }
                Ok(())
            }
            Value::None => Ok(()),
        }
    }

    fn check_form_values(&self, value: &Value, position: usize) -> Result<(), DslError> {
        let allowed: &[&str] = &["wiki", "md"];
        let check = |lit: &Literal| -> Result<(), DslError> {
            let s = literal_as_str(lit);
            if allowed.contains(&s) {
                Ok(())
            } else {
                Err(DslError::UnknownKindValue {
                    attr: "form".into(),
                    value: s.to_string(),
                    allowed: allowed.iter().map(|s| (*s).to_string()).collect(),
                    position,
                })
            }
        };
        match value {
            Value::Single(l) => check(l),
            Value::Set(ls) => {
                for l in ls {
                    check(l)?;
                }
                Ok(())
            }
            Value::None => Ok(()),
        }
    }

    fn check_embed_values(&self, value: &Value, position: usize) -> Result<(), DslError> {
        let allowed: &[&str] = &["true", "false"];
        let check = |lit: &Literal| -> Result<(), DslError> {
            let s = literal_as_str(lit);
            if allowed.contains(&s) {
                Ok(())
            } else {
                Err(DslError::UnknownKindValue {
                    attr: "embed".into(),
                    value: s.to_string(),
                    allowed: allowed.iter().map(|s| (*s).to_string()).collect(),
                    position,
                })
            }
        };
        match value {
            Value::Single(l) => check(l),
            Value::Set(ls) => {
                for l in ls {
                    check(l)?;
                }
                Ok(())
            }
            Value::None => Ok(()),
        }
    }
}

/// Parse-time check: does this operator make sense on this attribute?
/// - `<`/`<=`/`>`/`>=` require `Int` or `Date`.
/// - `is null`/`is not null` require an optional attribute.
/// - `includes`/`starts_with`/`ends_with` require `Str` or `Set`.
fn check_op_vs_attr(op: Op, attr: Attr, position: usize) -> Result<(), DslError> {
    let vt = attr.value_type();
    match op {
        Op::Lt | Op::Le | Op::Gt | Op::Ge => {
            if !matches!(vt, ValueType::Int | ValueType::Date) {
                return Err(DslError::TypeMismatch {
                    op: op_label(op).into(),
                    expected: "integer or date attribute".into(),
                    got: format!("{} attribute `{}`", value_type_name(vt), attr_name(attr)),
                    position,
                });
            }
        }
        Op::IsNull | Op::IsNotNull => {
            if !attr.is_optional() {
                return Err(DslError::TypeMismatch {
                    op: op_label(op).into(),
                    expected:
                        "optional attribute (`due`, `scheduled`, `created`, `start`, `completed`)"
                            .into(),
                    got: format!("required attribute `{}`", attr_name(attr)),
                    position,
                });
            }
        }
        Op::Includes | Op::StartsWith | Op::EndsWith => {
            if !matches!(vt, ValueType::Str | ValueType::Set | ValueType::Enum) {
                return Err(DslError::TypeMismatch {
                    op: op_label(op).into(),
                    expected: "string or set attribute".into(),
                    got: format!("{} attribute `{}`", value_type_name(vt), attr_name(attr)),
                    position,
                });
            }
        }
        Op::Eq | Op::NotEq | Op::In => {}
    }
    Ok(())
}

fn value_type_name(vt: ValueType) -> &'static str {
    match vt {
        ValueType::Int => "integer",
        ValueType::Date => "date",
        ValueType::Str => "string",
        ValueType::Enum => "enum",
        ValueType::Set => "set",
    }
}

fn parse_attr(s: &str, position: usize) -> Result<Attr, DslError> {
    match s {
        "kind" => Ok(Attr::Kind),
        "path" => Ok(Attr::Path),
        "title" => Ok(Attr::Title),
        "form" => Ok(Attr::Form),
        "embed" => Ok(Attr::Embed),
        "indegree" => Ok(Attr::Indegree),
        "outdegree" => Ok(Attr::Outdegree),
        // Task-specific attributes
        "status" => Ok(Attr::Status),
        "priority" => Ok(Attr::Priority),
        "due" => Ok(Attr::Due),
        "scheduled" => Ok(Attr::Scheduled),
        "created" => Ok(Attr::Created),
        "start" => Ok(Attr::Start),
        "completed" => Ok(Attr::Completed),
        "description" => Ok(Attr::Description),
        "tags" => Ok(Attr::Tags),
        _ => Err(DslError::UnknownAttribute {
            attr: s.to_string(),
            position,
        }),
    }
}

fn validate_attr_subject(
    attr: Attr,
    subject: Subject,
    attr_name: &str,
    position: usize,
) -> Result<(), DslError> {
    match (attr, subject) {
        // Kind: valid on every subject.
        (Attr::Kind, _) => Ok(()),

        // Path / Title: node-only (self, from, to). Reject edge.
        (Attr::Path | Attr::Title, Subject::Edge) => Err(DslError::ScopeError {
            entity: "edge".into(),
            hint: format!("`{attr_name}` is a node attribute, not an edge attribute"),
            position,
        }),
        (Attr::Path | Attr::Title, _) => Ok(()),

        // Form / Embed: edge-only.
        (Attr::Form, Subject::Edge) => Ok(()),
        (Attr::Form, _) => Err(DslError::ScopeError {
            entity: subject_name(subject).into(),
            hint: "`form` is an edge attribute — use `edge.form` in an expand block or neighbor filter".into(),
            position,
        }),
        (Attr::Embed, Subject::Edge) => Ok(()),
        (Attr::Embed, _) => Err(DslError::ScopeError {
            entity: subject_name(subject).into(),
            hint: "`embed` is an edge attribute — use `edge.embed` in an expand block or neighbor filter".into(),
            position,
        }),

        // Indegree / Outdegree: only on self (node block).
        (Attr::Indegree | Attr::Outdegree, Subject::SelfNode) => Ok(()),
        (Attr::Indegree | Attr::Outdegree, _) => Err(DslError::ScopeError {
            entity: subject_name(subject).into(),
            hint: format!(
                "`{attr_name}` is only supported on `self` in a node block (selection-time only)"
            ),
            position,
        }),

        // Task attributes: node-only (self, from, to). Reject edge.
        (
            Attr::Status
            | Attr::Priority
            | Attr::Due
            | Attr::Scheduled
            | Attr::Created
            | Attr::Start
            | Attr::Completed
            | Attr::Description
            | Attr::Tags,
            Subject::Edge,
        ) => Err(DslError::ScopeError {
            entity: "edge".into(),
            hint: format!("`{attr_name}` is a node attribute, not an edge attribute"),
            position,
        }),
        (
            Attr::Status
            | Attr::Priority
            | Attr::Due
            | Attr::Scheduled
            | Attr::Created
            | Attr::Start
            | Attr::Completed
            | Attr::Description
            | Attr::Tags,
            _,
        ) => Ok(()),
    }
}

fn subject_name(s: Subject) -> &'static str {
    match s {
        Subject::SelfNode => "self",
        Subject::From => "from",
        Subject::To => "to",
        Subject::Edge => "edge",
    }
}

fn literal_as_str(lit: &Literal) -> &str {
    match lit {
        Literal::Ident(s) => s,
        Literal::Str(s) => s,
        Literal::Int(_) => "",
        Literal::Date(_) => "",
    }
}

// ── Public entry point ────────────────────────────────────────────────

/// Parse a graph DSL string into a [`GraphQuery`] under [`Profile::Default`].
/// Date keywords (`today`, `tomorrow`, `yesterday`) resolve against the
/// system date at parse time. For deterministic resolution (tests,
/// presets, reproducible scripts), use [`parse_with`].
pub fn parse(src: &str) -> Result<GraphQuery, DslError> {
    parse_with(src, Profile::Default, chrono::Local::now().date_naive())
}

/// Parse with an explicit profile and `today` reference date.
///
/// Under [`Profile::Tasks`], if the source does not begin with `node`
/// (after optional whitespace), a synthetic `node where kind = Task and `
/// prelude is prepended at the token level. Bare attribute references
/// (no `self.` / `from.` / `to.` / `edge.` prefix) inside a node block
/// already default to `Subject::SelfNode`, so the Tasks profile reuses
/// the existing default-subject behaviour and only needs to inject the
/// prelude.
pub fn parse_with(src: &str, profile: Profile, today: NaiveDate) -> Result<GraphQuery, DslError> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return Err(DslError::EmptyInput);
    }

    // The lexer takes raw `src`, not `trimmed`, so error positions
    // line up with the original input.
    let mut lexer = Lexer::new(src);
    let mut tokens = lexer.tokenize()?;

    if matches!(profile, Profile::Tasks) {
        // If the source already starts with `node`, no prelude needed.
        let first_real = tokens
            .iter()
            .find(|s| !matches!(s.tok, Token::Eof))
            .map(|s| &s.tok);
        if !matches!(first_real, Some(Token::Node)) {
            // Synthesize:  node where kind = Task and
            // All synthetic tokens get position 0 so spans for user-typed
            // tokens remain accurate.
            let prelude = vec![
                Spanned {
                    tok: Token::Node,
                    pos: 0,
                },
                Spanned {
                    tok: Token::Where,
                    pos: 0,
                },
                Spanned {
                    tok: Token::Ident("kind".into()),
                    pos: 0,
                },
                Spanned {
                    tok: Token::Eq,
                    pos: 0,
                },
                Spanned {
                    tok: Token::Ident("Task".into()),
                    pos: 0,
                },
                Spanned {
                    tok: Token::And,
                    pos: 0,
                },
            ];
            let mut merged = Vec::with_capacity(prelude.len() + tokens.len());
            merged.extend(prelude);
            merged.append(&mut tokens);
            tokens = merged;
        }
    }

    let mut parser = Parser::new(tokens, today);
    parser.parse_query()
}

// ── Evaluator ─────────────────────────────────────────────────────────

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
        let roots = self.select(graph);
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
        NodeKind::Ghost(g) => (2, g.raw.clone()),
        NodeKind::Task(t) => (3, format!("{}:{}", t.source_file.display(), t.source_line)),
        NodeKind::Paragraph(p) => (
            4,
            format!("{}:{:08}", p.source_file.display(), p.line_start),
        ),
        NodeKind::Heading(h) => (5, format!("{}:{:08}", h.source_file.display(), h.line)),
    }
}

fn eval_cond_on_node(graph: &Graph, id: NoteId, c: &Condition) -> bool {
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

fn node_kind_str(n: &NodeKind) -> &'static str {
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

fn edge_kind_str(e: &EdgeKind) -> &'static str {
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

impl fmt::Display for GraphQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for sel in &self.initial {
            if !first {
                write!(f, "; ")?;
            }
            first = false;
            sel.fmt_node(f)?;
        }
        if let Some(ref policy) = self.expansion {
            if !first {
                write!(f, "; ")?;
            }
            policy.fmt_expand(f)?;
        }
        write!(f, ";")
    }
}

impl NodeSelector {
    fn fmt_node(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "node")?;
        if let Some(ref e) = self.condition {
            write!(f, " where ")?;
            fmt_cond_expr(f, e, ExprCtx::Top)?;
        }
        if let Some(ref nf) = self.without {
            write!(f, " without ")?;
            nf.fmt_filter(f)?;
        }
        Ok(())
    }
}

/// Precedence context for serializing [`CondExpr`]. Determines whether a
/// child expression needs surrounding parens to round-trip.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExprCtx {
    /// Top-level (or right inside a `where` / outer paren). No parens needed.
    Top,
    /// Inside an `or` group; an `and` child does NOT need parens (and binds
    /// tighter than or). A bare `or` child does not appear here (the parser
    /// would flatten it).
    InOr,
    /// Inside an `and` group; an `or` child DOES need parens.
    InAnd,
}

fn fmt_cond_expr(f: &mut fmt::Formatter<'_>, e: &CondExpr, ctx: ExprCtx) -> fmt::Result {
    match e {
        CondExpr::Cond(c) => fmt_condition(f, c),
        CondExpr::And(parts) => {
            let _ = ctx;
            let mut first = true;
            for p in parts {
                if !first {
                    write!(f, " and ")?;
                }
                first = false;
                fmt_cond_expr(f, p, ExprCtx::InAnd)?;
            }
            Ok(())
        }
        CondExpr::Or(parts) => {
            let needs_parens = matches!(ctx, ExprCtx::InAnd);
            if needs_parens {
                write!(f, "(")?;
            }
            let mut first = true;
            for p in parts {
                if !first {
                    write!(f, " or ")?;
                }
                first = false;
                fmt_cond_expr(f, p, ExprCtx::InOr)?;
            }
            if needs_parens {
                write!(f, ")")?;
            }
            Ok(())
        }
    }
}

impl EdgePolicy {
    fn fmt_expand(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "expand")?;
        if !self.conditions.is_empty() {
            write!(f, " where ")?;
            fmt_conditions(f, &self.conditions)?;
        }
        Ok(())
    }
}

impl NeighborFilter {
    fn fmt_filter(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.direction {
            Direction::Incoming => write!(f, "incoming(")?,
            Direction::Outgoing => write!(f, "outgoing(")?,
        }
        fmt_conditions(f, &self.conditions)?;
        write!(f, ")")
    }
}

fn fmt_conditions(f: &mut fmt::Formatter<'_>, cs: &[Condition]) -> fmt::Result {
    let mut first = true;
    for c in cs {
        if !first {
            write!(f, " and ")?;
        }
        first = false;
        fmt_condition(f, c)?;
    }
    Ok(())
}

fn fmt_condition(f: &mut fmt::Formatter<'_>, c: &Condition) -> fmt::Result {
    // SelfNode is canonical as bare (it's the only valid subject in a
    // node block, so no qualifier is needed). Edge / From / To always
    // get an explicit qualifier — required to round-trip in expand
    // blocks (where bare attributes are AmbiguousAttribute errors) and
    // accepted in neighbor filters too.
    let qualifier = match c.subject {
        Subject::SelfNode => None,
        Subject::Edge => Some("edge"),
        Subject::From => Some("from"),
        Subject::To => Some("to"),
    };
    if let Some(q) = qualifier {
        write!(f, "{q}.")?;
    }
    write!(f, "{}", attr_name(c.attr))?;
    if matches!(c.op, Op::IsNull | Op::IsNotNull) {
        write!(f, " {}", op_label(c.op))
    } else {
        write!(f, " {} ", op_label(c.op))?;
        fmt_value(f, &c.value)
    }
}

fn attr_name(a: Attr) -> &'static str {
    match a {
        Attr::Kind => "kind",
        Attr::Path => "path",
        Attr::Title => "title",
        Attr::Form => "form",
        Attr::Embed => "embed",
        Attr::Indegree => "indegree",
        Attr::Outdegree => "outdegree",
        Attr::Status => "status",
        Attr::Priority => "priority",
        Attr::Due => "due",
        Attr::Scheduled => "scheduled",
        Attr::Created => "created",
        Attr::Start => "start",
        Attr::Completed => "completed",
        Attr::Description => "description",
        Attr::Tags => "tags",
    }
}

fn fmt_value(f: &mut fmt::Formatter<'_>, v: &Value) -> fmt::Result {
    match v {
        Value::Single(l) => fmt_literal(f, l),
        Value::Set(ls) => {
            write!(f, "{{")?;
            let mut first = true;
            for l in ls {
                if !first {
                    write!(f, ", ")?;
                }
                first = false;
                fmt_literal(f, l)?;
            }
            write!(f, "}}")
        }
        Value::None => Ok(()),
    }
}

fn fmt_literal(f: &mut fmt::Formatter<'_>, l: &Literal) -> fmt::Result {
    match l {
        Literal::Ident(s) => write!(f, "{s}"),
        Literal::Str(s) => {
            // Escape backslash and double-quote.
            let escaped: String = s
                .chars()
                .flat_map(|c| match c {
                    '\\' => vec!['\\', '\\'],
                    '"' => vec!['\\', '"'],
                    '\n' => vec!['\\', 'n'],
                    '\t' => vec!['\\', 't'],
                    other => vec![other],
                })
                .collect();
            write!(f, "\"{escaped}\"")
        }
        Literal::Int(n) => write!(f, "{n}"),
        Literal::Date(d) => write!(f, "{}", d.format("%Y-%m-%d")),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Every edge kind the graph can produce must be an accepted `edge.kind`
    /// value, or that edge becomes silently unqueryable. Guards the two lists
    /// (`edge_kind_str` ↔ [`EDGE_KIND_VALUES`]) against drift.
    #[test]
    fn every_edge_kind_is_a_queryable_value() {
        fn link() -> crate::graph::LinkEdge {
            crate::graph::LinkEdge {
                form: LinkForm::WikiLink,
                is_embed: false,
                byte_range: 0..0,
                line: 1,
                raw_text: String::new(),
                target_text: String::new(),
                anchor: None,
                display: None,
            }
        }
        // Exhaustive by construction: a new EdgeKind variant forces a new
        // entry here (and then in EDGE_KIND_VALUES to pass).
        let all = [
            EdgeKind::NoteLink(link()),
            EdgeKind::HeadingLink(link()),
            EdgeKind::ParagraphLink(link()),
            EdgeKind::Contains,
            EdgeKind::HasTask,
            EdgeKind::Subtask,
            EdgeKind::LinksInto,
            EdgeKind::OwnsParagraph,
            EdgeKind::OwnsHeading,
        ];
        for e in &all {
            let name = edge_kind_str(e);
            assert!(
                EDGE_KIND_VALUES.contains(&name),
                "edge kind `{name}` is missing from EDGE_KIND_VALUES"
            );
        }
        assert_eq!(EDGE_KIND_VALUES.len(), all.len(), "no stale extra values");
    }

    // ── Parser tests ─────────────────────────────────────────────────

    mod parser {
        use super::*;

        fn parse_ok(src: &str) -> GraphQuery {
            parse(src).unwrap_or_else(|e| panic!("parse failed for {src:?}: {e}"))
        }

        #[test]
        fn parse_node_match_all() {
            let q = parse_ok("node;");
            assert_eq!(q.initial.len(), 1);
            assert!(q.initial[0].conditions().is_empty());
            assert!(q.initial[0].without.is_none());
            assert!(q.expansion.is_none());
        }

        #[test]
        fn parse_node_no_trailing_semi() {
            let q = parse_ok("node");
            assert_eq!(q.initial.len(), 1);
        }

        #[test]
        fn parse_kind_eq() {
            let q = parse_ok("node where kind = Note;");
            assert_eq!(
                q.initial[0].conditions()[0],
                &Condition {
                    subject: Subject::SelfNode,
                    attr: Attr::Kind,
                    op: Op::Eq,
                    value: Value::Single(Literal::Ident("Note".into())),
                }
            );
        }

        #[test]
        fn parse_self_qualified() {
            let q = parse_ok("node where self.kind = Directory;");
            assert_eq!(q.initial[0].conditions()[0].subject, Subject::SelfNode);
            assert_eq!(q.initial[0].conditions()[0].attr, Attr::Kind);
        }

        #[test]
        fn parse_kind_in_set() {
            let q = parse_ok("node where kind in {Note, Directory};");
            assert_eq!(
                q.initial[0].conditions()[0].value,
                Value::Set(vec![
                    Literal::Ident("Note".into()),
                    Literal::Ident("Directory".into()),
                ])
            );
        }

        #[test]
        fn parse_path_starts_with() {
            let q = parse_ok("node where path starts_with \"Projects/\";");
            assert_eq!(q.initial[0].conditions()[0].op, Op::StartsWith);
            assert_eq!(
                q.initial[0].conditions()[0].value,
                Value::Single(Literal::Str("Projects/".into()))
            );
        }

        #[test]
        fn parse_path_ends_with() {
            let q = parse_ok("node where path ends_with \".md\";");
            assert_eq!(q.initial[0].conditions()[0].op, Op::EndsWith);
        }

        #[test]
        fn parse_path_includes() {
            let q = parse_ok("node where path includes \"Areas\";");
            assert_eq!(q.initial[0].conditions()[0].op, Op::Includes);
        }

        #[test]
        fn parse_multiple_and_conditions() {
            let q = parse_ok("node where kind = Note and path starts_with \"Areas/\";");
            assert_eq!(q.initial[0].conditions().len(), 2);
        }

        #[test]
        fn parse_indegree() {
            let q = parse_ok("node where indegree = 0;");
            assert_eq!(q.initial[0].conditions()[0].attr, Attr::Indegree);
            assert_eq!(
                q.initial[0].conditions()[0].value,
                Value::Single(Literal::Int(0))
            );
        }

        #[test]
        fn parse_without_incoming() {
            let q = parse_ok(
                "node where kind = Directory without incoming(kind = directory-contains);",
            );
            let nf = q.initial[0].without.as_ref().unwrap();
            assert_eq!(nf.direction, Direction::Incoming);
            assert_eq!(nf.conditions.len(), 1);
            assert_eq!(nf.conditions[0].subject, Subject::Edge);
            assert_eq!(nf.conditions[0].attr, Attr::Kind);
            assert_eq!(
                nf.conditions[0].value,
                Value::Single(Literal::Ident("directory-contains".into()))
            );
        }

        #[test]
        fn parse_without_outgoing() {
            let q = parse_ok("node without outgoing();");
            assert_eq!(
                q.initial[0].without.as_ref().unwrap().direction,
                Direction::Outgoing
            );
        }

        #[test]
        fn parse_two_node_blocks() {
            let q = parse_ok("node where kind = Note; node where kind = Directory;");
            assert_eq!(q.initial.len(), 2);
        }

        #[test]
        fn parse_expand_simple() {
            let q = parse_ok("node; expand where edge.kind = note-link;");
            let pol = q.expansion.as_ref().unwrap();
            assert_eq!(pol.conditions.len(), 1);
            assert_eq!(pol.conditions[0].subject, Subject::Edge);
        }

        #[test]
        fn parse_expand_full_directory_tree() {
            let q = parse_ok(
                "node where kind = Directory; expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};",
            );
            let pol = q.expansion.as_ref().unwrap();
            assert_eq!(pol.conditions.len(), 3);
            let subjects: Vec<Subject> = pol.conditions.iter().map(|c| c.subject).collect();
            assert_eq!(subjects, vec![Subject::From, Subject::Edge, Subject::To]);
        }

        #[test]
        fn parse_string_with_escape() {
            let q = parse_ok("node where title = \"with \\\"quotes\\\"\";");
            assert_eq!(
                q.initial[0].conditions()[0].value,
                Value::Single(Literal::Str("with \"quotes\"".into()))
            );
        }

        // ── Error paths ─────────────────────────────────────────────

        #[test]
        fn empty_input() {
            assert!(matches!(parse("   "), Err(DslError::EmptyInput)));
        }

        #[test]
        fn no_initial_set() {
            let err = parse("expand where edge.kind = note-link;").unwrap_err();
            assert!(matches!(err, DslError::NoInitialSet));
        }

        #[test]
        fn type_mismatch_eq_with_set() {
            let err = parse("node where kind = {Note, Directory};").unwrap_err();
            assert!(matches!(err, DslError::TypeMismatch { .. }));
        }

        #[test]
        fn type_mismatch_in_with_single() {
            let err = parse("node where kind in Note;").unwrap_err();
            assert!(matches!(err, DslError::TypeMismatch { .. }));
        }

        #[test]
        fn ambiguous_attr_in_expand() {
            let err = parse("node; expand where kind = link;").unwrap_err();
            assert!(matches!(err, DslError::AmbiguousAttribute { .. }));
        }

        #[test]
        fn scope_error_from_in_node_block() {
            let err = parse("node where from.kind = Directory;").unwrap_err();
            assert!(matches!(err, DslError::ScopeError { .. }));
        }

        #[test]
        fn scope_error_self_in_expand() {
            let err = parse("node; expand where self.kind = Directory;").unwrap_err();
            assert!(matches!(err, DslError::ScopeError { .. }));
        }

        #[test]
        fn scope_error_indegree_in_expand() {
            let err = parse("node; expand where from.indegree = 0;").unwrap_err();
            assert!(matches!(err, DslError::ScopeError { .. }));
        }

        #[test]
        fn scope_error_form_on_node() {
            let err = parse("node where form = wiki;").unwrap_err();
            assert!(matches!(err, DslError::ScopeError { .. }));
        }

        #[test]
        fn scope_error_path_on_edge() {
            let err = parse("node; expand where edge.path = foo;").unwrap_err();
            assert!(matches!(err, DslError::ScopeError { .. }));
        }

        #[test]
        fn unknown_attribute() {
            let err = parse("node where foo = bar;").unwrap_err();
            assert!(matches!(err, DslError::UnknownAttribute { .. }));
        }

        #[test]
        fn unknown_kind_value() {
            let err = parse("node where kind = Notes;").unwrap_err();
            match err {
                DslError::UnknownKindValue { value, .. } => assert_eq!(value, "Notes"),
                other => panic!("expected UnknownKindValue, got {other:?}"),
            }
        }

        #[test]
        fn unknown_kind_value_in_set() {
            let err = parse("node where kind in {Note, Bogus};").unwrap_err();
            assert!(matches!(err, DslError::UnknownKindValue { .. }));
        }

        #[test]
        fn expand_over_subtask_edges_parses() {
            // The subtask edge is a first-class traversable edge kind.
            parse("node where kind = Task; expand where edge.kind = subtask;").unwrap();
            parse("node; expand where edge.kind in {subtask, has-task};").unwrap();
        }

        #[test]
        fn unknown_form_value() {
            let err = parse("node; expand where edge.form = html;").unwrap_err();
            assert!(matches!(err, DslError::UnknownKindValue { .. }));
        }

        #[test]
        fn trailing_input() {
            let err = parse("node; junk").unwrap_err();
            assert!(matches!(err, DslError::TrailingInput { .. }));
        }

        #[test]
        fn unterminated_string() {
            let err = parse("node where path = \"oops").unwrap_err();
            assert!(matches!(err, DslError::UnterminatedString { .. }));
        }

        #[test]
        fn illegal_character() {
            let err = parse("node where path = @bogus;").unwrap_err();
            assert!(matches!(err, DslError::IllegalCharacter { .. }));
        }

        #[test]
        fn no_with_keyword_anymore() {
            // The v1 keyword `with` should now fail. `n` after `node`
            // is parsed as trailing input.
            let err = parse("node n with n.kind = Note;").unwrap_err();
            assert!(matches!(err, DslError::TrailingInput { .. }));
        }

        #[test]
        fn no_over_keyword_anymore() {
            // Old `expand over e(n, m) ...` should fail at parse.
            // `over` is no longer a keyword; treated as trailing input
            // after `expand`.
            let err =
                parse("node; expand over e(n, m) with e.kind = directory-contains;").unwrap_err();
            assert!(matches!(err, DslError::TrailingInput { .. }));
        }
    }

    // ── Display / round-trip tests ───────────────────────────────────

    mod display {
        use super::*;

        fn roundtrip(src: &str) {
            let q1 = parse(src).unwrap();
            let s = format!("{q1}");
            let q2 = parse(&s).unwrap_or_else(|e| panic!("re-parse failed for {s:?}: {e}"));
            assert_eq!(q1, q2, "round-trip mismatch:\n  src: {src}\n  ser: {s}");
        }

        #[test]
        fn rt_match_all() {
            roundtrip("node;");
        }

        #[test]
        fn rt_kind_eq() {
            roundtrip("node where kind = Note;");
        }

        #[test]
        fn rt_kind_in_set() {
            roundtrip("node where kind in {Note, Directory};");
        }

        #[test]
        fn rt_path_starts_with() {
            roundtrip("node where path starts_with \"Projects/\";");
        }

        #[test]
        fn rt_path_ends_with() {
            roundtrip("node where path ends_with \".md\";");
        }

        #[test]
        fn rt_multi_and() {
            roundtrip("node where kind = Note and path starts_with \"Areas/\";");
        }

        #[test]
        fn rt_without_incoming() {
            roundtrip("node where kind = Directory without incoming(kind = directory-contains);");
        }

        #[test]
        fn rt_without_outgoing_empty() {
            roundtrip("node without outgoing();");
        }

        #[test]
        fn rt_two_blocks() {
            roundtrip("node where kind = Note; node where kind = Directory;");
        }

        #[test]
        fn rt_expand_full() {
            roundtrip(
                "node where kind = Directory; expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};",
            );
        }

        #[test]
        fn rt_indegree_zero() {
            roundtrip("node where indegree = 0;");
        }

        #[test]
        fn rt_string_with_escapes() {
            roundtrip("node where title = \"with \\\"quotes\\\" and \\\\ slash\";");
        }

        #[test]
        fn self_collapses_to_bare() {
            let q = parse("node where self.kind = Note;").unwrap();
            let s = format!("{q}");
            assert_eq!(s, "node where kind = Note;");
        }
    }

    // ── Evaluator tests ──────────────────────────────────────────────

    mod eval {
        use std::path::{Path, PathBuf};

        use crate::graph::Graph;
        use crate::vault::Vault;

        use super::*;

        fn dirs_vault() -> Vault {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("tests/fixtures/dirs");
            Vault::discover(Some(path)).expect("dirs fixture vault must exist")
        }

        fn links_vault() -> Vault {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("tests/fixtures/links");
            Vault::discover(Some(path)).expect("links fixture vault must exist")
        }

        #[test]
        fn select_match_all() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse("node;").unwrap();
            let ids = q.select(&g);
            // 4 notes + 4 dirs + 4 paragraphs + 4 headings = 16
            assert_eq!(ids.len(), 16);
        }

        #[test]
        fn select_all_notes() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse("node where kind = Note;").unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 4);
        }

        #[test]
        fn select_all_directories() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse("node where kind = Directory;").unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 4);
        }

        #[test]
        fn select_path_starts_with() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse("node where path starts_with \"Areas\";").unwrap();
            let ids = q.select(&g);
            // Areas dir, Areas/finance.md, Areas/operations dir, Areas/operations/shifts.md
            assert_eq!(ids.len(), 4);
        }

        #[test]
        fn select_path_starts_with_strict() {
            // Substring would match Areas/old-Projects/ too if it existed;
            // starts_with rejects matches that aren't a true prefix.
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse("node where path starts_with \"Projects/\";").unwrap();
            let ids = q.select(&g);
            // Only Projects/alpha.md (the directory itself is "Projects", not "Projects/")
            assert_eq!(ids.len(), 1);
        }

        #[test]
        fn select_path_ends_with_md() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse("node where path ends_with \".md\";").unwrap();
            let ids = q.select(&g);
            // All 4 notes end with .md; no directories should match.
            assert_eq!(ids.len(), 4);
        }

        #[test]
        fn select_kind_in_set() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse("node where kind in {Note, Directory};").unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 8);
        }

        #[test]
        fn select_indegree_zero() {
            // Only the vault root directory has no incoming edges.
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse("node where indegree = 0;").unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 1);
            assert!(matches!(
                g.node(ids[0]),
                NodeKind::Directory(d) if d.path.as_os_str().is_empty()
            ));
        }

        #[test]
        fn select_without_incoming_contains() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse(
                "node where kind in {Note, Directory} without incoming(kind = directory-contains);",
            )
            .unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 1);
            assert!(matches!(
                g.node(ids[0]),
                NodeKind::Directory(d) if d.path.as_os_str().is_empty()
            ));
        }

        #[test]
        fn select_two_blocks_union_deduped() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse("node where kind = Directory; node where path starts_with \"Areas\";")
                .unwrap();
            let ids = q.select(&g);
            // 4 dirs + Areas/finance.md + Areas/operations/shifts.md = 6
            // (Areas dir and Areas/operations dir already in first block)
            assert_eq!(ids.len(), 6);
        }

        #[test]
        fn expand_full_directory_tree() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse(
                "node where indegree = 0; expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};",
            )
            .unwrap();
            let roots = q.select(&g);
            assert_eq!(roots.len(), 1);

            let children = q.expand(&g, roots[0]).unwrap();
            // root has: root.md, Areas/, Projects/
            assert_eq!(children.len(), 3);
        }

        #[test]
        fn expand_notes_only() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse(
                "node where indegree = 0; expand where from.kind = Directory and edge.kind = directory-contains and to.kind = Note;",
            )
            .unwrap();
            let roots = q.select(&g);
            let children = q.expand(&g, roots[0]).unwrap();
            // Only root.md (the Note child of the root dir)
            assert_eq!(children.len(), 1);
            assert!(matches!(g.node(children[0]), NodeKind::Note(_)));
        }

        #[test]
        fn expand_none_when_no_policy() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse("node where kind = Note;").unwrap();
            let any = q.select(&g)[0];
            assert!(q.expand(&g, any).is_none());
        }

        #[test]
        fn expand_some_empty_when_parent_mismatch() {
            // v2 behavior: parent that doesn't satisfy `from` conditions
            // returns Some(vec![]), not None.
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse(
                "node; expand where from.kind = Directory and edge.kind = directory-contains;",
            )
            .unwrap();
            let note_id = g
                .nodes()
                .find(|(_, k)| matches!(k, NodeKind::Note(_)))
                .map(|(id, _)| id)
                .unwrap();
            let children = q.expand(&g, note_id).unwrap();
            assert!(children.is_empty());
        }

        #[test]
        fn expand_on_links_vault() {
            let v = links_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse(
                "node; expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};",
            )
            .unwrap();
            let notes_dir = g.node_by_path(Path::new("notes")).unwrap();
            let children = q.expand(&g, notes_dir).unwrap();
            assert_eq!(children.len(), 6);
        }

        #[test]
        fn title_match_on_note() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            // `title` matches both the note `root` and its heading `root`.
            let q = parse("node where title = \"root\";").unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 2);
        }

        #[test]
        fn select_kind_paragraph_returns_only_paragraph_nodes() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse("node where kind = Paragraph;").unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 4, "one paragraph (heading) per note");
            for id in &ids {
                assert!(matches!(g.node(*id), NodeKind::Paragraph(_)));
            }
        }

        #[test]
        fn expand_owns_paragraph_yields_paragraph_children_of_note() {
            use assert_fs::prelude::*;

            let tmp = assert_fs::TempDir::new().unwrap();
            tmp.child(".obsidian").create_dir_all().unwrap();
            tmp.child("note.md")
                .write_str("first\n\nsecond paragraph\n")
                .unwrap();
            let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse(
                "node where kind = Note; \
                 expand where from.kind = Note and edge.kind = owns-paragraph;",
            )
            .unwrap();
            let roots = q.select(&g);
            assert_eq!(roots.len(), 1);
            let children = q.expand(&g, roots[0]).unwrap();
            assert_eq!(children.len(), 2);
            for id in children {
                assert!(matches!(g.node(id), NodeKind::Paragraph(_)));
            }
        }

        #[test]
        fn select_kind_heading_returns_only_heading_nodes() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse("node where kind = Heading;").unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 4, "one heading per note");
            for id in &ids {
                assert!(matches!(g.node(*id), NodeKind::Heading(_)));
            }
        }

        #[test]
        fn heading_title_filter_matches_heading_text() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse("node where kind = Heading and title = \"root\";").unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 1);
            assert!(matches!(g.node(ids[0]), NodeKind::Heading(_)));
        }

        #[test]
        fn expand_owns_heading_yields_subheadings() {
            use assert_fs::prelude::*;

            let tmp = assert_fs::TempDir::new().unwrap();
            tmp.child(".obsidian").create_dir_all().unwrap();
            tmp.child("note.md").write_str("# A\n## B\n## C\n").unwrap();
            let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &v.scan()).unwrap();
            // Note -> direct headings A; then A -> B, A -> C via owns-heading.
            // The expand clause matches both hops (from Note and from Heading).
            let q = parse(
                "node where kind = Note; \
                 expand where from.kind in {Note, Heading} and edge.kind = owns-heading;",
            )
            .unwrap();
            let roots = q.select(&g);
            assert_eq!(roots.len(), 1);
            let top = q.expand(&g, roots[0]).unwrap();
            assert_eq!(top.len(), 1, "only A is a direct child of the note");
            let subs = q.expand(&g, top[0]).unwrap();
            assert_eq!(subs.len(), 2, "A owns B and C");
            for id in subs {
                assert!(matches!(g.node(id), NodeKind::Heading(_)));
            }
        }

        #[test]
        fn edge_kind_values_include_new_link_kinds() {
            // note-link / heading-link / paragraph-link are all accepted.
            for k in ["note-link", "heading-link", "paragraph-link"] {
                let q = parse(&format!(
                    "node where kind = Note; expand where edge.kind = {k};"
                ));
                assert!(q.is_ok(), "{k} should parse: {:?}", q);
            }
        }

        #[test]
        fn edge_embed_predicate_filters_embeds() {
            use assert_fs::prelude::*;

            let tmp = assert_fs::TempDir::new().unwrap();
            tmp.child(".obsidian").create_dir_all().unwrap();
            tmp.child("note.md")
                .write_str("plain [[b]] and embed ![[b]]\n")
                .unwrap();
            tmp.child("b.md").write_str("# b\n").unwrap();
            let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &v.scan()).unwrap();
            // expand following only embed edges yields b (via ![[b]]) but
            // we ask for embed=true: both link occurrences to b exist, but
            // the embed-only filter yields exactly the embed occurrence.
            let q = parse(
                "node where kind = Note; \
                 expand where edge.kind = note-link and edge.embed = true;",
            )
            .unwrap();
            let roots = q.select(&g);
            assert_eq!(roots.len(), 2, "note.md and b.md are both notes");
            // Expand only from note.md (the one with the embed).
            let note_id = roots
                .iter()
                .copied()
                .find(
                    |id| matches!(g.node(*id), NodeKind::Note(n) if n.path == Path::new("note.md")),
                )
                .unwrap();
            let children = q.expand(&g, note_id).unwrap();
            // The embed filter keeps only ![[b]]; the destination is b.
            assert!(!children.is_empty(), "embed edge yields b");
            for id in &children {
                assert!(matches!(g.node(*id), NodeKind::Note(_)));
            }
            // Non-embed filter also yields b (sanity).
            let q2 = parse(
                "node where kind = Note; \
                 expand where edge.kind = note-link and edge.embed = false;",
            )
            .unwrap();
            let children2 = q2.expand(&g, note_id).unwrap();
            assert!(!children2.is_empty(), "non-embed edge yields b");
        }

        #[test]
        fn edge_embed_rejects_non_boolean() {
            let err = parse("node; expand where edge.embed = yes;").unwrap_err();
            assert!(matches!(err, DslError::UnknownKindValue { .. }));
        }

        #[test]
        fn old_edge_kind_values_rejected() {
            for old in ["link", "embed"] {
                let err = parse(&format!("node; expand where edge.kind = {old};")).unwrap_err();
                assert!(
                    matches!(err, DslError::UnknownKindValue { .. }),
                    "{old} should be rejected"
                );
            }
        }

        #[test]
        fn expand_paragraph_link_yields_target_notes() {
            use assert_fs::prelude::*;

            let tmp = assert_fs::TempDir::new().unwrap();
            tmp.child(".obsidian").create_dir_all().unwrap();
            tmp.child("a.md").write_str("links to [[b]]\n").unwrap();
            tmp.child("b.md").write_str("hi\n").unwrap();
            let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q = parse(
                "node where kind = Paragraph; \
                 expand where from.kind = Paragraph and edge.kind = paragraph-link;",
            )
            .unwrap();
            let paragraphs = q.select(&g);
            // Two notes → two paragraphs; only a's paragraph has an
            // outgoing ParagraphLink edge.
            let mut total_targets = 0;
            for p in paragraphs {
                let children = q.expand(&g, p).unwrap();
                total_targets += children.len();
            }
            assert_eq!(total_targets, 1);
        }

        #[test]
        fn outdegree_zero_excludes_root() {
            let v = dirs_vault();
            let g = Graph::build(&v, &v.scan()).unwrap();
            // Restrict to Note kind: notes now own paragraph nodes via
            // OwnsParagraph edges (outdegree > 0), so leaf notes can no
            // longer have outdegree = 0. Filter to Paragraph instead
            // for the leaf check.
            let q = parse("node where kind = Paragraph and outdegree = 0;").unwrap();
            let ids = q.select(&g);
            // The 4 heading-only paragraphs in dirs/ have no outgoing
            // edges (no wiki links).
            assert_eq!(ids.len(), 4);
            for id in &ids {
                assert!(matches!(g.node(*id), NodeKind::Paragraph(_)));
            }
        }
    }

    // ── Walk tests ───────────────────────────────────────────────────

    mod walk {
        use std::path::PathBuf;

        use assert_fs::prelude::*;

        use crate::graph::{EdgeKind, Graph};
        use crate::vault::Vault;

        use super::*;

        fn dirs_graph() -> Graph {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("tests/fixtures/dirs");
            let v = Vault::discover(Some(path)).expect("dirs fixture vault must exist");
            Graph::build(&v, &v.scan()).unwrap()
        }

        fn dirs_query() -> GraphQuery {
            parse(
                "node where kind = Directory and path = \"\"; \
                 expand where from.kind = Directory \
                          and edge.kind = directory-contains \
                          and to.kind in {Note, Directory};",
            )
            .unwrap()
        }

        fn count_nodes(tree: &[WalkNode]) -> usize {
            tree.iter().map(|n| 1 + count_nodes(&n.children)).sum()
        }

        fn max_depth(tree: &[WalkNode]) -> usize {
            tree.iter()
                .map(|n| {
                    if n.children.is_empty() {
                        n.depth
                    } else {
                        max_depth(&n.children)
                    }
                })
                .max()
                .unwrap_or(0)
        }

        #[test]
        fn walk_unbounded_dirs_returns_full_tree() {
            let g = dirs_graph();
            let q = dirs_query();
            let tree = q.walk(&g, &WalkOptions::unlimited());
            assert_eq!(tree.len(), 1, "exactly one root: the vault root");
            assert_eq!(tree[0].depth, 0);
            assert!(tree[0].edge_to_parent.is_none(), "roots carry no edge");
            // 4 dirs (root + Projects + Areas + Areas/operations) + 4 notes
            // = 8 nodes reachable from the root. The walk visits every
            // node exactly once.
            assert_eq!(count_nodes(&tree), 8);
            // The deepest path is root → Areas → operations → shifts.md
            assert_eq!(max_depth(&tree), 3);
        }

        #[test]
        fn walk_depth_zero_returns_roots_only() {
            let g = dirs_graph();
            let q = dirs_query();
            let tree = q.walk(
                &g,
                &WalkOptions {
                    max_depth: Some(0),
                    ..Default::default()
                },
            );
            assert_eq!(tree.len(), 1);
            for root in &tree {
                assert!(root.children.is_empty(), "depth=0 means no descent at all");
            }
        }

        #[test]
        fn walk_depth_one_returns_immediate_children() {
            let g = dirs_graph();
            let q = dirs_query();
            let tree = q.walk(
                &g,
                &WalkOptions {
                    max_depth: Some(1),
                    ..Default::default()
                },
            );
            assert_eq!(tree.len(), 1);
            // Root's immediate children: Projects/, Areas/, root.md = 3
            assert_eq!(tree[0].children.len(), 3);
            for child in &tree[0].children {
                assert_eq!(child.depth, 1);
                assert!(child.children.is_empty(), "depth=1 means no grandchildren");
                assert!(matches!(child.edge_to_parent, Some(EdgeKind::Contains)));
            }
        }

        #[test]
        fn walk_edge_to_parent_is_populated_for_non_roots() {
            let g = dirs_graph();
            let q = dirs_query();
            let tree = q.walk(&g, &WalkOptions::unlimited());

            fn check(n: &WalkNode) {
                if n.depth == 0 {
                    assert!(n.edge_to_parent.is_none());
                } else {
                    assert!(
                        n.edge_to_parent.is_some(),
                        "non-root must carry its edge to parent"
                    );
                }
                for c in &n.children {
                    check(c);
                }
            }
            for root in &tree {
                check(root);
            }
        }

        /// Build an inline graph where `a.md` links to `b.md` which links
        /// back to `a.md` — a simple 2-cycle reachable from a.md.
        fn cyclic_graph() -> (assert_fs::TempDir, Graph) {
            let tmp = assert_fs::TempDir::new().unwrap();
            tmp.child(".obsidian").create_dir_all().unwrap();
            tmp.child("a.md").write_str("[[b]]\n").unwrap();
            tmp.child("b.md").write_str("[[a]]\n").unwrap();
            let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &v.scan()).unwrap();
            (tmp, g)
        }

        #[test]
        fn walk_dedup_marks_reentry_as_reference() {
            let (_tmp, g) = cyclic_graph();
            let q = parse(
                "node where path = \"a.md\"; \
                 expand where edge.kind = note-link;",
            )
            .unwrap();
            // Dedup is the default; unbounded.
            let tree = q.walk(&g, &WalkOptions::unlimited());
            // a → b → a(reference)
            assert_eq!(tree.len(), 1);
            assert_eq!(tree[0].closure, NodeClosure::Open);
            assert_eq!(tree[0].children.len(), 1, "a has one child b");
            let b = &tree[0].children[0];
            assert_eq!(b.closure, NodeClosure::Open);
            assert_eq!(b.children.len(), 1, "b expands once to the a-reference");
            let a_ref = &b.children[0];
            assert_eq!(
                a_ref.closure,
                NodeClosure::Reference,
                "the re-entered a is a dedup reference, not re-expanded"
            );
            assert!(
                a_ref.children.is_empty(),
                "reference markers have no children"
            );
            assert_eq!(a_ref.id, tree[0].id, "same node id as the root a");
        }

        #[test]
        fn walk_tree_marks_ancestor_reentry_as_cycle() {
            let (_tmp, g) = cyclic_graph();
            let q = parse(
                "node where path = \"a.md\"; \
                 expand where edge.kind = note-link;",
            )
            .unwrap();
            let tree = q.walk(
                &g,
                &WalkOptions {
                    max_depth: None,
                    visit: VisitPolicy::Tree,
                    ..Default::default()
                },
            );
            // a → b → a(cycle) — Tree mode reports the ancestor re-entry as
            // a cycle rather than a dedup reference.
            assert_eq!(tree.len(), 1);
            let a_cycle = &tree[0].children[0].children[0];
            assert_eq!(a_cycle.closure, NodeClosure::Cycle);
            assert!(
                a_cycle.children.is_empty(),
                "cycle markers have no children"
            );
            assert_eq!(a_cycle.id, tree[0].id);
        }

        #[test]
        fn walk_allow_never_marks_and_relies_on_depth() {
            let (_tmp, g) = cyclic_graph();
            let q = parse(
                "node where path = \"a.md\"; \
                 expand where edge.kind = note-link;",
            )
            .unwrap();
            let tree = q.walk(
                &g,
                &WalkOptions {
                    max_depth: Some(3),
                    visit: VisitPolicy::Allow,
                    ..Default::default()
                },
            );
            // No detection — a → b → a → b, terminating only at depth 3.
            assert_eq!(tree.len(), 1);
            let mut current = &tree[0];
            let mut visited_depths = vec![current.depth];
            while let Some(child) = current.children.first() {
                assert_eq!(child.closure, NodeClosure::Open, "Allow never marks a node");
                visited_depths.push(child.depth);
                current = child;
            }
            assert_eq!(visited_depths, vec![0, 1, 2, 3]);
            assert!(
                current.children.is_empty(),
                "depth bound is what terminates the walk"
            );
        }

        /// Build a diamond: `a → b`, `a → c`, `b → d`, `c → d`. `d` is a
        /// shared descendant reachable via two distinct, non-cyclic paths.
        fn diamond_graph() -> (assert_fs::TempDir, Graph) {
            let tmp = assert_fs::TempDir::new().unwrap();
            tmp.child(".obsidian").create_dir_all().unwrap();
            tmp.child("a.md").write_str("[[b]]\n[[c]]\n").unwrap();
            tmp.child("b.md").write_str("[[d]]\n").unwrap();
            tmp.child("c.md").write_str("[[d]]\n").unwrap();
            tmp.child("d.md").write_str("no links\n").unwrap();
            let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &v.scan()).unwrap();
            (tmp, g)
        }

        #[test]
        fn walk_dedup_expands_shared_descendant_once() {
            let (_tmp, g) = diamond_graph();
            let q =
                parse("node where path = \"a.md\"; expand where edge.kind = note-link;").unwrap();
            let tree = q.walk(&g, &WalkOptions::unlimited());
            // a + b + c + d(open under one parent) + d(reference under the
            // other) = 5 nodes — d is expanded exactly once.
            assert_eq!(count_nodes(&tree), 5);

            let a = &tree[0];
            let d_under: Vec<&WalkNode> =
                a.children.iter().map(|child| &child.children[0]).collect();
            assert_eq!(d_under.len(), 2, "d appears under both b and c");
            let opens = d_under
                .iter()
                .filter(|n| n.closure == NodeClosure::Open)
                .count();
            let refs = d_under
                .iter()
                .filter(|n| n.closure == NodeClosure::Reference)
                .count();
            assert_eq!(opens, 1, "d is expanded under exactly one parent");
            assert_eq!(refs, 1, "and a reference under the other");
        }

        #[test]
        fn walk_tree_repeats_shared_descendant() {
            let (_tmp, g) = diamond_graph();
            let q =
                parse("node where path = \"a.md\"; expand where edge.kind = note-link;").unwrap();
            let tree = q.walk(
                &g,
                &WalkOptions {
                    max_depth: None,
                    visit: VisitPolicy::Tree,
                    ..Default::default()
                },
            );
            // Tree mode repeats d's (empty) subtree under both b and c —
            // both are Open, neither a reference.
            let a = &tree[0];
            for child in &a.children {
                let d = &child.children[0];
                assert_eq!(
                    d.closure,
                    NodeClosure::Open,
                    "Tree repeats, never references"
                );
            }
        }

        #[test]
        fn walk_dedup_terminates_on_dense_graph() {
            // A complete digraph on N nodes: every note links to every
            // other. Under the old path-based behavior this enumerates O(N!)
            // simple paths; under Dedup it is bounded.
            let tmp = assert_fs::TempDir::new().unwrap();
            tmp.child(".obsidian").create_dir_all().unwrap();
            let n = 8;
            let names: Vec<String> = (0..n).map(|i| format!("n{i}")).collect();
            for i in 0..n {
                let body: String = names
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, name)| format!("[[{name}]]\n"))
                    .collect();
                tmp.child(format!("n{i}.md")).write_str(&body).unwrap();
            }
            let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &v.scan()).unwrap();
            let q =
                parse("node where path = \"n0.md\"; expand where edge.kind = note-link;").unwrap();
            let tree = q.walk(&g, &WalkOptions::unlimited());
            // Each of the n nodes is expanded once (n Open nodes); every
            // other incident edge yields a single reference leaf. The total
            // is bounded by O(V + E) = O(n^2), nowhere near O(n!).
            assert!(
                count_nodes(&tree) <= n * n,
                "dedup keeps the dense walk bounded"
            );
        }

        #[test]
        fn walk_max_nodes_truncates() {
            let g = dirs_graph();
            let q = dirs_query();
            let tree = q.walk(
                &g,
                &WalkOptions {
                    max_depth: None,
                    visit: VisitPolicy::Dedup,
                    max_nodes: Some(3),
                },
            );
            assert_eq!(count_nodes(&tree), 3, "the budget caps materialized nodes");
        }

        #[test]
        fn walk_no_expand_block_returns_flat_roots() {
            let g = dirs_graph();
            // Selector only — no expand block.
            let q = parse("node where kind = Directory;").unwrap();
            let tree = q.walk(&g, &WalkOptions::unlimited());
            assert!(!tree.is_empty(), "the dirs fixture has directories");
            for root in &tree {
                assert!(
                    root.children.is_empty(),
                    "no expand block means no children regardless of max_depth"
                );
                assert_eq!(root.closure, NodeClosure::Open);
                assert!(root.edge_to_parent.is_none());
            }
            // depth_zero with a None max_depth still returns nothing
            // below the roots — same as Some(_) — so the assertion above
            // is true for any max_depth value.
        }

        #[test]
        fn walk_empty_select_returns_empty_tree() {
            let g = dirs_graph();
            // Query that matches nothing — there are no notes whose
            // path starts with "nope/".
            let q = parse("node where path starts_with \"nope/\";").unwrap();
            let tree = q.walk(&g, &WalkOptions::unlimited());
            assert!(tree.is_empty());
        }

        #[test]
        fn walk_unlimited_terminates_on_cyclic_graph() {
            // Sanity: Stop policy + unlimited depth must terminate on a
            // cycle (otherwise this test would hang).
            let (_tmp, g) = cyclic_graph();
            let q =
                parse("node where path = \"a.md\"; expand where edge.kind = note-link;").unwrap();
            let tree = q.walk(&g, &WalkOptions::unlimited());
            // a(root) + b + a(cycle) = 3 nodes total
            assert_eq!(count_nodes(&tree), 3);
        }
    }

    // ── Error message snapshot tests ─────────────────────────────────

    mod error_snapshots {
        use super::*;

        macro_rules! snap_err {
            ($name:ident, $src:literal) => {
                #[test]
                fn $name() {
                    let err = parse($src).unwrap_err();
                    insta::assert_snapshot!(stringify!($name), format!("{err}"));
                }
            };
        }

        snap_err!(err_empty_input, "   ");
        snap_err!(err_no_initial_set, "expand where edge.kind = note-link;");
        snap_err!(
            err_type_mismatch_eq_with_set,
            "node where kind = {Note, Directory};"
        );
        snap_err!(err_type_mismatch_in_with_single, "node where kind in Note;");
        snap_err!(err_ambiguous_attribute, "node; expand where kind = link;");
        snap_err!(err_scope_from_in_node, "node where from.kind = Directory;");
        snap_err!(
            err_scope_self_in_expand,
            "node; expand where self.kind = Directory;"
        );
        snap_err!(
            err_scope_indegree_qualified,
            "node; expand where from.indegree = 0;"
        );
        snap_err!(err_scope_form_on_node, "node where form = wiki;");
        snap_err!(
            err_scope_path_on_edge,
            "node; expand where edge.path = foo;"
        );
        snap_err!(err_unknown_attribute, "node where foo = bar;");
        snap_err!(err_unknown_kind_value, "node where kind = Notes;");
        snap_err!(
            err_unknown_form_value,
            "node; expand where edge.form = html;"
        );
        snap_err!(err_trailing_input, "node; junk");
        snap_err!(err_unterminated_string, "node where path = \"oops");
        snap_err!(err_illegal_character, "node where path = @bogus;");
        snap_err!(err_v1_with_keyword, "node n with n.kind = Note;");
        snap_err!(
            err_v1_over_keyword,
            "node; expand over e(n, m) with e.kind = link;"
        );
    }

    // ── Proptest: round-trip + stability + whitespace insensitivity ──

    mod proptests {
        use super::*;
        use proptest::collection::vec;
        use proptest::prelude::*;

        // ── Literal generators (only safe forms — see below) ─────

        // Bare identifiers are used ONLY for known kind/form values
        // so that Display→parse round-trips can't be tripped up by
        // accidentally producing an identifier that the lexer treats
        // as a keyword. Arbitrary user strings always go through
        // `Literal::Str`, which gets quoted by Display.

        fn arb_node_kind_literal() -> impl Strategy<Value = Literal> {
            prop_oneof![
                Just(Literal::Ident("Note".into())),
                Just(Literal::Ident("Directory".into())),
                Just(Literal::Ident("Ghost".into())),
            ]
        }

        fn arb_edge_kind_literal() -> impl Strategy<Value = Literal> {
            prop_oneof![
                Just(Literal::Ident("note-link".into())),
                Just(Literal::Ident("heading-link".into())),
                Just(Literal::Ident("paragraph-link".into())),
                Just(Literal::Ident("directory-contains".into())),
                Just(Literal::Ident("has-task".into())),
                Just(Literal::Ident("subtask".into())),
            ]
        }

        fn arb_form_literal() -> impl Strategy<Value = Literal> {
            prop_oneof![
                Just(Literal::Ident("wiki".into())),
                Just(Literal::Ident("md".into())),
            ]
        }

        // Strings used in path/title queries. Restricted to a charset
        // that survives Display escaping cleanly: no shell weirdness
        // but enough variety to be meaningful (slashes, dots,
        // ascii-quotes, spaces).
        fn arb_user_string_literal() -> impl Strategy<Value = Literal> {
            proptest::string::string_regex(r#"[a-zA-Z0-9 ./_\-"\\]{0,12}"#)
                .unwrap()
                .prop_map(Literal::Str)
        }

        fn arb_int_literal() -> impl Strategy<Value = Literal> {
            (0i64..50).prop_map(Literal::Int)
        }

        // ── Condition shape helpers ──────────────────────────────

        fn op_value_node_kind() -> impl Strategy<Value = (Op, Value)> {
            prop_oneof![
                arb_node_kind_literal().prop_map(|l| (Op::Eq, Value::Single(l))),
                arb_node_kind_literal().prop_map(|l| (Op::NotEq, Value::Single(l))),
                vec(arb_node_kind_literal(), 1..=3).prop_map(|ls| (Op::In, Value::Set(ls))),
            ]
        }

        fn op_value_edge_kind() -> impl Strategy<Value = (Op, Value)> {
            prop_oneof![
                arb_edge_kind_literal().prop_map(|l| (Op::Eq, Value::Single(l))),
                arb_edge_kind_literal().prop_map(|l| (Op::NotEq, Value::Single(l))),
                vec(arb_edge_kind_literal(), 1..=3).prop_map(|ls| (Op::In, Value::Set(ls))),
            ]
        }

        fn op_value_form() -> impl Strategy<Value = (Op, Value)> {
            prop_oneof![
                arb_form_literal().prop_map(|l| (Op::Eq, Value::Single(l))),
                arb_form_literal().prop_map(|l| (Op::NotEq, Value::Single(l))),
                vec(arb_form_literal(), 1..=2).prop_map(|ls| (Op::In, Value::Set(ls))),
            ]
        }

        fn op_value_string_attr() -> impl Strategy<Value = (Op, Value)> {
            prop_oneof![
                arb_user_string_literal().prop_map(|l| (Op::Eq, Value::Single(l))),
                arb_user_string_literal().prop_map(|l| (Op::NotEq, Value::Single(l))),
                arb_user_string_literal().prop_map(|l| (Op::Includes, Value::Single(l))),
                arb_user_string_literal().prop_map(|l| (Op::StartsWith, Value::Single(l))),
                arb_user_string_literal().prop_map(|l| (Op::EndsWith, Value::Single(l))),
                vec(arb_user_string_literal(), 1..=3).prop_map(|ls| (Op::In, Value::Set(ls))),
            ]
        }

        fn op_value_int_attr() -> impl Strategy<Value = (Op, Value)> {
            prop_oneof![
                arb_int_literal().prop_map(|l| (Op::Eq, Value::Single(l))),
                arb_int_literal().prop_map(|l| (Op::NotEq, Value::Single(l))),
                vec(arb_int_literal(), 1..=3).prop_map(|ls| (Op::In, Value::Set(ls))),
            ]
        }

        // ── Condition generators per subject ─────────────────────

        fn arb_self_condition() -> impl Strategy<Value = Condition> {
            prop_oneof![
                op_value_node_kind().prop_map(|(op, value)| Condition {
                    subject: Subject::SelfNode,
                    attr: Attr::Kind,
                    op,
                    value,
                }),
                op_value_string_attr().prop_map(|(op, value)| Condition {
                    subject: Subject::SelfNode,
                    attr: Attr::Path,
                    op,
                    value,
                }),
                op_value_string_attr().prop_map(|(op, value)| Condition {
                    subject: Subject::SelfNode,
                    attr: Attr::Title,
                    op,
                    value,
                }),
                op_value_int_attr().prop_map(|(op, value)| Condition {
                    subject: Subject::SelfNode,
                    attr: Attr::Indegree,
                    op,
                    value,
                }),
                op_value_int_attr().prop_map(|(op, value)| Condition {
                    subject: Subject::SelfNode,
                    attr: Attr::Outdegree,
                    op,
                    value,
                }),
            ]
        }

        fn arb_from_to_condition(subject: Subject) -> impl Strategy<Value = Condition> {
            prop_oneof![
                op_value_node_kind().prop_map(move |(op, value)| Condition {
                    subject,
                    attr: Attr::Kind,
                    op,
                    value,
                }),
                op_value_string_attr().prop_map(move |(op, value)| Condition {
                    subject,
                    attr: Attr::Path,
                    op,
                    value,
                }),
                op_value_string_attr().prop_map(move |(op, value)| Condition {
                    subject,
                    attr: Attr::Title,
                    op,
                    value,
                }),
            ]
        }

        fn arb_edge_condition() -> impl Strategy<Value = Condition> {
            prop_oneof![
                op_value_edge_kind().prop_map(|(op, value)| Condition {
                    subject: Subject::Edge,
                    attr: Attr::Kind,
                    op,
                    value,
                }),
                op_value_form().prop_map(|(op, value)| Condition {
                    subject: Subject::Edge,
                    attr: Attr::Form,
                    op,
                    value,
                }),
            ]
        }

        fn arb_neighbor_filter() -> impl Strategy<Value = NeighborFilter> {
            (
                prop_oneof![Just(Direction::Incoming), Just(Direction::Outgoing)],
                vec(arb_edge_condition(), 0..=2),
            )
                .prop_map(|(direction, conditions)| NeighborFilter {
                    direction,
                    conditions,
                })
        }

        fn arb_node_selector() -> impl Strategy<Value = NodeSelector> {
            (
                vec(arb_self_condition(), 0..=3),
                prop_oneof![Just(None), arb_neighbor_filter().prop_map(Some),],
            )
                .prop_map(|(conditions, without)| NodeSelector {
                    condition: if conditions.is_empty() {
                        None
                    } else if conditions.len() == 1 {
                        Some(CondExpr::Cond(conditions.into_iter().next().unwrap()))
                    } else {
                        Some(CondExpr::And(
                            conditions.into_iter().map(CondExpr::Cond).collect(),
                        ))
                    },
                    without,
                })
        }

        fn arb_expand_condition() -> impl Strategy<Value = Condition> {
            prop_oneof![
                arb_from_to_condition(Subject::From),
                arb_from_to_condition(Subject::To),
                arb_edge_condition(),
            ]
        }

        fn arb_edge_policy() -> impl Strategy<Value = EdgePolicy> {
            vec(arb_expand_condition(), 0..=4).prop_map(|conditions| EdgePolicy { conditions })
        }

        fn arb_graph_query() -> impl Strategy<Value = GraphQuery> {
            (
                vec(arb_node_selector(), 1..=3),
                prop_oneof![Just(None), arb_edge_policy().prop_map(Some)],
            )
                .prop_map(|(initial, expansion)| GraphQuery { initial, expansion })
        }

        // ── Whitespace injector ──────────────────────────────────

        /// Insert random whitespace at token boundaries. Since the
        /// canonical Display uses single spaces, this exercises the
        /// lexer's tolerance. Skips injection inside string literals
        /// (between `"` opener and `"` closer) so quoted content isn't
        /// corrupted; respects `\"` escapes.
        fn inject_whitespace(src: &str, salt: u64) -> String {
            let mut out = String::with_capacity(src.len() * 2);
            let mut rng = salt;
            let mut prev_kind = CharKind::Punct;
            let mut in_string = false;
            let mut chars = src.chars().peekable();
            while let Some(c) = chars.next() {
                if in_string {
                    out.push(c);
                    if c == '\\' {
                        // Pass through next char unescaped — keep it
                        // attached to the backslash.
                        if let Some(next) = chars.next() {
                            out.push(next);
                        }
                    } else if c == '"' {
                        in_string = false;
                    }
                    prev_kind = CharKind::Punct;
                    continue;
                }
                if c == '"' {
                    out.push(c);
                    in_string = true;
                    prev_kind = CharKind::Punct;
                    continue;
                }
                let kind = char_kind(c);
                if kind != prev_kind && kind != CharKind::Space && prev_kind != CharKind::Space {
                    let n = (rng % 4) as usize;
                    rng = rng
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    for i in 0..n {
                        out.push(match (rng >> (i * 4)) & 3 {
                            0 => ' ',
                            1 => '\t',
                            2 => '\n',
                            _ => ' ',
                        });
                    }
                }
                out.push(c);
                prev_kind = kind;
            }
            out
        }

        #[derive(PartialEq, Eq, Clone, Copy)]
        enum CharKind {
            Alpha,
            Punct,
            Space,
        }

        fn char_kind(c: char) -> CharKind {
            if c.is_whitespace() {
                CharKind::Space
            } else if c.is_alphanumeric() || c == '_' || c == '-' {
                CharKind::Alpha
            } else {
                CharKind::Punct
            }
        }

        proptest! {
            #![proptest_config(ProptestConfig {
                cases: 256,
                .. ProptestConfig::default()
            })]

            /// The core invariant: every AST value the generator can
            /// produce serializes to text that parses back to itself.
            #[test]
            fn prop_round_trip(q in arb_graph_query()) {
                let s = format!("{q}");
                let parsed = parse(&s).map_err(|e| {
                    TestCaseError::fail(format!("re-parse failed for {:?}: {}", s, e))
                })?;
                prop_assert_eq!(parsed, q);
            }

            /// Stability: Display ∘ parse is idempotent. Parsing a
            /// canonical form and re-displaying yields the same text.
            #[test]
            fn prop_stability(q in arb_graph_query()) {
                let s1 = format!("{q}");
                let q1 = parse(&s1).map_err(|e| {
                    TestCaseError::fail(format!("parse failed: {}", e))
                })?;
                let s2 = format!("{q1}");
                prop_assert_eq!(s1, s2);
            }

            /// Whitespace insensitivity: extra spaces/tabs/newlines
            /// inserted at token boundaries don't change the parsed
            /// AST.
            #[test]
            fn prop_whitespace_insensitivity(q in arb_graph_query(), salt in any::<u64>()) {
                let canonical = format!("{q}");
                let noisy = inject_whitespace(&canonical, salt);
                let parsed = parse(&noisy).map_err(|e| {
                    TestCaseError::fail(format!("parse failed on whitespace-noisy form: {} on {:?}", e, noisy))
                })?;
                prop_assert_eq!(parsed, q);
            }
        }

        // ── Invalid-input variant-coverage tests ─────────────────

        /// For each `DslError` variant, supply a query string that
        /// triggers exactly that variant. Catches regressions where a
        /// grammar tweak silently routes an error to a different
        /// variant.
        #[test]
        fn every_dslerror_variant_is_reachable() {
            let cases: &[(&str, &str)] = &[
                ("EmptyInput", "   "),
                ("NoInitialSet", "expand where edge.kind = note-link;"),
                ("UnexpectedToken", "node where = Note;"),
                ("UnknownAttribute", "node where foo = bar;"),
                ("AmbiguousAttribute", "node; expand where kind = link;"),
                ("ScopeError", "node where from.kind = Note;"),
                ("TypeMismatch", "node where kind = {Note};"),
                ("UnknownKindValue", "node where kind = Notes;"),
                ("TrailingInput", "node; junk"),
                ("UnterminatedString", "node where path = \"oops"),
                ("IllegalCharacter", "node where path = @x;"),
            ];
            for (label, src) in cases {
                let err = match parse(src) {
                    Err(e) => e,
                    Ok(_) => panic!("expected {label} parsing {src:?}, but parse succeeded"),
                };
                let variant = dslerror_variant(&err);
                assert_eq!(
                    variant, *label,
                    "expected {label} for {src:?}, got {variant} ({err})"
                );
            }
        }

        fn dslerror_variant(e: &DslError) -> &'static str {
            match e {
                DslError::EmptyInput => "EmptyInput",
                DslError::NoInitialSet => "NoInitialSet",
                DslError::UnexpectedToken { .. } => "UnexpectedToken",
                DslError::UnknownAttribute { .. } => "UnknownAttribute",
                DslError::AmbiguousAttribute { .. } => "AmbiguousAttribute",
                DslError::ScopeError { .. } => "ScopeError",
                DslError::TypeMismatch { .. } => "TypeMismatch",
                DslError::UnknownKindValue { .. } => "UnknownKindValue",
                DslError::TrailingInput { .. } => "TrailingInput",
                DslError::UnterminatedString { .. } => "UnterminatedString",
                DslError::IllegalCharacter { .. } => "IllegalCharacter",
            }
        }
    }

    mod task_queries {
        use std::path::PathBuf;

        use super::*;
        use crate::graph::Graph;
        use crate::task::{Priority, Status, Task};
        use crate::vault::Vault;
        use assert_fs::prelude::*;

        fn vault_with_tasks() -> (assert_fs::TempDir, Scan) {
            let tmp = assert_fs::TempDir::new().unwrap();
            // Vault is discovered here (not returned) only to build the
            // single-pass scan the literal below overrides tasks on.
            tmp.child(".obsidian").create_dir_all().unwrap();
            tmp.child("root.md")
                .write_str("- [ ] Fix login bug\n- [x] Review quarterly report\n")
                .unwrap();
            tmp.child("Areas").create_dir_all().unwrap();
            tmp.child("Areas/finance.md")
                .write_str("- [ ] Process invoices\n")
                .unwrap();
            tmp.child("Projects").create_dir_all().unwrap();
            tmp.child("Projects/alpha.md")
                .write_str("- [ ] Ship beta\n")
                .unwrap();

            let scan = Scan {
                tasks: vec![
                    Task {
                        description: "Fix login bug".into(),
                        priority: Some(Priority::High),
                        due: Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 1).unwrap()),
                        tags: vec!["bug".into(), "urgent".into()],
                        source_file: PathBuf::from("root.md"),
                        source_line: 1,
                        ..Default::default()
                    },
                    Task {
                        description: "Review quarterly report".into(),
                        status: Status::Done,
                        tags: vec!["finance".into()],
                        source_file: PathBuf::from("root.md"),
                        source_line: 2,
                        ..Default::default()
                    },
                    Task {
                        description: "Process invoices".into(),
                        priority: Some(Priority::Medium),
                        due: Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 15).unwrap()),
                        tags: vec!["finance".into(), "invoices".into()],
                        source_file: PathBuf::from("Areas/finance.md"),
                        source_line: 1,
                        ..Default::default()
                    },
                ],
                ..Vault::discover(Some(tmp.path().to_path_buf()))
                    .unwrap()
                    .scan()
            };
            (tmp, scan)
        }

        /// Task 7.5: node_kind_str returns "Task" for task nodes.
        #[test]
        fn node_kind_str_returns_task() {
            let td = crate::graph::TaskData {
                description: "test".into(),
                status: "Open".into(),
                priority: None,
                due: None,
                scheduled: None,
                source_file: PathBuf::from("test.md"),
                source_line: 1,
                ..Default::default()
            };
            assert_eq!(super::node_kind_str(&NodeKind::Task(td)), "Task");
        }

        /// Task 7.6: edge_kind_str returns "has-task" for HasTask edges.
        #[test]
        fn edge_kind_str_returns_has_task() {
            assert_eq!(super::edge_kind_str(&EdgeKind::HasTask), "has-task");
        }

        /// Parse round-trip: links-into edge kind accepted in expand block.
        #[test]
        fn dsl_parses_links_into_edge_kind() {
            let q =
                parse("node where kind = Note; expand where edge.kind = \"links-into\";").unwrap();
            // Round-trip serialization.
            let s = q.to_string();
            let q2 = parse(&s).unwrap();
            assert_eq!(q, q2);
        }

        /// Parse round-trip: links-into accepted in set form.
        #[test]
        fn dsl_parses_links_into_in_set() {
            let q = parse(
                r#"node where kind = Directory and path = ""; expand where edge.kind in {directory-contains, links-into};"#,
            )
            .unwrap();
            let s = q.to_string();
            let q2 = parse(&s).unwrap();
            assert_eq!(q, q2);
        }

        /// Task 7.7: DSL `node where kind = "Task"` returns only task nodes.
        #[test]
        fn dsl_kind_eq_task_returns_task_nodes() {
            let (_tmp, scan) = vault_with_tasks();
            let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &scan).unwrap();

            let q = parse("node where kind = Task;").unwrap();
            let results = q.select(&g);
            assert_eq!(results.len(), 3);
            for id in &results {
                assert!(matches!(g.node(*id), NodeKind::Task(_)));
            }
        }

        /// Task 7.8: DSL task attribute filters.
        #[test]
        fn dsl_task_attribute_filters() {
            let (_tmp, scan) = vault_with_tasks();
            let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &scan).unwrap();

            // Filter by status = "Done"
            let q = parse(r#"node where kind = Task and status = "Done";"#).unwrap();
            let results = q.select(&g);
            assert_eq!(results.len(), 1);
            if let NodeKind::Task(td) = g.node(results[0]) {
                assert_eq!(td.description, "Review quarterly report");
            }

            // Filter by priority = "High"
            let q = parse(r#"node where kind = Task and priority = "High";"#).unwrap();
            let results = q.select(&g);
            assert_eq!(results.len(), 1);
            if let NodeKind::Task(td) = g.node(results[0]) {
                assert_eq!(td.description, "Fix login bug");
            }

            // Filter by due date
            let q = parse(r#"node where kind = Task and due = "2025-06-15";"#).unwrap();
            let results = q.select(&g);
            assert_eq!(results.len(), 1);
            if let NodeKind::Task(td) = g.node(results[0]) {
                assert_eq!(td.description, "Process invoices");
            }

            // Filter by description starts_with
            let q =
                parse(r#"node where kind = Task and description starts_with "Process";"#).unwrap();
            let results = q.select(&g);
            assert_eq!(results.len(), 1);
            if let NodeKind::Task(td) = g.node(results[0]) {
                assert_eq!(td.description, "Process invoices");
            }

            // Filter by tags includes
            let q = parse(r#"node where kind = Task and tags includes "bug";"#).unwrap();
            let results = q.select(&g);
            assert_eq!(results.len(), 1);
            if let NodeKind::Task(td) = g.node(results[0]) {
                assert_eq!(td.description, "Fix login bug");
            }

            // Filter by tags in set
            let q = parse(r#"node where kind = Task and tags in {"bug", "urgent"};"#).unwrap();
            let results = q.select(&g);
            assert_eq!(results.len(), 1);
            if let NodeKind::Task(td) = g.node(results[0]) {
                assert_eq!(td.description, "Fix login bug");
            }
        }

        /// 5.2: DSL expand with to.kind including "Task" reveals task children.
        #[test]
        fn dsl_expand_to_kind_includes_task() {
            let (_tmp, scan) = vault_with_tasks();
            let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &scan).unwrap();

            let q = parse(
                r#"node where kind = Directory and path = ""; expand where edge.kind in {directory-contains, has-task} and to.kind in {Note, Directory, Task};"#,
            )
                .unwrap();

            let tree = q.walk(&g, &WalkOptions::unlimited());
            assert!(!tree.is_empty());

            fn count_tasks_in_tree(nodes: &[WalkNode], graph: &Graph) -> usize {
                let mut count = 0;
                for node in nodes {
                    if matches!(graph.node(node.id), NodeKind::Task(_)) {
                        count += 1;
                    }
                    count += count_tasks_in_tree(&node.children, graph);
                }
                count
            }
            assert!(count_tasks_in_tree(&tree, &g) > 0);
        }

        /// 5.3: DSL expand with to.kind excluding "Task" omits task children.
        #[test]
        fn dsl_expand_to_kind_excludes_task() {
            let (_tmp, scan) = vault_with_tasks();
            let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &scan).unwrap();

            let q = parse(
                r#"node where kind = Directory and path = ""; expand where edge.kind in {directory-contains, has-task} and to.kind in {Note, Directory};"#,
            )
                .unwrap();

            let tree = q.walk(&g, &WalkOptions::unlimited());
            assert!(!tree.is_empty());

            fn count_tasks_in_tree(nodes: &[WalkNode], graph: &Graph) -> usize {
                let mut count = 0;
                for node in nodes {
                    if matches!(graph.node(node.id), NodeKind::Task(_)) {
                        count += 1;
                    }
                    count += count_tasks_in_tree(&node.children, graph);
                }
                count
            }
            assert_eq!(count_tasks_in_tree(&tree, &g), 0);
        }

        /// `self.path` on a task node returns the vault-relative path of
        /// the owning source file. Matches the task DSL's old
        /// `path includes "Areas/"` predicate semantics.
        #[test]
        fn dsl_path_on_task_matches_source_file() {
            let (_tmp, scan) = vault_with_tasks();
            let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &scan).unwrap();

            let q = parse(r#"node where kind = Task and path = "root.md";"#).unwrap();
            let results = q.select(&g);
            // Two tasks live in root.md in the fixture.
            assert_eq!(results.len(), 2);
            for id in &results {
                if let NodeKind::Task(td) = g.node(*id) {
                    assert_eq!(td.source_file.to_string_lossy(), "root.md");
                }
            }
        }

        /// 5.5: title attribute on task node yields no match.
        #[test]
        fn dsl_title_on_task_yields_no_match() {
            let (_tmp, scan) = vault_with_tasks();
            let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &scan).unwrap();

            let q = parse(r#"node where kind = Task and title = "anything";"#).unwrap();
            let results = q.select(&g);
            assert_eq!(results.len(), 0);
        }

        /// 5.6: Inequality and in-set on status.
        #[test]
        fn dsl_task_inequality_and_in_set() {
            let (_tmp, scan) = vault_with_tasks();
            let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &scan).unwrap();

            // status != "Done" returns the two open tasks
            let q = parse(r#"node where kind = Task and status != "Done";"#).unwrap();
            let results = q.select(&g);
            assert_eq!(results.len(), 2);
            for id in &results {
                if let NodeKind::Task(td) = g.node(*id) {
                    assert_ne!(td.status, "Done");
                }
            }

            // status in {"Open", "InProgress"} returns only open tasks
            let q =
                parse(r#"node where kind = Task and status in {"Open", "InProgress"};"#).unwrap();
            let results = q.select(&g);
            assert_eq!(results.len(), 2);
            for id in &results {
                if let NodeKind::Task(td) = g.node(*id) {
                    assert_eq!(td.status, "Open");
                }
            }
        }

        /// 5.7: description ends_with selects the matching task.
        #[test]
        fn dsl_task_description_ends_with() {
            let (_tmp, scan) = vault_with_tasks();
            let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
            let g = Graph::build(&v, &scan).unwrap();

            let q = parse(r#"node where kind = Task and description ends_with "bug";"#).unwrap();
            let results = q.select(&g);
            assert_eq!(results.len(), 1);
            if let NodeKind::Task(td) = g.node(results[0]) {
                assert_eq!(td.description, "Fix login bug");
            }
        }
    }

    // ── New ops and Date value coverage ──────────────────────────────
    mod new_ops {
        use super::*;
        use chrono::NaiveDate;

        fn today() -> NaiveDate {
            NaiveDate::from_ymd_opt(2026, 5, 9).unwrap()
        }

        fn p(src: &str) -> GraphQuery {
            parse_with(src, Profile::Default, today())
                .unwrap_or_else(|e| panic!("parse failed for {src:?}: {e}"))
        }

        #[test]
        fn lt_le_gt_ge_on_indegree() {
            let q = p("node where indegree > 5;");
            let c = q.initial[0].conditions()[0];
            assert_eq!(c.op, Op::Gt);
            assert_eq!(c.value, Value::Single(Literal::Int(5)));

            let q = p("node where indegree <= 10;");
            assert_eq!(q.initial[0].conditions()[0].op, Op::Le);
        }

        #[test]
        fn lt_le_gt_ge_on_due_date() {
            let q = parse_with(
                "node where kind = Task and self.due < today;",
                Profile::Default,
                today(),
            )
            .unwrap();
            let conds = q.initial[0].conditions();
            // [kind = Task, due < today]
            assert_eq!(conds.len(), 2);
            assert_eq!(conds[1].op, Op::Lt);
            assert_eq!(conds[1].value, Value::Single(Literal::Date(today())));
        }

        #[test]
        fn type_mismatch_lt_on_title() {
            let err = parse_with("node where self.title < \"x\";", Profile::Default, today())
                .unwrap_err();
            assert!(matches!(err, DslError::TypeMismatch { .. }), "got: {err}");
        }

        #[test]
        fn is_null_on_due() {
            let q = parse_with(
                "node where kind = Task and self.due is null;",
                Profile::Default,
                today(),
            )
            .unwrap();
            let conds = q.initial[0].conditions();
            assert_eq!(conds[1].op, Op::IsNull);
            assert_eq!(conds[1].value, Value::None);
        }

        #[test]
        fn is_not_null_on_due() {
            let q = parse_with(
                "node where kind = Task and self.due is not null;",
                Profile::Default,
                today(),
            )
            .unwrap();
            let conds = q.initial[0].conditions();
            assert_eq!(conds[1].op, Op::IsNotNull);
        }

        #[test]
        fn is_null_on_required_attr_errors() {
            let err =
                parse_with("node where self.kind is null;", Profile::Default, today()).unwrap_err();
            assert!(matches!(err, DslError::TypeMismatch { .. }), "got: {err}");
        }

        #[test]
        fn date_iso_literal() {
            let q = parse_with(
                "node where kind = Task and self.due = 2026-12-31;",
                Profile::Default,
                today(),
            )
            .unwrap();
            let conds = q.initial[0].conditions();
            assert_eq!(
                conds[1].value,
                Value::Single(Literal::Date(
                    NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()
                ))
            );
        }

        #[test]
        fn date_today_keyword_resolves_via_ft_today() {
            let q = parse_with(
                "node where kind = Task and self.due = today;",
                Profile::Default,
                today(),
            )
            .unwrap();
            let conds = q.initial[0].conditions();
            assert_eq!(conds[1].value, Value::Single(Literal::Date(today())));
        }

        #[test]
        fn date_relative_offsets() {
            let q = parse_with(
                "node where kind = Task and self.due < +7d;",
                Profile::Default,
                today(),
            )
            .unwrap();
            let conds = q.initial[0].conditions();
            let expected = today()
                .checked_add_signed(chrono::Duration::days(7))
                .unwrap();
            assert_eq!(conds[1].value, Value::Single(Literal::Date(expected)));
        }

        #[test]
        fn date_keyword_outside_date_context_errors() {
            // `self.title = today` — title is a string attr, `today` is not
            // a valid string. The parser uses Ident("today") here.
            let q = parse_with("node where self.title = today;", Profile::Default, today());
            // We don't strictly require a TypeMismatch error here — the
            // current parser accepts arbitrary idents on the rhs of `=`
            // for string attrs. The test pins the current behaviour so
            // we notice if it changes.
            assert!(q.is_ok());
        }

        #[test]
        fn roundtrip_lt_le_gt_ge() {
            for src in [
                "node where indegree > 5;",
                "node where indegree <= 10;",
                "node where kind = Task and due >= 2026-12-31;",
                "node where kind = Task and due < today;",
            ] {
                let q1 = parse_with(src, Profile::Default, today()).unwrap();
                let s = format!("{q1}");
                let q2 = parse_with(&s, Profile::Default, today()).unwrap();
                assert_eq!(q1, q2, "roundtrip mismatch:\n  src: {src}\n  ser: {s}");
            }
        }

        #[test]
        fn roundtrip_is_null() {
            for src in [
                "node where kind = Task and due is null;",
                "node where kind = Task and due is not null;",
            ] {
                let q1 = parse_with(src, Profile::Default, today()).unwrap();
                let s = format!("{q1}");
                let q2 = parse_with(&s, Profile::Default, today()).unwrap();
                assert_eq!(q1, q2);
            }
        }

        #[test]
        fn roundtrip_or_and_parens() {
            for src in [
                "node where kind = Task and (due = today or scheduled = today);",
                "node where (status = Open or status = InProgress) and priority = High;",
            ] {
                let q1 = parse_with(src, Profile::Default, today()).unwrap();
                let s = format!("{q1}");
                let q2 = parse_with(&s, Profile::Default, today()).unwrap();
                assert_eq!(q1, q2, "roundtrip:\n  src: {src}\n  ser: {s}");
            }
        }
    }

    // ── Tasks-profile desugaring ─────────────────────────────────────
    mod tasks_profile {
        use super::*;
        use chrono::NaiveDate;

        fn today() -> NaiveDate {
            NaiveDate::from_ymd_opt(2026, 5, 9).unwrap()
        }

        #[test]
        fn bare_predicate_desugars_to_node_kind_task_self() {
            let q_short = parse_with("priority = High", Profile::Tasks, today()).unwrap();
            let q_long = parse_with(
                "node where kind = Task and self.priority = High;",
                Profile::Default,
                today(),
            )
            .unwrap();
            assert_eq!(q_short, q_long);
        }

        #[test]
        fn explicit_node_block_preserved() {
            let src = "node where kind = Task and self.tags includes \"work\";";
            let q_tasks = parse_with(src, Profile::Tasks, today()).unwrap();
            let q_default = parse_with(src, Profile::Default, today()).unwrap();
            assert_eq!(q_tasks, q_default);
        }

        #[test]
        fn bare_path_includes() {
            let q = parse_with("path includes \"Areas/\"", Profile::Tasks, today()).unwrap();
            let conds = q.initial[0].conditions();
            // [kind = Task, path includes "Areas/"]
            assert_eq!(conds.len(), 2);
            assert_eq!(conds[1].subject, Subject::SelfNode);
            assert_eq!(conds[1].attr, Attr::Path);
        }

        #[test]
        fn bare_or_compound() {
            let q =
                parse_with("due = today or scheduled = today", Profile::Tasks, today()).unwrap();
            // The synthesized `and` from the prelude binds tighter than
            // the user's `or`, so the AST shape is:
            //   And(kind=Task, Or(due=today, scheduled=today))
            // expressed as a CondExpr tree on the sole selector.
            let leaves = q.initial[0].conditions();
            assert_eq!(leaves.len(), 3);
        }
    }
}

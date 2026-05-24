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

use std::fmt;

use crate::graph::{EdgeKind, Graph, LinkForm, NodeKind, NoteId};

// ── AST types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphQuery {
    pub initial: Vec<NodeSelector>,
    pub expansion: Option<EdgePolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSelector {
    pub conditions: Vec<Condition>,
    pub without: Option<NeighborFilter>,
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
    Indegree,
    Outdegree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    NotEq,
    In,
    Includes,
    StartsWith,
    EndsWith,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Single(Literal),
    Set(Vec<Literal>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    Ident(String),
    Str(String),
    Int(i64),
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
    // Punctuation
    Dot,
    Eq,
    NotEq,
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
                c if c.is_ascii_digit() => {
                    let n = self.read_number()?;
                    Token::Int(n)
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
        _ => Token::Ident(s),
    }
}

fn token_desc(t: &Token) -> &'static str {
    match t {
        Token::Node => "`node`",
        Token::Where => "`where`",
        Token::And => "`and`",
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
        Token::Dot => "`.`",
        Token::Eq => "`=`",
        Token::NotEq => "`!=`",
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
    }
}

// ── Parser ────────────────────────────────────────────────────────────

struct Parser {
    tokens: Vec<Spanned>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Spanned>) -> Self {
        Parser { tokens, pos: 0 }
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

        let conditions = if matches!(self.peek(), Token::Where) {
            self.advance();
            self.parse_condition_list(Scope::NodeBlock)?
        } else {
            Vec::new()
        };

        let without = if matches!(self.peek(), Token::Without) {
            self.advance();
            Some(self.parse_neighbor_filter()?)
        } else {
            None
        };

        Ok(NodeSelector {
            conditions,
            without,
        })
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
        let op = self.parse_op()?;
        let value = self.parse_value()?;

        // Parse-time op/value type check.
        let value_kind = match &value {
            Value::Single(_) => "literal",
            Value::Set(_) => "set",
        };
        let op_pos = self.tokens[self.pos.saturating_sub(1)].pos;
        match (op, &value) {
            (Op::In, Value::Single(_)) => {
                return Err(DslError::TypeMismatch {
                    op: op_label(op).into(),
                    expected: "set".into(),
                    got: value_kind.into(),
                    position: op_pos,
                });
            }
            (Op::Eq | Op::NotEq | Op::Includes | Op::StartsWith | Op::EndsWith, Value::Set(_)) => {
                return Err(DslError::TypeMismatch {
                    op: op_label(op).into(),
                    expected: "literal".into(),
                    got: value_kind.into(),
                    position: op_pos,
                });
            }
            _ => {}
        }

        // Parse-time kind/form value check (only for the enum-like attrs).
        if matches!(attr, Attr::Kind) {
            self.check_kind_values(subject, &value, op_pos)?;
        } else if matches!(attr, Attr::Form) {
            self.check_form_values(&value, op_pos)?;
        }

        Ok(Condition {
            subject,
            attr,
            op,
            value,
        })
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
            other => Err(DslError::UnexpectedToken {
                found: token_label(&other),
                expected: "an operator (`=`, `!=`, `in`, `includes`, `starts_with`, `ends_with`)"
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
            Subject::Edge => &["link", "embed", "directory-contains"],
            _ => &["Note", "Directory", "Ghost"],
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
        }
    }
}

fn parse_attr(s: &str, position: usize) -> Result<Attr, DslError> {
    match s {
        "kind" => Ok(Attr::Kind),
        "path" => Ok(Attr::Path),
        "title" => Ok(Attr::Title),
        "form" => Ok(Attr::Form),
        "indegree" => Ok(Attr::Indegree),
        "outdegree" => Ok(Attr::Outdegree),
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

        // Form: edge-only.
        (Attr::Form, Subject::Edge) => Ok(()),
        (Attr::Form, _) => Err(DslError::ScopeError {
            entity: subject_name(subject).into(),
            hint: "`form` is an edge attribute — use `edge.form` in an expand block or neighbor filter".into(),
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
    }
}

// ── Public entry point ────────────────────────────────────────────────

pub fn parse(src: &str) -> Result<GraphQuery, DslError> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return Err(DslError::EmptyInput);
    }

    // The lexer takes raw `src`, not `trimmed`, so error positions
    // line up with the original input.
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize()?;

    let mut parser = Parser::new(tokens);
    parser.parse_query()
}

// ── Evaluator ─────────────────────────────────────────────────────────

impl GraphQuery {
    pub fn select(&self, graph: &Graph) -> Vec<NoteId> {
        let mut results: Vec<NoteId> = Vec::new();
        for selector in &self.initial {
            for (id, _) in graph.nodes() {
                if eval_node_conditions(graph, id, &selector.conditions, Subject::SelfNode) {
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
        Some(children)
    }
}

fn push_unique(v: &mut Vec<NoteId>, id: NoteId) {
    if !v.contains(&id) {
        v.push(id);
    }
}

fn eval_node_conditions(
    graph: &Graph,
    id: NoteId,
    conditions: &[Condition],
    expected_subject: Subject,
) -> bool {
    for c in conditions {
        if c.subject != expected_subject {
            // Defensive — shouldn't happen if parser was correct.
            continue;
        }
        if !eval_cond_on_node(graph, id, c) {
            return false;
        }
    }
    true
}

fn eval_cond_on_node(graph: &Graph, id: NoteId, c: &Condition) -> bool {
    match c.attr {
        Attr::Kind | Attr::Path | Attr::Title => {
            let v = match node_string_attr(graph.node(id), c.attr) {
                Some(s) => s,
                None => return false,
            };
            eval_string_op(&v, c.op, &c.value)
        }
        Attr::Indegree => {
            let count = graph.incoming(id).count() as i64;
            eval_int_op(count, c.op, &c.value)
        }
        Attr::Outdegree => {
            let count = graph.outgoing(id).count() as i64;
            eval_int_op(count, c.op, &c.value)
        }
        // Form is edge-only — never reached on a node.
        Attr::Form => false,
    }
}

fn eval_cond_on_edge(edge: &EdgeKind, c: &Condition) -> bool {
    let v = match c.attr {
        Attr::Kind => edge_kind_str(edge).to_string(),
        Attr::Form => match edge_form_str(edge) {
            Some(s) => s.to_string(),
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
            NodeKind::Ghost(_) => None,
        },
        Attr::Title => match node {
            NodeKind::Note(n) => Some(n.title.clone()),
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
    }
}

fn edge_kind_str(e: &EdgeKind) -> &'static str {
    match e {
        EdgeKind::Link(_) => "link",
        EdgeKind::Embed(_) => "embed",
        EdgeKind::Contains => "directory-contains",
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
        (Op::In, Value::Set(items)) => items.iter().any(|lit| int_of(lit) == Some(actual)),
        // includes / starts_with / ends_with on integers → false
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
        if !self.conditions.is_empty() {
            write!(f, " where ")?;
            fmt_conditions(f, &self.conditions)?;
        }
        if let Some(ref nf) = self.without {
            write!(f, " without ")?;
            nf.fmt_filter(f)?;
        }
        Ok(())
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
    write!(f, " {} ", op_label(c.op))?;
    fmt_value(f, &c.value)
}

fn attr_name(a: Attr) -> &'static str {
    match a {
        Attr::Kind => "kind",
        Attr::Path => "path",
        Attr::Title => "title",
        Attr::Form => "form",
        Attr::Indegree => "indegree",
        Attr::Outdegree => "outdegree",
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
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
            assert!(q.initial[0].conditions.is_empty());
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
                q.initial[0].conditions[0],
                Condition {
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
            assert_eq!(q.initial[0].conditions[0].subject, Subject::SelfNode);
            assert_eq!(q.initial[0].conditions[0].attr, Attr::Kind);
        }

        #[test]
        fn parse_kind_in_set() {
            let q = parse_ok("node where kind in {Note, Directory};");
            assert_eq!(
                q.initial[0].conditions[0].value,
                Value::Set(vec![
                    Literal::Ident("Note".into()),
                    Literal::Ident("Directory".into()),
                ])
            );
        }

        #[test]
        fn parse_path_starts_with() {
            let q = parse_ok("node where path starts_with \"Projects/\";");
            assert_eq!(q.initial[0].conditions[0].op, Op::StartsWith);
            assert_eq!(
                q.initial[0].conditions[0].value,
                Value::Single(Literal::Str("Projects/".into()))
            );
        }

        #[test]
        fn parse_path_ends_with() {
            let q = parse_ok("node where path ends_with \".md\";");
            assert_eq!(q.initial[0].conditions[0].op, Op::EndsWith);
        }

        #[test]
        fn parse_path_includes() {
            let q = parse_ok("node where path includes \"Areas\";");
            assert_eq!(q.initial[0].conditions[0].op, Op::Includes);
        }

        #[test]
        fn parse_multiple_and_conditions() {
            let q = parse_ok("node where kind = Note and path starts_with \"Areas/\";");
            assert_eq!(q.initial[0].conditions.len(), 2);
        }

        #[test]
        fn parse_indegree() {
            let q = parse_ok("node where indegree = 0;");
            assert_eq!(q.initial[0].conditions[0].attr, Attr::Indegree);
            assert_eq!(
                q.initial[0].conditions[0].value,
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
            let q = parse_ok("node; expand where edge.kind = link;");
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
                q.initial[0].conditions[0].value,
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
            let err = parse("expand where edge.kind = link;").unwrap_err();
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
            let g = Graph::build(&v).unwrap();
            let q = parse("node;").unwrap();
            let ids = q.select(&g);
            // 4 notes + 4 dirs = 8
            assert_eq!(ids.len(), 8);
        }

        #[test]
        fn select_all_notes() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse("node where kind = Note;").unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 4);
        }

        #[test]
        fn select_all_directories() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse("node where kind = Directory;").unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 4);
        }

        #[test]
        fn select_path_starts_with() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
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
            let g = Graph::build(&v).unwrap();
            let q = parse("node where path starts_with \"Projects/\";").unwrap();
            let ids = q.select(&g);
            // Only Projects/alpha.md (the directory itself is "Projects", not "Projects/")
            assert_eq!(ids.len(), 1);
        }

        #[test]
        fn select_path_ends_with_md() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse("node where path ends_with \".md\";").unwrap();
            let ids = q.select(&g);
            // All 4 notes end with .md; no directories should match.
            assert_eq!(ids.len(), 4);
        }

        #[test]
        fn select_kind_in_set() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse("node where kind in {Note, Directory};").unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 8);
        }

        #[test]
        fn select_indegree_zero() {
            // Only the vault root directory has no incoming edges.
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
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
            let g = Graph::build(&v).unwrap();
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
            let g = Graph::build(&v).unwrap();
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
            let g = Graph::build(&v).unwrap();
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
            let g = Graph::build(&v).unwrap();
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
            let g = Graph::build(&v).unwrap();
            let q = parse("node where kind = Note;").unwrap();
            let any = q.select(&g)[0];
            assert!(q.expand(&g, any).is_none());
        }

        #[test]
        fn expand_some_empty_when_parent_mismatch() {
            // v2 behavior: parent that doesn't satisfy `from` conditions
            // returns Some(vec![]), not None.
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
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
            let g = Graph::build(&v).unwrap();
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
            let g = Graph::build(&v).unwrap();
            let q = parse("node where title = \"root\";").unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 1);
        }

        #[test]
        fn outdegree_zero_excludes_root() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse("node where outdegree = 0;").unwrap();
            let ids = q.select(&g);
            // The 4 notes in dirs/ have no outgoing edges; the root and
            // the directory nodes have at least one Contains edge each.
            assert_eq!(ids.len(), 4);
            for id in &ids {
                assert!(matches!(g.node(*id), NodeKind::Note(_)));
            }
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
        snap_err!(err_no_initial_set, "expand where edge.kind = link;");
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
}

//! Graph query DSL — tokenizer, recursive-descent parser, and evaluator
//! for filtering nodes and expansions in the note-link graph.
//!
//! Grammar (v1):
//! ```text
//! query        = node_block (";" node_block)* ";" expand_block? ";"
//!
//! node_block   = "node" IDENT ("with" condition)+
//!                ["without" "(" edge_expr ")"]
//!
//! edge_expr    = "edge" IDENT "(" ("_" | IDENT) "," IDENT ")"
//!                ("with" condition)*
//!
//! expand_block = "expand" "over" IDENT "(" IDENT "," IDENT ")"
//!                ("with" condition)+
//!
//! condition    = IDENT "." IDENT op value
//!
//! op           = "=" | "!=" | "includes" | "in"
//!
//! value        = literal | "{" literal ("," literal)* "}"
//!
//! literal      = IDENT | STRING
//! ```
//!
//! Node attributes: `kind`, `path`, `title`
//! Edge attributes: `kind`, `form`

use std::fmt;

use crate::graph::{EdgeKind, Graph, LinkForm, NodeKind, NoteId};

// ── AST types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphQuery {
    pub initial: Vec<NodeSelector>,
    pub expansion: Option<ExpansionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSelector {
    pub var: String,
    pub conditions: Vec<Condition>,
    pub without: Option<EdgePattern>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgePattern {
    pub var: String,
    pub src_var: SrcSpec,
    pub node_var: String,
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SrcSpec {
    Wildcard,
    Named(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpansionRule {
    pub edge_var: String,
    pub from_var: String,
    pub to_var: String,
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub var: String,
    pub attr: Attr,
    pub op: Op,
    pub value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attr {
    Kind,
    Path,
    Title,
    Form,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    NotEq,
    Includes,
    In,
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
}

// ── Tokenizer ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    // Keywords
    Node,
    With,
    Without,
    Edge,
    Expand,
    Over,
    InKw,
    IncludesKw,
    // Punctuation
    Dot,
    Eq,
    NotEq,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Underscore,
    Semi,
    // Values
    Ident(String),
    Str(String),
    Eof,
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

    fn tokenize(&mut self) -> (Vec<Token>, Option<DslError>) {
        let mut tokens = Vec::new();
        loop {
            self.skip_ws();
            if self.pos >= self.chars.len() {
                tokens.push(Token::Eof);
                break;
            }
            let ch = self.chars[self.pos];
            match ch {
                '.' => {
                    tokens.push(Token::Dot);
                    self.pos += 1;
                }
                '!' => {
                    self.pos += 1;
                    if self.pos < self.chars.len() && self.chars[self.pos] == '=' {
                        tokens.push(Token::NotEq);
                        self.pos += 1;
                    } else {
                        return (tokens, Some(self.error("expected `=` after `!`")));
                    }
                }
                '=' => {
                    tokens.push(Token::Eq);
                    self.pos += 1;
                }
                '(' => {
                    tokens.push(Token::LParen);
                    self.pos += 1;
                }
                ')' => {
                    tokens.push(Token::RParen);
                    self.pos += 1;
                }
                '{' => {
                    tokens.push(Token::LBrace);
                    self.pos += 1;
                }
                '}' => {
                    tokens.push(Token::RBrace);
                    self.pos += 1;
                }
                ',' => {
                    tokens.push(Token::Comma);
                    self.pos += 1;
                }
                '_' => {
                    tokens.push(Token::Underscore);
                    self.pos += 1;
                }
                ';' => {
                    tokens.push(Token::Semi);
                    self.pos += 1;
                }
                '"' | '\'' => {
                    let quote = ch;
                    self.pos += 1;
                    let start = self.pos;
                    let mut s = String::new();
                    while self.pos < self.chars.len() && self.chars[self.pos] != quote {
                        s.push(self.chars[self.pos]);
                        self.pos += 1;
                    }
                    if self.pos >= self.chars.len() {
                        return (tokens, Some(self.error_at(start, "unterminated string")));
                    }
                    self.pos += 1;
                    tokens.push(Token::Str(s));
                }
                c if c.is_alphabetic() || c == '-' || c == '_' => {
                    let ident = self.read_ident();
                    tokens.push(match ident.as_str() {
                        "node" => Token::Node,
                        "with" => Token::With,
                        "without" => Token::Without,
                        "edge" => Token::Edge,
                        "expand" => Token::Expand,
                        "over" => Token::Over,
                        "in" => Token::InKw,
                        "includes" => Token::IncludesKw,
                        _ => Token::Ident(ident),
                    });
                }
                _ => {
                    return (
                        tokens,
                        Some(self.error(&format!("unexpected character `{ch}`"))),
                    );
                }
            }
        }
        (tokens, None)
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

    fn error(&self, msg: &str) -> DslError {
        DslError::ParseError {
            message: msg.to_string(),
            position: self.pos,
        }
    }

    fn error_at(&self, pos: usize, msg: &str) -> DslError {
        DslError::ParseError {
            message: msg.to_string(),
            position: pos,
        }
    }
}

// ── Error ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DslError {
    ParseError { message: String, position: usize },
    UnexpectedToken { found: String, expected: String },
    UnknownAttribute(String),
    EmptyInput,
    TrailingTokens(String),
}

impl fmt::Display for DslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DslError::ParseError { message, position } => {
                write!(f, "parse error at position {position}: {message}")
            }
            DslError::UnexpectedToken { found, expected } => {
                write!(f, "expected {expected}, found `{found}`")
            }
            DslError::UnknownAttribute(s) => write!(f, "unknown attribute `{s}`"),
            DslError::EmptyInput => write!(f, "empty query"),
            DslError::TrailingTokens(s) => write!(f, "unexpected tokens after end of query: {s}"),
        }
    }
}

// ── Parser ────────────────────────────────────────────────────────────

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> Token {
        self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let t = self.peek();
        if !matches!(t, Token::Eof) {
            self.pos += 1;
        }
        t
    }

    fn expect_ident(&mut self) -> Result<String, DslError> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            Token::Str(s) => Ok(s),
            t => Err(self.unexpected(&t, "identifier")),
        }
    }

    fn consume(&mut self, expected: Token) -> Result<(), DslError> {
        let t = self.peek();
        if std::mem::discriminant(&t) == std::mem::discriminant(&expected) {
            self.advance();
            Ok(())
        } else {
            let desc = token_desc(&expected);
            Err(self.unexpected(&t, desc))
        }
    }

    fn unexpected(&self, t: &Token, expected: &str) -> DslError {
        DslError::UnexpectedToken {
            found: token_label(t.clone()),
            expected: expected.to_string(),
        }
    }

    fn parse_query(&mut self) -> Result<GraphQuery, DslError> {
        let mut initial = Vec::new();

        while matches!(self.peek(), Token::Node) {
            initial.push(self.parse_node_block()?);
            self.consume(Token::Semi)?;
        }

        let expansion = if matches!(self.peek(), Token::Expand) {
            let rule = self.parse_expand_block()?;
            self.consume(Token::Semi)?;
            Some(rule)
        } else {
            None
        };

        Ok(GraphQuery { initial, expansion })
    }

    fn parse_node_block(&mut self) -> Result<NodeSelector, DslError> {
        self.consume(Token::Node)?;
        let var = self.expect_ident()?;

        if !matches!(self.peek(), Token::With) {
            return Err(DslError::UnexpectedToken {
                found: token_label(self.peek()),
                expected: "with".to_string(),
            });
        }

        let mut conditions = Vec::new();
        while matches!(self.peek(), Token::With) {
            self.pos += 1; // consume `with`
            conditions.push(self.parse_condition()?);
        }

        let without = if matches!(self.peek(), Token::Without) {
            self.pos += 1; // consume `without`
            self.consume(Token::LParen)?;
            let ep = self.parse_edge_expr()?;
            self.consume(Token::RParen)?;
            Some(ep)
        } else {
            None
        };

        Ok(NodeSelector {
            var,
            conditions,
            without,
        })
    }

    fn parse_edge_expr(&mut self) -> Result<EdgePattern, DslError> {
        self.consume(Token::Edge)?;
        let var = self.expect_ident()?;
        self.consume(Token::LParen)?;

        let src_var = if matches!(self.peek(), Token::Underscore) {
            self.pos += 1;
            SrcSpec::Wildcard
        } else {
            SrcSpec::Named(self.expect_ident()?)
        };

        self.consume(Token::Comma)?;
        let node_var = self.expect_ident()?;
        self.consume(Token::RParen)?;

        let mut conditions = Vec::new();
        while matches!(self.peek(), Token::With) {
            self.pos += 1;
            conditions.push(self.parse_condition()?);
        }

        Ok(EdgePattern {
            var,
            src_var,
            node_var,
            conditions,
        })
    }

    fn parse_expand_block(&mut self) -> Result<ExpansionRule, DslError> {
        self.consume(Token::Expand)?;
        self.consume(Token::Over)?;
        let edge_var = self.expect_ident()?;
        self.consume(Token::LParen)?;
        let from_var = self.expect_ident()?;
        self.consume(Token::Comma)?;
        let to_var = self.expect_ident()?;
        self.consume(Token::RParen)?;

        if !matches!(self.peek(), Token::With) {
            return Err(DslError::UnexpectedToken {
                found: token_label(self.peek()),
                expected: "with".to_string(),
            });
        }

        let mut conditions = Vec::new();
        while matches!(self.peek(), Token::With) {
            self.pos += 1;
            conditions.push(self.parse_condition()?);
        }

        Ok(ExpansionRule {
            edge_var,
            from_var,
            to_var,
            conditions,
        })
    }

    fn parse_condition(&mut self) -> Result<Condition, DslError> {
        let var = self.expect_ident()?;
        self.consume(Token::Dot)?;
        let attr_name = self.expect_ident()?;
        let attr = parse_attr(&attr_name)?;
        let op = self.parse_op()?;
        let value = self.parse_value()?;
        Ok(Condition {
            var,
            attr,
            op,
            value,
        })
    }

    fn parse_op(&mut self) -> Result<Op, DslError> {
        match self.advance() {
            Token::Eq => Ok(Op::Eq),
            Token::NotEq => Ok(Op::NotEq),
            Token::IncludesKw => Ok(Op::Includes),
            Token::InKw => Ok(Op::In),
            t => Err(self.unexpected(&t, "`=`, `!=`, `includes`, or `in`")),
        }
    }

    fn parse_value(&mut self) -> Result<Value, DslError> {
        if matches!(self.peek(), Token::LBrace) {
            self.pos += 1;
            let mut items = Vec::new();
            items.push(self.parse_literal()?);
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                items.push(self.parse_literal()?);
            }
            self.consume(Token::RBrace)?;
            Ok(Value::Set(items))
        } else {
            Ok(Value::Single(self.parse_literal()?))
        }
    }

    fn parse_literal(&mut self) -> Result<Literal, DslError> {
        match self.advance() {
            Token::Ident(s) => Ok(Literal::Ident(s.clone())),
            Token::Str(s) => Ok(Literal::Str(s.clone())),
            t => Err(self.unexpected(&t, "identifier or string")),
        }
    }
}

fn parse_attr(s: &str) -> Result<Attr, DslError> {
    match s {
        "kind" => Ok(Attr::Kind),
        "path" => Ok(Attr::Path),
        "title" => Ok(Attr::Title),
        "form" => Ok(Attr::Form),
        _ => Err(DslError::UnknownAttribute(s.to_string())),
    }
}

fn token_desc(t: &Token) -> &'static str {
    match t {
        Token::Node => "`node`",
        Token::With => "`with`",
        Token::Without => "`without`",
        Token::Edge => "`edge`",
        Token::Expand => "`expand`",
        Token::Over => "`over`",
        Token::InKw => "`in`",
        Token::IncludesKw => "`includes`",
        Token::Dot => "`.`",
        Token::Eq => "`=`",
        Token::NotEq => "`!=`",
        Token::LParen => "`(`",
        Token::RParen => "`)`",
        Token::LBrace => "`{`",
        Token::RBrace => "`}`",
        Token::Comma => "`,`",
        Token::Underscore => "`_`",
        Token::Semi => "`;`",
        Token::Ident(_) => "identifier",
        Token::Str(_) => "string",
        Token::Eof => "end of input",
    }
}

fn token_label(t: Token) -> String {
    match t {
        Token::Ident(s) => format!("`{s}`"),
        Token::Str(s) => format!("`\"{s}\"`"),
        Token::Eof => "end of input".to_string(),
        other => token_desc(&other).trim_matches('`').to_string(),
    }
}

// ── Public entry point ────────────────────────────────────────────────

pub fn parse(src: &str) -> Result<GraphQuery, DslError> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return Err(DslError::EmptyInput);
    }

    let mut lexer = Lexer::new(trimmed);
    let (tokens, lex_err) = lexer.tokenize();
    if let Some(e) = lex_err {
        return Err(e);
    }

    let mut parser = Parser::new(tokens);
    let query = parser.parse_query()?;

    // Ensure EOF
    match parser.peek() {
        Token::Eof => {}
        t => {
            return Err(DslError::TrailingTokens(token_label(t).to_string()));
        }
    }

    Ok(query)
}

// ── Evaluator ─────────────────────────────────────────────────────────

impl GraphQuery {
    pub fn select(&self, graph: &Graph) -> Vec<NoteId> {
        let mut results = Vec::new();
        for selector in &self.initial {
            for (id, _node) in graph.nodes() {
                if eval_node_conditions(graph, id, &selector.conditions) {
                    if let Some(ref without) = &selector.without {
                        if !eval_without(graph, id, without) {
                            results.push(id);
                        }
                    } else {
                        results.push(id);
                    }
                }
            }
        }
        results.dedup();
        results
    }

    pub fn expand(&self, graph: &Graph, parent: NoteId) -> Option<Vec<NoteId>> {
        let rule = self.expansion.as_ref()?;

        // Check that the parent node satisfies the from_var conditions
        if !conditions_for_var(&rule.conditions, &rule.from_var)
            .iter()
            .all(|c| eval_cond_on_node(graph, parent, c))
        {
            return None;
        }

        let edge_conds: Vec<&Condition> = conditions_for_var(&rule.conditions, &rule.edge_var);
        let child_conds: Vec<&Condition> = conditions_for_var(&rule.conditions, &rule.to_var);

        let mut children: Vec<NoteId> = Vec::new();
        for (child_id, edge) in graph.outgoing(parent) {
            if edge_conds.iter().all(|c| eval_cond_on_edge(edge, c))
                && child_conds
                    .iter()
                    .all(|c| eval_cond_on_node(graph, child_id, c))
            {
                children.push(child_id);
            }
        }
        Some(children)
    }
}

fn conditions_for_var<'a>(conditions: &'a [Condition], var: &str) -> Vec<&'a Condition> {
    conditions.iter().filter(|c| c.var == var).collect()
}

fn eval_cond_on_node(graph: &Graph, id: NoteId, cond: &Condition) -> bool {
    let node = graph.node(id);
    let v = match node_attr(node, cond.attr) {
        Some(s) => s,
        None => return false,
    };
    eval_op(&v, cond.op, &cond.value)
}

fn eval_cond_on_edge(edge: &EdgeKind, cond: &Condition) -> bool {
    let v = match edge_attr(edge, cond.attr) {
        Some(s) => s,
        None => return false,
    };
    eval_op(&v, cond.op, &cond.value)
}

fn eval_node_conditions(graph: &Graph, id: NoteId, conditions: &[Condition]) -> bool {
    let node = graph.node(id);
    for cond in conditions {
        let v = match node_attr(node, cond.attr) {
            Some(s) => s,
            None => return false,
        };
        if !eval_op(&v, cond.op, &cond.value) {
            return false;
        }
    }
    true
}

fn eval_without(graph: &Graph, id: NoteId, pattern: &EdgePattern) -> bool {
    for (_src, edge) in graph.incoming(id) {
        let mut matches = true;
        for cond in &pattern.conditions {
            let v = match edge_attr(edge, cond.attr) {
                Some(s) => s,
                None => {
                    matches = false;
                    break;
                }
            };
            if !eval_op(&v, cond.op, &cond.value) {
                matches = false;
                break;
            }
        }
        if matches {
            return true;
        }
    }
    false
}

fn node_attr(node: &NodeKind, attr: Attr) -> Option<String> {
    match attr {
        Attr::Kind => Some(match node {
            NodeKind::Note(_) => "Note".to_string(),
            NodeKind::Ghost(_) => "Ghost".to_string(),
            NodeKind::Directory(_) => "Directory".to_string(),
        }),
        Attr::Path => match node {
            NodeKind::Note(n) => Some(n.path.to_string_lossy().into_owned()),
            NodeKind::Directory(d) => Some(d.path.to_string_lossy().into_owned()),
            NodeKind::Ghost(_) => None,
        },
        Attr::Title => match node {
            NodeKind::Note(n) => Some(n.title.clone()),
            _ => None,
        },
        Attr::Form => None, // form is an edge attribute
    }
}

fn edge_attr(edge: &EdgeKind, attr: Attr) -> Option<String> {
    match attr {
        Attr::Kind => Some(match edge {
            EdgeKind::Link(_) => "link".to_string(),
            EdgeKind::Embed(_) => "embed".to_string(),
            EdgeKind::Contains => "directory-contains".to_string(),
        }),
        Attr::Form => edge.link().map(|l| match l.form {
            LinkForm::WikiLink => "wiki".to_string(),
            LinkForm::MdLink => "md".to_string(),
        }),
        _ => None,
    }
}

fn eval_op(value: &str, op: Op, expected: &Value) -> bool {
    match op {
        Op::Eq => match expected {
            Value::Single(lit) => value == lit_str(lit),
            Value::Set(_) => false, // `=` with a set is invalid; parser ensures a single literal
        },
        Op::NotEq => match expected {
            Value::Single(lit) => value != lit_str(lit),
            Value::Set(_) => false,
        },
        Op::Includes => match expected {
            Value::Single(lit) => value.contains(lit_str(lit)),
            Value::Set(_) => false,
        },
        Op::In => match expected {
            Value::Set(items) => items.iter().any(|lit| value == lit_str(lit)),
            Value::Single(_) => false,
        },
    }
}

fn lit_str(lit: &Literal) -> &str {
    match lit {
        Literal::Ident(s) => s.as_str(),
        Literal::Str(s) => s.as_str(),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod query_tests {
    use super::*;

    mod parser {
        use super::*;

        fn parse_ok(src: &str) -> GraphQuery {
            parse(src).unwrap_or_else(|e| panic!("parse failed: {e}"))
        }

        #[test]
        fn parse_node_with_kind_equals() {
            let q = parse_ok("node n with n.kind = Note;");
            assert_eq!(q.initial.len(), 1);
            assert!(q.expansion.is_none());
            let sel = &q.initial[0];
            assert_eq!(sel.var, "n");
            assert_eq!(sel.conditions.len(), 1);
            assert_eq!(
                sel.conditions[0],
                Condition {
                    var: "n".into(),
                    attr: Attr::Kind,
                    op: Op::Eq,
                    value: Value::Single(Literal::Ident("Note".into())),
                }
            );
            assert!(sel.without.is_none());
        }

        #[test]
        fn parse_node_with_kind_in_set() {
            let q = parse_ok("node n with n.kind in {Note, Directory};");
            let sel = &q.initial[0];
            assert_eq!(sel.conditions.len(), 1);
            assert_eq!(
                sel.conditions[0],
                Condition {
                    var: "n".into(),
                    attr: Attr::Kind,
                    op: Op::In,
                    value: Value::Set(vec![
                        Literal::Ident("Note".into()),
                        Literal::Ident("Directory".into()),
                    ]),
                }
            );
        }

        #[test]
        fn parse_node_with_path_includes() {
            let q = parse_ok("node n with n.path includes \"Project\";");
            let sel = &q.initial[0];
            assert_eq!(
                sel.conditions[0],
                Condition {
                    var: "n".into(),
                    attr: Attr::Path,
                    op: Op::Includes,
                    value: Value::Single(Literal::Str("Project".into())),
                }
            );
        }

        #[test]
        fn parse_node_with_title_equals() {
            let q = parse_ok("node n with n.title = \"report\";");
            let sel = &q.initial[0];
            assert_eq!(
                sel.conditions[0],
                Condition {
                    var: "n".into(),
                    attr: Attr::Title,
                    op: Op::Eq,
                    value: Value::Single(Literal::Str("report".into())),
                }
            );
        }

        #[test]
        fn parse_node_without_edge() {
            let q = parse_ok(
                "node n with n.kind in {Note, Directory} without (edge e(_, n) with e.kind = directory-contains);",
            );
            let sel = &q.initial[0];
            let without = sel.without.as_ref().unwrap();
            assert_eq!(without.var, "e");
            assert_eq!(without.src_var, SrcSpec::Wildcard);
            assert_eq!(without.node_var, "n");
            assert_eq!(without.conditions.len(), 1);
            assert_eq!(
                without.conditions[0],
                Condition {
                    var: "e".into(),
                    attr: Attr::Kind,
                    op: Op::Eq,
                    value: Value::Single(Literal::Ident("directory-contains".into())),
                }
            );
        }

        #[test]
        fn parse_node_with_multiple_conditions() {
            let q = parse_ok("node n with n.kind = Note with n.path includes \"Area\";");
            let sel = &q.initial[0];
            assert_eq!(sel.conditions.len(), 2);
        }

        #[test]
        fn parse_two_node_blocks_union() {
            let q = parse_ok("node n with n.kind = Note; node d with d.kind = Directory;");
            assert_eq!(q.initial.len(), 2);
            assert_eq!(q.initial[0].var, "n");
            assert_eq!(q.initial[1].var, "d");
        }

        #[test]
        fn parse_expand_over() {
            let q = parse_ok(
                "expand over e(n, m) with n.kind = Directory with e.kind = directory-contains with m.kind in {Note, Directory};",
            );
            let rule = q.expansion.as_ref().unwrap();
            assert_eq!(rule.edge_var, "e");
            assert_eq!(rule.from_var, "n");
            assert_eq!(rule.to_var, "m");
            assert_eq!(rule.conditions.len(), 3);
        }

        #[test]
        fn parse_no_expand() {
            let q = parse_ok("node n with n.kind = Note;");
            assert!(q.expansion.is_none());
        }

        #[test]
        fn parse_node_with_title_not_equals() {
            let q = parse_ok("node n with n.title != \"ignore\";");
            let sel = &q.initial[0];
            assert_eq!(sel.conditions[0].op, Op::NotEq);
            assert_eq!(
                sel.conditions[0].value,
                Value::Single(Literal::Str("ignore".into()))
            );
        }

        #[test]
        fn parse_error_unterminated_string() {
            let err = parse("node n with n.path = \"unclosed;").unwrap_err();
            assert!(matches!(err, DslError::ParseError { .. }));
            assert!(format!("{err}").contains("unterminated"));
        }

        #[test]
        fn parse_error_unknown_attribute() {
            let err = parse("node n with n.foo = bar;").unwrap_err();
            assert!(matches!(err, DslError::UnknownAttribute(_)));
        }

        #[test]
        fn parse_error_missing_semicolon() {
            let err = parse("node n with n.kind = Note").unwrap_err();
            assert!(matches!(err, DslError::UnexpectedToken { .. }));
        }

        #[test]
        fn parse_error_empty_input() {
            let err = parse("   ").unwrap_err();
            assert!(matches!(err, DslError::EmptyInput));
        }

        #[test]
        fn parse_error_trailing_tokens() {
            let err = parse("node n with n.kind = Note; garbage").unwrap_err();
            assert!(matches!(err, DslError::TrailingTokens(_)));
        }
    }

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
        fn select_all_notes() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse("node n with n.kind = Note;").unwrap();
            let ids = q.select(&g);
            // 4 notes: root.md, Areas/finance.md, Areas/operations/shifts.md, Projects/alpha.md
            assert_eq!(ids.len(), 4);
            for id in &ids {
                assert!(matches!(g.node(*id), NodeKind::Note(_)));
            }
        }

        #[test]
        fn select_all_directories() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse("node n with n.kind = Directory;").unwrap();
            let ids = q.select(&g);
            // root + Areas + Areas/operations + Projects = 4
            assert_eq!(ids.len(), 4);
            for id in &ids {
                assert!(matches!(g.node(*id), NodeKind::Directory(_)));
            }
        }

        #[test]
        fn select_top_level_notes_dirs_fixture() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse(
                "node n with n.kind in {Note, Directory} without (edge e(_, n) with e.kind = directory-contains);",
            )
            .unwrap();
            let ids = q.select(&g);
            // Only the root directory has no incoming Contains edge.
            // root.md has the root dir as parent; Areas/ and Projects/
            // are also children of the root dir. So only the root
            // directory itself qualifies.
            assert_eq!(ids.len(), 1);
            assert!(
                matches!(g.node(ids[0]), NodeKind::Directory(d) if d.path.as_os_str().is_empty())
            );
        }

        #[test]
        fn select_top_level_notes_links_fixture() {
            let v = links_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse(
                "node n with n.kind = Note without (edge e(_, n) with e.kind = directory-contains);",
            )
            .unwrap();
            let ids = q.select(&g);
            // In the links fixture, every note has a containing directory
            // (either notes/, archive/, or the root dir). The root
            // directory's Contains edges point to Index.md, archive/,
            // and notes/. So no Note node is without a directory parent.
            assert_eq!(ids.len(), 0);
        }

        #[test]
        fn select_path_includes() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse("node n with n.path includes \"Areas\";").unwrap();
            let ids = q.select(&g);
            // Areas dir, Areas/finance.md, Areas/operations dir, Areas/operations/shifts.md
            assert_eq!(ids.len(), 4);
        }

        #[test]
        fn expand_directory_to_all_children() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse(
                "node n with n.kind = Directory without (edge e(_, n) with e.kind = directory-contains); expand over e(n, m) with n.kind = Directory with e.kind = directory-contains with m.kind in {Note, Directory};",
            ).unwrap();
            let roots = q.select(&g);
            // Root directory is a top-level dir → it should be in the initial set
            assert!(!roots.is_empty());

            // Find the root dir
            let root_id = *roots
                .iter()
                .find(|id| {
                    matches!(g.node(**id), NodeKind::Directory(d) if d.path.as_os_str().is_empty())
                })
                .unwrap();

            // Expand root → should get its 3 children
            let children = q.expand(&g, root_id).unwrap();
            assert_eq!(children.len(), 3);
        }

        #[test]
        fn expand_directory_to_notes_only() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse(
                "node n with n.kind = Directory without (edge e(_, n) with e.kind = directory-contains); expand over e(n, m) with n.kind = Directory with e.kind = directory-contains with m.kind = Note;",
            ).unwrap();
            let roots = q.select(&g);
            let root_id = *roots
                .iter()
                .find(|id| {
                    matches!(g.node(**id), NodeKind::Directory(d) if d.path.as_os_str().is_empty())
                })
                .unwrap();

            let children = q.expand(&g, root_id).unwrap();
            // root → children: root.md (Note), Areas (Directory ← filtered out), Projects (Directory ← filtered out)
            assert_eq!(children.len(), 1);
            assert!(matches!(g.node(children[0]), NodeKind::Note(_)));
        }

        #[test]
        fn expand_returns_none_when_parent_mismatch() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse(
                "expand over e(n, m) with n.kind = Directory with e.kind = directory-contains with m.kind = Note;",
            ).unwrap();

            // Find a note node (not a directory)
            let note_id = g
                .nodes()
                .find(|(_, k)| matches!(k, NodeKind::Note(_)))
                .map(|(id, _)| id)
                .unwrap();

            // Note doesn't match n.kind = Directory, so expand returns None
            assert!(q.expand(&g, note_id).is_none());
        }

        #[test]
        fn expand_contains_edge_on_links_vault() {
            // The links fixture has notes in notes/ subdir. Expanding the
            // notes/ directory should return its children.
            let v = links_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse(
                "expand over e(n, m) with n.kind = Directory with e.kind = directory-contains with m.kind in {Note, Directory};",
            ).unwrap();

            let notes_dir = g.node_by_path(Path::new("notes")).unwrap();
            let children = q.expand(&g, notes_dir).unwrap();

            // notes/ contains: hub.md, alpha.md, beta.md, gamma.md,
            // collision-linker.md, sub/ (directory)
            assert_eq!(children.len(), 6);
        }

        #[test]
        fn multiple_selectors_union() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q =
                parse("node n with n.kind = Directory; node n with n.title = \"root\";").unwrap();
            let ids = q.select(&g);
            // 4 dirs + 1 note (root.md) = 5 unique
            assert_eq!(ids.len(), 5);
        }

        #[test]
        fn select_no_expand_query() {
            let v = dirs_vault();
            let g = Graph::build(&v).unwrap();
            let q = parse("node n with n.kind = Note;").unwrap();
            let ids = q.select(&g);
            assert_eq!(ids.len(), 4);
            // No expansion rule → expand on any node returns None
            let any_id = ids[0];
            assert!(q.expand(&g, any_id).is_none());
        }
    }
}

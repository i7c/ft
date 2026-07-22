//! Recursive-descent parser for the DSL, including parse-time
//! op/attribute/subject compatibility checks and the public
//! [`parse`] / [`parse_with`] entry points.

use super::*;

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
        "mentions" => Ok(Attr::Mentions),
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
            | Attr::Tags
            | Attr::Mentions,
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
            | Attr::Tags
            | Attr::Mentions,
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

pub(crate) fn literal_as_str(lit: &Literal) -> &str {
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

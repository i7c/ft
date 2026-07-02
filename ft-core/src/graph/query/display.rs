//! Canonical serializer: `Display` for [`GraphQuery`] and the
//! formatting helpers (round-trips through [`parse`]).

use super::*;

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

pub(crate) fn attr_name(a: Attr) -> &'static str {
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

//! Tokenizer for the graph query DSL: `Token`, `Spanned`, `Lexer`,
//! and the token-description helpers shared with parser errors.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token {
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
pub(crate) struct Spanned {
    pub(crate) tok: Token,
    pub(crate) pos: usize,
}

pub(crate) struct Lexer {
    chars: Vec<char>,
    pos: usize,
}

impl Lexer {
    pub(crate) fn new(src: &str) -> Self {
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

    pub(crate) fn tokenize(&mut self) -> Result<Vec<Spanned>, DslError> {
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

pub(crate) fn token_desc(t: &Token) -> &'static str {
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

pub(crate) fn token_label(t: &Token) -> String {
    match t {
        Token::Ident(s) => format!("`{s}`"),
        Token::Str(s) => format!("\"{s}\""),
        Token::Int(n) => format!("{n}"),
        Token::Eof => "end of input".to_string(),
        other => token_desc(other).to_string(),
    }
}

pub(crate) fn op_label(op: Op) -> &'static str {
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

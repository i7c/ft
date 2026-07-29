//! `@`-sigil interpolation for the unified graph/task query DSL.
//!
//! A pre-parse string→string pass that expands `@`-prefixed placeholders
//! into ordinary double-quoted DSL string literals, resolved against
//! `dates::today()` and the vault's `[periodic_notes.<period>]` config.
//! The predicate grammar, AST, and evaluator are unchanged — the parser
//! only ever sees normal `"…"` strings.
//!
//! ## Sigils
//!
//! | Sigil | Expands to | Needs periodic config? |
//! |---|---|---|
//! | `@today` | ISO date `YYYY-MM-DD` for today | no |
//! | `@daily` / `@weekly` / `@monthly` / `@quarterly` / `@yearly` | vault-relative path of the periodic note for today | the matching `[periodic_notes.<period>]` |
//!
//! Each sigil accepts an optional signed integer offset
//! (`@daily-1`, `@today+7`, `@weekly-2`); offset units are the period's
//! own units via [`Period::offset_date`].
//!
//! A `@` inside an existing DSL string literal (`"…"` / `'…'`) is left
//! untouched, so `title includes "me@you"` still works. A `@` outside a
//! string that isn't a recognized sigil is a hard [`InterpolationError`].
//!
//! [`Period::offset_date`]: crate::periodic::Period::offset_date

use std::path::Path;

use chrono::NaiveDate;
use thiserror::Error;

use crate::config::PeriodicNotes;
use crate::periodic::{self, Period};

/// Borrowed resolution context for [`interpolate`].
///
/// Kept small (no `&Vault`) so the interpolator is unit-testable without
/// constructing a full vault. Real call sites build this from
/// `vault.path`, `&vault.config.config.periodic_notes`, and
/// [`crate::dates::today`].
#[derive(Debug, Clone, Copy)]
pub struct SigilCtx<'a> {
    pub today: NaiveDate,
    pub vault_root: &'a Path,
    pub periodic: &'a PeriodicNotes,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InterpolationError {
    #[error(
        "unknown sigil `@{name}` at position {pos} (valid: today, daily, weekly, monthly, quarterly, yearly)"
    )]
    UnknownSigil { name: String, pos: usize },
    #[error(
        "sigil `@{period}` at position {pos} requires [periodic_notes.{period}] to be configured"
    )]
    MissingPeriodicConfig { period: &'static str, pos: usize },
    #[error("invalid offset `{raw}` in sigil at position {pos}")]
    InvalidOffset { raw: String, pos: usize },
}

/// Expand `@`-sigils in `src` into double-quoted DSL string literals.
///
/// Sigil-free input passes through byte-for-byte unchanged. See the
/// module docs for the sigil grammar.
pub fn interpolate(src: &str, ctx: SigilCtx<'_>) -> Result<String, InterpolationError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            // Copy a string span verbatim — sigils inside strings are
            // left untouched. Honor `\\` escapes the same way the DSL
            // lexer does so a closing quote inside an escape isn't
            // mistaken for the terminator.
            let quote = c;
            out.push(quote);
            i += 1;
            while i < chars.len() {
                let d = chars[i];
                if d == '\\' && i + 1 < chars.len() {
                    out.push(d);
                    out.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                out.push(d);
                i += 1;
                if d == quote {
                    break;
                }
            }
            continue;
        }
        if c == '@' {
            let pos = i; // char index of the `@`; byte position derived below
            let (expanded, consumed) = expand_sigil(&chars, i, ctx)?;
            out.push_str(&expanded);
            i = pos + consumed;
            continue;
        }
        out.push(c);
        i += 1;
    }
    Ok(out)
}

/// Parse and resolve one sigil starting at `chars[start]` (which is `@`).
/// Returns `(expanded_string, chars_consumed)`. The expanded string is a
/// double-quoted DSL string literal.
fn expand_sigil(
    chars: &[char],
    start: usize,
    ctx: SigilCtx<'_>,
) -> Result<(String, usize), InterpolationError> {
    // `@` + ASCII-alpha run.
    let mut cur = start + 1;
    let name_start = cur;
    while cur < chars.len() && chars[cur].is_ascii_alphabetic() {
        cur += 1;
    }
    let name: String = chars[name_start..cur].iter().collect();
    // Byte position of the `@` in the original source, for errors. The
    // scanner works in char indices; map back to bytes by counting the
    // UTF-8 length of the preceding chars.
    let byte_pos = byte_pos(chars, start);

    let (period, is_today) = match name.as_str() {
        "today" => (Period::Daily, true),
        "daily" => (Period::Daily, false),
        "weekly" => (Period::Weekly, false),
        "monthly" => (Period::Monthly, false),
        "quarterly" => (Period::Quarterly, false),
        "yearly" => (Period::Yearly, false),
        _ => {
            return Err(InterpolationError::UnknownSigil {
                name,
                pos: byte_pos,
            });
        }
    };

    // Optional `[+-]\d+` offset.
    let mut offset: Option<i32> = None;
    let mut offset_raw = String::new();
    if cur < chars.len() && (chars[cur] == '+' || chars[cur] == '-') {
        let sign = chars[cur];
        let digits_start = cur + 1;
        let mut d = digits_start;
        while d < chars.len() && chars[d].is_ascii_digit() {
            d += 1;
        }
        if d == digits_start {
            // Sign with no digits — invalid offset.
            offset_raw.push(sign);
            // Include any trailing alphanumerics to make the error message
            // useful (e.g. `@daily-x`).
            while d < chars.len() && (chars[d].is_ascii_alphanumeric() || chars[d] == '-') {
                offset_raw.push(chars[d]);
                d += 1;
            }
            return Err(InterpolationError::InvalidOffset {
                raw: offset_raw,
                pos: byte_pos,
            });
        }
        let digits: String = chars[digits_start..d].iter().collect();
        let raw_num: String = format!("{sign}{digits}");
        match raw_num.parse::<i32>() {
            Ok(n) => offset = Some(n),
            Err(_) => {
                return Err(InterpolationError::InvalidOffset {
                    raw: raw_num,
                    pos: byte_pos,
                });
            }
        }
        cur = d;
    }

    let date =
        match offset {
            Some(n) => period.offset_date(ctx.today, n).ok_or_else(|| {
                InterpolationError::InvalidOffset {
                    raw: n.to_string(),
                    pos: byte_pos,
                }
            })?,
            None => ctx.today,
        };

    let value = if is_today {
        date.format("%Y-%m-%d").to_string()
    } else {
        let cfg = period_config(period, ctx.periodic).ok_or_else(|| {
            InterpolationError::MissingPeriodicConfig {
                period: period.as_str(),
                pos: byte_pos,
            }
        })?;
        let abs = periodic::resolve_periodic_path(ctx.vault_root, cfg, date).map_err(|_| {
            InterpolationError::InvalidOffset {
                raw: date.format("%Y-%m-%d").to_string(),
                pos: byte_pos,
            }
        })?;
        let rel = abs.strip_prefix(ctx.vault_root).unwrap_or(&abs);
        rel.to_string_lossy().into_owned()
    };

    Ok((quote_dsl_string(&value), cur - start))
}

fn period_config(
    period: Period,
    periodic: &PeriodicNotes,
) -> Option<&crate::config::PeriodicPeriod> {
    match period {
        Period::Daily => periodic.daily.as_ref(),
        Period::Weekly => periodic.weekly.as_ref(),
        Period::Monthly => periodic.monthly.as_ref(),
        Period::Quarterly => periodic.quarterly.as_ref(),
        Period::Yearly => periodic.yearly.as_ref(),
    }
}

/// Render `s` as a double-quoted DSL string literal, escaping `\` and `"`
/// per the lexer's `read_string` rules.
fn quote_dsl_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Byte offset in `src` of the char at `char_idx` (the scanner indexes
/// chars; error positions are byte-based to match the DSL parser).
fn byte_pos(chars: &[char], char_idx: usize) -> usize {
    chars[..char_idx].iter().map(|c| c.len_utf8()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PeriodicNotes, PeriodicPeriod};
    use assert_fs::TempDir;

    fn ctx<'a>(
        today: NaiveDate,
        vault_root: &'a Path,
        periodic: &'a PeriodicNotes,
    ) -> SigilCtx<'a> {
        SigilCtx {
            today,
            vault_root,
            periodic,
        }
    }

    fn daily_cfg(path: &str, format: &str) -> PeriodicNotes {
        PeriodicNotes {
            daily: Some(PeriodicPeriod {
                path: path.into(),
                format: format.into(),
                template: None,
            }),
            ..Default::default()
        }
    }

    fn weekly_cfg(path: &str, format: &str) -> PeriodicNotes {
        PeriodicNotes {
            weekly: Some(PeriodicPeriod {
                path: path.into(),
                format: format.into(),
                template: None,
            }),
            ..Default::default()
        }
    }

    fn monthly_cfg(format: &str) -> PeriodicNotes {
        PeriodicNotes {
            monthly: Some(PeriodicPeriod {
                path: "journal/%Y".into(),
                format: format.into(),
                template: None,
            }),
            ..Default::default()
        }
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn no_sigils_pass_through() {
        let dir = TempDir::new().unwrap();
        let pn = PeriodicNotes::default();
        let s = "status = Open and due < today";
        assert_eq!(
            interpolate(s, ctx(d(2026, 7, 29), dir.path(), &pn)).unwrap(),
            s
        );
    }

    #[test]
    fn today_expands_to_iso_date() {
        let dir = TempDir::new().unwrap();
        let pn = PeriodicNotes::default();
        assert_eq!(
            interpolate("path includes @today", ctx(d(2026, 7, 29), dir.path(), &pn)).unwrap(),
            r#"path includes "2026-07-29""#
        );
    }

    #[test]
    fn today_honors_ctx_today() {
        let dir = TempDir::new().unwrap();
        let pn = PeriodicNotes::default();
        assert_eq!(
            interpolate("path includes @today", ctx(d(2025, 1, 2), dir.path(), &pn)).unwrap(),
            r#"path includes "2025-01-02""#
        );
    }

    #[test]
    fn daily_expands_to_vault_relative_path() {
        let dir = TempDir::new().unwrap();
        let pn = daily_cfg("journal/%Y", "%Y-%m-%d");
        assert_eq!(
            interpolate("path = @daily", ctx(d(2026, 7, 29), dir.path(), &pn)).unwrap(),
            r#"path = "journal/2026/2026-07-29.md""#
        );
    }

    #[test]
    fn weekly_expands_iso_week() {
        let dir = TempDir::new().unwrap();
        let pn = weekly_cfg("journal/%Y", "%G-W%V");
        assert_eq!(
            interpolate("path = @weekly", ctx(d(2026, 5, 14), dir.path(), &pn)).unwrap(),
            r#"path = "journal/2026/2026-W20.md""#
        );
    }

    #[test]
    fn daily_minus_one_is_yesterday() {
        let dir = TempDir::new().unwrap();
        let pn = daily_cfg("journal/%Y", "%Y-%m-%d");
        assert_eq!(
            interpolate("path = @daily-1", ctx(d(2026, 7, 29), dir.path(), &pn)).unwrap(),
            r#"path = "journal/2026/2026-07-28.md""#
        );
    }

    #[test]
    fn today_plus_seven() {
        let dir = TempDir::new().unwrap();
        let pn = PeriodicNotes::default();
        assert_eq!(
            interpolate(
                "path includes @today+7",
                ctx(d(2026, 7, 29), dir.path(), &pn)
            )
            .unwrap(),
            r#"path includes "2026-08-05""#
        );
    }

    #[test]
    fn weekly_minus_two() {
        let dir = TempDir::new().unwrap();
        let pn = weekly_cfg("journal/%Y", "%G-W%V");
        assert_eq!(
            interpolate("path = @weekly-2", ctx(d(2026, 5, 14), dir.path(), &pn)).unwrap(),
            r#"path = "journal/2026/2026-W18.md""#
        );
    }

    #[test]
    fn monthly_plus_one_clamps_jan31() {
        let dir = TempDir::new().unwrap();
        let pn = monthly_cfg("%Y-%m");
        assert_eq!(
            interpolate("path = @monthly+1", ctx(d(2026, 1, 31), dir.path(), &pn)).unwrap(),
            r#"path = "journal/2026/2026-02.md""#
        );
    }

    #[test]
    fn sigil_inside_string_is_untouched() {
        let dir = TempDir::new().unwrap();
        let pn = PeriodicNotes::default();
        assert_eq!(
            interpolate(
                r#"title includes "me@daily""#,
                ctx(d(2026, 7, 29), dir.path(), &pn)
            )
            .unwrap(),
            r#"title includes "me@daily""#
        );
    }

    #[test]
    fn sigil_inside_single_quoted_string_is_untouched() {
        let dir = TempDir::new().unwrap();
        let pn = daily_cfg("journal/%Y", "%Y-%m-%d");
        assert_eq!(
            interpolate(
                r#"title includes 'me@daily'"#,
                ctx(d(2026, 7, 29), dir.path(), &pn)
            )
            .unwrap(),
            r#"title includes 'me@daily'"#
        );
    }

    #[test]
    fn escaped_quote_inside_string_does_not_terminate_span() {
        let dir = TempDir::new().unwrap();
        let pn = PeriodicNotes::default();
        // The `@today` after the escaped quote is inside the string span
        // and must NOT be expanded.
        assert_eq!(
            interpolate(
                r#"title includes "a\"@today b""#,
                ctx(d(2026, 7, 29), dir.path(), &pn)
            )
            .unwrap(),
            r#"title includes "a\"@today b""#
        );
    }

    #[test]
    fn idempotent_on_expanded_output() {
        let dir = TempDir::new().unwrap();
        let pn = daily_cfg("journal/%Y", "%Y-%m-%d");
        let once = interpolate("path = @daily", ctx(d(2026, 7, 29), dir.path(), &pn)).unwrap();
        let twice = interpolate(&once, ctx(d(2026, 7, 29), dir.path(), &pn)).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn unknown_sigil_errors() {
        let dir = TempDir::new().unwrap();
        let pn = PeriodicNotes::default();
        let err =
            interpolate("path includes @datly", ctx(d(2026, 7, 29), dir.path(), &pn)).unwrap_err();
        assert!(
            matches!(err, InterpolationError::UnknownSigil { ref name, .. } if name == "datly")
        );
        assert!(err.to_string().contains("@datly"));
        assert!(err.to_string().contains("today"));
    }

    #[test]
    fn missing_periodic_config_errors() {
        let dir = TempDir::new().unwrap();
        let pn = PeriodicNotes::default();
        let err = interpolate("path = @daily", ctx(d(2026, 7, 29), dir.path(), &pn)).unwrap_err();
        assert!(matches!(
            err,
            InterpolationError::MissingPeriodicConfig {
                period: "daily",
                ..
            }
        ));
        assert!(err.to_string().contains("[periodic_notes.daily]"));
    }

    #[test]
    fn invalid_offset_no_digits_errors() {
        let dir = TempDir::new().unwrap();
        let pn = daily_cfg("journal/%Y", "%Y-%m-%d");
        let err = interpolate("path = @daily-x", ctx(d(2026, 7, 29), dir.path(), &pn)).unwrap_err();
        assert!(matches!(err, InterpolationError::InvalidOffset { .. }));
        assert!(err.to_string().contains("-x"));
    }

    #[test]
    fn bare_at_end_errors() {
        let dir = TempDir::new().unwrap();
        let pn = PeriodicNotes::default();
        let err = interpolate("path includes @", ctx(d(2026, 7, 29), dir.path(), &pn)).unwrap_err();
        // `@` with no alpha name → UnknownSigil with empty name.
        assert!(matches!(err, InterpolationError::UnknownSigil { .. }));
    }

    #[test]
    fn multiple_sigils_in_one_query() {
        let dir = TempDir::new().unwrap();
        let pn = daily_cfg("journal/%Y", "%Y-%m-%d");
        assert_eq!(
            interpolate(
                "path includes @today or path = @daily",
                ctx(d(2026, 7, 29), dir.path(), &pn)
            )
            .unwrap(),
            r#"path includes "2026-07-29" or path = "journal/2026/2026-07-29.md""#
        );
    }

    #[test]
    fn offset_zero_resolves_today() {
        let dir = TempDir::new().unwrap();
        let pn = PeriodicNotes::default();
        assert_eq!(
            interpolate(
                "path includes @today+0",
                ctx(d(2026, 7, 29), dir.path(), &pn)
            )
            .unwrap(),
            r#"path includes "2026-07-29""#
        );
    }

    #[test]
    fn negative_offset_today() {
        let dir = TempDir::new().unwrap();
        let pn = PeriodicNotes::default();
        assert_eq!(
            interpolate(
                "path includes @today-3",
                ctx(d(2026, 7, 29), dir.path(), &pn)
            )
            .unwrap(),
            r#"path includes "2026-07-26""#
        );
    }
}

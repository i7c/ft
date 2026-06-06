//! Date parsing for user-facing flags (`--due tomorrow`, `--due +3d`, etc).
//!
//! [`parse`] tries each form in order:
//! 1. ISO-8601 `YYYY-MM-DD`.
//! 2. Keywords `today` / `tomorrow` / `yesterday`.
//! 3. Relative offsets `+Nd`, `+Nw`, `-Nd`, `-Nw`.
//! 4. Natural language via `chrono-english` (e.g. `next monday`).
//!
//! All forms are anchored to the `today` argument so callers can inject a
//! deterministic date (matches the `FT_TODAY` override used elsewhere).

use chrono::NaiveDate;
use chrono_english::{parse_date_string, Dialect};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DateError {
    #[error(
        "could not parse `{input}` as a date (try YYYY-MM-DD, `tomorrow`, `+3d`, or `next monday`)"
    )]
    Unparseable { input: String },
}

/// Parse `s` against `today`. Returns the resolved date or an error.
pub fn parse(s: &str, today: NaiveDate) -> Result<NaiveDate, DateError> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(DateError::Unparseable { input: s.into() });
    }

    if let Ok(d) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        return Ok(d);
    }

    match trimmed.to_ascii_lowercase().as_str() {
        "today" => return Ok(today),
        "tomorrow" => {
            return today
                .succ_opt()
                .ok_or_else(|| DateError::Unparseable { input: s.into() })
        }
        "yesterday" => {
            return today
                .pred_opt()
                .ok_or_else(|| DateError::Unparseable { input: s.into() })
        }
        _ => {}
    }

    if let Some(d) = parse_relative(trimmed, today) {
        return Ok(d);
    }

    let datetime = today
        .and_hms_opt(12, 0, 0)
        .expect("12:00 is a valid time")
        .and_local_timezone(chrono::Local)
        .single()
        .ok_or_else(|| DateError::Unparseable { input: s.into() })?;
    parse_date_string(trimmed, datetime, Dialect::Us)
        .map(|dt| dt.date_naive())
        .map_err(|_| DateError::Unparseable { input: s.into() })
}

/// Parse `+Nd` / `+Nw` / `-Nd` / `-Nw` / `+Nm` / `-Nm`. Returns `None` if
/// the input doesn't match this shape so the caller can fall through to
/// other strategies. `m` adds calendar months (clamped on overflow days).
fn parse_relative(s: &str, today: NaiveDate) -> Option<NaiveDate> {
    let (sign, rest) = match s.chars().next()? {
        '+' => (1i64, &s[1..]),
        '-' => (-1i64, &s[1..]),
        _ => return None,
    };
    let (digits, unit) = rest.split_at(rest.find(|c: char| !c.is_ascii_digit())?);
    if digits.is_empty() {
        return None;
    }
    let n: i64 = digits.parse().ok()?;
    match unit {
        "d" | "day" | "days" => today.checked_add_signed(chrono::Duration::days(sign * n)),
        "w" | "week" | "weeks" => today.checked_add_signed(chrono::Duration::days(sign * n * 7)),
        "m" | "month" | "months" => add_months(today, (sign * n) as i32),
        _ => None,
    }
}

fn add_months(d: NaiveDate, months: i32) -> Option<NaiveDate> {
    use chrono::Datelike;
    let total = d.year() * 12 + (d.month() as i32 - 1) + months;
    let year = total.div_euclid(12);
    let month = (total.rem_euclid(12) + 1) as u32;
    let day = d.day();
    // Clamp day to the last day of the target month.
    for try_day in (1..=day).rev() {
        if let Some(out) = NaiveDate::from_ymd_opt(year, month, try_day) {
            return Some(out);
        }
    }
    None
}

/// Strict date-value parser for the graph DSL. Accepts only:
/// `YYYY-MM-DD`, `today`, `tomorrow`, `yesterday`, and the relative
/// offsets `+Nd`/`-Nd`/`+Nw`/`-Nw`/`+Nm`/`-Nm`. Unlike [`parse`], this
/// does NOT fall through to natural-language parsing — the DSL grammar
/// is closed.
pub fn parse_date_value(s: &str, today: NaiveDate) -> Option<NaiveDate> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(d) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        return Some(d);
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "today" => return Some(today),
        "tomorrow" => return today.succ_opt(),
        "yesterday" => return today.pred_opt(),
        _ => {}
    }
    parse_relative(trimmed, today)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn today() -> NaiveDate {
        d(2026, 5, 9)
    }

    #[test]
    fn iso() {
        assert_eq!(parse("2026-05-10", today()).unwrap(), d(2026, 5, 10));
    }

    #[test]
    fn keyword_today() {
        assert_eq!(parse("today", today()).unwrap(), today());
    }

    #[test]
    fn keyword_tomorrow() {
        assert_eq!(parse("tomorrow", today()).unwrap(), d(2026, 5, 10));
    }

    #[test]
    fn keyword_yesterday() {
        assert_eq!(parse("yesterday", today()).unwrap(), d(2026, 5, 8));
    }

    #[test]
    fn keyword_case_insensitive() {
        assert_eq!(parse("TOMORROW", today()).unwrap(), d(2026, 5, 10));
    }

    #[test]
    fn relative_plus_days() {
        assert_eq!(parse("+3d", today()).unwrap(), d(2026, 5, 12));
    }

    #[test]
    fn relative_minus_days() {
        assert_eq!(parse("-3d", today()).unwrap(), d(2026, 5, 6));
    }

    #[test]
    fn relative_plus_weeks() {
        assert_eq!(parse("+2w", today()).unwrap(), d(2026, 5, 23));
    }

    #[test]
    fn relative_long_unit() {
        assert_eq!(parse("+10days", today()).unwrap(), d(2026, 5, 19));
    }

    #[test]
    fn natural_language_next_monday() {
        use chrono::Datelike;
        // 2026-05-09 is a Saturday; next Monday is 2026-05-11.
        let parsed = parse("next monday", today()).unwrap();
        assert_eq!(parsed.weekday(), chrono::Weekday::Mon);
        assert!(parsed > today());
    }

    #[test]
    fn empty_rejected() {
        assert!(parse("", today()).is_err());
    }

    #[test]
    fn nonsense_rejected() {
        assert!(parse("zzzzz", today()).is_err());
    }

    #[test]
    fn whitespace_trimmed() {
        assert_eq!(parse("  2026-05-10  ", today()).unwrap(), d(2026, 5, 10));
    }

    #[test]
    fn date_value_iso() {
        assert_eq!(
            parse_date_value("2026-12-31", today()).unwrap(),
            d(2026, 12, 31)
        );
    }

    #[test]
    fn date_value_today() {
        assert_eq!(parse_date_value("today", today()).unwrap(), today());
    }

    #[test]
    fn date_value_tomorrow() {
        assert_eq!(
            parse_date_value("tomorrow", today()).unwrap(),
            d(2026, 5, 10)
        );
    }

    #[test]
    fn date_value_yesterday() {
        assert_eq!(
            parse_date_value("yesterday", today()).unwrap(),
            d(2026, 5, 8)
        );
    }

    #[test]
    fn date_value_plus_days() {
        assert_eq!(parse_date_value("+7d", today()).unwrap(), d(2026, 5, 16));
    }

    #[test]
    fn date_value_minus_weeks() {
        assert_eq!(parse_date_value("-2w", today()).unwrap(), d(2026, 4, 25));
    }

    #[test]
    fn date_value_plus_months() {
        assert_eq!(parse_date_value("+1m", today()).unwrap(), d(2026, 6, 9));
    }

    #[test]
    fn date_value_rejects_natural_language() {
        // Strict parser — `next monday` is NOT accepted (unlike `parse`).
        assert!(parse_date_value("next monday", today()).is_none());
    }

    #[test]
    fn date_value_rejects_unknown() {
        assert!(parse_date_value("zzzzz", today()).is_none());
    }

    #[test]
    fn date_value_empty_is_none() {
        assert!(parse_date_value("", today()).is_none());
    }
}

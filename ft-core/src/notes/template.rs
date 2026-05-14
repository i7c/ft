//! MiniJinja-based template renderer for note creation.
//!
//! Templates use Jinja syntax with strict undefined mode and autoescape
//! disabled — we render Markdown, not HTML. The variable surface is
//! intentionally small:
//!
//! - `title` — the new file's basename without `.md`.
//! - `today` — a [`NaiveDate`] for the current day (or `FT_TODAY` override).
//! - `now` — a [`NaiveDateTime`] for the current instant (or `FT_TODAY`
//!   at `00:00:00` when overridden, for deterministic snapshots).
//! - `vars` — a map of caller-supplied custom variables.
//!
//! ## Filters
//!
//! All date filters preserve the typed [`NaiveDate`] / [`NaiveDateTime`]
//! through chains, so you can compose them: `parse_date → weekday_of →
//! date` runs without re-serialising through strings.
//!
//! - `date(format="...")` — format a date/datetime with a chrono
//!   `strftime` format. Defaults to `"%Y-%m-%d"`.
//! - `parse_date(format="...")` — parse a string into a date. Defaults
//!   to `"%Y-%m-%d"`.
//! - `add_days(n)` / `add_weeks(n)` / `add_months(n)` — date arithmetic.
//! - `weekday_of(n)` — given a date, return the date of weekday `n`
//!   (1=Mon..7=Sun, ISO) in the same ISO week.
//! - `quarter` — return the quarter number (1..=4) for a date.
//!
//! ## Strict undefined
//!
//! Unknown variables — including typos like `{{ titel }}` and missing
//! `vars.*` entries — raise render errors instead of emitting empty
//! strings. The cost is paid willingly: silent blanks in generated
//! Markdown are worse than a clear error at create-time.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use chrono::format::{parse as parse_strftime, Parsed, StrftimeItems};
use chrono::{Datelike, Duration, Months, NaiveDate, NaiveDateTime, Weekday};
use minijinja::value::{Kwargs, Object, ObjectRepr, Value};
use minijinja::{Environment, Error as MjError, ErrorKind as MjErrorKind, UndefinedBehavior};

use crate::error::{Error, Result};

/// Inputs to a template render.
///
/// Construct with [`TemplateContext::new`] for the common case and then
/// populate [`Self::vars`] for custom prompts.
#[derive(Debug, Clone)]
pub struct TemplateContext {
    /// Title of the note being created (basename without `.md`).
    pub title: String,
    /// Today's date (typically local today, or `FT_TODAY` override).
    pub today: NaiveDate,
    /// Current instant (typically local now, or `FT_TODAY` at midnight).
    pub now: NaiveDateTime,
    /// Caller-supplied custom variables, surfaced in templates as `vars.KEY`.
    pub vars: BTreeMap<String, String>,
}

impl TemplateContext {
    /// Build a context with no custom `vars`.
    pub fn new(title: impl Into<String>, today: NaiveDate, now: NaiveDateTime) -> Self {
        Self {
            title: title.into(),
            today,
            now,
            vars: BTreeMap::new(),
        }
    }
}

/// Render `template_source` against `ctx`.
///
/// Errors carry the engine's diagnostic — including line numbers when
/// MiniJinja provides them — wrapped in [`Error::Notes`].
pub fn render(template_source: &str, ctx: &TemplateContext) -> Result<String> {
    let env = build_env();
    let value = ctx_to_value(ctx);
    env.render_str(template_source, value)
        .map_err(|e| Error::Notes(format!("template render: {e:#}")))
}

/// Read a template from disk and render it against `ctx`.
pub fn render_path(path: &Path, ctx: &TemplateContext) -> Result<String> {
    let source = std::fs::read_to_string(path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    render(&source, ctx)
}

fn build_env() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_keep_trailing_newline(true);
    env.add_filter("date", date_filter);
    env.add_filter("parse_date", parse_date_filter);
    env.add_filter("add_days", add_days_filter);
    env.add_filter("add_weeks", add_weeks_filter);
    env.add_filter("add_months", add_months_filter);
    env.add_filter("weekday_of", weekday_of_filter);
    env.add_filter("quarter", quarter_filter);
    env
}

fn ctx_to_value(ctx: &TemplateContext) -> Value {
    minijinja::context! {
        title => ctx.title.clone(),
        today => Value::from_object(DateValue(ctx.today)),
        now => Value::from_object(DateTimeValue(ctx.now)),
        vars => Value::from_serialize(&ctx.vars),
    }
}

// ----- Typed date wrappers ---------------------------------------------------

#[derive(Debug)]
struct DateValue(NaiveDate);

impl Object for DateValue {
    fn repr(self: &Arc<Self>) -> ObjectRepr {
        ObjectRepr::Plain
    }
    fn render(self: &Arc<Self>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.format("%Y-%m-%d"))
    }
}

#[derive(Debug)]
struct DateTimeValue(NaiveDateTime);

impl Object for DateTimeValue {
    fn repr(self: &Arc<Self>) -> ObjectRepr {
        ObjectRepr::Plain
    }
    fn render(self: &Arc<Self>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.format("%Y-%m-%d %H:%M:%S"))
    }
}

// ----- Filters ---------------------------------------------------------------

fn date_filter(value: Value, kwargs: Kwargs) -> std::result::Result<String, MjError> {
    let format = kwargs
        .get::<Option<String>>("format")?
        .unwrap_or_else(|| "%Y-%m-%d".to_string());
    kwargs.assert_all_used()?;
    format_date(&value, &format)
}

fn parse_date_filter(value: Value, kwargs: Kwargs) -> std::result::Result<Value, MjError> {
    let format = kwargs
        .get::<Option<String>>("format")?
        .unwrap_or_else(|| "%Y-%m-%d".to_string());
    kwargs.assert_all_used()?;

    let s = value.as_str().ok_or_else(|| {
        MjError::new(
            MjErrorKind::InvalidOperation,
            "parse_date: expected a string value",
        )
    })?;

    let mut parsed = Parsed::new();
    parse_strftime(&mut parsed, s, StrftimeItems::new(&format)).map_err(|e| {
        MjError::new(
            MjErrorKind::InvalidOperation,
            format!("parse_date: could not parse {s:?} with format {format:?}: {e}"),
        )
    })?;

    // Try the regular path first — works for `%Y-%m-%d`, `%Y%m%d`, etc.
    if let Ok(date) = parsed.to_naive_date() {
        return Ok(Value::from_object(DateValue(date)));
    }

    // Fall back to ISO-year + ISO-week → Monday of that week. This is
    // how `weeks.md` titles like "2026 Week 19" land on a concrete date.
    if let (Some(iyear), Some(iweek)) = (parsed.isoyear, parsed.isoweek) {
        if let Some(d) = NaiveDate::from_isoywd_opt(iyear, iweek, Weekday::Mon) {
            return Ok(Value::from_object(DateValue(d)));
        }
    }

    // Fall back to year + month → 1st of the month.
    if let (Some(year), Some(month)) = (parsed.year, parsed.month) {
        if let Some(d) = NaiveDate::from_ymd_opt(year, month, 1) {
            return Ok(Value::from_object(DateValue(d)));
        }
    }

    Err(MjError::new(
        MjErrorKind::InvalidOperation,
        format!("parse_date: could not derive a complete date from {s:?} with format {format:?}"),
    ))
}

fn add_days_filter(value: Value, n: i64) -> std::result::Result<Value, MjError> {
    let date = extract_date(&value)?;
    let new = date.checked_add_signed(Duration::days(n)).ok_or_else(|| {
        MjError::new(
            MjErrorKind::InvalidOperation,
            format!("add_days: {n} days from {date} overflows"),
        )
    })?;
    Ok(Value::from_object(DateValue(new)))
}

fn add_weeks_filter(value: Value, n: i64) -> std::result::Result<Value, MjError> {
    let date = extract_date(&value)?;
    let new = date.checked_add_signed(Duration::weeks(n)).ok_or_else(|| {
        MjError::new(
            MjErrorKind::InvalidOperation,
            format!("add_weeks: {n} weeks from {date} overflows"),
        )
    })?;
    Ok(Value::from_object(DateValue(new)))
}

fn add_months_filter(value: Value, n: i32) -> std::result::Result<Value, MjError> {
    let date = extract_date(&value)?;
    let new = if n >= 0 {
        date.checked_add_months(Months::new(n as u32))
    } else {
        date.checked_sub_months(Months::new((-n) as u32))
    }
    .ok_or_else(|| {
        MjError::new(
            MjErrorKind::InvalidOperation,
            format!("add_months: {n} months from {date} overflows"),
        )
    })?;
    Ok(Value::from_object(DateValue(new)))
}

fn quarter_filter(value: Value) -> std::result::Result<u32, MjError> {
    let date = extract_date(&value)?;
    Ok((date.month() - 1) / 3 + 1)
}

fn weekday_of_filter(value: Value, weekday: u32) -> std::result::Result<Value, MjError> {
    if !(1..=7).contains(&weekday) {
        return Err(MjError::new(
            MjErrorKind::InvalidOperation,
            format!("weekday_of: weekday must be 1..=7 (Mon..Sun, ISO), got {weekday}"),
        ));
    }
    let date = extract_date(&value)?;
    let dow_from_mon = i64::from(date.weekday().num_days_from_monday());
    let monday = date - Duration::days(dow_from_mon);
    let result = monday + Duration::days(i64::from(weekday - 1));
    Ok(Value::from_object(DateValue(result)))
}

fn format_date(value: &Value, format: &str) -> std::result::Result<String, MjError> {
    if let Some(d) = value.downcast_object_ref::<DateValue>() {
        Ok(d.0.format(format).to_string())
    } else if let Some(dt) = value.downcast_object_ref::<DateTimeValue>() {
        Ok(dt.0.format(format).to_string())
    } else {
        Err(MjError::new(
            MjErrorKind::InvalidOperation,
            "date: expected a date or datetime value",
        ))
    }
}

fn extract_date(value: &Value) -> std::result::Result<NaiveDate, MjError> {
    if let Some(d) = value.downcast_object_ref::<DateValue>() {
        Ok(d.0)
    } else if let Some(dt) = value.downcast_object_ref::<DateTimeValue>() {
        Ok(dt.0.date())
    } else {
        Err(MjError::new(
            MjErrorKind::InvalidOperation,
            "expected a date or datetime value",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn dt(y: i32, m: u32, day: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, day)
            .unwrap()
            .and_time(NaiveTime::from_hms_opt(h, mi, 0).unwrap())
    }

    fn ctx() -> TemplateContext {
        let mut c = TemplateContext::new("My Note", d(2026, 5, 13), dt(2026, 5, 13, 14, 30));
        c.vars.insert("name".into(), "quick".into());
        c
    }

    #[test]
    fn renders_plain_title() {
        let out = render("# {{ title }}\n", &ctx()).unwrap();
        assert_eq!(out, "# My Note\n");
    }

    #[test]
    fn renders_today_default_format() {
        let out = render("{{ today }}", &ctx()).unwrap();
        assert_eq!(out, "2026-05-13");
    }

    #[test]
    fn renders_today_with_explicit_format() {
        let out = render(r#"{{ today | date(format="%Y/%m/%d") }}"#, &ctx()).unwrap();
        assert_eq!(out, "2026/05/13");
    }

    #[test]
    fn renders_now_with_time_format() {
        let out = render(r#"{{ now | date(format="%H%M") }}"#, &ctx()).unwrap();
        assert_eq!(out, "1430");
    }

    #[test]
    fn renders_vars_entry() {
        let out = render("{{ vars.name }}", &ctx()).unwrap();
        assert_eq!(out, "quick");
    }

    #[test]
    fn strict_undefined_errors_on_typo() {
        let err = render("{{ titel }}", &ctx()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("titel") || msg.contains("undefined"), "{msg}");
    }

    #[test]
    fn strict_undefined_errors_on_missing_var() {
        let err = render("{{ vars.missing }}", &ctx()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("missing") || msg.contains("undefined"),
            "{msg}"
        );
    }

    #[test]
    fn date_filter_default_format_on_now() {
        let out = render("{{ now | date }}", &ctx()).unwrap();
        assert_eq!(out, "2026-05-13");
    }

    #[test]
    fn date_filter_rejects_non_date() {
        let err = render(r#"{{ title | date(format="%Y") }}"#, &ctx()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("date"), "{msg}");
    }

    #[test]
    fn parse_date_round_trip() {
        let out = render(
            r#"{{ "2026-05-13" | parse_date | date(format="%Y/%m/%d") }}"#,
            &ctx(),
        )
        .unwrap();
        assert_eq!(out, "2026/05/13");
    }

    #[test]
    fn parse_date_with_custom_format() {
        let out = render(
            r#"{{ "2026 Week 19" | parse_date(format="%G Week %V") | date(format="%Y-%m-%d") }}"#,
            &ctx(),
        )
        .unwrap();
        // ISO week 19 of 2026 starts Monday 2026-05-04. The parse_date
        // ISO-week fallback fills in Monday.
        assert_eq!(out, "2026-05-04");
    }

    #[test]
    fn parse_date_error_path() {
        let err = render(
            r#"{{ "not-a-date" | parse_date(format="%Y-%m-%d") }}"#,
            &ctx(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("parse_date"), "{msg}");
    }

    #[test]
    fn add_days_positive() {
        let out = render("{{ (today | add_days(7)) | date }}", &ctx()).unwrap();
        assert_eq!(out, "2026-05-20");
    }

    #[test]
    fn add_days_negative() {
        let out = render("{{ (today | add_days(-1)) | date }}", &ctx()).unwrap();
        assert_eq!(out, "2026-05-12");
    }

    #[test]
    fn add_days_error_on_non_date() {
        let err = render("{{ title | add_days(1) }}", &ctx()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("date") || msg.contains("expected"), "{msg}");
    }

    #[test]
    fn add_weeks_basic() {
        let out = render("{{ (today | add_weeks(2)) | date }}", &ctx()).unwrap();
        assert_eq!(out, "2026-05-27");
    }

    #[test]
    fn add_weeks_error_on_non_date() {
        let err = render("{{ title | add_weeks(1) }}", &ctx()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("date") || msg.contains("expected"), "{msg}");
    }

    #[test]
    fn add_months_positive() {
        let out = render("{{ (today | add_months(3)) | date }}", &ctx()).unwrap();
        assert_eq!(out, "2026-08-13");
    }

    #[test]
    fn add_months_negative_wraps_year() {
        let out = render("{{ (today | add_months(-6)) | date }}", &ctx()).unwrap();
        assert_eq!(out, "2025-11-13");
    }

    #[test]
    fn add_months_error_on_non_date() {
        let err = render("{{ title | add_months(1) }}", &ctx()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("date") || msg.contains("expected"), "{msg}");
    }

    #[test]
    fn weekday_of_monday() {
        // 2026-05-13 is a Wednesday (ISO week 20). Monday of that week
        // is 2026-05-11.
        let out = render("{{ (today | weekday_of(1)) | date }}", &ctx()).unwrap();
        assert_eq!(out, "2026-05-11");
    }

    #[test]
    fn weekday_of_sunday() {
        // Sunday (7) of the same ISO week is 2026-05-17.
        let out = render("{{ (today | weekday_of(7)) | date }}", &ctx()).unwrap();
        assert_eq!(out, "2026-05-17");
    }

    #[test]
    fn quarter_basic() {
        // 2026-05-13 is in Q2.
        let out = render("{{ today | quarter }}", &ctx()).unwrap();
        assert_eq!(out, "2");
    }

    #[test]
    fn quarter_boundaries() {
        let mut c = ctx();
        c.today = d(2026, 1, 1);
        assert_eq!(render("{{ today | quarter }}", &c).unwrap(), "1");
        c.today = d(2026, 3, 31);
        assert_eq!(render("{{ today | quarter }}", &c).unwrap(), "1");
        c.today = d(2026, 4, 1);
        assert_eq!(render("{{ today | quarter }}", &c).unwrap(), "2");
        c.today = d(2026, 12, 31);
        assert_eq!(render("{{ today | quarter }}", &c).unwrap(), "4");
    }

    #[test]
    fn weekday_of_out_of_range_errors() {
        let err = render("{{ (today | weekday_of(8)) | date }}", &ctx()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("weekday_of"), "{msg}");
    }

    #[test]
    fn parse_then_weekday_then_date_full_chain() {
        // Mimics the weeks.md port: parse a "YYYY Week WW" title, take
        // the Monday of that week, format as YYYY-MM-DD.
        let mut c = ctx();
        c.title = "2026 Week 19".into();
        let out = render(
            r#"{{ title | parse_date(format="%G Week %V") | weekday_of(1) | date(format="%Y-%m-%d") }}"#,
            &c,
        )
        .unwrap();
        // ISO 2026-W19 starts Monday 2026-05-04.
        assert_eq!(out, "2026-05-04");
    }

    #[test]
    fn render_path_reads_template_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.md");
        std::fs::write(&path, "# {{ title }}\n").unwrap();
        let out = render_path(&path, &ctx()).unwrap();
        assert_eq!(out, "# My Note\n");
    }

    #[test]
    fn unused_kwargs_error() {
        // `assert_all_used` should reject unknown kwargs so typos like
        // `formaat=` don't silently fall back to the default format.
        let err = render(r#"{{ today | date(formaat="%Y") }}"#, &ctx()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("formaat") || msg.contains("kwarg"), "{msg}");
    }
}

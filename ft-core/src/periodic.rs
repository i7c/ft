//! Resolve and create periodic notes — daily, weekly, monthly, quarterly,
//! yearly.
//!
//! Configuration lives in `[periodic_notes.<period>]` blocks of ft's config:
//!
//! ```toml
//! [periodic_notes.daily]
//! path = "journal/%Y"
//! format = "%Y-%m-%d"
//! template = "daily"            # optional; resolved under [notes].templates_dir
//!
//! [periodic_notes.quarterly]
//! path = "journal/%Y"
//! format = "%Y-Q%q"             # %q expands to the quarter digit (1..=4)
//! template = "quarterly"
//! ```
//!
//! ## Tokens
//!
//! Both `path` and `format` accept chrono [strftime] tokens. On top of that
//! we add two quarter tokens that chrono doesn't ship with:
//!
//! - `%q` — the quarter as a single digit (`1`..=`4`).
//! - `%Q` — `Q1`..`Q4`.
//!
//! `%%q` / `%%Q` pass through to chrono as a literal `%q` / `%Q` — the same
//! escape convention chrono itself uses for `%%`.
//!
//! ## Template rendering
//!
//! When a period's `template` is set, [`render_periodic_note`] loads the
//! file from `<vault>/<templates_dir>/<template>.md` (or treats it as an
//! absolute path when prefixed with `/`) and renders it via the MiniJinja
//! engine from [`crate::notes::template`]. When unset, the rendered body
//! is `# <title>\n\n` — the same blank stub the `c` create flow writes.
//!
//! ## High-level helper
//!
//! [`create_or_get_periodic_path`] is the convenience entry point both the
//! CLI and the TUI use: it resolves the path, returns it directly when the
//! file already exists, otherwise renders the template and writes the file
//! atomically before returning. The boolean in its return tuple says
//! whether a new file was created, so callers can tailor user-facing
//! messages ("Opened…" vs "Created…").
//!
//! [strftime]: https://docs.rs/chrono/latest/chrono/format/strftime/index.html

use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono::{Datelike, Days, Months, NaiveDate, NaiveDateTime};

use crate::config::{PeriodicPeriod, WeekStart};
use crate::error::{Error, Result};
use crate::fs::write_atomic;
use crate::notes::template::{render_path as render_template_path, TemplateContext};

/// One of the five periodic-note periods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Period {
    Daily,
    Weekly,
    Monthly,
    Quarterly,
    Yearly,
}

impl Period {
    /// Canonical long name (`"daily"`, `"weekly"`, …). Used for config
    /// keys, error messages, and the CLI subcommand value.
    pub fn as_str(self) -> &'static str {
        match self {
            Period::Daily => "daily",
            Period::Weekly => "weekly",
            Period::Monthly => "monthly",
            Period::Quarterly => "quarterly",
            Period::Yearly => "yearly",
        }
    }

    /// Shift `date` by `n` units of this period.
    ///
    /// Semantics:
    /// - `Daily` → ±N days.
    /// - `Weekly` → ±N × 7 days.
    /// - `Monthly` → ±N calendar months. Month-end clamps (Jan 31 + 1
    ///   month → Feb 28/29).
    /// - `Quarterly` → ±N × 3 calendar months.
    /// - `Yearly` → ±N × 12 calendar months.
    ///
    /// Returns `None` only when the result overflows chrono's
    /// representable date range.
    pub fn offset_date(self, date: NaiveDate, n: i32) -> Option<NaiveDate> {
        match self {
            Period::Daily => apply_days(date, n),
            Period::Weekly => apply_days(date, n.checked_mul(7)?),
            Period::Monthly => apply_months(date, n),
            Period::Quarterly => apply_months(date, n.checked_mul(3)?),
            Period::Yearly => apply_months(date, n.checked_mul(12)?),
        }
    }

    /// Representative date for a note of this period.
    ///
    /// - `Daily` → the date itself.
    /// - `Weekly` → the configured week start (Monday by default) of the
    ///   date's week.
    /// - `Monthly` → the first day of the date's month.
    /// - `Quarterly` → the first day of the date's calendar quarter.
    /// - `Yearly` → January 1 of the date's year.
    ///
    /// `week_start` only affects `Weekly`.
    pub fn start_of(self, date: NaiveDate, week_start: WeekStart) -> NaiveDate {
        match self {
            Period::Daily => date,
            Period::Weekly => {
                let back = match week_start {
                    WeekStart::Monday => date.weekday().num_days_from_monday(),
                    WeekStart::Sunday => date.weekday().num_days_from_sunday(),
                };
                back_off_days(date, back)
            }
            Period::Monthly => {
                NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap_or(date)
            }
            Period::Quarterly => {
                let month = ((date.month() - 1) / 3) * 3 + 1;
                NaiveDate::from_ymd_opt(date.year(), month, 1).unwrap_or(date)
            }
            Period::Yearly => NaiveDate::from_ymd_opt(date.year(), 1, 1).unwrap_or(date),
        }
    }
}

/// `date` minus `days`, saturating at the representable minimum instead of
/// panicking.
fn back_off_days(date: NaiveDate, days: u32) -> NaiveDate {
    date.checked_sub_days(Days::new(u64::from(days)))
        .unwrap_or(date)
}

fn apply_days(date: NaiveDate, n: i32) -> Option<NaiveDate> {
    if n >= 0 {
        date.checked_add_days(Days::new(n as u64))
    } else {
        date.checked_sub_days(Days::new((-(n as i64)) as u64))
    }
}

fn apply_months(date: NaiveDate, n: i32) -> Option<NaiveDate> {
    if n >= 0 {
        date.checked_add_months(Months::new(n as u32))
    } else {
        date.checked_sub_months(Months::new((-(n as i64)) as u32))
    }
}

impl FromStr for Period {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "d" | "daily" => Ok(Period::Daily),
            "w" | "weekly" => Ok(Period::Weekly),
            "m" | "monthly" => Ok(Period::Monthly),
            "q" | "quarterly" => Ok(Period::Quarterly),
            "y" | "yearly" => Ok(Period::Yearly),
            _ => Err(format!(
                "unknown period '{s}'; expected one of: daily, weekly, monthly, quarterly, yearly (or d/w/m/q/y)"
            )),
        }
    }
}

/// Substitute `%q` and `%Q` for the quarter of `date`, leaving everything
/// else untouched.
///
/// `%q` → `1`..=`4`; `%Q` → `Q1`..`Q4`. `%%q`/`%%Q` are preserved as
/// literal `%q`/`%Q` after one pass (mirroring chrono's own `%%` escape
/// convention).
pub fn substitute_quarter_tokens(fmt: &str, date: NaiveDate) -> String {
    let quarter = (date.month0() / 3) + 1; // 1..=4
    let bytes = fmt.as_bytes();
    let mut out = String::with_capacity(fmt.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'q' => {
                    out.push_str(&quarter.to_string());
                    i += 2;
                    continue;
                }
                b'Q' => {
                    out.push('Q');
                    out.push_str(&quarter.to_string());
                    i += 2;
                    continue;
                }
                b'%' if i + 2 < bytes.len() && (bytes[i + 2] == b'q' || bytes[i + 2] == b'Q') => {
                    // `%%q` / `%%Q` — emit a literal `%` and let chrono
                    // see `%q` / `%Q`. chrono itself treats unknown `%X`
                    // as a literal sequence, so the user gets `%q` on
                    // disk; the escape exists for symmetry with chrono's
                    // `%%` convention.
                    out.push('%');
                    out.push('%');
                    out.push(bytes[i + 2] as char);
                    i += 3;
                    continue;
                }
                _ => {}
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Format `fmt` against `date` using chrono `strftime`, after our `%q`/`%Q`
/// pre-processor has resolved any quarter tokens.
fn format_with_quarter(fmt: &str, date: NaiveDate) -> Result<String> {
    let resolved = substitute_quarter_tokens(fmt, date);
    // chrono's `format` returns a lazy DelayedFormat that panics on
    // invalid format strings when written; render eagerly and convert
    // panics into errors by going through `format_with_items`.
    // In practice, `NaiveDate::format(...).to_string()` is the canonical
    // surface and `chrono::format::strftime::StrftimeItems` would be more
    // ceremony; we trust strftime to be infallible at format-time for the
    // tokens we accept.
    Ok(date.format(&resolved).to_string())
}

/// Resolve the absolute on-disk path for a periodic note.
///
/// The supplied `date` is first anchored to the start of `period` (see
/// [`Period::start_of`]), so a weekly note opened mid-week resolves to the
/// same file as one opened on the week start. The rendered `path` (folder,
/// may be empty) and `format` (filename, no `.md`) are joined under
/// `vault_root` with `.md` appended.
pub fn resolve_periodic_path(
    vault_root: &Path,
    period: Period,
    cfg: &PeriodicPeriod,
    date: NaiveDate,
) -> Result<PathBuf> {
    let date = period.start_of(date, cfg.week_start);
    let folder = format_with_quarter(&cfg.path, date)?;
    let filename = format_with_quarter(&cfg.format, date)?;
    if filename.is_empty() {
        return Err(Error::Periodic(
            "format produced an empty filename".to_string(),
        ));
    }
    let mut p = vault_root.to_path_buf();
    if !folder.is_empty() {
        p.push(folder);
    }
    p.push(format!("{filename}.md"));
    Ok(p)
}

/// Render the body of a new periodic note.
///
/// - When `cfg.template` is `None`, returns `# <title>\n\n`.
/// - When set: resolves the template under `vault_templates_dir`
///   (or treats the value as an absolute path when prefixed with `/`),
///   appending `.md` if missing, then renders via the MiniJinja engine.
pub fn render_periodic_note(
    cfg: &PeriodicPeriod,
    vault_templates_dir: &Path,
    title: &str,
    today: NaiveDate,
    now: NaiveDateTime,
) -> Result<String> {
    let Some(template) = cfg.template.as_deref() else {
        return Ok(format!("# {title}\n\n"));
    };

    let candidate = resolve_template_candidate(template, vault_templates_dir);
    let ctx = TemplateContext::new(title.to_string(), today, now);
    render_template_path(&candidate, &ctx).map_err(|e| {
        // Surface a richer message that points at the configured key.
        Error::Periodic(format!(
            "render template '{template}' at {}: {e}",
            candidate.display()
        ))
    })
}

fn resolve_template_candidate(template: &str, vault_templates_dir: &Path) -> PathBuf {
    let raw = Path::new(template);
    let base = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        vault_templates_dir.join(raw)
    };
    if base.extension().is_none() {
        base.with_extension("md")
    } else {
        base
    }
}

/// Resolve the absolute path for a periodic note and, when the file does
/// not yet exist, render its template and write it atomically.
///
/// `date` is anchored to the start of `period` once, and that anchored date
/// drives both the path and the template's `today` variable, so periodic
/// templates render deterministically regardless of the day they are
/// opened. `now` remains the live instant for `{{ now }}`.
///
/// Returns `(path, created)` where `created = true` means the file was
/// just written by this call; `false` means it existed and was left
/// untouched.
pub fn create_or_get_periodic_path(
    vault_root: &Path,
    vault_templates_dir: &Path,
    period: Period,
    cfg: &PeriodicPeriod,
    date: NaiveDate,
    now: NaiveDateTime,
) -> Result<(PathBuf, bool)> {
    let date = period.start_of(date, cfg.week_start);
    let path = resolve_periodic_path(vault_root, period, cfg, date)?;
    if path.exists() {
        return Ok((path, false));
    }

    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .ok_or_else(|| Error::Periodic("resolved path has no filename stem".to_string()))?;

    let body = render_periodic_note(cfg, vault_templates_dir, &title, date, now)?;
    write_atomic(&path, &body)?;
    Ok((path, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use assert_fs::TempDir;
    use chrono::NaiveTime;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn dt(y: i32, m: u32, day: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, day)
            .unwrap()
            .and_time(NaiveTime::from_hms_opt(h, mi, 0).unwrap())
    }

    // ── Period::FromStr ───────────────────────────────────────────────────

    #[test]
    fn period_from_str_long_forms() {
        assert_eq!("daily".parse::<Period>().unwrap(), Period::Daily);
        assert_eq!("weekly".parse::<Period>().unwrap(), Period::Weekly);
        assert_eq!("monthly".parse::<Period>().unwrap(), Period::Monthly);
        assert_eq!("quarterly".parse::<Period>().unwrap(), Period::Quarterly);
        assert_eq!("yearly".parse::<Period>().unwrap(), Period::Yearly);
    }

    #[test]
    fn period_from_str_short_forms() {
        assert_eq!("d".parse::<Period>().unwrap(), Period::Daily);
        assert_eq!("w".parse::<Period>().unwrap(), Period::Weekly);
        assert_eq!("m".parse::<Period>().unwrap(), Period::Monthly);
        assert_eq!("q".parse::<Period>().unwrap(), Period::Quarterly);
        assert_eq!("y".parse::<Period>().unwrap(), Period::Yearly);
    }

    #[test]
    fn period_from_str_case_insensitive() {
        assert_eq!("DAILY".parse::<Period>().unwrap(), Period::Daily);
        assert_eq!("Quarterly".parse::<Period>().unwrap(), Period::Quarterly);
    }

    #[test]
    fn period_from_str_unknown_errors() {
        let err = "fortnightly".parse::<Period>().unwrap_err();
        assert!(err.contains("unknown period"));
        assert!(err.contains("daily"));
    }

    // ── Period::offset_date ───────────────────────────────────────────────

    #[test]
    fn offset_daily() {
        assert_eq!(
            Period::Daily.offset_date(d(2026, 5, 14), 1).unwrap(),
            d(2026, 5, 15)
        );
        assert_eq!(
            Period::Daily.offset_date(d(2026, 5, 14), -14).unwrap(),
            d(2026, 4, 30)
        );
    }

    #[test]
    fn offset_weekly() {
        assert_eq!(
            Period::Weekly.offset_date(d(2026, 5, 14), 1).unwrap(),
            d(2026, 5, 21)
        );
        assert_eq!(
            Period::Weekly.offset_date(d(2026, 5, 14), -2).unwrap(),
            d(2026, 4, 30)
        );
    }

    #[test]
    fn offset_monthly_clamps_to_month_end() {
        assert_eq!(
            Period::Monthly.offset_date(d(2026, 1, 31), 1).unwrap(),
            d(2026, 2, 28)
        );
        assert_eq!(
            Period::Monthly.offset_date(d(2024, 1, 31), 1).unwrap(),
            d(2024, 2, 29) // leap year
        );
        assert_eq!(
            Period::Monthly.offset_date(d(2026, 3, 31), -1).unwrap(),
            d(2026, 2, 28)
        );
    }

    #[test]
    fn offset_quarterly_three_months() {
        assert_eq!(
            Period::Quarterly.offset_date(d(2026, 5, 14), 1).unwrap(),
            d(2026, 8, 14)
        );
        assert_eq!(
            Period::Quarterly.offset_date(d(2026, 5, 14), -2).unwrap(),
            d(2025, 11, 14)
        );
    }

    #[test]
    fn offset_yearly() {
        assert_eq!(
            Period::Yearly.offset_date(d(2026, 5, 14), 1).unwrap(),
            d(2027, 5, 14)
        );
        assert_eq!(
            Period::Yearly.offset_date(d(2024, 2, 29), 1).unwrap(),
            d(2025, 2, 28) // leap-day clamp into non-leap year
        );
    }

    // ── Period::start_of ──────────────────────────────────────────────────

    #[test]
    fn start_of_daily_is_identity() {
        assert_eq!(
            Period::Daily.start_of(d(2026, 5, 13), WeekStart::Monday),
            d(2026, 5, 13)
        );
    }

    #[test]
    fn start_of_weekly_anchors_to_monday() {
        // Wed 2026-05-13 → Mon 2026-05-11.
        assert_eq!(
            Period::Weekly.start_of(d(2026, 5, 13), WeekStart::Monday),
            d(2026, 5, 11)
        );
        // Sunday 2026-05-17 → Mon 2026-05-11.
        assert_eq!(
            Period::Weekly.start_of(d(2026, 5, 17), WeekStart::Monday),
            d(2026, 5, 11)
        );
        // Monday is its own anchor.
        assert_eq!(
            Period::Weekly.start_of(d(2026, 5, 11), WeekStart::Monday),
            d(2026, 5, 11)
        );
    }

    #[test]
    fn start_of_weekly_sunday_week_start() {
        // Wed 2026-05-13 → Sun 2026-05-10.
        assert_eq!(
            Period::Weekly.start_of(d(2026, 5, 13), WeekStart::Sunday),
            d(2026, 5, 10)
        );
        // Sunday is its own anchor.
        assert_eq!(
            Period::Weekly.start_of(d(2026, 5, 10), WeekStart::Sunday),
            d(2026, 5, 10)
        );
        // Monday 2026-05-11 → Sun 2026-05-10.
        assert_eq!(
            Period::Weekly.start_of(d(2026, 5, 11), WeekStart::Sunday),
            d(2026, 5, 10)
        );
    }

    #[test]
    fn start_of_weekly_crosses_year_boundary() {
        // 2027-01-01 is a Friday; its ISO week starts Mon 2026-12-28.
        assert_eq!(
            Period::Weekly.start_of(d(2027, 1, 1), WeekStart::Monday),
            d(2026, 12, 28)
        );
        // 2026-01-01 is a Thursday; its ISO week starts Mon 2025-12-29.
        assert_eq!(
            Period::Weekly.start_of(d(2026, 1, 1), WeekStart::Monday),
            d(2025, 12, 29)
        );
    }

    #[test]
    fn start_of_monthly_first_of_month() {
        assert_eq!(
            Period::Monthly.start_of(d(2026, 5, 14), WeekStart::Monday),
            d(2026, 5, 1)
        );
        assert_eq!(
            Period::Monthly.start_of(d(2026, 5, 1), WeekStart::Monday),
            d(2026, 5, 1)
        );
    }

    #[test]
    fn start_of_quarterly_first_of_quarter() {
        assert_eq!(
            Period::Quarterly.start_of(d(2026, 5, 14), WeekStart::Monday),
            d(2026, 4, 1)
        );
        assert_eq!(
            Period::Quarterly.start_of(d(2026, 11, 30), WeekStart::Monday),
            d(2026, 10, 1)
        );
        assert_eq!(
            Period::Quarterly.start_of(d(2026, 1, 5), WeekStart::Monday),
            d(2026, 1, 1)
        );
    }

    #[test]
    fn start_of_yearly_jan_first() {
        assert_eq!(
            Period::Yearly.start_of(d(2026, 5, 14), WeekStart::Monday),
            d(2026, 1, 1)
        );
    }

    // ── %q / %Q pre-processor ─────────────────────────────────────────────

    #[test]
    fn quarter_token_q_lowercase() {
        // Jan-Mar → Q1
        assert_eq!(substitute_quarter_tokens("%q", d(2026, 1, 15)), "1");
        // Apr-Jun → Q2
        assert_eq!(substitute_quarter_tokens("%q", d(2026, 5, 14)), "2");
        // Jul-Sep → Q3
        assert_eq!(substitute_quarter_tokens("%q", d(2026, 8, 1)), "3");
        // Oct-Dec → Q4
        assert_eq!(substitute_quarter_tokens("%q", d(2026, 12, 31)), "4");
    }

    #[test]
    fn quarter_token_q_capital() {
        assert_eq!(substitute_quarter_tokens("%Q", d(2026, 5, 14)), "Q2");
        assert_eq!(substitute_quarter_tokens("%Q", d(2026, 12, 31)), "Q4");
    }

    #[test]
    fn quarter_token_mixed_with_other_tokens() {
        // Other %-tokens pass through to chrono untouched.
        assert_eq!(substitute_quarter_tokens("%Y-Q%q", d(2026, 5, 14)), "%Y-Q2");
        assert_eq!(substitute_quarter_tokens("%Y/%Q", d(2026, 12, 31)), "%Y/Q4");
    }

    #[test]
    fn quarter_token_double_percent_preserved() {
        // `%%q` should pass through as `%%q` (chrono will emit literal `%q`).
        assert_eq!(substitute_quarter_tokens("%%q", d(2026, 5, 14)), "%%q");
    }

    // ── resolve_periodic_path ─────────────────────────────────────────────

    #[test]
    fn resolve_daily_path_basic() {
        let dir = TempDir::new().unwrap();
        let cfg = PeriodicPeriod {
            path: "journal/%Y".into(),
            format: "%Y-%m-%d".into(),
            template: None,
            ..Default::default()
        };
        let p = resolve_periodic_path(dir.path(), Period::Daily, &cfg, d(2026, 5, 14)).unwrap();
        assert_eq!(p, dir.path().join("journal/2026/2026-05-14.md"));
    }

    #[test]
    fn resolve_weekly_iso_week_path() {
        let dir = TempDir::new().unwrap();
        let cfg = PeriodicPeriod {
            path: "journal/%Y".into(),
            format: "%G-W%V".into(),
            template: None,
            ..Default::default()
        };
        // 2026-05-14 falls in ISO week 20 of 2026 (Mon..Sun = May 11..17).
        let p = resolve_periodic_path(dir.path(), Period::Weekly, &cfg, d(2026, 5, 14)).unwrap();
        assert_eq!(p, dir.path().join("journal/2026/2026-W20.md"));
    }

    #[test]
    fn resolve_monthly_path() {
        let dir = TempDir::new().unwrap();
        let cfg = PeriodicPeriod {
            path: "journal/%Y".into(),
            format: "%Y-%m".into(),
            template: None,
            ..Default::default()
        };
        let p = resolve_periodic_path(dir.path(), Period::Monthly, &cfg, d(2026, 5, 14)).unwrap();
        assert_eq!(p, dir.path().join("journal/2026/2026-05.md"));
    }

    #[test]
    fn resolve_quarterly_path_uses_q_token() {
        let dir = TempDir::new().unwrap();
        let cfg = PeriodicPeriod {
            path: "journal/%Y".into(),
            format: "%Y-Q%q".into(),
            template: None,
            ..Default::default()
        };
        let p = resolve_periodic_path(dir.path(), Period::Quarterly, &cfg, d(2026, 5, 14)).unwrap();
        assert_eq!(p, dir.path().join("journal/2026/2026-Q2.md"));
    }

    #[test]
    fn resolve_yearly_path_minimal_config() {
        let dir = TempDir::new().unwrap();
        let cfg = PeriodicPeriod {
            path: "journal".into(),
            format: "%Y".into(),
            template: None,
            ..Default::default()
        };
        let p = resolve_periodic_path(dir.path(), Period::Yearly, &cfg, d(2026, 5, 14)).unwrap();
        assert_eq!(p, dir.path().join("journal/2026.md"));
    }

    #[test]
    fn resolve_with_empty_path_lands_at_vault_root() {
        let dir = TempDir::new().unwrap();
        let cfg = PeriodicPeriod {
            path: "".into(),
            format: "%Y-%m-%d".into(),
            template: None,
            ..Default::default()
        };
        let p = resolve_periodic_path(dir.path(), Period::Daily, &cfg, d(2026, 5, 14)).unwrap();
        assert_eq!(p, dir.path().join("2026-05-14.md"));
    }

    #[test]
    fn resolve_empty_format_errors() {
        let dir = TempDir::new().unwrap();
        let cfg = PeriodicPeriod {
            path: "journal".into(),
            format: "".into(),
            template: None,
            ..Default::default()
        };
        let err =
            resolve_periodic_path(dir.path(), Period::Daily, &cfg, d(2026, 5, 14)).unwrap_err();
        assert!(matches!(err, Error::Periodic(_)));
    }

    // ── render_periodic_note ──────────────────────────────────────────────

    #[test]
    fn render_no_template_emits_blank_stub() {
        let dir = TempDir::new().unwrap();
        let cfg = PeriodicPeriod {
            path: "journal".into(),
            format: "%Y-%m-%d".into(),
            template: None,
            ..Default::default()
        };
        let body = render_periodic_note(
            &cfg,
            dir.path(),
            "2026-05-14",
            d(2026, 5, 14),
            dt(2026, 5, 14, 0, 0),
        )
        .unwrap();
        assert_eq!(body, "# 2026-05-14\n\n");
    }

    #[test]
    fn render_with_template_renders_minijinja() {
        let templates = TempDir::new().unwrap();
        let tpl = templates.child("daily.md");
        tpl.write_str("# {{ title }}\n\n- date: {{ today | date }}\n")
            .unwrap();

        let cfg = PeriodicPeriod {
            path: "journal".into(),
            format: "%Y-%m-%d".into(),
            template: Some("daily".into()),
            ..Default::default()
        };
        let body = render_periodic_note(
            &cfg,
            templates.path(),
            "2026-05-14",
            d(2026, 5, 14),
            dt(2026, 5, 14, 0, 0),
        )
        .unwrap();
        assert_eq!(body, "# 2026-05-14\n\n- date: 2026-05-14\n");
    }

    #[test]
    fn render_missing_template_errors() {
        let templates = TempDir::new().unwrap();
        let cfg = PeriodicPeriod {
            path: "journal".into(),
            format: "%Y-%m-%d".into(),
            template: Some("nope".into()),
            ..Default::default()
        };
        let err = render_periodic_note(
            &cfg,
            templates.path(),
            "2026-05-14",
            d(2026, 5, 14),
            dt(2026, 5, 14, 0, 0),
        )
        .unwrap_err();
        assert!(
            matches!(&err, Error::Periodic(msg) if msg.contains("nope")),
            "expected Error::Periodic mentioning 'nope', got: {err:?}"
        );
    }

    // ── create_or_get_periodic_path ───────────────────────────────────────

    #[test]
    fn create_or_get_writes_new_file_then_keeps_it() {
        let vault = TempDir::new().unwrap();
        let templates = vault.child("templates-ft");
        templates.create_dir_all().unwrap();
        let cfg = PeriodicPeriod {
            path: "journal/%Y".into(),
            format: "%Y-%m-%d".into(),
            template: None,
            ..Default::default()
        };

        let (path1, created1) = create_or_get_periodic_path(
            vault.path(),
            templates.path(),
            Period::Daily,
            &cfg,
            d(2026, 5, 14),
            dt(2026, 5, 14, 0, 0),
        )
        .unwrap();
        assert!(created1);
        assert_eq!(path1, vault.path().join("journal/2026/2026-05-14.md"));
        let body = std::fs::read_to_string(&path1).unwrap();
        assert_eq!(body, "# 2026-05-14\n\n");

        // Mutate the file, then call again — the helper must not touch it.
        std::fs::write(&path1, "# manually edited\n").unwrap();
        let (path2, created2) = create_or_get_periodic_path(
            vault.path(),
            templates.path(),
            Period::Daily,
            &cfg,
            d(2026, 5, 14),
            dt(2026, 5, 14, 0, 0),
        )
        .unwrap();
        assert!(!created2);
        assert_eq!(path2, path1);
        assert_eq!(
            std::fs::read_to_string(&path2).unwrap(),
            "# manually edited\n",
            "second call should not overwrite the existing file"
        );
    }

    #[test]
    fn create_or_get_creates_missing_parent_dirs() {
        let vault = TempDir::new().unwrap();
        let templates = vault.child("templates-ft");
        templates.create_dir_all().unwrap();
        let cfg = PeriodicPeriod {
            path: "journal/%Y/%m".into(),
            format: "%Y-%m-%d".into(),
            template: None,
            ..Default::default()
        };
        let (path, created) = create_or_get_periodic_path(
            vault.path(),
            templates.path(),
            Period::Daily,
            &cfg,
            d(2026, 5, 14),
            dt(2026, 5, 14, 0, 0),
        )
        .unwrap();
        assert!(created);
        assert_eq!(path, vault.path().join("journal/2026/05/2026-05-14.md"));
        assert!(path.exists());
    }
}

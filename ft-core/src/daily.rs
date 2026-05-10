//! Resolve daily-note paths from Obsidian's core "Daily notes" plugin config.
//!
//! Reads `<vault>/.obsidian/daily-notes.json` (folder, format, template),
//! translates the moment.js format string subset to chrono format, and
//! resolves the path for a given date. Unsupported moment.js tokens reject
//! with the offending substring named.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::Deserialize;
use thiserror::Error;

/// Default folder when daily-notes.json doesn't set one (matches Obsidian).
const DEFAULT_FOLDER: &str = "";
/// Default moment.js format when daily-notes.json doesn't set one.
const DEFAULT_FORMAT: &str = "YYYY-MM-DD";

#[derive(Debug, Error)]
pub enum DailyError {
    #[error("daily notes config not found at {}", .path.display())]
    NotFound { path: PathBuf },
    #[error("could not read {}: {source}", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse {}: {source}", .path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("daily notes format token `{token}` is not supported")]
    UnsupportedToken { token: String },
}

#[derive(Debug, Deserialize, Default)]
pub struct DailyNotesConfig {
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    /// Other keys (`autorun`, etc.) are ignored.
    #[serde(flatten, default)]
    _other: serde_json::Map<String, serde_json::Value>,
}

/// Read `<vault>/.obsidian/daily-notes.json`. Missing file → `NotFound`.
pub fn load(vault_root: &Path) -> Result<DailyNotesConfig, DailyError> {
    let path = vault_root.join(".obsidian").join("daily-notes.json");
    if !path.exists() {
        return Err(DailyError::NotFound { path });
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| DailyError::Read {
        path: path.clone(),
        source: e,
    })?;
    serde_json::from_str(&raw).map_err(|e| DailyError::Parse { path, source: e })
}

/// Resolve the absolute path of the daily note for `date`, using the loaded
/// config (or sensible defaults if it's missing).
pub fn resolve_path(
    vault_root: &Path,
    cfg: &DailyNotesConfig,
    date: NaiveDate,
) -> Result<PathBuf, DailyError> {
    let folder = cfg.folder.as_deref().unwrap_or(DEFAULT_FOLDER);
    let format = cfg.format.as_deref().unwrap_or(DEFAULT_FORMAT);
    let chrono_fmt = translate_format(format)?;
    let filename = date.format(&chrono_fmt).to_string();

    let mut p = vault_root.to_path_buf();
    if !folder.is_empty() {
        p.push(folder);
    }
    p.push(format!("{filename}.md"));
    Ok(p)
}

/// Translate the supported subset of moment.js format tokens to chrono format.
///
/// Supported tokens (greedy, longest-first):
///   `YYYY` → `%Y`        4-digit year
///   `YY`   → `%y`        2-digit year
///   `MMMM` → `%B`        full month name
///   `MMM`  → `%b`        abbreviated month name
///   `MM`   → `%m`        2-digit month
///   `M`    → `%-m`       1- or 2-digit month
///   `DDDD` → `%j`        day of year (zero-padded)
///   `DD`   → `%d`        2-digit day of month
///   `D`    → `%-d`       1- or 2-digit day of month
///   `dddd` → `%A`        full weekday name
///   `ddd`  → `%a`        abbreviated weekday name
///   `HH`   → `%H`        24-hour hour
///   `mm`   → `%M`        minutes
///   `ss`   → `%S`        seconds
///
/// Inside `[...]` brackets, content is passed through verbatim. Anything
/// outside the supported tokens that *looks* like a moment.js token (long
/// runs of letters) rejects with `UnsupportedToken`.
pub fn translate_format(moment: &str) -> Result<String, DailyError> {
    const TOKENS: &[(&str, &str)] = &[
        ("YYYY", "%Y"),
        ("YY", "%y"),
        ("MMMM", "%B"),
        ("MMM", "%b"),
        ("MM", "%m"),
        ("M", "%-m"),
        ("DDDD", "%j"),
        ("DD", "%d"),
        ("D", "%-d"),
        ("dddd", "%A"),
        ("ddd", "%a"),
        ("HH", "%H"),
        ("mm", "%M"),
        ("ss", "%S"),
    ];

    let mut out = String::with_capacity(moment.len());
    let bytes = moment.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Literal escape: [text] is passed through verbatim.
        if bytes[i] == b'[' {
            if let Some(end) = moment[i + 1..].find(']') {
                out.push_str(&moment[i + 1..i + 1 + end]);
                i += end + 2;
                continue;
            }
        }

        // Try the longest matching token first.
        let rest = &moment[i..];
        let mut matched = false;
        for (token, repl) in TOKENS {
            if rest.starts_with(token) {
                out.push_str(repl);
                i += token.len();
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }

        // Reject any unsupported run of ASCII letters (moment.js tokens are
        // letter sequences). A single stray letter is suspicious enough to
        // call out.
        let ch = bytes[i] as char;
        if ch.is_ascii_alphabetic() {
            let end = i + rest
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(rest.len());
            return Err(DailyError::UnsupportedToken {
                token: moment[i..end].to_string(),
            });
        }

        // Pass through punctuation and digits unchanged. Escape `%` so chrono
        // doesn't read it as a directive.
        if ch == '%' {
            out.push_str("%%");
        } else {
            out.push(ch);
        }
        i += 1;
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use assert_fs::TempDir;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    // ── translate_format ─────────────────────────────────────────────────────

    #[test]
    fn translate_default() {
        assert_eq!(translate_format("YYYY-MM-DD").unwrap(), "%Y-%m-%d");
    }

    #[test]
    fn translate_long_year_and_month() {
        assert_eq!(translate_format("YYYY/MMMM/DD").unwrap(), "%Y/%B/%d");
    }

    #[test]
    fn translate_with_literal_brackets() {
        // `[Daily]` should be a literal.
        assert_eq!(
            translate_format("[Daily-]YYYY-MM-DD").unwrap(),
            "Daily-%Y-%m-%d"
        );
    }

    #[test]
    fn translate_unsupported_token_rejected() {
        let err = translate_format("YYYY-Q-MM").unwrap_err();
        assert!(matches!(err, DailyError::UnsupportedToken { .. }));
        if let DailyError::UnsupportedToken { token } = err {
            assert_eq!(token, "Q");
        }
    }

    #[test]
    fn translate_all_supported_tokens() {
        // Just confirm none reject.
        let fmts = [
            "YYYY", "YY", "MMMM", "MMM", "MM", "M", "DD", "D", "dddd", "ddd", "HH", "mm", "ss",
        ];
        for f in fmts {
            translate_format(f).unwrap_or_else(|e| panic!("token {f} rejected: {e}"));
        }
    }

    // ── resolve_path ─────────────────────────────────────────────────────────

    #[test]
    fn resolve_with_folder_and_default_format() {
        let cfg = DailyNotesConfig {
            folder: Some("journal/2026".into()),
            format: None,
            template: None,
            _other: Default::default(),
        };
        let p = resolve_path(Path::new("/v"), &cfg, date(2026, 5, 9)).unwrap();
        assert_eq!(p, Path::new("/v/journal/2026/2026-05-09.md"));
    }

    #[test]
    fn resolve_with_no_folder() {
        let cfg = DailyNotesConfig::default();
        let p = resolve_path(Path::new("/v"), &cfg, date(2026, 5, 9)).unwrap();
        assert_eq!(p, Path::new("/v/2026-05-09.md"));
    }

    #[test]
    fn resolve_with_custom_format() {
        let cfg = DailyNotesConfig {
            folder: Some("J".into()),
            format: Some("YYYY-MM-DD-dddd".into()),
            template: None,
            _other: Default::default(),
        };
        let p = resolve_path(Path::new("/v"), &cfg, date(2026, 5, 9)).unwrap();
        // 2026-05-09 is a Saturday.
        assert_eq!(p, Path::new("/v/J/2026-05-09-Saturday.md"));
    }

    // ── load ─────────────────────────────────────────────────────────────────

    #[test]
    fn load_missing_returns_notfound() {
        let dir = TempDir::new().unwrap();
        dir.child(".obsidian").create_dir_all().unwrap();
        let err = load(dir.path()).unwrap_err();
        assert!(matches!(err, DailyError::NotFound { .. }));
    }

    #[test]
    fn load_real_shape() {
        let dir = TempDir::new().unwrap();
        dir.child(".obsidian/daily-notes.json")
            .write_str(r#"{"folder":"journal/2024","autorun":true,"template":"templates/journal"}"#)
            .unwrap();
        let cfg = load(dir.path()).unwrap();
        assert_eq!(cfg.folder.as_deref(), Some("journal/2024"));
        assert_eq!(cfg.template.as_deref(), Some("templates/journal"));
        assert!(cfg.format.is_none());
    }
}

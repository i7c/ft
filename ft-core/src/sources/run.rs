//! Running a source end to end, shared by `ft tasks sync` and the TUI:
//! resolve credentials, build the backend, fetch (network), classify the
//! outcome. Planning and writing stay with the caller ([`super::sync`]).
//! Also the machine-local attempt log (`.ft/cache/sources.json`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::credentials;
use super::github::GithubSource;
use super::http::Secret;
use super::jira::JiraSource;
use super::{fetch_all, FetchResult, IssueRef, SourceError, TaskSource};
use crate::config::{SourceCfg, SourceKind};

/// Why a source produced no fetch result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchFailure {
    NoCredentials,
    Offline(String),
    Unauthorized(u16),
    Error(String),
}

impl FetchFailure {
    /// Stable status word (`ft tasks sync --format json`).
    pub fn status(&self) -> &'static str {
        match self {
            FetchFailure::NoCredentials => "no-credentials",
            FetchFailure::Offline(_) => "offline",
            FetchFailure::Unauthorized(_) => "unauthorized",
            FetchFailure::Error(_) => "error",
        }
    }

    /// Skips that are normal (no network, no token on this machine).
    pub fn is_benign(&self) -> bool {
        matches!(self, FetchFailure::NoCredentials | FetchFailure::Offline(_))
    }

    /// One-line human message for `source`.
    pub fn message(&self, source: &str) -> String {
        match self {
            FetchFailure::NoCredentials => {
                format!("skipped (no credentials — run ft sources login {source})")
            }
            FetchFailure::Offline(_) => "skipped (offline)".to_string(),
            FetchFailure::Unauthorized(code) => {
                format!("unauthorized ({code}) — run ft sources login {source}")
            }
            FetchFailure::Error(e) => format!("error: {e}"),
        }
    }
}

impl From<SourceError> for FetchFailure {
    fn from(e: SourceError) -> Self {
        match e {
            SourceError::Offline(m) => FetchFailure::Offline(m),
            SourceError::Unauthorized { status } => FetchFailure::Unauthorized(status),
            other => FetchFailure::Error(other.to_string()),
        }
    }
}

/// The backend for a configured source.
pub fn build_source(cfg: &SourceCfg, token: &Secret) -> Box<dyn TaskSource> {
    match cfg.kind {
        SourceKind::Jira => Box::new(JiraSource::new(
            cfg.base_url.as_deref().unwrap_or_default(),
            cfg.email.as_deref().unwrap_or_default(),
            token,
            cfg.status_map.clone(),
        )),
        SourceKind::Github => Box::new(GithubSource::new(
            cfg.base_url.as_deref(),
            token,
            cfg.status_map.clone(),
        )),
    }
}

/// Who `token` authenticates as — verifies a token before it is stored.
pub fn verify_token(cfg: &SourceCfg, token: &Secret) -> Result<String, FetchFailure> {
    let who = match cfg.kind {
        SourceKind::Jira => JiraSource::new(
            cfg.base_url.as_deref().unwrap_or_default(),
            cfg.email.as_deref().unwrap_or_default(),
            token,
            BTreeMap::new(),
        )
        .whoami(),
        SourceKind::Github => {
            GithubSource::new(cfg.base_url.as_deref(), token, BTreeMap::new()).whoami()
        }
    };
    who.map_err(FetchFailure::from)
}

/// Resolve credentials and fetch everything for one source.
pub fn fetch_source(
    name: &str,
    cfg: &SourceCfg,
    known: &[IssueRef],
) -> Result<FetchResult, FetchFailure> {
    let (token, _) = credentials::resolve(name, cfg).ok_or(FetchFailure::NoCredentials)?;
    let source = build_source(cfg, &token);
    Ok(fetch_all(
        source.as_ref(),
        &cfg.effective_queries(),
        cfg.fetch_parents,
        known,
    )?)
}

/// Machine-local sync attempt times, informational only.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttemptLog {
    #[serde(default)]
    pub sources: BTreeMap<String, Attempt>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Attempt {
    /// RFC 3339 local time of the last attempt.
    pub last_attempt: Option<String>,
    /// RFC 3339 local time of the last successful fetch.
    pub last_ok: Option<String>,
    /// Status word of the last attempt (`ok`, `offline`, …).
    pub last_status: Option<String>,
}

fn log_path(vault_root: &Path) -> PathBuf {
    vault_root.join(".ft/cache/sources.json")
}

impl AttemptLog {
    pub fn load(vault_root: &Path) -> Self {
        std::fs::read_to_string(log_path(vault_root))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Record an attempt and save. Failures to write are ignored — the log
    /// is informational.
    pub fn record(vault_root: &Path, source: &str, status: &str) {
        let mut log = Self::load(vault_root);
        let now = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
        let entry = log.sources.entry(source.to_string()).or_default();
        entry.last_attempt = Some(now.clone());
        entry.last_status = Some(status.to_string());
        if status == "ok" {
            entry.last_ok = Some(now);
        }
        if let Ok(json) = serde_json::to_string_pretty(&log) {
            let _ = crate::fs::write_atomic(&log_path(vault_root), &json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_classification() {
        assert_eq!(
            FetchFailure::from(SourceError::Offline("x".into())).status(),
            "offline"
        );
        assert_eq!(
            FetchFailure::from(SourceError::Unauthorized { status: 401 }).status(),
            "unauthorized"
        );
        assert_eq!(
            FetchFailure::from(SourceError::RateLimited).status(),
            "error"
        );
        assert!(
            FetchFailure::NoCredentials.is_benign()
                && FetchFailure::Offline("x".into()).is_benign()
        );
        assert!(!FetchFailure::Unauthorized(401).is_benign());
        assert_eq!(
            FetchFailure::NoCredentials.message("jira"),
            "skipped (no credentials — run ft sources login jira)"
        );
    }

    #[test]
    fn attempt_log_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        AttemptLog::record(dir.path(), "jira", "offline");
        AttemptLog::record(dir.path(), "jira", "ok");
        let log = AttemptLog::load(dir.path());
        let a = &log.sources["jira"];
        assert_eq!(a.last_status.as_deref(), Some("ok"));
        assert!(a.last_ok.is_some() && a.last_attempt.is_some());
    }
}

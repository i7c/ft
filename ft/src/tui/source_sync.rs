//! Background sync of remote task sources (Jira, GitHub).
//!
//! The worker scans the vault, fetches each due source, and plans the note
//! edits — all reads. The main thread applies only the edits whose files
//! are still exactly as planned, so nothing edited in the TUI meanwhile is
//! overwritten; skipped notes are picked up by the next sync.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use ft_core::config::SourceCfg;
use ft_core::scan::Scan;
use ft_core::sources::run::SourceOutcome;
use ft_core::sources::run::{self, FetchFailure};
use ft_core::sources::sync::{self, SyncPlan};
use ft_core::vault::Vault;

/// Fetches one source; `run::fetch_source` in production, a fake in tests.
pub type Fetcher =
    Arc<dyn Fn(&str, &SourceCfg, &Scan, bool) -> Result<SourceOutcome, FetchFailure> + Send + Sync>;

pub fn default_fetcher() -> Fetcher {
    Arc::new(run::run_source)
}

/// One source's planned sync (or why it couldn't run).
#[derive(Debug, Clone)]
pub struct SourceRun {
    pub name: String,
    pub outcome: Result<SyncPlan, FetchFailure>,
}

/// What the worker posts back.
#[derive(Debug, Clone)]
pub struct SourcesJobResult {
    pub runs: Vec<SourceRun>,
    /// Started by `g r` (always toast a summary) vs. startup/interval.
    pub manual: bool,
}

/// Everything the worker owns.
pub struct SourcesJob {
    pub vault: Arc<Vault>,
    pub sources: Vec<String>,
    pub fetcher: Fetcher,
    pub today: NaiveDate,
    pub manual: bool,
}

impl SourcesJob {
    pub fn run(self) -> SourcesJobResult {
        let scan = self.vault.scan();
        let templates_dir = self.vault.templates_dir();
        let runs = self
            .sources
            .iter()
            .filter_map(|name| {
                let cfg = self.vault.config.config.sources.get(name)?;
                let outcome = (self.fetcher)(name, cfg, &scan, true).map(|o| {
                    let ctx = sync::SyncCtx {
                        vault_root: &self.vault.path,
                        templates_dir: &templates_dir,
                        scan: &scan,
                        source: name,
                        cfg,
                        format: self.vault.task_format(),
                        today: self.today,
                    };
                    let mut plan = sync::plan_sync(&ctx, &o.fetched);
                    plan.report.pushed = o.push.pushed;
                    plan.report.push_failures = o.push.failures;
                    plan
                });
                Some(SourceRun {
                    name: name.clone(),
                    outcome,
                })
            })
            .collect();
        SourcesJobResult {
            runs,
            manual: self.manual,
        }
    }
}

/// When each source is next due. A source with no entry is due now
/// (startup sync).
#[derive(Default)]
pub struct Schedule {
    next: HashMap<String, Instant>,
}

impl Schedule {
    /// Sources due at `now` (all of them when `manual`).
    pub fn due(
        &self,
        sources: &std::collections::BTreeMap<String, SourceCfg>,
        now: Instant,
        manual: bool,
    ) -> Vec<String> {
        sources
            .keys()
            .filter(|name| manual || self.next.get(*name).is_none_or(|t| *t <= now))
            .cloned()
            .collect()
    }

    /// Record an attempt; interval `0` means "startup only".
    pub fn attempted(&mut self, name: &str, cfg: &SourceCfg, now: Instant) {
        let next = if cfg.interval_minutes == 0 {
            now + Duration::from_secs(100 * 365 * 24 * 3600)
        } else {
            now + Duration::from_secs(cfg.interval_minutes * 60)
        };
        self.next.insert(name.to_string(), next);
    }
}

/// Apply a planned sync: write each edit whose file still holds the
/// planned original. Returns the number of edits skipped because the file
/// changed meanwhile.
pub fn apply_plan(plan: &SyncPlan) -> Result<usize, String> {
    let mut skipped = 0;
    for e in plan.edits.iter().filter(|e| e.original != e.new) {
        let current = std::fs::read_to_string(&e.path).unwrap_or_default();
        if current != e.original {
            skipped += 1;
            continue;
        }
        ft_core::fs::write_atomic(&e.path, &e.new).map_err(|err| err.to_string())?;
    }
    Ok(skipped)
}

/// The toast for a finished job, if one is warranted, and whether it is
/// an error. Background runs stay quiet unless notes changed, a conflict
/// appeared, or a source failed for a reason the user must act on.
pub fn summary(
    result: &SourcesJobResult,
    applied: &[(String, Result<usize, String>)],
    fresh_failures: &[(String, Vec<String>)],
) -> Option<(String, bool)> {
    let mut parts = Vec::new();
    let mut error = false;
    let mut noteworthy = result.manual;
    for run in &result.runs {
        match &run.outcome {
            Ok(plan) => {
                let r = &plan.report;
                let changed = r.created.len() + r.updated.len();
                let mut s = if changed == 0 {
                    format!("{}: no changes", run.name)
                } else {
                    format!(
                        "{}: {} new · {} updated",
                        run.name,
                        r.created.len(),
                        r.updated.len()
                    )
                };
                if !r.pushed.is_empty() {
                    s.push_str(&format!(" · {} pushed", r.pushed.len()));
                    noteworthy = true;
                } else if !r.pending.is_empty() {
                    s.push_str(&format!(" · {} pending push", r.pending.len()));
                }
                // Only failures whose reason is new — no nagging every tick.
                let fresh = fresh_failures
                    .iter()
                    .find(|(n, _)| n == &run.name)
                    .map(|(_, keys)| keys.as_slice())
                    .unwrap_or(&[]);
                for (key, reason) in r.push_failures.iter().filter(|(k, _)| fresh.contains(k)) {
                    s.push_str(&format!(" · push failed {key}: {reason}"));
                    error = true;
                    noteworthy = true;
                }
                if !r.conflicts.is_empty() {
                    s.push_str(&format!(
                        " · {} conflicts (see ft tasks sync)",
                        r.conflicts.len()
                    ));
                    noteworthy = true;
                }
                match applied.iter().find(|(n, _)| n == &run.name).map(|(_, a)| a) {
                    Some(Ok(skipped)) if *skipped > 0 => {
                        s.push_str(&format!(
                            " · {skipped} changed meanwhile, retried next sync"
                        ));
                    }
                    Some(Err(e)) => {
                        s = format!("{}: write failed: {e}", run.name);
                        error = true;
                        noteworthy = true;
                    }
                    _ => {}
                }
                if changed > 0 {
                    noteworthy = true;
                }
                parts.push(s);
            }
            Err(f) if f.is_benign() => {
                if result.manual {
                    parts.push(match f {
                        FetchFailure::Offline(_) => {
                            format!("{}: offline — nothing synced", run.name)
                        }
                        other => format!("{}: {}", run.name, other.message(&run.name)),
                    });
                }
            }
            Err(f) => {
                error = true;
                noteworthy = true;
                parts.push(format!("{}: {}", run.name, f.message(&run.name)));
            }
        }
    }
    (noteworthy && !parts.is_empty()).then(|| (parts.join("  |  "), error))
}

/// What the promote worker posts back: the created key, the planned
/// vault edits (applied on the main thread with the unchanged-file guard),
/// and where the new anchor will be.
#[derive(Debug, Clone)]
pub struct PromoteJobResult {
    pub outcome: Result<PromotePlanned, String>,
}

#[derive(Debug, Clone)]
pub struct PromotePlanned {
    pub key: String,
    pub edits: Vec<ft_core::task::ops::FileEdit>,
    /// Vault-relative note + 1-indexed anchor line, for the cursor.
    pub anchor: ft_core::task::TaskKey,
    pub push_failure: Option<String>,
}

/// The promote worker: create the remote issue, then scan and plan the
/// local restructuring (reads only).
/// The remote half of a promote; `run::promote` in production.
pub type Promoter = Arc<
    dyn Fn(
            &str,
            &SourceCfg,
            &ft_core::sources::IssueRef,
            &ft_core::sources::NewIssue,
            ft_core::task::Status,
        ) -> Result<(ft_core::sources::RemoteIssue, Option<String>), FetchFailure>
        + Send
        + Sync,
>;

pub fn default_promoter() -> Promoter {
    Arc::new(run::promote)
}

pub struct PromoteJob {
    pub promoter: Promoter,
    pub vault: Arc<Vault>,
    pub task: ft_core::task::Task,
    pub target: ft_core::sources::promote::PromoteTarget,
    pub today: NaiveDate,
}

impl PromoteJob {
    pub fn run(self) -> PromoteJobResult {
        use ft_core::sources::promote;
        let outcome = (|| {
            let cfg = self
                .vault
                .config
                .config
                .sources
                .get(&self.target.source)
                .ok_or_else(|| format!("source `{}` is not configured", self.target.source))?;
            let name = &self.target.source;
            let (created, push_failure) = (self.promoter)(
                name,
                cfg,
                &self.target.parent,
                &promote::new_issue(&self.task),
                self.task.status,
            )
            .map_err(|f| match f {
                FetchFailure::Offline(_) => format!("{name}: offline — promote needs a connection"),
                other => format!("{name}: {}", other.message(name)),
            })?;
            let scan = self.vault.scan();
            let plan = promote::plan_local(
                &self.vault.path,
                &self.vault.templates_dir(),
                &scan,
                name,
                cfg,
                self.vault.task_format(),
                self.today,
                &self.task,
                &created,
                &self.target.parent.key,
            )
            .map_err(|e| format!("created {} but couldn't update the vault: {e}", created.key))?;
            let note_abs = self.vault.path.join(&plan.note);
            let line = plan
                .edits
                .iter()
                .find(|e| e.path == note_abs)
                .and_then(|e| {
                    e.new
                        .split('\n')
                        .position(|l| l.contains(&format!("🆔 {}", created.key)))
                })
                .map_or(1, |i| i + 1);
            Ok(PromotePlanned {
                key: created.key,
                edits: plan.edits,
                anchor: (plan.note, line),
                push_failure,
            })
        })();
        PromoteJobResult { outcome }
    }
}

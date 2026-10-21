//! Pushing local status changes. Pending pushes are derived from the
//! notes — an anchor whose status differs from `ft.source.synced.status`,
//! without a stored conflict, in an active or context note — so any machine
//! with credentials computes the same set; there is no outbox.

use std::collections::BTreeMap;

use super::sync::{index_notes, STATE_ACTIVE, STATE_CONTEXT};
use super::{note, IssueRef, PushOutcome, SourceError, TaskSource};
use crate::scan::Scan;
use crate::task::Status;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub issue: IssueRef,
    pub target: Status,
}

/// What a push step did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushReport {
    pub pushed: Vec<String>,
    /// `(key, reason)`.
    pub failures: Vec<(String, String)>,
}

/// The source's pending pushes, from the scan.
pub fn pending(scan: &Scan, source: &str) -> Vec<Pending> {
    index_notes(scan, source)
        .0
        .into_values()
        .filter(|k| k.fm.conflict.is_none())
        .filter(|k| {
            matches!(
                k.fm.state.as_deref(),
                Some(STATE_ACTIVE) | Some(STATE_CONTEXT)
            )
        })
        .filter_map(|k| {
            let base = note::parse_status(k.fm.synced_status.as_deref()?)?;
            let anchor = scan
                .tasks
                .iter()
                .find(|t| t.source_file == k.rel && t.id.as_deref() == Some(k.fm.key.as_str()))?;
            (anchor.status != base).then(|| Pending {
                issue: IssueRef {
                    key: k.fm.key.clone(),
                    url: k.fm.url.clone(),
                },
                target: anchor.status,
            })
        })
        .collect()
}

/// Push every pending status. `Offline` / `Unauthorized` abort the whole
/// step (returned as `Err`); any other failure is recorded per issue.
pub fn push_pending(
    source: &dyn TaskSource,
    pending: &[Pending],
    transitions: &BTreeMap<Status, String>,
) -> Result<PushReport, SourceError> {
    let mut report = PushReport::default();
    for p in pending {
        match source.push_status(&p.issue, p.target, transitions) {
            Ok(PushOutcome::Pushed) => report.pushed.push(p.issue.key.clone()),
            Ok(PushOutcome::AlreadyThere) => {}
            Ok(PushOutcome::NotPushable(reason)) => {
                report.failures.push((p.issue.key.clone(), reason))
            }
            Err(e @ (SourceError::Offline(_) | SourceError::Unauthorized { .. })) => return Err(e),
            Err(e) => report.failures.push((p.issue.key.clone(), e.to_string())),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_of(files: &[(&str, &str)]) -> (tempfile::TempDir, Scan) {
        let dir = tempfile::tempdir().unwrap();
        for (rel, body) in files {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        let s = crate::scan::scan_vault(dir.path(), &[]);
        (dir, s)
    }

    fn note(key: &str, checkbox: char, synced: &str, state: &str, conflict: bool) -> String {
        let c = if conflict {
            "    conflict: Cancelled\n"
        } else {
            ""
        };
        format!(
            "---\nft:\n  tasks:\n    anchor: {key}\n  source:\n    name: jira\n    key: {key}\n    url: https://x.test/browse/{key}\n    state: {state}\n    synced:\n      status: {synced}\n{c}---\n- [{checkbox}] [{key}](https://x.test/browse/{key}) t 🆔 {key}\n"
        )
    }

    #[test]
    fn pending_rules() {
        let (_d, s) = scan_of(&[
            (
                "jira/A-1.md",
                &note("A-1", 'x', "InProgress", "active", false),
            ),
            (
                "jira/A-2.md",
                &note("A-2", '/', "InProgress", "active", false),
            ),
            (
                "jira/A-3.md",
                &note("A-3", 'x', "InProgress", "active", true),
            ),
            (
                "jira/A-4.md",
                &note("A-4", '-', "Open", "out-of-scope", false),
            ),
            ("jira/A-5.md", &note("A-5", ' ', "Done", "context", false)),
        ]);
        let p = pending(&s, "jira");
        let got: Vec<(&str, Status)> = p.iter().map(|p| (p.issue.key.as_str(), p.target)).collect();
        assert_eq!(got, [("A-1", Status::Done), ("A-5", Status::Open)]);
        assert_eq!(p[0].issue.url.as_deref(), Some("https://x.test/browse/A-1"));
        assert!(pending(&s, "gh").is_empty());
    }

    struct Scripted(
        Vec<Result<PushOutcome, SourceError>>,
        std::cell::RefCell<usize>,
    );

    impl TaskSource for Scripted {
        fn fetch_query(&self, _: &str) -> Result<Vec<super::super::RemoteIssue>, SourceError> {
            Ok(Vec::new())
        }
        fn fetch_keys(
            &self,
            _: &[IssueRef],
        ) -> Result<Vec<super::super::RemoteIssue>, SourceError> {
            Ok(Vec::new())
        }
        fn push_status(
            &self,
            _: &IssueRef,
            _: Status,
            _: &BTreeMap<Status, String>,
        ) -> Result<PushOutcome, SourceError> {
            let i = *self.1.borrow();
            *self.1.borrow_mut() += 1;
            self.0[i].clone()
        }
    }

    fn p(key: &str) -> Pending {
        Pending {
            issue: IssueRef::key(key),
            target: Status::Done,
        }
    }

    #[test]
    fn push_pending_collects_and_aborts() {
        let src = Scripted(
            vec![
                Ok(PushOutcome::Pushed),
                Ok(PushOutcome::AlreadyThere),
                Ok(PushOutcome::NotPushable("no transition".into())),
                Err(SourceError::Remote {
                    status: 500,
                    excerpt: "boom".into(),
                }),
            ],
            Default::default(),
        );
        let r = push_pending(&src, &[p("A"), p("B"), p("C"), p("D")], &BTreeMap::new()).unwrap();
        assert_eq!(r.pushed, ["A"]);
        assert_eq!(r.failures.len(), 2);
        assert_eq!(
            r.failures[0],
            ("C".to_string(), "no transition".to_string())
        );
        let src = Scripted(
            vec![Err(SourceError::Offline("x".into()))],
            Default::default(),
        );
        assert!(matches!(
            push_pending(&src, &[p("A")], &BTreeMap::new()),
            Err(SourceError::Offline(_))
        ));
    }
}

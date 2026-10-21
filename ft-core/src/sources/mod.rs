//! Remote task sources (Jira, GitHub). Fetching is blocking and network
//! bound; planning the resulting note edits ([`sync`]) is pure and works on
//! the fetched data, so the TUI can fetch off-thread and apply on the main
//! thread. Nothing here runs during a vault scan.

pub mod adf;
pub mod credentials;
pub mod github;
pub mod http;
pub mod jira;
pub mod note;
pub mod push;
pub mod run;
pub mod sync;

use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use thiserror::Error;

use crate::task::{Priority, Status};

/// One remote issue, normalised across backends.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteIssue {
    /// Key used as `🆔`, file stem, and `ft.source.key` (e.g. `ABC-123`).
    pub key: String,
    pub url: String,
    pub title: String,
    /// Remote status name, verbatim (e.g. `In Review`).
    pub status_name: String,
    /// `status_name` mapped to a local status.
    pub status: Status,
    pub priority: Option<Priority>,
    pub due: Option<NaiveDate>,
    /// Resolution / close date.
    pub resolved: Option<NaiveDate>,
    pub assignee: Option<String>,
    /// Parent issue key.
    pub parent: Option<String>,
    /// Remote `updated` timestamp, verbatim.
    pub updated: String,
    /// Description as markdown (not yet neutralised).
    pub description: String,
    /// Comments, oldest first.
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Comment {
    pub author: String,
    /// RFC 3339 timestamp, verbatim.
    pub created: String,
    /// Markdown body (not yet neutralised).
    pub body: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SourceError {
    #[error("offline ({0})")]
    Offline(String),
    #[error("unauthorized ({status})")]
    Unauthorized { status: u16 },
    #[error("rate limited")]
    RateLimited,
    #[error("remote error {status}: {excerpt}")]
    Remote { status: u16, excerpt: String },
    #[error("unexpected response: {0}")]
    Parse(String),
    /// The remote can't do what was asked (e.g. nest a sub-task).
    #[error("{0}")]
    Refused(String),
}

/// Fields for a new remote issue (promote).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NewIssue {
    pub title: String,
    pub due: Option<NaiveDate>,
    pub priority: Option<Priority>,
}

/// An issue to look up by key. `url` (the note's `ft.source.url`) lets
/// backends whose keys aren't reversible (GitHub) find the issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRef {
    pub key: String,
    pub url: Option<String>,
}

impl IssueRef {
    pub fn key(key: impl Into<String>) -> Self {
        IssueRef {
            key: key.into(),
            url: None,
        }
    }
}

/// A backend that can list issues. Implementations are blocking and spawn
/// no threads.
pub trait TaskSource {
    /// All issues matching a backend query (JQL, GitHub search).
    fn fetch_query(&self, query: &str) -> Result<Vec<RemoteIssue>, SourceError>;
    /// The issues with these keys; keys that no longer exist (or aren't
    /// visible) are simply absent from the result.
    fn fetch_keys(&self, refs: &[IssueRef]) -> Result<Vec<RemoteIssue>, SourceError>;
    /// Move the issue to `target` (Jira transition, GitHub close/reopen).
    /// Checks the current remote status first.
    fn push_status(
        &self,
        issue: &IssueRef,
        target: Status,
        transitions: &BTreeMap<Status, String>,
    ) -> Result<PushOutcome, SourceError>;
    /// Create an issue as a child of `parent`, assigned to the
    /// authenticated user, and return it as fetched.
    fn create_child(&self, parent: &IssueRef, new: &NewIssue) -> Result<RemoteIssue, SourceError>;
}

/// Result of one status push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushOutcome {
    Pushed,
    /// The remote already had the target status.
    AlreadyThere,
    /// Nothing was sent; the reason says why (no transition, not found…).
    NotPushable(String),
}

/// Everything one sync of one source fetched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchResult {
    /// Issues returned by at least one query.
    pub active: Vec<RemoteIssue>,
    /// Ancestors of active issues that aren't active themselves.
    pub context: Vec<RemoteIssue>,
    /// Previously known issues that are neither, fetched by key.
    pub rechecked: Vec<RemoteIssue>,
    /// Previously known keys the remote no longer returns at all.
    pub missing: Vec<String>,
}

/// How far up a parent chain [`fetch_all`] follows.
const MAX_PARENT_DEPTH: usize = 5;

/// Run every query, then (optionally) the parent chains of the results,
/// then re-check `known` (issues the vault already has notes for) that
/// neither returned.
pub fn fetch_all(
    source: &dyn TaskSource,
    queries: &BTreeMap<String, String>,
    fetch_parents: bool,
    known: &[IssueRef],
) -> Result<FetchResult, SourceError> {
    let mut active: BTreeMap<String, RemoteIssue> = BTreeMap::new();
    for query in queries.values() {
        for issue in source.fetch_query(query)? {
            active.entry(issue.key.clone()).or_insert(issue);
        }
    }

    let mut context: BTreeMap<String, RemoteIssue> = BTreeMap::new();
    if fetch_parents {
        let mut frontier: BTreeSet<String> = active
            .values()
            .filter_map(|i| i.parent.clone())
            .filter(|k| !active.contains_key(k))
            .collect();
        for _ in 0..MAX_PARENT_DEPTH {
            if frontier.is_empty() {
                break;
            }
            let refs: Vec<IssueRef> = frontier
                .iter()
                .map(|k| {
                    known
                        .iter()
                        .find(|r| &r.key == k)
                        .cloned()
                        .unwrap_or_else(|| IssueRef::key(k.clone()))
                })
                .collect();
            let fetched = source.fetch_keys(&refs)?;
            frontier = fetched
                .iter()
                .filter_map(|i| i.parent.clone())
                .filter(|k| !active.contains_key(k) && !context.contains_key(k))
                .collect();
            for issue in fetched {
                context.entry(issue.key.clone()).or_insert(issue);
            }
            frontier.retain(|k| !context.contains_key(k));
        }
    }

    let mut unseen: Vec<IssueRef> = known
        .iter()
        .filter(|r| !active.contains_key(&r.key) && !context.contains_key(&r.key))
        .cloned()
        .collect();
    unseen.sort_by(|a, b| a.key.cmp(&b.key));
    unseen.dedup_by(|a, b| a.key == b.key);
    let rechecked = if unseen.is_empty() {
        Vec::new()
    } else {
        source.fetch_keys(&unseen)?
    };
    let missing = unseen
        .into_iter()
        .map(|r| r.key)
        .filter(|k| !rechecked.iter().any(|i| &i.key == k))
        .collect();

    Ok(FetchResult {
        active: active.into_values().collect(),
        context: context.into_values().collect(),
        rechecked,
        missing,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;

    /// In-memory source for planner and flow tests.
    #[derive(Default)]
    pub(crate) struct FakeSource {
        pub issues: Vec<RemoteIssue>,
        /// query string → keys it returns
        pub queries: BTreeMap<String, Vec<String>>,
        pub calls: RefCell<Vec<String>>,
    }

    pub(crate) fn issue(key: &str, parent: Option<&str>) -> RemoteIssue {
        RemoteIssue {
            key: key.into(),
            url: format!("https://example.test/browse/{key}"),
            title: format!("Title of {key}"),
            status_name: "To Do".into(),
            parent: parent.map(str::to_string),
            updated: "2026-10-21T09:00:00.000+0000".into(),
            ..Default::default()
        }
    }

    impl TaskSource for FakeSource {
        fn fetch_query(&self, query: &str) -> Result<Vec<RemoteIssue>, SourceError> {
            self.calls.borrow_mut().push(format!("query:{query}"));
            let keys = self.queries.get(query).cloned().unwrap_or_default();
            Ok(self
                .issues
                .iter()
                .filter(|i| keys.contains(&i.key))
                .cloned()
                .collect())
        }
        fn push_status(
            &self,
            issue: &IssueRef,
            target: Status,
            _transitions: &BTreeMap<Status, String>,
        ) -> Result<PushOutcome, SourceError> {
            self.calls
                .borrow_mut()
                .push(format!("push:{}:{}", issue.key, target.as_str()));
            Ok(PushOutcome::Pushed)
        }

        fn create_child(
            &self,
            parent: &IssueRef,
            new: &NewIssue,
        ) -> Result<RemoteIssue, SourceError> {
            self.calls
                .borrow_mut()
                .push(format!("create:{}:{}", parent.key, new.title));
            Ok(RemoteIssue {
                key: "NEW-1".into(),
                title: new.title.clone(),
                parent: Some(parent.key.clone()),
                ..issue("NEW-1", Some(&parent.key))
            })
        }

        fn fetch_keys(&self, refs: &[IssueRef]) -> Result<Vec<RemoteIssue>, SourceError> {
            let keys: Vec<String> = refs.iter().map(|r| r.key.clone()).collect();
            self.calls
                .borrow_mut()
                .push(format!("keys:{}", keys.join(",")));
            Ok(self
                .issues
                .iter()
                .filter(|i| keys.contains(&i.key))
                .cloned()
                .collect())
        }
    }

    #[test]
    fn fetch_all_classifies_active_context_rechecked_missing() {
        let src = FakeSource {
            issues: vec![
                issue("A-1", Some("E-1")),
                issue("A-2", Some("A-1")),
                issue("E-1", Some("I-1")),
                issue("I-1", None),
                issue("OLD-1", None),
            ],
            queries: [
                ("q1".to_string(), vec!["A-1".to_string()]),
                ("q2".to_string(), vec!["A-1".to_string(), "A-2".to_string()]),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let queries = [
            ("a".to_string(), "q1".to_string()),
            ("b".to_string(), "q2".to_string()),
        ]
        .into_iter()
        .collect();
        let known = ["OLD-1", "GONE-1", "A-1", "E-1"].map(IssueRef::key);
        let r = fetch_all(&src, &queries, true, &known).unwrap();
        let keys = |v: &[RemoteIssue]| v.iter().map(|i| i.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&r.active), ["A-1", "A-2"]);
        assert_eq!(keys(&r.context), ["E-1", "I-1"]);
        assert_eq!(keys(&r.rechecked), ["OLD-1"]);
        assert_eq!(r.missing, ["GONE-1"]);
    }

    #[test]
    fn fetch_all_without_parents_skips_context() {
        let src = FakeSource {
            issues: vec![issue("A-1", Some("E-1")), issue("E-1", None)],
            queries: [("q".to_string(), vec!["A-1".to_string()])]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let queries = [("mine".to_string(), "q".to_string())]
            .into_iter()
            .collect();
        let r = fetch_all(&src, &queries, false, &[]).unwrap();
        assert!(r.context.is_empty());
        assert_eq!(src.calls.borrow().len(), 1);
    }
}

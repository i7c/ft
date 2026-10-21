//! GitHub issues: search (`Link` pagination, pull requests excluded),
//! per-issue lookups, comments, and the sub-issue parent. Keys are
//! `<owner>-<repo>-<number>`, normalised so they work as `🆔` and file
//! stems; lookups go through the issue URL because keys aren't reversible.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use chrono::NaiveDate;
use serde_json::Value;

use super::http::{Http, Secret};
use super::{Comment, IssueRef, PushOutcome, RemoteIssue, SourceError, TaskSource};
use crate::task::Status;

const ACCEPT: &str = "application/vnd.github+json";

pub struct GithubSource {
    api: String,
    http: Http,
    status_map: BTreeMap<String, Status>,
    /// Raw issues seen this run (search results, parents), by key.
    seen: RefCell<HashMap<String, Value>>,
}

/// `owner-repo-42`: lower-cased, anything outside `[a-z0-9-]` → `-`.
pub fn normalise_key(owner: &str, repo: &str, number: u64) -> String {
    format!("{owner}-{repo}-{number}")
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// `(owner, repo, number)` from an issue's html or API URL.
fn parse_issue_url(url: &str) -> Option<(String, String, u64)> {
    let parts: Vec<&str> = url.trim_end_matches('/').rsplit('/').take(4).collect();
    // [number, "issues", repo, owner]
    match parts.as_slice() {
        [n, "issues", repo, owner] => Some((owner.to_string(), repo.to_string(), n.parse().ok()?)),
        _ => None,
    }
}

/// The `rel="next"` target of a `Link` header.
fn next_link(link: Option<&str>) -> Option<String> {
    link?.split(',').find_map(|part| {
        let (url, rel) = part.split_once(';')?;
        rel.contains("rel=\"next\"").then(|| {
            url.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string()
        })
    })
}

impl GithubSource {
    pub fn new(
        base_url: Option<&str>,
        token: &Secret,
        status_map: BTreeMap<String, Status>,
    ) -> Self {
        GithubSource {
            api: base_url
                .unwrap_or("https://api.github.com")
                .trim_end_matches('/')
                .to_string(),
            http: Http::new(Secret::new(format!("Bearer {}", token.expose()))),
            status_map,
            seen: RefCell::new(HashMap::new()),
        }
    }

    /// The token's login (`/user`), to verify a token.
    pub fn whoami(&self) -> Result<String, SourceError> {
        let r = self.http.get_json(&format!("{}/user", self.api), ACCEPT)?;
        Ok(r.json["login"].as_str().unwrap_or("").to_string())
    }

    fn repo_of(raw: &Value) -> Option<(String, String)> {
        // repository_url: https://api.github.com/repos/<owner>/<repo>
        let url = raw["repository_url"].as_str()?;
        let mut it = url.rsplit('/');
        let repo = it.next()?.to_string();
        let owner = it.next()?.to_string();
        Some((owner, repo))
    }

    fn key_of(raw: &Value) -> Option<String> {
        let (owner, repo) = Self::repo_of(raw)?;
        Some(normalise_key(&owner, &repo, raw["number"].as_u64()?))
    }

    fn complete(&self, raw: Value) -> Result<RemoteIssue, SourceError> {
        let (owner, repo) = Self::repo_of(&raw)
            .ok_or_else(|| SourceError::Parse("issue without repository_url".into()))?;
        let number = raw["number"].as_u64().unwrap_or(0);
        let base = format!("{}/repos/{owner}/{repo}/issues/{number}", self.api);
        let comments = match self
            .http
            .get_json(&format!("{base}/comments?per_page=100"), ACCEPT)
        {
            Ok(r) => r.json.as_array().cloned().unwrap_or_default(),
            Err(SourceError::Remote { status: 404, .. }) => Vec::new(),
            Err(e) => return Err(e),
        };
        let parent = match self.http.get_json(&format!("{base}/parent"), ACCEPT) {
            Ok(r) => {
                let key = Self::key_of(&r.json);
                if let Some(k) = &key {
                    self.seen.borrow_mut().insert(k.clone(), r.json);
                }
                key
            }
            // No parent (404) or sub-issues unavailable on this host.
            Err(SourceError::Remote { .. }) => None,
            Err(e) => return Err(e),
        };
        let mut issue = map_issue(&raw, &self.status_map)?;
        issue.parent = parent;
        issue.comments = map_comments(&comments);
        Ok(issue)
    }
}

impl TaskSource for GithubSource {
    fn push_status(
        &self,
        issue: &IssueRef,
        target: Status,
        _transitions: &BTreeMap<Status, String>,
    ) -> Result<PushOutcome, SourceError> {
        let Some((owner, repo, n)) = issue.url.as_deref().and_then(parse_issue_url) else {
            return Ok(PushOutcome::NotPushable("no issue URL in the note".into()));
        };
        let url = format!("{}/repos/{owner}/{repo}/issues/{n}", self.api);
        let raw = match self.http.get_json(&url, ACCEPT) {
            Ok(r) => r.json,
            Err(SourceError::Remote {
                status: 404 | 410, ..
            }) => return Ok(PushOutcome::NotPushable("issue not found".into())),
            Err(e) => return Err(e),
        };
        let current = map_issue(&raw, &self.status_map)?.status;
        let open = |s: Status| matches!(s, Status::Open | Status::InProgress);
        if current == target || (open(current) && open(target)) {
            return Ok(PushOutcome::AlreadyThere);
        }
        let body = match target {
            Status::Done => serde_json::json!({"state": "closed", "state_reason": "completed"}),
            Status::Cancelled => {
                serde_json::json!({"state": "closed", "state_reason": "not_planned"})
            }
            Status::Open | Status::InProgress => serde_json::json!({"state": "open"}),
        };
        match self.http.patch_json(&url, ACCEPT, &body) {
            Ok(_) => Ok(PushOutcome::Pushed),
            Err(SourceError::Remote { status, excerpt })
                if status == 404 || status == 410 || status == 422 =>
            {
                Ok(PushOutcome::NotPushable(format!(
                    "rejected ({status}): {excerpt}"
                )))
            }
            Err(e) => Err(e),
        }
    }

    fn fetch_query(&self, query: &str) -> Result<Vec<RemoteIssue>, SourceError> {
        let wants_prs = query.contains("is:pr") || query.contains("type:pr");
        let mut url = Some(format!(
            "{}/search/issues?q={}&per_page=100",
            self.api,
            urlencoding::encode(query)
        ));
        let mut raws = Vec::new();
        while let Some(u) = url {
            let page = self.http.get_json(&u, ACCEPT)?;
            let items = page.json["items"]
                .as_array()
                .ok_or_else(|| SourceError::Parse("search response has no `items`".into()))?;
            raws.extend(
                items
                    .iter()
                    .filter(|i| wants_prs || i.get("pull_request").is_none())
                    .cloned(),
            );
            url = next_link(page.link.as_deref());
        }
        let mut out = Vec::with_capacity(raws.len());
        for raw in raws {
            if let Some(k) = Self::key_of(&raw) {
                self.seen.borrow_mut().insert(k, raw.clone());
            }
            out.push(self.complete(raw)?);
        }
        Ok(out)
    }

    fn fetch_keys(&self, refs: &[IssueRef]) -> Result<Vec<RemoteIssue>, SourceError> {
        let mut out = Vec::new();
        for r in refs {
            let cached = self.seen.borrow().get(&r.key).cloned();
            let raw = match cached {
                Some(raw) => raw,
                None => {
                    let Some((owner, repo, n)) = r.url.as_deref().and_then(parse_issue_url) else {
                        continue;
                    };
                    match self.http.get_json(
                        &format!("{}/repos/{owner}/{repo}/issues/{n}", self.api),
                        ACCEPT,
                    ) {
                        Ok(resp) => resp.json,
                        Err(SourceError::Remote {
                            status: 404 | 410, ..
                        }) => continue,
                        Err(e) => return Err(e),
                    }
                }
            };
            out.push(self.complete(raw)?);
        }
        Ok(out)
    }
}

/// Map an issue object (without comments / parent).
pub fn map_issue(
    raw: &Value,
    status_map: &BTreeMap<String, Status>,
) -> Result<RemoteIssue, SourceError> {
    let key = GithubSource::key_of(raw)
        .ok_or_else(|| SourceError::Parse("issue without repository/number".into()))?;
    let state = raw["state"].as_str().unwrap_or("open");
    let reason = raw["state_reason"].as_str();
    let status_name = match (state, reason) {
        ("closed", Some(r)) => format!("closed:{r}"),
        (s, _) => s.to_string(),
    };
    let status = status_map
        .get(&status_name)
        .copied()
        .unwrap_or(match (state, reason) {
            ("closed", Some("not_planned")) => Status::Cancelled,
            ("closed", _) => Status::Done,
            _ => Status::Open,
        });
    Ok(RemoteIssue {
        url: raw["html_url"].as_str().unwrap_or("").to_string(),
        title: raw["title"].as_str().unwrap_or("").to_string(),
        status_name,
        status,
        priority: None,
        due: None,
        resolved: raw["closed_at"]
            .as_str()
            .and_then(|s| NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()),
        assignee: raw["assignees"][0]["login"].as_str().map(str::to_string),
        parent: None,
        updated: raw["updated_at"].as_str().unwrap_or("").to_string(),
        description: raw["body"].as_str().unwrap_or("").replace("\r\n", "\n"),
        comments: Vec::new(),
        key,
    })
}

fn map_comments(raw: &[Value]) -> Vec<Comment> {
    let mut out: Vec<Comment> = raw
        .iter()
        .map(|c| Comment {
            author: c["user"]["login"].as_str().unwrap_or("unknown").to_string(),
            created: c["created_at"].as_str().unwrap_or("").to_string(),
            body: c["body"].as_str().unwrap_or("").replace("\r\n", "\n"),
        })
        .collect();
    out.sort_by(|a, b| (&a.created, &a.author).cmp(&(&b.created, &b.author)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::http::tests::{ok_json, serve};
    use serde_json::json;

    fn raw(owner: &str, repo: &str, n: u64, state: &str, reason: Option<&str>) -> Value {
        json!({
            "number": n,
            "title": "Crash on start",
            "state": state,
            "state_reason": reason,
            "closed_at": if state == "closed" { json!("2026-10-20T12:00:00Z") } else { Value::Null },
            "html_url": format!("https://github.com/{owner}/{repo}/issues/{n}"),
            "repository_url": format!("https://api.github.com/repos/{owner}/{repo}"),
            "updated_at": "2026-10-21T08:00:00Z",
            "body": "line1\r\n- [ ] sub",
            "assignees": [{"login": "octo"}],
        })
    }

    #[test]
    fn key_normalisation_and_url_parsing() {
        assert_eq!(
            normalise_key("My-Org", "Some.Repo", 42),
            "my-org-some-repo-42"
        );
        assert_eq!(
            parse_issue_url("https://github.com/My-Org/Some.Repo/issues/42"),
            Some(("My-Org".into(), "Some.Repo".into(), 42))
        );
        assert_eq!(parse_issue_url("https://github.com/o/r/pull/1"), None);
        assert_eq!(
            next_link(Some(
                "<https://x.test/a?page=2>; rel=\"next\", <https://x.test/a?page=5>; rel=\"last\""
            )),
            Some("https://x.test/a?page=2".into())
        );
    }

    #[test]
    fn maps_states() {
        let none = BTreeMap::new();
        let open = map_issue(&raw("o", "r", 1, "open", None), &none).unwrap();
        assert_eq!(
            (open.status, open.status_name.as_str()),
            (Status::Open, "open")
        );
        assert_eq!(open.description, "line1\n- [ ] sub");
        assert_eq!(open.assignee.as_deref(), Some("octo"));
        let done = map_issue(&raw("o", "r", 1, "closed", Some("completed")), &none).unwrap();
        assert_eq!(
            (done.status, done.resolved),
            (Status::Done, NaiveDate::from_ymd_opt(2026, 10, 20))
        );
        let np = map_issue(&raw("o", "r", 1, "closed", Some("not_planned")), &none).unwrap();
        assert_eq!(
            (np.status, np.status_name.as_str()),
            (Status::Cancelled, "closed:not_planned")
        );
        let custom = [("closed:duplicate".to_string(), Status::Cancelled)]
            .into_iter()
            .collect();
        assert_eq!(
            map_issue(&raw("o", "r", 1, "closed", Some("duplicate")), &custom)
                .unwrap()
                .status,
            Status::Cancelled
        );
    }

    #[test]
    fn search_paginates_excludes_prs_and_completes_issues() {
        let mut pr = raw("o", "r", 9, "open", None);
        pr["pull_request"] = json!({});
        let p1 = json!({"items": [raw("o", "r", 1, "open", None), pr]}).to_string();
        let parent = raw("o", "r", 7, "open", None).to_string();
        let comments = json!([
            {"user": {"login": "b"}, "created_at": "2026-10-21T00:00:00Z", "body": "later"},
            {"user": {"login": "a"}, "created_at": "2026-10-20T00:00:00Z", "body": "earlier"}
        ])
        .to_string();
        let not_found =
            "HTTP/1.1 404 NF\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_string();
        let (url, h) = serve(vec![
            ok_json(&p1),
            ok_json(&comments),
            ok_json(&parent),
            // fetch_keys for the parent: served from the cache → only
            // comments + parent lookups hit the wire.
            ok_json("[]"),
            not_found,
        ]);
        // NOT VALID — test data only.
        let src = GithubSource::new(Some(&url), &Secret::new("FAKE_TEST_token"), BTreeMap::new());
        let issues = src.fetch_query("is:issue is:open assignee:@me").unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, "o-r-1");
        assert_eq!(issues[0].parent.as_deref(), Some("o-r-7"));
        assert_eq!(
            issues[0]
                .comments
                .iter()
                .map(|c| c.body.as_str())
                .collect::<Vec<_>>(),
            ["earlier", "later"]
        );
        let parents = src.fetch_keys(&[IssueRef::key("o-r-7")]).unwrap();
        assert_eq!(parents[0].key, "o-r-7");
        assert_eq!(parents[0].parent, None);
        let reqs = h.join().unwrap();
        assert!(reqs[0].starts_with(
            "GET /search/issues?q=is%3Aissue%20is%3Aopen%20assignee%3A%40me&per_page=100"
        ));
        assert!(reqs[0]
            .to_lowercase()
            .contains("authorization: bearer fake_test_token"));
    }

    #[test]
    fn fetch_keys_by_url_and_404_absent() {
        let not_found =
            "HTTP/1.1 404 NF\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_string();
        let (url, _h) = serve(vec![not_found]);
        // NOT VALID — test data only.
        let src = GithubSource::new(Some(&url), &Secret::new("FAKE_TEST_token"), BTreeMap::new());
        let refs = [
            IssueRef {
                key: "o-r-3".into(),
                url: Some("https://github.com/o/r/issues/3".into()),
            },
            IssueRef::key("no-url-1"),
        ];
        assert!(src.fetch_keys(&refs).unwrap().is_empty());
    }

    #[test]
    fn push_status_closes_and_reopens() {
        use crate::sources::http::tests::body_json;
        let open = raw("o", "r", 3, "open", None).to_string();
        let closed = raw("o", "r", 3, "closed", Some("completed")).to_string();
        let (url, h) = serve(vec![
            ok_json(&open),
            ok_json("{}"),
            ok_json(&closed),
            ok_json("{}"),
            ok_json(&open),
        ]);
        // NOT VALID — test data only.
        let src = GithubSource::new(Some(&url), &Secret::new("FAKE_TEST_token"), BTreeMap::new());
        let r = IssueRef {
            key: "o-r-3".into(),
            url: Some("https://github.com/o/r/issues/3".into()),
        };
        assert_eq!(
            src.push_status(&r, Status::Cancelled, &BTreeMap::new())
                .unwrap(),
            PushOutcome::Pushed
        );
        assert_eq!(
            src.push_status(&r, Status::InProgress, &BTreeMap::new())
                .unwrap(),
            PushOutcome::Pushed
        );
        assert_eq!(
            src.push_status(&r, Status::Open, &BTreeMap::new()).unwrap(),
            PushOutcome::AlreadyThere
        );
        let reqs = h.join().unwrap();
        assert!(reqs[1].starts_with("PATCH /repos/o/r/issues/3 "));
        assert_eq!(
            body_json(&reqs[1]),
            json!({"state": "closed", "state_reason": "not_planned"})
        );
        assert_eq!(body_json(&reqs[3]), json!({"state": "open"}));
    }
}

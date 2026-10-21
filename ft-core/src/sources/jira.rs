//! Jira Cloud (REST v3): token-paginated JQL search, key lookups, and the
//! field mapping onto [`RemoteIssue`].

use std::collections::BTreeMap;

use base64::Engine;
use chrono::NaiveDate;
use serde_json::Value;

use super::http::{Http, Secret};
use super::{adf, Comment, IssueRef, NewIssue, PushOutcome, RemoteIssue, SourceError, TaskSource};
use crate::task::{Priority, Status};

/// Fields requested from the search endpoint — exactly what's mapped.
const FIELDS: &str =
    "summary,status,priority,duedate,resolution,resolutiondate,assignee,parent,updated,description,comment";
const PAGE: usize = 100;

/// `done`-category status names that mean "won't be done".
const CANCELLED_STATUSES: &[&str] = &[
    "Canceled",
    "Cancelled",
    "Abandoned",
    "Won't Do",
    "Won't Fix",
    "Rejected",
    "Declined",
];

/// Resolutions that mean "won't be done" for a `done`-category status.
const CANCELLED_RESOLUTIONS: &[&str] = &[
    "Won't Do",
    "Won't Fix",
    "Duplicate",
    "Cannot Reproduce",
    "Declined",
];

pub struct JiraSource {
    base_url: String,
    http: Http,
    status_map: BTreeMap<String, Status>,
}

impl JiraSource {
    pub fn new(
        base_url: &str,
        email: &str,
        token: &Secret,
        status_map: BTreeMap<String, Status>,
    ) -> Self {
        let basic =
            base64::engine::general_purpose::STANDARD.encode(format!("{email}:{}", token.expose()));
        JiraSource {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: Http::new(Secret::new(format!("Basic {basic}"))),
            status_map,
        }
    }

    /// The authenticated user's display name (`/rest/api/3/myself`); used
    /// to verify a token before storing it.
    pub fn whoami(&self) -> Result<String, SourceError> {
        let r = self.http.get_json(
            &format!("{}/rest/api/3/myself", self.base_url),
            "application/json",
        )?;
        Ok(r.json["displayName"].as_str().unwrap_or("").to_string())
    }

    fn search(&self, jql: &str) -> Result<Vec<RemoteIssue>, SourceError> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/rest/api/3/search/jql?jql={}&fields={FIELDS}&maxResults={PAGE}",
                self.base_url,
                urlencoding::encode(jql)
            );
            if let Some(t) = &token {
                url.push_str(&format!("&nextPageToken={}", urlencoding::encode(t)));
            }
            let page = self.http.get_json(&url, "application/json")?.json;
            let issues = page["issues"].as_array().ok_or_else(|| {
                SourceError::Parse("search response has no `issues` array".into())
            })?;
            for raw in issues {
                out.push(map_issue(raw, &self.base_url, &self.status_map)?);
            }
            token = page["nextPageToken"].as_str().map(str::to_string);
            if token.is_none() || page["isLast"].as_bool() == Some(true) {
                return Ok(out);
            }
        }
    }
}

/// One available workflow transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub id: String,
    pub name: String,
    /// Target status name.
    pub to: String,
    /// Target status category key (`new`, `indeterminate`, `done`).
    pub category: String,
}

/// Choose the transition that moves an issue to `target`: the configured
/// name when there is one, otherwise the unique transition into the
/// target's status category (preferring the category's default status
/// name). `Cancelled` is only ever pushed through a configured name.
pub fn select_transition<'a>(
    available: &'a [Transition],
    target: Status,
    overrides: &BTreeMap<Status, String>,
) -> Result<&'a Transition, String> {
    if let Some(name) = overrides.get(&target) {
        return available
            .iter()
            .find(|t| &t.name == name)
            .ok_or_else(|| format!("no transition \"{name}\" available"));
    }
    let (category, default_name) = match target {
        Status::Open => ("new", "To Do"),
        Status::InProgress => ("indeterminate", "In Progress"),
        Status::Done => ("done", "Done"),
        Status::Cancelled => return Err("cancel isn't pushed — set transitions.Cancelled".into()),
    };
    let matching: Vec<&Transition> = available
        .iter()
        .filter(|t| t.category == category)
        .collect();
    match matching.as_slice() {
        [] => Err(format!("no transition into a {category} status available")),
        [one] => Ok(one),
        many => {
            let preferred: Vec<&&Transition> =
                many.iter().filter(|t| t.to == default_name).collect();
            match preferred.as_slice() {
                [one] => Ok(one),
                _ => Err(format!(
                    "ambiguous transitions: {} — set transitions.{}",
                    many.iter()
                        .map(|t| format!("\"{}\"", t.name))
                        .collect::<Vec<_>>()
                        .join(", "),
                    target.as_str()
                )),
            }
        }
    }
}

impl JiraSource {
    /// The issue's current state, or `None` when it doesn't exist.
    fn current(&self, key: &str) -> Result<Option<RemoteIssue>, SourceError> {
        let url = format!(
            "{}/rest/api/3/issue/{}?fields={FIELDS}",
            self.base_url,
            urlencoding::encode(key)
        );
        match self.http.get_json(&url, "application/json") {
            Ok(r) => Ok(Some(map_issue(&r.json, &self.base_url, &self.status_map)?)),
            Err(SourceError::Remote { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn transitions(&self, key: &str) -> Result<Vec<Transition>, SourceError> {
        let url = format!(
            "{}/rest/api/3/issue/{}/transitions",
            self.base_url,
            urlencoding::encode(key)
        );
        let r = self.http.get_json(&url, "application/json")?;
        Ok(r.json["transitions"]
            .as_array()
            .map(|ts| {
                ts.iter()
                    .map(|t| Transition {
                        id: t["id"].as_str().unwrap_or("").to_string(),
                        name: t["name"].as_str().unwrap_or("").to_string(),
                        to: t["to"]["name"].as_str().unwrap_or("").to_string(),
                        category: t["to"]["statusCategory"]["key"]
                            .as_str()
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

impl TaskSource for JiraSource {
    fn create_child(&self, parent: &IssueRef, new: &NewIssue) -> Result<RemoteIssue, SourceError> {
        let base = &self.base_url;
        let meta = self
            .http
            .get_json(
                &format!(
                    "{base}/rest/api/3/issue/{}?fields=issuetype,project",
                    urlencoding::encode(&parent.key)
                ),
                "application/json",
            )?
            .json;
        let level = meta["fields"]["issuetype"]["hierarchyLevel"]
            .as_i64()
            .unwrap_or(0);
        if meta["fields"]["issuetype"]["subtask"].as_bool() == Some(true) || level < 0 {
            return Err(SourceError::Refused(format!(
                "{} is a sub-task; Jira can't nest sub-tasks",
                parent.key
            )));
        }
        let project = meta["fields"]["project"]["key"]
            .as_str()
            .ok_or_else(|| SourceError::Parse("parent without project".into()))?
            .to_string();
        let types = self
            .http
            .get_json(
                &format!(
                    "{base}/rest/api/3/issue/createmeta/{}/issuetypes",
                    urlencoding::encode(&project)
                ),
                "application/json",
            )?
            .json;
        let types = types["issueTypes"]
            .as_array()
            .or_else(|| types["values"].as_array())
            .cloned()
            .unwrap_or_default();
        let is_sub = |t: &Value| t["subtask"].as_bool() == Some(true);
        let chosen = if level >= 1 {
            types
                .iter()
                .find(|t| !is_sub(t) && t["name"].as_str() == Some("Task"))
                .or_else(|| {
                    types
                        .iter()
                        .find(|t| !is_sub(t) && t["hierarchyLevel"].as_i64().unwrap_or(0) == 0)
                })
        } else {
            types.iter().find(|t| is_sub(t))
        }
        .ok_or_else(|| {
            SourceError::Refused(format!("no suitable issue type in project {project}"))
        })?;
        let me = self
            .http
            .get_json(&format!("{base}/rest/api/3/myself"), "application/json")?
            .json;
        let fields = serde_json::json!({
            "project": {"key": project},
            "parent": {"key": parent.key},
            "summary": new.title,
            "issuetype": {"id": chosen["id"]},
            "assignee": {"accountId": me["accountId"]},
        });
        let mut optional = serde_json::Map::new();
        if let Some(d) = new.due {
            optional.insert("duedate".into(), Value::String(d.to_string()));
        }
        if let Some(p) = new.priority {
            optional.insert("priority".into(), serde_json::json!({"name": p.as_str()}));
        }
        let mut with_optional = fields.clone();
        if let Some(obj) = with_optional.as_object_mut() {
            obj.extend(optional.clone());
        }
        let url = format!("{base}/rest/api/3/issue");
        let created = match self.http.post_json(
            &url,
            "application/json",
            &serde_json::json!({"fields": with_optional}),
        ) {
            // Due date / priority may not be on the project's create screen.
            Err(SourceError::Remote { status: 400, .. }) if !optional.is_empty() => {
                self.http.post_json(
                    &url,
                    "application/json",
                    &serde_json::json!({"fields": fields}),
                )?
            }
            other => other?,
        };
        let key = created.json["key"]
            .as_str()
            .ok_or_else(|| SourceError::Parse("create response without key".into()))?;
        self.current(key)?
            .ok_or_else(|| SourceError::Parse(format!("created {key} but can't read it back")))
    }

    fn push_status(
        &self,
        issue: &IssueRef,
        target: Status,
        transitions: &BTreeMap<Status, String>,
    ) -> Result<PushOutcome, SourceError> {
        let Some(current) = self.current(&issue.key)? else {
            return Ok(PushOutcome::NotPushable("issue not found".into()));
        };
        if current.status == target {
            return Ok(PushOutcome::AlreadyThere);
        }
        let available = self.transitions(&issue.key)?;
        let chosen = match select_transition(&available, target, transitions) {
            Ok(t) => t,
            Err(reason) => {
                return Ok(PushOutcome::NotPushable(format!(
                    "{reason} (from {})",
                    current.status_name
                )))
            }
        };
        let url = format!(
            "{}/rest/api/3/issue/{}/transitions",
            self.base_url,
            urlencoding::encode(&issue.key)
        );
        match self.http.post_json(
            &url,
            "application/json",
            &serde_json::json!({"transition": {"id": chosen.id}}),
        ) {
            Ok(_) => Ok(PushOutcome::Pushed),
            // A 403 on a write is a per-issue permission, not bad credentials.
            Err(SourceError::Unauthorized { status: 403 }) => {
                Ok(PushOutcome::NotPushable("forbidden (403)".into()))
            }
            Err(SourceError::Remote { status, excerpt })
                if status == 400 || status == 404 || status == 409 =>
            {
                Ok(PushOutcome::NotPushable(format!(
                    "rejected ({status}): {excerpt}"
                )))
            }
            Err(e) => Err(e),
        }
    }

    fn fetch_query(&self, query: &str) -> Result<Vec<RemoteIssue>, SourceError> {
        self.search(query)
    }

    fn fetch_keys(&self, refs: &[IssueRef]) -> Result<Vec<RemoteIssue>, SourceError> {
        let keys: Vec<String> = refs.iter().map(|r| r.key.clone()).collect();
        let mut out = Vec::new();
        for chunk in keys.chunks(PAGE) {
            let quoted: Vec<String> = chunk
                .iter()
                .map(|k| format!("\"{}\"", k.replace('"', "")))
                .collect();
            match self.search(&format!("key in ({})", quoted.join(","))) {
                Ok(found) => out.extend(found),
                // Jira rejects the whole JQL when a key doesn't exist; fall
                // back to one key at a time so the rest still resolve.
                Err(SourceError::Remote { status: 400, .. }) if chunk.len() > 1 => {
                    for k in chunk {
                        match self.search(&format!("key = \"{}\"", k.replace('"', ""))) {
                            Ok(found) => out.extend(found),
                            Err(SourceError::Remote {
                                status: 400 | 404, ..
                            }) => {}
                            Err(e) => return Err(e),
                        }
                    }
                }
                Err(SourceError::Remote {
                    status: 400 | 404, ..
                }) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }
}

/// Map one search-result issue.
pub fn map_issue(
    raw: &Value,
    base_url: &str,
    status_map: &BTreeMap<String, Status>,
) -> Result<RemoteIssue, SourceError> {
    let key = raw["key"]
        .as_str()
        .ok_or_else(|| SourceError::Parse("issue without `key`".into()))?
        .to_string();
    let f = &raw["fields"];
    let status_name = f["status"]["name"].as_str().unwrap_or("").to_string();
    let status = status_map.get(&status_name).copied().unwrap_or_else(|| {
        match f["status"]["statusCategory"]["key"].as_str() {
            Some("done") => {
                let resolution = f["resolution"]["name"].as_str().unwrap_or("");
                if CANCELLED_RESOLUTIONS.contains(&resolution)
                    || CANCELLED_STATUSES.contains(&status_name.as_str())
                {
                    Status::Cancelled
                } else {
                    Status::Done
                }
            }
            Some("indeterminate") => Status::InProgress,
            _ => Status::Open,
        }
    });
    let date = |v: &Value| {
        v.as_str()
            .and_then(|s| NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok())
    };
    let mut comments: Vec<Comment> = f["comment"]["comments"]
        .as_array()
        .map(|cs| {
            cs.iter()
                .map(|c| Comment {
                    author: c["author"]["displayName"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string(),
                    created: c["created"].as_str().unwrap_or("").to_string(),
                    body: adf::to_markdown(&c["body"]),
                })
                .collect()
        })
        .unwrap_or_default();
    comments.sort_by(|a, b| (&a.created, &a.author).cmp(&(&b.created, &b.author)));
    Ok(RemoteIssue {
        url: format!("{base_url}/browse/{key}"),
        title: f["summary"].as_str().unwrap_or("").to_string(),
        status_name,
        status,
        priority: match f["priority"]["name"].as_str() {
            Some("Highest") => Some(Priority::Highest),
            Some("High") => Some(Priority::High),
            Some("Medium") => Some(Priority::Medium),
            Some("Low") => Some(Priority::Low),
            Some("Lowest") => Some(Priority::Lowest),
            _ => None,
        },
        due: date(&f["duedate"]),
        resolved: date(&f["resolutiondate"]),
        assignee: f["assignee"]["displayName"].as_str().map(str::to_string),
        parent: f["parent"]["key"].as_str().map(str::to_string),
        updated: f["updated"].as_str().unwrap_or("").to_string(),
        description: adf::to_markdown(&f["description"]),
        comments,
        key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::http::tests::{ok_json, serve};
    use serde_json::json;

    fn raw_issue(key: &str, category: &str, status: &str) -> Value {
        json!({
            "id": "10001",
            "key": key,
            "fields": {
                "summary": "Fix login",
                "status": {"name": status, "statusCategory": {"key": category}},
                "priority": {"name": "High"},
                "duedate": "2026-10-30",
                "resolution": null,
                "resolutiondate": null,
                "assignee": {"displayName": "Jane Doe"},
                "parent": {"key": "EPIC-7"},
                "updated": "2026-10-21T09:12:44.000+0000",
                "description": {"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Steps"}]}]},
                "comment": {"comments": [
                    {"author": {"displayName": "Zed"}, "created": "2026-10-20T10:00:00.000+0000",
                     "body": {"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "second"}]}]}},
                    {"author": {"displayName": "Amy"}, "created": "2026-10-19T10:00:00.000+0000",
                     "body": {"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "first"}]}]}}
                ]}
            }
        })
    }

    #[test]
    fn maps_fields() {
        let i = map_issue(
            &raw_issue("ABC-123", "indeterminate", "In Review"),
            "https://x.test",
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(i.key, "ABC-123");
        assert_eq!(i.url, "https://x.test/browse/ABC-123");
        assert_eq!(i.title, "Fix login");
        assert_eq!(
            (i.status_name.as_str(), i.status),
            ("In Review", Status::InProgress)
        );
        assert_eq!(i.priority, Some(Priority::High));
        assert_eq!(i.due, NaiveDate::from_ymd_opt(2026, 10, 30));
        assert_eq!(i.parent.as_deref(), Some("EPIC-7"));
        assert_eq!(i.assignee.as_deref(), Some("Jane Doe"));
        assert_eq!(i.description, "Steps");
        assert_eq!(
            i.comments
                .iter()
                .map(|c| c.body.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }

    #[test]
    fn status_categories_resolutions_and_map() {
        let map = |cat: &str, name: &str, res: Option<&str>, m: &BTreeMap<String, Status>| {
            let mut r = raw_issue("A-1", cat, name);
            if let Some(res) = res {
                r["fields"]["resolution"] = json!({"name": res});
                r["fields"]["resolutiondate"] = json!("2026-10-20T08:00:00.000+0000");
            }
            map_issue(&r, "https://x.test", m).unwrap()
        };
        let none = BTreeMap::new();
        assert_eq!(map("new", "To Do", None, &none).status, Status::Open);
        let done = map("done", "Done", Some("Done"), &none);
        assert_eq!(
            (done.status, done.resolved),
            (Status::Done, NaiveDate::from_ymd_opt(2026, 10, 20))
        );
        assert_eq!(
            map("done", "Closed", Some("Won't Do"), &none).status,
            Status::Cancelled
        );
        // Cancel-like done statuses, whatever the resolution.
        assert_eq!(
            map("done", "Canceled", Some("Done"), &none).status,
            Status::Cancelled
        );
        assert_eq!(
            map("done", "Abandoned", None, &none).status,
            Status::Cancelled
        );
        let custom = [("Blocked".to_string(), Status::Open)]
            .into_iter()
            .collect();
        assert_eq!(
            map("indeterminate", "Blocked", None, &custom).status,
            Status::Open
        );
    }

    #[test]
    fn search_follows_next_page_token() {
        let p1 = json!({"issues": [raw_issue("A-1", "new", "To Do")], "nextPageToken": "tok2", "isLast": false});
        let p2 = json!({"issues": [raw_issue("A-2", "new", "To Do")], "isLast": true});
        let (url, h) = serve(vec![ok_json(&p1.to_string()), ok_json(&p2.to_string())]);
        // NOT VALID — test data only.
        let src = JiraSource::new(
            &url,
            "fake-user@example.test",
            &Secret::new("FAKE_TEST_token"),
            BTreeMap::new(),
        );
        let issues = src.fetch_query("assignee = currentUser()").unwrap();
        assert_eq!(
            issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(),
            ["A-1", "A-2"]
        );
        let reqs = h.join().unwrap();
        assert!(reqs[0].starts_with(
            "GET /rest/api/3/search/jql?jql=assignee%20%3D%20currentUser%28%29&fields="
        ));
        assert!(reqs[1].contains("&nextPageToken=tok2"));
        assert!(reqs[0].to_lowercase().contains("authorization: basic "));
    }

    #[test]
    fn fetch_keys_falls_back_per_key_on_400() {
        let bad =
            "HTTP/1.1 400 Bad\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_string();
        let one = json!({"issues": [raw_issue("A-1", "new", "To Do")], "isLast": true});
        let (url, _h) = serve(vec![bad.clone(), ok_json(&one.to_string()), bad]);
        // NOT VALID — test data only.
        let src = JiraSource::new(
            &url,
            "fake-user@example.test",
            &Secret::new("FAKE_TEST_token"),
            BTreeMap::new(),
        );
        let got = src
            .fetch_keys(&[IssueRef::key("A-1"), IssueRef::key("GONE-9")])
            .unwrap();
        assert_eq!(got.len(), 1);
    }

    fn t(id: &str, name: &str, to: &str, cat: &str) -> Transition {
        Transition {
            id: id.into(),
            name: name.into(),
            to: to.into(),
            category: cat.into(),
        }
    }

    #[test]
    fn transition_selection() {
        let none = BTreeMap::new();
        let avail = [
            t("11", "Start Progress", "In Progress", "indeterminate"),
            t("31", "Resolve", "Done", "done"),
            t("41", "Won't Do", "Closed", "done"),
        ];
        assert_eq!(
            select_transition(&avail, Status::InProgress, &none)
                .unwrap()
                .id,
            "11"
        );
        // Two done-category targets: the one into `Done` wins.
        assert_eq!(
            select_transition(&avail, Status::Done, &none).unwrap().id,
            "31"
        );
        assert!(select_transition(&avail, Status::Open, &none)
            .unwrap_err()
            .contains("no transition into a new"));
        assert!(select_transition(&avail, Status::Cancelled, &none)
            .unwrap_err()
            .contains("set transitions.Cancelled"));
        let o = [(Status::Cancelled, "Won't Do".to_string())]
            .into_iter()
            .collect();
        assert_eq!(
            select_transition(&avail, Status::Cancelled, &o).unwrap().id,
            "41"
        );
        let o = [(Status::Done, "Ship".to_string())].into_iter().collect();
        assert!(select_transition(&avail, Status::Done, &o)
            .unwrap_err()
            .contains("\"Ship\""));
        let amb = [
            t("1", "A", "Closed", "done"),
            t("2", "B", "Shipped", "done"),
        ];
        assert!(select_transition(&amb, Status::Done, &none)
            .unwrap_err()
            .contains("ambiguous"));
    }

    fn raw_with_status(category: &str, name: &str) -> Value {
        let mut r = raw_issue("ABC-123", category, name);
        r["fields"]["comment"] = json!({"comments": []});
        r
    }

    #[test]
    fn push_status_applies_transition() {
        use crate::sources::http::tests::body_json;
        let current = raw_with_status("indeterminate", "In Review").to_string();
        let transitions = json!({"transitions": [
            {"id": "31", "name": "Resolve", "to": {"name": "Done", "statusCategory": {"key": "done"}}}
        ]})
        .to_string();
        let no_content = "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_string();
        let (url, h) = serve(vec![ok_json(&current), ok_json(&transitions), no_content]);
        // NOT VALID — test data only.
        let src = JiraSource::new(
            &url,
            "fake-user@example.test",
            &Secret::new("FAKE_TEST_token"),
            BTreeMap::new(),
        );
        let out = src
            .push_status(&IssueRef::key("ABC-123"), Status::Done, &BTreeMap::new())
            .unwrap();
        assert_eq!(out, PushOutcome::Pushed);
        let reqs = h.join().unwrap();
        assert!(reqs[0].starts_with("GET /rest/api/3/issue/ABC-123?fields="));
        assert!(reqs[1].starts_with("GET /rest/api/3/issue/ABC-123/transitions"));
        assert!(reqs[2].starts_with("POST /rest/api/3/issue/ABC-123/transitions"));
        assert_eq!(body_json(&reqs[2]), json!({"transition": {"id": "31"}}));
    }

    #[test]
    fn push_status_already_there_and_not_pushable() {
        let done = raw_with_status("done", "Done").to_string();
        let open = raw_with_status("new", "To Do").to_string();
        let (url, h) = serve(vec![
            ok_json(&done),
            ok_json(&open),
            ok_json(r#"{"transitions": []}"#),
        ]);
        // NOT VALID — test data only.
        let src = JiraSource::new(
            &url,
            "fake-user@example.test",
            &Secret::new("FAKE_TEST_token"),
            BTreeMap::new(),
        );
        assert_eq!(
            src.push_status(&IssueRef::key("ABC-123"), Status::Done, &BTreeMap::new())
                .unwrap(),
            PushOutcome::AlreadyThere
        );
        let out = src
            .push_status(&IssueRef::key("ABC-123"), Status::Done, &BTreeMap::new())
            .unwrap();
        assert!(
            matches!(&out, PushOutcome::NotPushable(r) if r.contains("from To Do")),
            "{out:?}"
        );
        assert_eq!(h.join().unwrap().len(), 3);
    }

    #[test]
    fn create_child_subtask_under_story_and_refusals() {
        use crate::sources::http::tests::body_json;
        let story = json!({"fields": {"issuetype": {"subtask": false, "hierarchyLevel": 0}, "project": {"key": "ABC"}}});
        let types = json!({"issueTypes": [
            {"id": "10001", "name": "Task", "subtask": false, "hierarchyLevel": 0},
            {"id": "10003", "name": "Sub-task", "subtask": true, "hierarchyLevel": -1}
        ]});
        let me = json!({"accountId": "acc-1", "displayName": "Me"});
        let created = json!({"id": "1", "key": "ABC-124"});
        let mut fetched = raw_issue("ABC-124", "new", "To Do");
        fetched["fields"]["parent"] = json!({"key": "ABC-123"});
        let (url, h) = serve(vec![
            ok_json(&story.to_string()),
            ok_json(&types.to_string()),
            ok_json(&me.to_string()),
            ok_json(&created.to_string()),
            ok_json(&fetched.to_string()),
        ]);
        // NOT VALID — test data only.
        let src = JiraSource::new(
            &url,
            "fake-user@example.test",
            &Secret::new("FAKE_TEST_token"),
            BTreeMap::new(),
        );
        let new = NewIssue {
            title: "write regression test".into(),
            due: NaiveDate::from_ymd_opt(2026, 10, 28),
            priority: None,
        };
        let issue = src.create_child(&IssueRef::key("ABC-123"), &new).unwrap();
        assert_eq!(issue.key, "ABC-124");
        assert_eq!(issue.parent.as_deref(), Some("ABC-123"));
        let reqs = h.join().unwrap();
        assert!(reqs[1].starts_with("GET /rest/api/3/issue/createmeta/ABC/issuetypes"));
        assert!(reqs[3].starts_with("POST /rest/api/3/issue "));
        assert_eq!(
            body_json(&reqs[3]),
            json!({"fields": {
                "project": {"key": "ABC"}, "parent": {"key": "ABC-123"}, "summary": "write regression test",
                "issuetype": {"id": "10003"}, "assignee": {"accountId": "acc-1"}, "duedate": "2026-10-28"
            }})
        );

        let sub = json!({"fields": {"issuetype": {"subtask": true, "hierarchyLevel": -1}, "project": {"key": "ABC"}}});
        let (url, _h) = serve(vec![ok_json(&sub.to_string())]);
        // NOT VALID — test data only.
        let src = JiraSource::new(
            &url,
            "fake-user@example.test",
            &Secret::new("FAKE_TEST_token"),
            BTreeMap::new(),
        );
        let err = src.create_child(&IssueRef::key("ABC-9"), &new).unwrap_err();
        assert_eq!(
            err.to_string(),
            "ABC-9 is a sub-task; Jira can't nest sub-tasks"
        );
    }

    #[test]
    fn create_child_task_under_epic() {
        use crate::sources::http::tests::body_json;
        let epic = json!({"fields": {"issuetype": {"subtask": false, "hierarchyLevel": 1}, "project": {"key": "ABC"}}});
        let types = json!({"issueTypes": [
            {"id": "10000", "name": "Epic", "subtask": false, "hierarchyLevel": 1},
            {"id": "10001", "name": "Task", "subtask": false, "hierarchyLevel": 0},
            {"id": "10003", "name": "Sub-task", "subtask": true, "hierarchyLevel": -1}
        ]});
        let (url, h) = serve(vec![
            ok_json(&epic.to_string()),
            ok_json(&types.to_string()),
            ok_json(r#"{"accountId": "acc-1"}"#),
            ok_json(r#"{"key": "ABC-200"}"#),
            ok_json(&raw_issue("ABC-200", "new", "To Do").to_string()),
        ]);
        // NOT VALID — test data only.
        let src = JiraSource::new(
            &url,
            "fake-user@example.test",
            &Secret::new("FAKE_TEST_token"),
            BTreeMap::new(),
        );
        src.create_child(
            &IssueRef::key("EPIC-7"),
            &NewIssue {
                title: "t".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let reqs = h.join().unwrap();
        assert_eq!(body_json(&reqs[3])["fields"]["issuetype"]["id"], "10001");
    }
}

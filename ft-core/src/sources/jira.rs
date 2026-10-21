//! Jira Cloud (REST v3): token-paginated JQL search, key lookups, and the
//! field mapping onto [`RemoteIssue`].

use std::collections::BTreeMap;

use base64::Engine;
use chrono::NaiveDate;
use serde_json::Value;

use super::http::{Http, Secret};
use super::{adf, Comment, RemoteIssue, SourceError, TaskSource};
use crate::task::{Priority, Status};

/// Fields requested from the search endpoint — exactly what's mapped.
const FIELDS: &str =
    "summary,status,priority,duedate,resolution,resolutiondate,assignee,parent,updated,description,comment";
const PAGE: usize = 100;

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

impl TaskSource for JiraSource {
    fn fetch_query(&self, query: &str) -> Result<Vec<RemoteIssue>, SourceError> {
        self.search(query)
    }

    fn fetch_keys(&self, keys: &[String]) -> Result<Vec<RemoteIssue>, SourceError> {
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
                if CANCELLED_RESOLUTIONS.contains(&resolution) {
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
        let got = src.fetch_keys(&["A-1".into(), "GONE-9".into()]).unwrap();
        assert_eq!(got.len(), 1);
    }
}

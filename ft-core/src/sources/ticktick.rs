//! TickTick (Open API): lists are scanned through `/project/{id}/data`
//! (every undone task, uncapped) and filtered client-side by an ft-owned
//! query syntax, since TickTick has none. Keys are `tt-<taskId>`; lookups
//! need the project id too, which the issue URL carries (and, for parents,
//! the child's `projectId`). Dates are read in the task's own time zone —
//! all-day dates are local midnight expressed in UTC.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, NaiveDate, TimeZone};
use serde_json::Value;

use super::http::{Http, Secret};
use super::{Comment, IssueRef, NewIssue, PushOutcome, RemoteIssue, SourceError, TaskSource};
use crate::task::{Priority, Status};

const ACCEPT: &str = "application/json";
const KEY_PREFIX: &str = "tt-";
/// The project id the API accepts for the inbox.
const INBOX: &str = "inbox";

pub struct TicktickSource {
    /// `<base>/open/v1`.
    api: String,
    /// Web-app origin for issue URLs.
    web: String,
    http: Http,
    status_map: BTreeMap<String, Status>,
    comments: bool,
    /// `GET /project`, once per run.
    projects: RefCell<Option<Vec<Project>>>,
    /// Undone tasks per selected list id, once per run.
    lists: RefCell<HashMap<String, Vec<Value>>>,
    /// Raw tasks seen this run, by key.
    seen: RefCell<HashMap<String, Value>>,
    /// Project id of keys seen this run, including parents of seen tasks.
    project_of: RefCell<HashMap<String, String>>,
}

#[derive(Debug, Clone)]
struct Project {
    id: String,
    name: String,
}

/// A parsed TickTick query; see [`parse_query`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Query {
    pub lists: Vec<String>,
    pub exclude: Vec<String>,
    pub tags: Vec<String>,
    /// TickTick priority codes (0, 1, 3, 5).
    pub priorities: Vec<i64>,
}

/// Parse `list:<name> -list:<name> tag:<tag> priority:<level>` terms.
/// Values with spaces are double-quoted (`list:"Side projects"`).
pub fn parse_query(q: &str) -> Result<Query, SourceError> {
    let mut out = Query::default();
    for term in split_terms(q)? {
        let (name, value) = term
            .split_once(':')
            .filter(|(_, v)| !v.is_empty())
            .ok_or_else(|| unknown_term(&term))?;
        match name {
            "list" => out.lists.push(value.to_string()),
            "-list" => out.exclude.push(value.to_string()),
            "tag" => out.tags.push(value.to_lowercase()),
            "priority" => out.priorities.push(match value.to_lowercase().as_str() {
                "none" => 0,
                "low" => 1,
                "medium" => 3,
                "high" => 5,
                _ => return Err(unknown_term(&term)),
            }),
            _ => return Err(unknown_term(&term)),
        }
    }
    Ok(out)
}

fn unknown_term(term: &str) -> SourceError {
    SourceError::Refused(format!("unknown TickTick query term \"{term}\""))
}

/// Whitespace-separated terms; a `"…"` span keeps its spaces (quotes
/// dropped).
fn split_terms(q: &str) -> Result<Vec<String>, SourceError> {
    let mut terms = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in q.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    terms.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if quoted {
        return Err(SourceError::Refused(format!(
            "unterminated quote in TickTick query \"{q}\""
        )));
    }
    if !cur.is_empty() {
        terms.push(cur);
    }
    Ok(terms)
}

/// `tt-<id>`, lower-cased.
pub fn key_of_id(id: &str) -> String {
    format!("{KEY_PREFIX}{}", id.to_lowercase())
}

/// `(projectId, taskId)` from an issue URL (`…/webapp/#p/<p>/tasks/<t>`).
pub fn parse_task_url(url: &str) -> Option<(String, String)> {
    let (_, rest) = url.split_once("#p/")?;
    let (project, rest) = rest.split_once('/')?;
    let task = rest.strip_prefix("tasks/")?.trim_end_matches('/');
    (!project.is_empty() && !task.is_empty() && !task.contains('/'))
        .then(|| (project.to_string(), task.to_string()))
}

/// Web-app origin for an API base: `https://api.X` → `https://X`.
fn web_origin(base: &str) -> String {
    match base.split_once("://") {
        Some((scheme, host)) => match host.strip_prefix("api.") {
            Some(h) => format!("{scheme}://{h}"),
            None => base.to_string(),
        },
        None => base.to_string(),
    }
}

impl TicktickSource {
    pub fn new(
        base_url: Option<&str>,
        token: &Secret,
        status_map: BTreeMap<String, Status>,
        comments: bool,
    ) -> Self {
        let base = base_url
            .unwrap_or("https://api.ticktick.com")
            .trim_end_matches('/');
        TicktickSource {
            api: format!("{base}/open/v1"),
            web: web_origin(base),
            http: Http::new(Secret::new(format!("Bearer {}", token.expose()))),
            status_map,
            comments,
            projects: RefCell::new(None),
            lists: RefCell::new(HashMap::new()),
            seen: RefCell::new(HashMap::new()),
            project_of: RefCell::new(HashMap::new()),
        }
    }

    /// Verify a token by listing projects.
    pub fn whoami(&self) -> Result<String, SourceError> {
        let n = self.projects()?.len();
        Ok(format!("a TickTick account with {n} lists"))
    }

    fn projects(&self) -> Result<Vec<Project>, SourceError> {
        if let Some(p) = self.projects.borrow().as_ref() {
            return Ok(p.clone());
        }
        let raw = self
            .http
            .get_json(&format!("{}/project", self.api), ACCEPT)?;
        let arr = raw
            .json
            .as_array()
            .ok_or_else(|| SourceError::Parse("project list is not an array".into()))?;
        let projects: Vec<Project> = arr
            .iter()
            .filter(|p| !p["closed"].as_bool().unwrap_or(false))
            .filter_map(|p| {
                Some(Project {
                    id: p["id"].as_str()?.to_string(),
                    name: p["name"].as_str().unwrap_or("").to_string(),
                })
            })
            .collect();
        *self.projects.borrow_mut() = Some(projects.clone());
        Ok(projects)
    }

    /// A list id for a query value: `inbox`, a project name
    /// (case-insensitive), or a project id.
    fn resolve_list(&self, projects: &[Project], name: &str) -> Result<String, SourceError> {
        if name.eq_ignore_ascii_case(INBOX) {
            return Ok(INBOX.to_string());
        }
        projects
            .iter()
            .find(|p| p.id == name)
            .or_else(|| projects.iter().find(|p| p.name.eq_ignore_ascii_case(name)))
            .map(|p| p.id.clone())
            .ok_or_else(|| SourceError::Refused(format!("unknown TickTick list \"{name}\"")))
    }

    fn list_tasks(&self, list: &str) -> Result<Vec<Value>, SourceError> {
        if let Some(t) = self.lists.borrow().get(list) {
            return Ok(t.clone());
        }
        let raw = self
            .http
            .get_json(&format!("{}/project/{list}/data", self.api), ACCEPT)?;
        let tasks = raw.json["tasks"].as_array().cloned().unwrap_or_default();
        for t in &tasks {
            self.remember(t);
        }
        self.lists
            .borrow_mut()
            .insert(list.to_string(), tasks.clone());
        Ok(tasks)
    }

    /// Cache a raw task, and its parent's project (subtasks share it).
    fn remember(&self, raw: &Value) {
        let (Some(id), Some(project)) = (raw["id"].as_str(), raw["projectId"].as_str()) else {
            return;
        };
        let key = key_of_id(id);
        let mut of = self.project_of.borrow_mut();
        of.insert(key.clone(), project.to_string());
        if let Some(parent) = raw["parentId"].as_str().filter(|p| !p.is_empty()) {
            of.entry(key_of_id(parent))
                .or_insert_with(|| project.to_string());
        }
        self.seen.borrow_mut().insert(key, raw.clone());
    }

    fn task_url(&self, project: &str, task: &str) -> String {
        format!("{}/webapp/#p/{project}/tasks/{task}", self.web)
    }

    /// `(projectId, taskId)` for a ref: its URL, else what this run saw.
    fn locate(&self, r: &IssueRef) -> Option<(String, String)> {
        if let Some(pt) = r.url.as_deref().and_then(parse_task_url) {
            return Some(pt);
        }
        let id = r.key.strip_prefix(KEY_PREFIX)?.to_string();
        let project = self.project_of.borrow().get(&r.key).cloned()?;
        Some((project, id))
    }

    fn get_task(&self, project: &str, task: &str) -> Result<Option<Value>, SourceError> {
        match self.http.get_json(
            &format!("{}/project/{project}/task/{task}", self.api),
            ACCEPT,
        ) {
            Ok(r) => Ok(Some(r.json)),
            Err(SourceError::Remote { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn complete(&self, raw: &Value) -> Result<RemoteIssue, SourceError> {
        let mut issue = map_task(raw, &self.status_map, &|p, t| self.task_url(p, t))?;
        if self.comments {
            let (p, t) = (
                raw["projectId"].as_str().unwrap_or_default(),
                raw["id"].as_str().unwrap_or_default(),
            );
            let comments = match self.http.get_json(
                &format!("{}/project/{p}/task/{t}/comments", self.api),
                ACCEPT,
            ) {
                Ok(r) => r.json.as_array().cloned().unwrap_or_default(),
                Err(SourceError::Remote { status: 404, .. }) => Vec::new(),
                Err(e) => return Err(e),
            };
            issue.comments = map_comments(&comments);
        }
        Ok(issue)
    }

    /// `POST /task/{id}` with the whole task — the update endpoint's
    /// treatment of omitted fields is undocumented, so nothing is omitted.
    fn update(&self, raw: &Value) -> Result<Value, SourceError> {
        let id = raw["id"].as_str().unwrap_or_default();
        Ok(self
            .http
            .post_json(&format!("{}/task/{id}", self.api), ACCEPT, raw)?
            .json)
    }

    fn user_time_zone(&self) -> Result<Option<chrono_tz::Tz>, SourceError> {
        let r = self.http.post_json(
            &format!("{}/preference", self.api),
            ACCEPT,
            &serde_json::json!({}),
        )?;
        Ok(r.json["timeZone"].as_str().and_then(|z| z.parse().ok()))
    }
}

impl TaskSource for TicktickSource {
    fn fetch_query(&self, query: &str) -> Result<Vec<RemoteIssue>, SourceError> {
        let q = parse_query(query)?;
        let projects = self.projects()?;
        let mut selected: Vec<String> = if q.lists.is_empty() {
            projects
                .iter()
                .map(|p| p.id.clone())
                .chain([INBOX.to_string()])
                .collect()
        } else {
            q.lists
                .iter()
                .map(|l| self.resolve_list(&projects, l))
                .collect::<Result<_, _>>()?
        };
        let excluded: Vec<String> = q
            .exclude
            .iter()
            .map(|l| self.resolve_list(&projects, l))
            .collect::<Result<_, _>>()?;
        selected.retain(|l| !excluded.contains(l));
        selected.dedup();

        let mut out = Vec::new();
        for list in selected {
            for raw in self.list_tasks(&list)? {
                if matches(&raw, &q) {
                    out.push(self.complete(&raw)?);
                }
            }
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
                    let Some((project, task)) = self.locate(r) else {
                        continue;
                    };
                    match self.get_task(&project, &task)? {
                        Some(raw) => {
                            self.remember(&raw);
                            raw
                        }
                        None => continue,
                    }
                }
            };
            out.push(self.complete(&raw)?);
        }
        Ok(out)
    }

    fn push_status(
        &self,
        issue: &IssueRef,
        target: Status,
        _transitions: &BTreeMap<Status, String>,
    ) -> Result<PushOutcome, SourceError> {
        let Some((project, task)) = issue.url.as_deref().and_then(parse_task_url) else {
            return Ok(PushOutcome::NotPushable("no task URL in the note".into()));
        };
        let Some(mut raw) = self.get_task(&project, &task)? else {
            return Ok(PushOutcome::NotPushable("task not found".into()));
        };
        let current = status_of(&raw, &self.status_map);
        let open = |s: Status| matches!(s, Status::Open | Status::InProgress);
        if current == target || (open(current) && open(target)) {
            return Ok(PushOutcome::AlreadyThere);
        }
        let result = match target {
            Status::Done => self
                .http
                .post_json(
                    &format!("{}/project/{project}/task/{task}/complete", self.api),
                    ACCEPT,
                    &serde_json::json!({}),
                )
                .map(drop),
            Status::Cancelled | Status::Open | Status::InProgress => {
                raw["status"] = Value::from(if target == Status::Cancelled { -1 } else { 0 });
                self.update(&raw).map(drop)
            }
        };
        match result {
            Ok(()) => Ok(PushOutcome::Pushed),
            // A 403 on a write is a per-task permission, not bad credentials.
            Err(SourceError::Unauthorized { status: 403 }) => {
                Ok(PushOutcome::NotPushable("forbidden (403)".into()))
            }
            Err(SourceError::Remote { status: 404, .. }) => {
                Ok(PushOutcome::NotPushable("task not found".into()))
            }
            Err(e) => Err(e),
        }
    }

    fn create_child(&self, parent: &IssueRef, new: &NewIssue) -> Result<RemoteIssue, SourceError> {
        let (project, parent_id) =
            parent
                .url
                .as_deref()
                .and_then(parse_task_url)
                .ok_or_else(|| {
                    SourceError::Refused(format!("{}: no task URL in the note", parent.key))
                })?;
        let mut body = serde_json::json!({
            "title": new.title,
            "projectId": project,
            "parentId": parent_id,
            "priority": new.priority.map_or(0, priority_code),
        });
        if let Some(due) = new.due {
            let tz = self.user_time_zone()?.unwrap_or(chrono_tz::UTC);
            body["dueDate"] = Value::from(all_day(due, tz));
            body["isAllDay"] = Value::from(true);
            body["timeZone"] = Value::from(tz.name());
        }
        let mut raw = self
            .http
            .post_json(&format!("{}/task", self.api), ACCEPT, &body)?
            .json;
        if raw["id"].as_str().is_none() {
            return Err(SourceError::Parse("created task without id".into()));
        }
        if raw["parentId"].as_str() != Some(parent_id.as_str()) {
            raw["parentId"] = Value::from(parent_id.as_str());
            raw = self.update(&raw)?;
            if raw["parentId"].as_str() != Some(parent_id.as_str()) {
                let key = raw["id"].as_str().map(key_of_id).unwrap_or_default();
                return Err(SourceError::Refused(format!(
                    "TickTick did not accept the parent for {key}"
                )));
            }
        }
        self.remember(&raw);
        self.complete(&raw)
    }
}

/// Client-side filters of `q` (lists were already selected).
fn matches(raw: &Value, q: &Query) -> bool {
    if raw["status"].as_i64().unwrap_or(0) != 0 {
        return false;
    }
    let tags: Vec<String> = raw["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(str::to_lowercase))
                .collect()
        })
        .unwrap_or_default();
    let priority = raw["priority"].as_i64().unwrap_or(0);
    q.tags.iter().all(|t| tags.contains(t))
        && (q.priorities.is_empty() || q.priorities.contains(&priority))
}

fn priority_code(p: Priority) -> i64 {
    match p {
        Priority::Highest | Priority::High => 5,
        Priority::Medium => 3,
        Priority::Low | Priority::Lowest => 1,
    }
}

/// `yyyy-MM-dd'T'HH:mm:ssZ` for local midnight of `date` in `tz`.
fn all_day(date: NaiveDate, tz: chrono_tz::Tz) -> String {
    let midnight = date.and_hms_opt(0, 0, 0).expect("midnight is valid");
    let at = tz
        .from_local_datetime(&midnight)
        .earliest()
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|| midnight.and_utc());
    at.format("%Y-%m-%dT%H:%M:%S%z").to_string()
}

/// The local date of a TickTick timestamp in `zone` (UTC when unknown).
fn local_date(ts: Option<&str>, zone: Option<&str>) -> Option<NaiveDate> {
    let at = DateTime::parse_from_str(ts?, "%Y-%m-%dT%H:%M:%S%.f%z").ok()?;
    match zone.and_then(|z| z.parse::<chrono_tz::Tz>().ok()) {
        Some(tz) => Some(at.with_timezone(&tz).date_naive()),
        None => Some(at.naive_utc().date()),
    }
}

/// Status name: `completed`, `wont-do`, or the open task's kanban column
/// (else `open`).
fn status_name(raw: &Value) -> String {
    match raw["status"].as_i64().unwrap_or(0) {
        2 => "completed".to_string(),
        -1 => "wont-do".to_string(),
        _ => raw["columnName"]
            .as_str()
            .filter(|c| !c.trim().is_empty())
            .unwrap_or("open")
            .to_string(),
    }
}

fn status_of(raw: &Value, status_map: &BTreeMap<String, Status>) -> Status {
    status_map.get(&status_name(raw)).copied().unwrap_or(
        match raw["status"].as_i64().unwrap_or(0) {
            2 => Status::Done,
            -1 => Status::Cancelled,
            _ => Status::Open,
        },
    )
}

/// Map a task object (without comments). `url` builds the web-app link
/// from `(projectId, taskId)`.
pub fn map_task(
    raw: &Value,
    status_map: &BTreeMap<String, Status>,
    url: &dyn Fn(&str, &str) -> String,
) -> Result<RemoteIssue, SourceError> {
    let id = raw["id"]
        .as_str()
        .ok_or_else(|| SourceError::Parse("task without id".into()))?;
    let project = raw["projectId"].as_str().unwrap_or_default();
    let zone = raw["timeZone"].as_str();
    let s = |k: &str| raw[k].as_str().unwrap_or("").replace("\r\n", "\n");
    let description = if raw["kind"].as_str() == Some("CHECKLIST") {
        let mut items: Vec<&Value> = raw["items"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        items.sort_by_key(|i| i["sortOrder"].as_i64().unwrap_or(0));
        let lines: Vec<String> = items
            .iter()
            .map(|i| {
                let done = i["status"].as_i64().unwrap_or(0) != 0;
                let title = i["title"].as_str().unwrap_or("").replace('\n', " ");
                format!("- [{}] {title}", if done { 'x' } else { ' ' })
            })
            .collect();
        [s("desc").trim().to_string(), lines.join("\n")]
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    } else {
        s("content")
    };
    Ok(RemoteIssue {
        key: key_of_id(id),
        url: url(project, id),
        title: s("title"),
        status_name: status_name(raw),
        status: status_of(raw, status_map),
        priority: match raw["priority"].as_i64().unwrap_or(0) {
            5 => Some(Priority::High),
            3 => Some(Priority::Medium),
            1 => Some(Priority::Low),
            _ => None,
        },
        due: local_date(raw["dueDate"].as_str(), zone),
        resolved: local_date(raw["completedTime"].as_str(), zone),
        assignee: raw["assigneeUsername"]
            .as_str()
            .filter(|a| !a.is_empty())
            .map(str::to_string),
        parent: raw["parentId"]
            .as_str()
            .filter(|p| !p.is_empty())
            .map(key_of_id),
        updated: raw["modifiedTime"]
            .as_str()
            .or(raw["etag"].as_str())
            .unwrap_or("")
            .to_string(),
        description,
        comments: Vec::new(),
    })
}

/// TickTick comments carry no author.
fn map_comments(raw: &[Value]) -> Vec<Comment> {
    let mut out: Vec<Comment> = raw
        .iter()
        .map(|c| Comment {
            author: "comment".to_string(),
            created: c["createdTime"].as_str().unwrap_or("").to_string(),
            body: c["title"].as_str().unwrap_or("").replace("\r\n", "\n"),
        })
        .collect();
    out.sort_by(|a, b| a.created.cmp(&b.created));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::http::tests::{body_json, ok_json, serve};
    use serde_json::json;

    // Shapes from the verified responses; ids and text are made up.
    const P_WORK: &str = "6aec746604183a21c6cf0001";
    const P_HOME: &str = "6aec746604183a21c6cf0002";

    fn projects() -> String {
        json!([
            {"id": P_WORK, "name": "Work", "kind": "TASK", "sortOrder": 0, "viewMode": "kanban"},
            {"id": P_HOME, "name": "Home", "kind": "TASK", "sortOrder": 1, "viewMode": "list"},
            {"id": "6aec746604183a21c6cf0003", "name": "Old", "closed": true}
        ])
        .to_string()
    }

    fn task(id: &str, project: &str, extra: Value) -> Value {
        let mut t = json!({
            "id": id, "projectId": project, "title": format!("Task {id}"),
            "content": "", "timeZone": "America/Sao_Paulo", "isAllDay": false,
            "priority": 0, "status": 0, "etag": "e1", "kind": "TEXT",
            "modifiedTime": "2026-11-05T12:19:57.000+0000",
            "createdTime": "2026-11-01T10:00:00.000+0000",
        });
        for (k, v) in extra.as_object().unwrap() {
            t[k] = v.clone();
        }
        t
    }

    fn data(tasks: Vec<Value>) -> String {
        json!({"tasks": tasks, "columns": []}).to_string()
    }

    fn src(url: &str) -> TicktickSource {
        // NOT VALID — test data only.
        TicktickSource::new(
            Some(url),
            &Secret::new("FAKE_TEST_token"),
            BTreeMap::new(),
            false,
        )
    }

    fn url_of(p: &str, t: &str) -> String {
        format!("https://ticktick.com/webapp/#p/{p}/tasks/{t}")
    }

    #[test]
    fn query_terms() {
        assert_eq!(parse_query("").unwrap(), Query::default());
        let q = parse_query(
            r#"list:Work list:"Side projects" -list:inbox tag:Urgent priority:high priority:none"#,
        )
        .unwrap();
        assert_eq!(q.lists, ["Work", "Side projects"]);
        assert_eq!(q.exclude, ["inbox"]);
        assert_eq!(q.tags, ["urgent"]);
        assert_eq!(q.priorities, [5, 0]);
        for bad in [
            "assignee:me",
            "list:",
            "priority:urgent",
            "Work",
            "list:\"open",
        ] {
            let e = parse_query(bad).unwrap_err().to_string();
            assert!(e.contains("TickTick query"), "{bad}: {e}");
        }
    }

    #[test]
    fn keys_and_urls() {
        assert_eq!(
            key_of_id("6AEC746604183A21C6CFFE42"),
            "tt-6aec746604183a21c6cffe42"
        );
        assert_eq!(
            parse_task_url(&url_of("p1", "t1")),
            Some(("p1".into(), "t1".into()))
        );
        assert_eq!(parse_task_url("https://ticktick.com/webapp/#p/p1"), None);
        assert_eq!(
            web_origin("https://api.ticktick.com"),
            "https://ticktick.com"
        );
        assert_eq!(web_origin("https://api.dida365.com"), "https://dida365.com");
        assert_eq!(web_origin("http://127.0.0.1:9"), "http://127.0.0.1:9");
    }

    #[test]
    fn mapping_dates_status_priority_parent() {
        let raw = task(
            "6aec746604183a21c6cffe42",
            P_WORK,
            json!({
                "isAllDay": true, "dueDate": "2026-11-05T03:00:00.000+0000",
                "priority": 3, "parentId": "6aec746604183a21c6cffe41",
                "columnName": "Doing", "content": "line\r\nnext",
                "assigneeUsername": "someone@example.test",
            }),
        );
        let map: BTreeMap<String, Status> = [("Doing".to_string(), Status::InProgress)].into();
        let i = map_task(&raw, &map, &url_of).unwrap();
        assert_eq!(i.key, "tt-6aec746604183a21c6cffe42");
        assert_eq!(i.url, url_of(P_WORK, "6aec746604183a21c6cffe42"));
        assert_eq!(i.due, NaiveDate::from_ymd_opt(2026, 11, 5));
        assert_eq!(
            (i.status_name.as_str(), i.status),
            ("Doing", Status::InProgress)
        );
        assert_eq!(i.priority, Some(Priority::Medium));
        assert_eq!(i.parent.as_deref(), Some("tt-6aec746604183a21c6cffe41"));
        assert_eq!(i.updated, "2026-11-05T12:19:57.000+0000");
        assert_eq!(i.description, "line\nnext");
        assert_eq!(i.assignee.as_deref(), Some("someone@example.test"));

        // East of UTC the UTC date would be the day before.
        let raw = task(
            "a",
            P_WORK,
            json!({"timeZone": "Asia/Shanghai",
            "dueDate": "2026-11-04T16:00:00+0000", "status": 2,
            "completedTime": "2026-11-04T17:30:00.000+0000"}),
        );
        let i = map_task(&raw, &BTreeMap::new(), &url_of).unwrap();
        assert_eq!(i.due, NaiveDate::from_ymd_opt(2026, 11, 5));
        assert_eq!(i.resolved, NaiveDate::from_ymd_opt(2026, 11, 5));
        assert_eq!(
            (i.status_name.as_str(), i.status),
            ("completed", Status::Done)
        );

        let raw = task(
            "b",
            P_WORK,
            json!({"status": -1, "timeZone": "Mars/Base",
            "dueDate": "2026-11-04T23:00:00+0000", "etag": "x9", "modifiedTime": null}),
        );
        let i = map_task(&raw, &BTreeMap::new(), &url_of).unwrap();
        assert_eq!(
            (i.status_name.as_str(), i.status),
            ("wont-do", Status::Cancelled)
        );
        assert_eq!(i.due, NaiveDate::from_ymd_opt(2026, 11, 4), "UTC fallback");
        assert_eq!(i.updated, "x9");
        assert_eq!(i.status_name, "wont-do");
        let open = map_task(&task("c", P_WORK, json!({})), &BTreeMap::new(), &url_of).unwrap();
        assert_eq!(
            (open.status_name.as_str(), open.status),
            ("open", Status::Open)
        );
    }

    #[test]
    fn checklist_description() {
        let raw = task(
            "c",
            P_WORK,
            json!({"kind": "CHECKLIST", "desc": "Packing",
            "items": [
                {"id": "i2", "title": "socks", "status": 1, "sortOrder": 2},
                {"id": "i1", "title": "passport", "status": 0, "sortOrder": 1}
            ]}),
        );
        let i = map_task(&raw, &BTreeMap::new(), &url_of).unwrap();
        assert_eq!(i.description, "Packing\n\n- [ ] passport\n- [x] socks");
    }

    #[test]
    fn default_query_scans_every_open_list_and_inbox_once() {
        let (url, h) = serve(vec![
            ok_json(&projects()),
            ok_json(&data(vec![task("w1", P_WORK, json!({}))])),
            ok_json(&data(vec![task("h1", P_HOME, json!({"tags": ["Errand"]}))])),
            ok_json(&data(vec![task("n1", "inbox115", json!({}))])),
        ]);
        let s = src(&url);
        let keys = |v: Vec<RemoteIssue>| v.into_iter().map(|i| i.key).collect::<Vec<_>>();
        assert_eq!(
            keys(s.fetch_query("").unwrap()),
            ["tt-w1", "tt-h1", "tt-n1"]
        );
        // Overlapping queries reuse the cached lists: no more requests.
        assert_eq!(
            keys(s.fetch_query("list:home tag:errand").unwrap()),
            ["tt-h1"]
        );
        assert!(keys(s.fetch_query("-list:Work -list:Home -list:inbox").unwrap()).is_empty());
        let reqs = h.join().unwrap();
        let paths: Vec<&str> = reqs.iter().map(|r| r.split(' ').nth(1).unwrap()).collect();
        assert_eq!(
            paths,
            [
                "/open/v1/project".to_string(),
                format!("/open/v1/project/{P_WORK}/data"),
                format!("/open/v1/project/{P_HOME}/data"),
                "/open/v1/project/inbox/data".to_string(),
            ]
        );
        assert!(reqs[0].contains("Bearer FAKE_TEST_token"));
    }

    #[test]
    fn unknown_list_is_an_error() {
        let (url, _h) = serve(vec![ok_json(&projects())]);
        let e = src(&url).fetch_query("list:Wrok").unwrap_err();
        assert_eq!(e.to_string(), "unknown TickTick list \"Wrok\"");
        let (url, _h) = serve(vec![ok_json(&projects())]);
        assert!(
            src(&url).fetch_query("list:Old").is_err(),
            "closed lists don't resolve"
        );
    }

    #[test]
    fn fetch_keys_by_url_and_by_childs_project() {
        let not_found =
            "HTTP/1.1 404 X\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_string();
        let (url, h) = serve(vec![
            ok_json(&projects()),
            ok_json(&data(vec![task("kid", P_WORK, json!({"parentId": "dad"}))])),
            // dad: located via kid's project
            ok_json(&task("dad", P_WORK, json!({"status": 2})).to_string()),
            // done: located via its URL
            ok_json(&task("done", P_HOME, json!({"status": 2})).to_string()),
            not_found,
        ]);
        let s = src(&url);
        s.fetch_query("list:Work").unwrap();
        let refs = [
            IssueRef::key("tt-kid"),
            IssueRef::key("tt-dad"),
            IssueRef {
                key: "tt-done".into(),
                url: Some(url_of(P_HOME, "done")),
            },
            IssueRef {
                key: "tt-gone".into(),
                url: Some(url_of(P_HOME, "gone")),
            },
            IssueRef::key("tt-nowhere"),
        ];
        let got: Vec<String> = s
            .fetch_keys(&refs)
            .unwrap()
            .into_iter()
            .map(|i| i.key)
            .collect();
        assert_eq!(got, ["tt-kid", "tt-dad", "tt-done"]);
        let reqs = h.join().unwrap();
        assert!(reqs[2].starts_with(&format!("GET /open/v1/project/{P_WORK}/task/dad ")));
        assert!(reqs[3].starts_with(&format!("GET /open/v1/project/{P_HOME}/task/done ")));
    }

    #[test]
    fn push_done_completes_and_cancel_sends_whole_task() {
        let raw = task(
            "t1",
            P_WORK,
            json!({"dueDate": "2026-11-05T03:00:00.000+0000", "tags": ["a"]}),
        );
        let (url, h) = serve(vec![
            ok_json(&raw.to_string()),
            ok_json("{}"),
            ok_json(&raw.to_string()),
            ok_json(&raw.to_string()),
            ok_json(&raw.to_string()),
        ]);
        let s = src(&url);
        let r = IssueRef {
            key: "tt-t1".into(),
            url: Some(url_of(P_WORK, "t1")),
        };
        let none = BTreeMap::new();
        assert_eq!(
            s.push_status(&r, Status::Done, &none).unwrap(),
            PushOutcome::Pushed
        );
        assert_eq!(
            s.push_status(&r, Status::Cancelled, &none).unwrap(),
            PushOutcome::Pushed
        );
        assert_eq!(
            s.push_status(&r, Status::InProgress, &none).unwrap(),
            PushOutcome::AlreadyThere
        );
        let reqs = h.join().unwrap();
        assert!(reqs[1].starts_with(&format!("POST /open/v1/project/{P_WORK}/task/t1/complete ")));
        assert!(reqs[3].starts_with("POST /open/v1/task/t1 "), "{}", reqs[3]);
        let mut want = raw.clone();
        want["status"] = json!(-1);
        assert_eq!(body_json(&reqs[3]), want);
    }

    #[test]
    fn push_reopen_and_missing() {
        let done = task("t1", P_WORK, json!({"status": 2}));
        let not_found =
            "HTTP/1.1 404 X\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_string();
        let (url, h) = serve(vec![ok_json(&done.to_string()), ok_json("{}"), not_found]);
        let s = src(&url);
        let r = IssueRef {
            key: "tt-t1".into(),
            url: Some(url_of(P_WORK, "t1")),
        };
        let none = BTreeMap::new();
        assert_eq!(
            s.push_status(&r, Status::Open, &none).unwrap(),
            PushOutcome::Pushed
        );
        assert_eq!(
            s.push_status(&r, Status::Done, &none).unwrap(),
            PushOutcome::NotPushable("task not found".into())
        );
        assert_eq!(
            s.push_status(&IssueRef::key("tt-x"), Status::Done, &none)
                .unwrap(),
            PushOutcome::NotPushable("no task URL in the note".into())
        );
        let reqs = h.join().unwrap();
        assert_eq!(body_json(&reqs[1])["status"], 0);
    }

    #[test]
    fn create_child_with_due_priority_and_parent() {
        let created = task("new1", P_WORK, json!({"parentId": "dad", "priority": 3}));
        let (url, h) = serve(vec![
            ok_json(r#"{"timeZone":"America/Sao_Paulo"}"#),
            ok_json(&created.to_string()),
        ]);
        let parent = IssueRef {
            key: "tt-dad".into(),
            url: Some(url_of(P_WORK, "dad")),
        };
        let new = NewIssue {
            title: "buy cable".into(),
            due: NaiveDate::from_ymd_opt(2026, 11, 10),
            priority: Some(Priority::Medium),
        };
        let i = src(&url).create_child(&parent, &new).unwrap();
        assert_eq!(
            (i.key.as_str(), i.parent.as_deref()),
            ("tt-new1", Some("tt-dad"))
        );
        let reqs = h.join().unwrap();
        assert!(reqs[0].starts_with("POST /open/v1/preference "));
        assert!(reqs[1].starts_with("POST /open/v1/task "));
        assert_eq!(
            body_json(&reqs[1]),
            json!({"title": "buy cable", "projectId": P_WORK, "parentId": "dad",
                "priority": 3, "dueDate": "2026-11-10T03:00:00+0000",
                "isAllDay": true, "timeZone": "America/Sao_Paulo"})
        );
    }

    #[test]
    fn create_child_sets_parent_when_create_ignores_it() {
        let orphan = task("new1", P_WORK, json!({}));
        let (url, h) = serve(vec![
            ok_json(&orphan.to_string()),
            ok_json(&orphan.to_string()),
        ]);
        let parent = IssueRef {
            key: "tt-dad".into(),
            url: Some(url_of(P_WORK, "dad")),
        };
        let new = NewIssue {
            title: "x".into(),
            ..Default::default()
        };
        let e = src(&url).create_child(&parent, &new).unwrap_err();
        assert_eq!(
            e.to_string(),
            "TickTick did not accept the parent for tt-new1"
        );
        let reqs = h.join().unwrap();
        assert!(reqs[1].starts_with("POST /open/v1/task/new1 "));
        assert_eq!(body_json(&reqs[1])["parentId"], "dad");
    }

    #[test]
    fn comments_only_when_enabled() {
        let (url, h) = serve(vec![
            ok_json(&projects()),
            ok_json(&data(vec![task("w1", P_WORK, json!({}))])),
            ok_json(
                r#"[{"id":"c2","title":"later","createdTime":"2026-04-08T09:00:00+0000"},{"id":"c1","title":"first","createdTime":"2026-04-07T09:00:00+0000"}]"#,
            ),
        ]);
        let s = TicktickSource::new(
            Some(&url),
            &Secret::new("FAKE_TEST_token"),
            BTreeMap::new(),
            true,
        );
        let i = s.fetch_query("list:Work").unwrap().remove(0);
        let bodies: Vec<&str> = i.comments.iter().map(|c| c.body.as_str()).collect();
        assert_eq!(bodies, ["first", "later"]);
        let reqs = h.join().unwrap();
        assert!(reqs[2].starts_with(&format!("GET /open/v1/project/{P_WORK}/task/w1/comments ")));
    }
}

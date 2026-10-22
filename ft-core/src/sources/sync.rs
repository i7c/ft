//! The sync planner: given what a source fetched ([`FetchResult`]) and the
//! vault's issue notes, produce file edits and a report. Pure apart from
//! reading the notes it edits (fresh from disk) and the template; nothing
//! is written until [`apply_sync_plan`].
//!
//! All sync state lives in the notes (`ft.source.*`), and the output only
//! depends on remote data and the note itself — two machines syncing the
//! same remote state produce identical bytes.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use super::note::{self, COMMENTS, DESCRIPTION};
use super::{FetchResult, IssueRef, RemoteIssue};
use crate::config::SourceCfg;
use crate::frontmatter::{self, FmValue, FtSource};
use crate::scan::Scan;
use crate::task::format::TaskFormat;
use crate::task::ops::FileEdit;
use crate::task::Status;

pub const STATE_ACTIVE: &str = "active";
pub const STATE_CONTEXT: &str = "context";
pub const STATE_OUT_OF_SCOPE: &str = "out-of-scope";

/// What a sync did (or, for a dry run, would do).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    /// Keys whose local status awaits a push.
    pub pending: Vec<String>,
    /// `(key, local, remote)` where both sides changed status.
    pub conflicts: Vec<(String, Status, Status)>,
    /// Vault-relative note of each entry in `conflicts`, same order.
    pub conflict_notes: Vec<PathBuf>,
    /// Keys that moved out of scope this sync.
    pub out_of_scope: Vec<String>,
    pub warnings: Vec<String>,
    /// Keys whose status was pushed before this sync's fetch.
    pub pushed: Vec<String>,
    /// `(key, reason)` for pushes that failed or weren't possible.
    pub push_failures: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct SyncPlan {
    pub edits: Vec<FileEdit>,
    pub report: SyncReport,
}

/// Write every changed file in `plan`.
pub fn apply_sync_plan(plan: &SyncPlan) -> crate::error::Result<()> {
    for e in &plan.edits {
        if e.original != e.new {
            crate::fs::write_atomic(&e.path, &e.new)?;
        }
    }
    Ok(())
}

/// Everything the planner needs besides the fetched data.
pub struct SyncCtx<'a> {
    pub vault_root: &'a Path,
    pub templates_dir: &'a Path,
    pub scan: &'a Scan,
    pub source: &'a str,
    pub cfg: &'a SourceCfg,
    pub format: &'a dyn TaskFormat,
    /// Fallback date for `✅` / `❌` when the remote has none.
    pub today: NaiveDate,
}

/// An existing issue note of this source, from the scan.
#[derive(Debug, Clone)]
pub(crate) struct Known {
    pub(crate) rel: PathBuf,
    pub(crate) fm: FtSource,
}

/// The source's issue notes, by key (first path wins on duplicates).
pub(crate) fn index_notes(scan: &Scan, source: &str) -> (BTreeMap<String, Known>, Vec<String>) {
    let mut by_key: BTreeMap<String, Known> = BTreeMap::new();
    let mut warnings = Vec::new();
    let mut files: Vec<_> = scan.files.iter().collect();
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    for f in files {
        let Some(fm) = f.frontmatter.as_deref().and_then(frontmatter::ft_source_in) else {
            continue;
        };
        if fm.name != source {
            continue;
        }
        match by_key.get(&fm.key) {
            Some(first) => warnings.push(format!(
                "{}: duplicate note for {} (using {})",
                f.rel.display(),
                fm.key,
                first.rel.display()
            )),
            None => {
                by_key.insert(
                    fm.key.clone(),
                    Known {
                        rel: f.rel.clone(),
                        fm,
                    },
                );
            }
        }
    }
    (by_key, warnings)
}

/// Issues the vault tracks as active or context for `source` — the
/// `known` input to [`super::fetch_all`].
pub fn known_issues(scan: &Scan, source: &str) -> Vec<IssueRef> {
    index_notes(scan, source)
        .0
        .into_values()
        .filter(|k| {
            matches!(
                k.fm.state.as_deref(),
                Some(STATE_ACTIVE) | Some(STATE_CONTEXT)
            )
        })
        .map(|k| IssueRef {
            key: k.fm.key,
            url: k.fm.url,
        })
        .collect()
}

/// How one issue's status resolves (three-way on status only).
enum StatusDecision {
    /// Write the remote status and record it as synced.
    Write(Status),
    /// Keep the local status; `synced` unchanged.
    Keep {
        pending: bool,
        conflict: Option<Status>,
    },
}

fn decide(local: Option<Status>, base: Option<Status>, remote: Status) -> StatusDecision {
    let Some(local) = local else {
        return StatusDecision::Write(remote);
    };
    let base = base.unwrap_or(local);
    if local == base || remote == local {
        StatusDecision::Write(remote)
    } else if remote == base {
        StatusDecision::Keep {
            pending: true,
            conflict: None,
        }
    } else {
        StatusDecision::Keep {
            pending: false,
            conflict: Some(remote),
        }
    }
}

/// The status a fetched issue should have locally: an out-of-scope issue
/// that is still open remotely is cancelled here.
fn target_status(issue: &RemoteIssue, state: &str) -> Status {
    if state == STATE_OUT_OF_SCOPE && !matches!(issue.status, Status::Done | Status::Cancelled) {
        Status::Cancelled
    } else {
        issue.status
    }
}

/// Plan one source's sync.
pub fn plan_sync(ctx: &SyncCtx<'_>, fetched: &FetchResult) -> SyncPlan {
    let (known, mut warnings) = index_notes(ctx.scan, ctx.source);
    let mut report = SyncReport::default();
    let folder = ctx.cfg.folder_or(ctx.source);

    let all: Vec<(&RemoteIssue, &str)> = fetched
        .active
        .iter()
        .map(|i| (i, STATE_ACTIVE))
        .chain(fetched.context.iter().map(|i| (i, STATE_CONTEXT)))
        .chain(fetched.rechecked.iter().map(|i| (i, STATE_OUT_OF_SCOPE)))
        .collect();

    // Where every issue's note lives: existing note, or a new path.
    let mut paths: BTreeMap<String, PathBuf> = known
        .iter()
        .map(|(k, n)| (k.clone(), n.rel.clone()))
        .collect();
    let existing: HashSet<&Path> = ctx.scan.files.iter().map(|f| f.rel.as_path()).collect();
    for (issue, _) in &all {
        if paths.contains_key(&issue.key) {
            continue;
        }
        let mut rel = PathBuf::from(folder).join(format!("{}.md", issue.key));
        if existing.contains(rel.as_path()) || ctx.vault_root.join(&rel).exists() {
            rel = PathBuf::from(folder).join(format!("{} ({}).md", issue.key, ctx.source));
            warnings.push(format!(
                "{}: name taken, using {}",
                issue.key,
                rel.display()
            ));
        }
        paths.insert(issue.key.clone(), rel);
    }
    let mut edits = Vec::new();
    for (issue, state) in &all {
        let rel = &paths[&issue.key];
        let parent = issue
            .parent
            .as_ref()
            .filter(|p| paths.contains_key(*p))
            .cloned();
        let edit = match known.get(&issue.key) {
            Some(k) => update_note(ctx, k, issue, state, parent.as_deref(), &mut report),
            None => {
                report.created.push(issue.key.clone());
                Some(new_note(
                    ctx,
                    rel,
                    issue,
                    state,
                    parent.as_deref(),
                    &mut warnings,
                ))
            }
        };
        edits.extend(edit);
    }
    for key in &fetched.missing {
        if let Some(k) = known.get(key) {
            edits.extend(retire_note(ctx, k, &mut report));
        }
    }
    report.warnings = warnings;
    SyncPlan { edits, report }
}

fn read(ctx: &SyncCtx<'_>, rel: &Path) -> Option<String> {
    std::fs::read_to_string(ctx.vault_root.join(rel)).ok()
}

/// Line index of the anchor task (`🆔 == key`) in `content`.
fn find_anchor(content: &str, key: &str, format: &dyn TaskFormat) -> Option<usize> {
    content
        .split('\n')
        .position(|l| note::parse(l, format).is_some_and(|t| t.id.as_deref() == Some(key)))
}

fn synced_entry(s: Status) -> FmValue {
    FmValue::Map(vec![("status".into(), FmValue::scalar(s.as_str()))])
}

fn update_note(
    ctx: &SyncCtx<'_>,
    known: &Known,
    issue: &RemoteIssue,
    state: &str,
    parent: Option<&str>,
    report: &mut SyncReport,
) -> Option<FileEdit> {
    let original = read(ctx, &known.rel)?;
    let mut lines: Vec<String> = original.split('\n').map(str::to_string).collect();
    let local = find_anchor(&original, &issue.key, ctx.format)
        .and_then(|i| note::parse(&lines[i], ctx.format).map(|t| (i, t)));
    let was_out = known.fm.state.as_deref() == Some(STATE_OUT_OF_SCOPE);
    // Coming back into scope: ft wrote `Cancelled` when it left.
    let base = if was_out && state != STATE_OUT_OF_SCOPE {
        Some(Status::Cancelled)
    } else {
        known
            .fm
            .synced_status
            .as_deref()
            .and_then(note::parse_status)
    };
    let remote = target_status(issue, state);
    let local_status = local.as_ref().map(|(_, t)| t.status);
    let (status, synced, conflict) = match decide(local_status, base, remote) {
        StatusDecision::Write(s) => (s, s, None),
        StatusDecision::Keep { pending, conflict } => {
            let l = local_status.unwrap_or(remote);
            if pending {
                report.pending.push(issue.key.clone());
            }
            if let Some(r) = conflict {
                report.conflicts.push((issue.key.clone(), l, r));
                report.conflict_notes.push(known.rel.clone());
            }
            (l, base.unwrap_or(l), conflict)
        }
    };
    let new_anchor = note::anchor_line(
        issue,
        local.as_ref().map(|(i, t)| (lines[*i].as_str(), t)),
        status,
        issue.resolved.or(Some(ctx.today)),
        ctx.format,
    );
    match &local {
        Some((i, _)) => lines[*i] = new_anchor,
        None => {
            let at = insertion_point(&lines);
            lines.splice(at..at, [new_anchor, String::new()]);
        }
    }
    let mut content = lines.join("\n");
    if ctx.cfg.description {
        if let Some(c) =
            note::replace_section(&content, DESCRIPTION, &note::description_body(issue))
        {
            content = c;
        }
    }
    if ctx.cfg.comments > 0 {
        let body = note::comments_body(issue, ctx.cfg.comments);
        if let Some(c) = note::replace_section(&content, COMMENTS, &body) {
            content = c;
        }
    }
    let content = write_frontmatter(&content, ctx.source, issue, state, synced, conflict, parent);
    if state == STATE_OUT_OF_SCOPE && !was_out {
        report.out_of_scope.push(issue.key.clone());
    }
    if content == original {
        return None;
    }
    report.updated.push(issue.key.clone());
    Some(FileEdit {
        path: ctx.vault_root.join(&known.rel),
        original,
        new: content,
    })
}

/// A tracked issue the remote no longer returns at all: treated like a
/// still-open out-of-scope issue — cancelled (unless a local status change
/// is pending) and marked out of scope.
fn retire_note(ctx: &SyncCtx<'_>, known: &Known, report: &mut SyncReport) -> Option<FileEdit> {
    let original = read(ctx, &known.rel)?;
    let mut lines: Vec<String> = original.split('\n').map(str::to_string).collect();
    let base = known
        .fm
        .synced_status
        .as_deref()
        .and_then(note::parse_status);
    let mut synced = base;
    if let Some(i) = find_anchor(&original, &known.fm.key, ctx.format) {
        if let Some(mut t) = note::parse(&lines[i], ctx.format) {
            let untouched = base.is_none_or(|b| b == t.status);
            if untouched && t.status != Status::Cancelled {
                let ws = lines[i][..lines[i].len() - lines[i].trim_start().len()].to_string();
                t.status = Status::Cancelled;
                t.done = None;
                t.cancelled = Some(ctx.today);
                t.indent_level = 0;
                lines[i] = format!("{ws}{}", ctx.format.serialize_line(&t));
                synced = Some(Status::Cancelled);
            } else if !untouched {
                report.pending.push(known.fm.key.clone());
            }
        }
    }
    let mut updates = vec![("state", Some(FmValue::scalar(STATE_OUT_OF_SCOPE)))];
    if let Some(s) = synced {
        updates.push(("synced", Some(synced_entry(s))));
    }
    let content = frontmatter::upsert_ft_keys(&lines.join("\n"), "source", &updates);
    if content == original {
        return None;
    }
    report.out_of_scope.push(known.fm.key.clone());
    report.updated.push(known.fm.key.clone());
    Some(FileEdit {
        path: ctx.vault_root.join(&known.rel),
        original,
        new: content,
    })
}

/// After the first `# ` heading, else right after the frontmatter.
fn insertion_point(lines: &[String]) -> usize {
    let body_start = if lines.first().map(String::as_str) == Some("---") {
        lines
            .iter()
            .skip(1)
            .position(|l| l == "---")
            .map_or(0, |p| p + 2)
    } else {
        0
    };
    lines[body_start..]
        .iter()
        .position(|l| l.starts_with("# "))
        .map_or(body_start, |p| body_start + p + 1)
}

fn write_frontmatter(
    content: &str,
    source: &str,
    issue: &RemoteIssue,
    state: &str,
    synced: Status,
    conflict: Option<Status>,
    parent: Option<&str>,
) -> String {
    let content = frontmatter::upsert_ft_keys(
        content,
        "tasks",
        &[
            ("anchor", Some(FmValue::scalar(&issue.key))),
            ("parent", parent.map(FmValue::scalar)),
        ],
    );
    frontmatter::upsert_ft_keys(
        &content,
        "source",
        &note::source_entries(source, issue, state, synced, conflict),
    )
}

fn new_note(
    ctx: &SyncCtx<'_>,
    rel: &Path,
    issue: &RemoteIssue,
    state: &str,
    parent: Option<&str>,
    warnings: &mut Vec<String>,
) -> FileEdit {
    let status = target_status(issue, state);
    let anchor = note::anchor_line(
        issue,
        None,
        status,
        issue.resolved.or(Some(ctx.today)),
        ctx.format,
    );
    let title = format!("{} {}", issue.key, note::clean_title(&issue.title));
    let (template_fm, template_body) = render_template(ctx, issue, &title, warnings);
    let mut parts = vec![format!("# {title}"), anchor];
    if !template_body.trim().is_empty() {
        parts.push(template_body.trim().to_string());
    }
    if ctx.cfg.description {
        parts.push(note::section(DESCRIPTION, &note::description_body(issue)));
    }
    if ctx.cfg.comments > 0 {
        parts.push(note::section(
            COMMENTS,
            &note::comments_body(issue, ctx.cfg.comments),
        ));
    }
    let body = parts.join("\n\n") + "\n";
    let content = match template_fm {
        Some(fm) => format!("---\n{fm}\n---\n{body}"),
        None => body,
    };
    FileEdit {
        path: ctx.vault_root.join(rel),
        original: String::new(),
        new: write_frontmatter(&content, ctx.source, issue, state, status, None, parent),
    }
}

/// Render the source's template (if any) into `(frontmatter, body)`. The
/// render date is the sync date — only used when a note is first created.
fn render_template(
    ctx: &SyncCtx<'_>,
    issue: &RemoteIssue,
    title: &str,
    warnings: &mut Vec<String>,
) -> (Option<String>, String) {
    let Some(name) = &ctx.cfg.template else {
        return (None, String::new());
    };
    let mut path = ctx.templates_dir.join(name);
    if path.extension().is_none() {
        path.set_extension("md");
    }
    let Ok(src) = std::fs::read_to_string(&path) else {
        warnings.push(format!("template {} not found", path.display()));
        return (None, String::new());
    };
    let mut tctx = crate::notes::template::TemplateContext::new(
        title,
        ctx.today,
        ctx.today.and_hms_opt(0, 0, 0).expect("midnight exists"),
    );
    for (k, v) in [
        ("key", issue.key.as_str()),
        ("title", issue.title.as_str()),
        ("url", issue.url.as_str()),
        ("source", ctx.source),
    ] {
        tctx.vars.insert(k.into(), v.into());
    }
    let out = match crate::notes::template::render(&src, &tctx) {
        Ok(out) => out,
        Err(e) => {
            warnings.push(format!("template {}: {e}", path.display()));
            return (None, String::new());
        }
    };
    match (
        frontmatter::block(&out),
        frontmatter::frontmatter_end_line(&out),
    ) {
        (Some(fm), Some(end)) => {
            let body: Vec<&str> = out.split('\n').skip(end as usize).collect();
            (Some(fm.to_string()), body.join("\n"))
        }
        _ => (None, out),
    }
}

/// Which side wins when resolving a stored conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Take {
    Local,
    Remote,
}

/// Resolve the conflict stored in an issue note's frontmatter. Take remote
/// writes the remote status (dating `✅` / `❌` with `today`); take local
/// re-bases `synced.status` on the remote status so the local status
/// becomes a pending push. `None` when the note has no stored conflict.
pub fn resolve_conflict(
    content: &str,
    take: Take,
    today: NaiveDate,
    format: &dyn TaskFormat,
) -> Option<String> {
    let fm = frontmatter::ft_source_in(frontmatter::block(content)?)?;
    let remote = note::parse_status(fm.conflict.as_deref()?)?;
    let mut lines: Vec<String> = content.split('\n').map(str::to_string).collect();
    if take == Take::Remote {
        let i = find_anchor(content, &fm.key, format)?;
        let mut t = note::parse(&lines[i], format)?;
        let ws = lines[i][..lines[i].len() - lines[i].trim_start().len()].to_string();
        if t.status != remote {
            t.status = remote;
            t.done = (remote == Status::Done).then_some(today);
            t.cancelled = (remote == Status::Cancelled).then_some(today);
        }
        t.indent_level = 0;
        lines[i] = format!("{ws}{}", format.serialize_line(&t));
    }
    Some(frontmatter::upsert_ft_keys(
        &lines.join("\n"),
        "source",
        &[("synced", Some(synced_entry(remote))), ("conflict", None)],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::Comment;
    use crate::task::emoji::EmojiFormat;
    use crate::task::Priority;

    fn cfg() -> SourceCfg {
        toml::from_str("kind = \"jira\"\nemail = \"fake-user@example.test\"\nbase_url = \"https://x.test\"\ncomments = 2\n").unwrap()
    }

    fn issue(key: &str) -> RemoteIssue {
        RemoteIssue {
            key: key.into(),
            url: format!("https://x.test/browse/{key}"),
            title: "Fix login".into(),
            status_name: "In Review".into(),
            status: Status::InProgress,
            priority: Some(Priority::High),
            due: NaiveDate::from_ymd_opt(2026, 10, 30),
            assignee: Some("Jane Doe".into()),
            updated: "2026-10-21T09:12:44.000+0000".into(),
            description: "Steps:\n- [ ] run migration".into(),
            comments: vec![Comment {
                author: "Amy".into(),
                created: "2026-10-19T10:00:00.000+0000".into(),
                body: "looks good".into(),
            }],
            ..Default::default()
        }
    }

    struct V {
        dir: tempfile::TempDir,
        cfg: SourceCfg,
    }

    impl V {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join(".obsidian")).unwrap();
            V { dir, cfg: cfg() }
        }
        fn write(&self, rel: &str, body: &str) {
            let p = self.dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        fn read(&self, rel: &str) -> String {
            std::fs::read_to_string(self.dir.path().join(rel)).unwrap()
        }
        fn scan(&self) -> Scan {
            crate::scan::scan_vault(self.dir.path(), &[])
        }
        fn sync(&self, fetched: &FetchResult) -> SyncReport {
            let scan = self.scan();
            let tdir = self.dir.path().join("templates-ft");
            let ctx = SyncCtx {
                vault_root: self.dir.path(),
                templates_dir: &tdir,
                scan: &scan,
                source: "jira",
                cfg: &self.cfg,
                format: &EmojiFormat,
                today: NaiveDate::from_ymd_opt(2026, 10, 21).unwrap(),
            };
            let plan = plan_sync(&ctx, fetched);
            apply_sync_plan(&plan).unwrap();
            plan.report
        }
    }

    fn active(issues: Vec<RemoteIssue>) -> FetchResult {
        FetchResult {
            active: issues,
            ..Default::default()
        }
    }

    const NEW_NOTE: &str = "---
ft:
  tasks:
    anchor: ABC-123
    parent: EPIC-7
  source:
    name: jira
    key: ABC-123
    url: https://x.test/browse/ABC-123
    status: In Review
    assignee: Jane Doe
    state: active
    updated: 2026-10-21T09:12:44.000+0000
    synced:
      status: InProgress
---
# ABC-123 Fix login

- [/] [ABC-123](https://x.test/browse/ABC-123) Fix login ⏫ 📅 2026-10-30 🆔 ABC-123

<!-- ft:source:description -->
## Description

Steps:
- ☐ run migration
<!-- /ft:source:description -->

<!-- ft:source:comments -->
## Comments

### Amy — 2026-10-19

looks good
<!-- /ft:source:comments -->
";

    #[test]
    fn creates_issue_and_context_parent_notes() {
        let v = V::new();
        let mut i = issue("ABC-123");
        i.parent = Some("EPIC-7".into());
        let mut epic = issue("EPIC-7");
        epic.title = "Login revamp".into();
        let r = v.sync(&FetchResult {
            active: vec![i],
            context: vec![epic],
            ..Default::default()
        });
        assert_eq!(r.created, ["ABC-123", "EPIC-7"]);
        assert_eq!(v.read("jira/ABC-123.md"), NEW_NOTE);
        assert!(v.read("jira/EPIC-7.md").contains("    state: context\n"));
        // The remote checklist is not a task; the anchor is; the parent
        // link resolves through the epic's anchor note.
        let scan = v.scan();
        let tasks: Vec<_> = scan
            .tasks
            .iter()
            .filter(|t| t.source_file == Path::new("jira/ABC-123.md"))
            .collect();
        assert_eq!(tasks.len(), 1);
        assert!(scan
            .tasks
            .iter()
            .all(|t| !t.description.contains("run migration")));
        let parents = crate::task::hierarchy::parent_keys(tasks[0]);
        assert_eq!(parents.len(), 1, "epic parent resolves");
        assert_eq!(parents[0].0, Path::new("jira/EPIC-7.md"));
    }

    #[test]
    fn second_sync_is_a_noop_and_output_is_deterministic() {
        let v = V::new();
        let w = V::new();
        v.sync(&active(vec![issue("ABC-123")]));
        w.sync(&active(vec![issue("ABC-123")]));
        assert_eq!(v.read("jira/ABC-123.md"), w.read("jira/ABC-123.md"));
        let r = v.sync(&active(vec![issue("ABC-123")]));
        assert!(r.created.is_empty() && r.updated.is_empty(), "{r:?}");
    }

    #[test]
    fn update_overwrites_owned_fields_and_keeps_user_text() {
        let v = V::new();
        v.sync(&active(vec![issue("ABC-123")]));
        let mut note = v.read("jira/ABC-123.md");
        note = note.replace(
            "Fix login ⏫ 📅 2026-10-30 🆔",
            "Fix login #work ⏫ ⏳ 2026-10-25 📅 2026-10-30 🆔",
        );
        note.push_str("\n## My notes\n\n- [ ] local subtask [[ABC-123]]\n");
        note = note.replace(
            "📅 2026-10-30 🆔 ABC-123\n",
            "📅 2026-10-30 🆔 ABC-123\n  - [ ] nested local\n",
        );
        v.write("jira/ABC-123.md", &note);

        let mut i = issue("ABC-123");
        i.title = "Fix login on Safari".into();
        i.due = None;
        i.status_name = "Code Review".into();
        let r = v.sync(&active(vec![i]));
        assert_eq!(r.updated, ["ABC-123"]);
        let after = v.read("jira/ABC-123.md");
        assert!(after.contains(
            "- [/] [ABC-123](https://x.test/browse/ABC-123) Fix login on Safari #work ⏫ ⏳ 2026-10-25 🆔 ABC-123\n  - [ ] nested local\n"
        ), "{after}");
        assert!(after.contains("    status: Code Review\n"));
        assert!(after.ends_with("## My notes\n\n- [ ] local subtask [[ABC-123]]\n"));
    }

    #[test]
    fn pending_and_conflict() {
        let v = V::new();
        v.sync(&active(vec![issue("ABC-123")]));
        let ticked = v
            .read("jira/ABC-123.md")
            .replace("- [/] [ABC-123]", "- [x] [ABC-123]");
        v.write("jira/ABC-123.md", &ticked);
        let r = v.sync(&active(vec![issue("ABC-123")]));
        assert_eq!(r.pending, ["ABC-123"]);
        assert!(v.read("jira/ABC-123.md").contains("- [x] [ABC-123]"));
        assert!(v
            .read("jira/ABC-123.md")
            .contains("      status: InProgress\n"));

        let mut cancelled = issue("ABC-123");
        cancelled.status = Status::Cancelled;
        let r = v.sync(&active(vec![cancelled]));
        assert_eq!(
            r.conflicts,
            [("ABC-123".to_string(), Status::Done, Status::Cancelled)]
        );
        assert!(v.read("jira/ABC-123.md").contains("- [x] [ABC-123]"));

        let mut done = issue("ABC-123");
        done.status = Status::Done;
        let r = v.sync(&active(vec![done]));
        assert!(r.pending.is_empty() && r.conflicts.is_empty());
        assert!(v.read("jira/ABC-123.md").contains("      status: Done\n"));
    }

    #[test]
    fn remote_status_change_is_written_with_dates() {
        let v = V::new();
        v.sync(&active(vec![issue("ABC-123")]));
        let mut done = issue("ABC-123");
        done.status = Status::Done;
        done.resolved = NaiveDate::from_ymd_opt(2026, 10, 20);
        v.sync(&active(vec![done]));
        assert!(v.read("jira/ABC-123.md").contains("- [x] [ABC-123](https://x.test/browse/ABC-123) Fix login ⏫ 📅 2026-10-30 ✅ 2026-10-20 🆔 ABC-123"));
    }

    #[test]
    fn out_of_scope_open_is_cancelled_closed_is_done_and_reactivation() {
        let v = V::new();
        v.sync(&active(vec![issue("A-1"), issue("A-2")]));
        assert_eq!(
            known_issues(&v.scan(), "jira")
                .iter()
                .map(|r| r.key.as_str())
                .collect::<Vec<_>>(),
            ["A-1", "A-2"]
        );
        let mut closed = issue("A-2");
        closed.status = Status::Done;
        closed.resolved = NaiveDate::from_ymd_opt(2026, 10, 20);
        let r = v.sync(&FetchResult {
            rechecked: vec![issue("A-1"), closed],
            ..Default::default()
        });
        assert_eq!(r.out_of_scope, ["A-1", "A-2"]);
        let a1 = v.read("jira/A-1.md");
        assert!(
            a1.contains("- [-] [A-1]")
                && a1.contains("❌ 2026-10-21")
                && a1.contains("state: out-of-scope"),
            "{a1}"
        );
        assert!(a1.contains("      status: Cancelled\n"));
        assert!(v.read("jira/A-2.md").contains("✅ 2026-10-20"));
        assert!(known_issues(&v.scan(), "jira").is_empty());

        let r = v.sync(&active(vec![issue("A-1")]));
        assert_eq!(r.updated, ["A-1"]);
        let a1 = v.read("jira/A-1.md");
        assert!(
            a1.contains("- [/] [A-1]") && !a1.contains("❌") && a1.contains("state: active"),
            "{a1}"
        );
    }

    #[test]
    fn missing_issue_is_retired_unless_pending() {
        let v = V::new();
        v.sync(&active(vec![issue("A-1"), issue("A-2")]));
        let pending = v.read("jira/A-2.md").replace("- [/] [A-2]", "- [x] [A-2]");
        v.write("jira/A-2.md", &pending);
        let r = v.sync(&FetchResult {
            missing: vec!["A-1".into(), "A-2".into()],
            ..Default::default()
        });
        assert_eq!(r.out_of_scope, ["A-1", "A-2"]);
        assert_eq!(r.pending, ["A-2"]);
        assert!(v.read("jira/A-1.md").contains("- [-] [A-1]"));
        assert!(v.read("jira/A-2.md").contains("- [x] [A-2]"));
    }

    #[test]
    fn moved_note_collisions_removed_sections_and_missing_anchor() {
        let v = V::new();
        v.sync(&active(vec![issue("A-1")]));
        // Move the note; drop the comments section and the anchor line.
        let moved = v.read("jira/A-1.md");
        let start = moved.find("<!-- ft:source:comments -->").unwrap();
        let mut moved = moved[..start].to_string();
        let anchor = moved
            .lines()
            .find(|l| l.contains("🆔 A-1"))
            .unwrap()
            .to_string();
        moved = moved.replace(&format!("{anchor}\n"), "");
        std::fs::remove_file(v.dir.path().join("jira/A-1.md")).unwrap();
        v.write("projects/A-1.md", &moved);
        // An unrelated note squats the default path of a new issue.
        v.write("jira/A-2.md", "my own note\n");

        let r = v.sync(&active(vec![issue("A-1"), issue("A-2")]));
        assert_eq!(r.created, ["A-2"]);
        assert!(!v.dir.path().join("jira/A-1.md").exists());
        let a1 = v.read("projects/A-1.md");
        assert!(a1.contains("# A-1 Fix login\n- [/] [A-1]"), "{a1}");
        assert!(!a1.contains("ft:source:comments"));
        assert_eq!(v.read("jira/A-2.md"), "my own note\n");
        assert!(v.read("jira/A-2 (jira).md").contains("key: A-2"));
        assert!(r.warnings.iter().any(|w| w.contains("name taken")));
    }

    #[test]
    fn template_body_and_frontmatter() {
        let v = V::new();
        v.write(
            "templates-ft/jira-issue.md",
            "---\ntags: [work]\n---\n## Notes for {{ vars.key }}\n\n- [ ] triage {{ vars.title }}\n",
        );
        let mut c = cfg();
        c.template = Some("jira-issue".into());
        let v = V { cfg: c, ..v };
        v.sync(&active(vec![issue("A-1")]));
        let n = v.read("jira/A-1.md");
        assert!(
            n.starts_with("---\ntags: [work]\nft:\n  tasks:\n    anchor: A-1\n"),
            "{n}"
        );
        assert!(n.contains("🆔 A-1\n\n## Notes for A-1\n\n- [ ] triage Fix login\n\n<!-- ft:source:description -->"), "{n}");
    }

    #[test]
    fn duplicate_notes_warn_and_update_first() {
        let v = V::new();
        v.sync(&active(vec![issue("A-1")]));
        let copy = v.read("jira/A-1.md");
        v.write("zz/A-1 copy.md", &copy);
        let mut i = issue("A-1");
        i.title = "Renamed".into();
        let r = v.sync(&active(vec![i]));
        assert!(r
            .warnings
            .iter()
            .any(|w| w.contains("duplicate note for A-1")));
        assert!(v.read("jira/A-1.md").contains("Renamed"));
        assert!(!v.read("zz/A-1 copy.md").contains("Renamed"));
    }

    fn query(scan: &Scan, dsl: &str) -> Vec<String> {
        use crate::graph::query::{parse_with, Profile};
        let g = crate::graph::Graph::build(scan).unwrap();
        let q = parse_with(
            dsl,
            Profile::Tasks,
            NaiveDate::from_ymd_opt(2026, 10, 21).unwrap(),
        )
        .unwrap();
        let keys = crate::task::resolve::by_query(&g, &q);
        let mut out: Vec<String> = scan
            .tasks
            .iter()
            .filter(|t| keys.contains(&(t.source_file.clone(), t.source_line)))
            .map(|t| t.id.clone().unwrap_or_else(|| t.description.clone()))
            .collect();
        out.sort();
        out
    }

    #[test]
    fn remote_dsl_attributes_and_preset() {
        let v = V::new();
        let mut review = issue("A-2");
        review.status_name = "Code Review".into();
        let mut done = issue("A-3");
        done.status = Status::Done;
        v.sync(&active(vec![issue("A-1"), review, done]));
        v.write("j.md", "- [ ] local sub ⤴ A-1\n");
        let scan = v.scan();
        assert_eq!(query(&scan, "source = jira"), ["A-1", "A-2", "A-3"]);
        assert_eq!(query(&scan, "key = \"A-2\""), ["A-2"]);
        assert_eq!(query(&scan, "remote_status = \"Code Review\""), ["A-2"]);
        assert_eq!(
            query(&scan, crate::query::preset::builtin("remote").unwrap()),
            ["A-1", "A-2"]
        );
        assert_eq!(query(&scan, "source is null"), ["local sub"]);
        assert_eq!(query(&scan, "parent includes \"Fix login\""), ["local sub"]);
    }

    #[test]
    fn conflict_is_stored_cleared_and_resolvable() {
        let v = V::new();
        v.sync(&active(vec![issue("ABC-123")]));
        let ticked = v
            .read("jira/ABC-123.md")
            .replace("- [/] [ABC-123]", "- [x] [ABC-123]");
        v.write("jira/ABC-123.md", &ticked);
        let mut cancelled = issue("ABC-123");
        cancelled.status = Status::Cancelled;
        v.sync(&active(vec![cancelled.clone()]));
        let n = v.read("jira/ABC-123.md");
        assert!(n.contains("    conflict: Cancelled\n"), "{n}");
        let today = NaiveDate::from_ymd_opt(2026, 10, 22).unwrap();

        // Take local: synced re-based on remote, conflict gone → pending.
        let local = resolve_conflict(&n, Take::Local, today, &EmojiFormat).unwrap();
        assert!(local.contains("      status: Cancelled\n") && !local.contains("conflict:"));
        assert!(local.contains("- [x] [ABC-123]"));
        v.write("jira/ABC-123.md", &local);
        let r = v.sync(&active(vec![cancelled.clone()]));
        assert_eq!(r.pending, ["ABC-123"]);
        assert!(r.conflicts.is_empty());

        // Take remote on a fresh conflict.
        let remote = resolve_conflict(&n, Take::Remote, today, &EmojiFormat).unwrap();
        assert!(
            remote.contains("- [-] [ABC-123]") && remote.contains("❌ 2026-10-22"),
            "{remote}"
        );
        assert!(remote.contains("      status: Cancelled\n") && !remote.contains("conflict:"));
        assert_eq!(
            resolve_conflict(&remote, Take::Remote, today, &EmojiFormat),
            None
        );

        // Remote agreeing with local clears a stored conflict.
        v.write("jira/ABC-123.md", &n);
        let mut done = issue("ABC-123");
        done.status = Status::Done;
        v.sync(&active(vec![done]));
        assert!(!v.read("jira/ABC-123.md").contains("conflict:"));
    }
}

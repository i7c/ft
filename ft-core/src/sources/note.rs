//! The issue-note format: neutralising remote text, the anchor task line,
//! the managed `Description` / `Comments` sections, and frontmatter values.
//! Pure string functions; [`super::sync`] decides what to write where.

use chrono::NaiveDate;

use super::RemoteIssue;
use crate::frontmatter::FmValue;
use crate::task::format::{ParseContext, TaskFormat};
use crate::task::{Status, Task};

pub const DESCRIPTION: &str = "description";
pub const COMMENTS: &str = "comments";

/// `<!-- ft:source:<name> -->` / `<!-- /ft:source:<name> -->`.
pub fn markers(name: &str) -> (String, String) {
    (
        format!("<!-- ft:source:{name} -->"),
        format!("<!-- /ft:source:{name} -->"),
    )
}

/// Make remote markdown safe to embed in a vault note: task-shaped lines
/// become `☐` / `☑` items (no phantom tasks), headings are demoted two
/// levels (never shallower than `###`), a bare `---` becomes `***` (no fake
/// frontmatter fence), and managed-section markers are stripped. Fenced
/// code is left alone apart from marker stripping.
pub fn neutralise(md: &str) -> String {
    let mut out = Vec::new();
    let mut fence: Option<&str> = None;
    for raw in md.lines() {
        let line = raw
            .replace("<!-- ft:source:", "<!-- ft-source:")
            .replace("<!-- /ft:source:", "<!-- /ft-source:");
        let trimmed = line.trim_start();
        if let Some(f) = fence {
            if trimmed.starts_with(f) {
                fence = None;
            }
            out.push(line);
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(if trimmed.starts_with("```") {
                "```"
            } else {
                "~~~"
            });
            out.push(line);
            continue;
        }
        if line.trim() == "---" {
            out.push("***".to_string());
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            let ws = &line[..line.len() - trimmed.len()];
            out.push(format!(
                "{ws}{}{}",
                "#".repeat((hashes + 2).min(6)),
                &trimmed[hashes..]
            ));
            continue;
        }
        out.push(neutralise_checkbox(&line));
    }
    out.join("\n")
}

/// `- [ ] x` → `- ☐ x`, `1. [x] y` → `1. ☑ y`; other lines unchanged.
fn neutralise_checkbox(line: &str) -> String {
    let trimmed = line.trim_start();
    let ws = &line[..line.len() - trimmed.len()];
    let marker_len =
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            2
        } else {
            let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
            if digits > 0
                && (trimmed[digits..].starts_with(". ") || trimmed[digits..].starts_with(") "))
            {
                digits + 2
            } else {
                return line.to_string();
            }
        };
    let rest = &trimmed[marker_len..];
    let mut chars = rest.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some('['), Some(c), Some(']')) => {
            let after = &rest[2 + c.len_utf8()..];
            let mark = if c == ' ' { "☐" } else { "☑" };
            format!("{ws}{}{mark}{after}", &trimmed[..marker_len])
        }
        _ => line.to_string(),
    }
}

/// Characters that would be read as task fields or links on an anchor
/// line, stripped from remote titles.
const TITLE_HAZARDS: &[&str] = &[
    "📅", "⏳", "🛫", "➕", "✅", "❌", "🆔", "⛔", "🔁", "🏁", "🔺", "⏫", "🔼", "🔽", "⏬",
];

/// A remote title made safe for an anchor line.
pub fn clean_title(title: &str) -> String {
    let mut t = title
        .replace(['\n', '\r'], " ")
        .replace("[[", "[")
        .replace("]]", "]");
    for h in TITLE_HAZARDS {
        t = t.replace(h, "");
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The managed `Description` section body.
pub fn description_body(issue: &RemoteIssue) -> String {
    let d = neutralise(issue.description.trim());
    let d = if d.trim().is_empty() {
        "_No description._".to_string()
    } else {
        d
    };
    format!("## Description\n\n{d}")
}

/// The managed `Comments` section body: the last `n` comments, oldest first.
pub fn comments_body(issue: &RemoteIssue, n: usize) -> String {
    let start = issue.comments.len().saturating_sub(n);
    let shown: Vec<String> = issue.comments[start..]
        .iter()
        .map(|c| {
            let date = c.created.get(..10).unwrap_or(&c.created);
            let author = c.author.replace(['\n', '\r'], " ");
            format!("### {author} — {date}\n\n{}", neutralise(c.body.trim()))
        })
        .collect();
    let body = if shown.is_empty() {
        "_No comments._".to_string()
    } else {
        shown.join("\n\n")
    };
    format!("## Comments\n\n{body}")
}

/// A managed section with its markers.
pub fn section(name: &str, body: &str) -> String {
    let (open, close) = markers(name);
    format!("{open}\n{body}\n{close}")
}

/// Replace the text between `name`'s markers in `content`. `None` when the
/// markers aren't both present (the user removed the section).
pub fn replace_section(content: &str, name: &str, body: &str) -> Option<String> {
    let (open, close) = markers(name);
    let start = content.find(&open)?;
    let end = content[start..].find(&close)? + start + close.len();
    Some(format!(
        "{}{}{}",
        &content[..start],
        section(name, body),
        &content[end..]
    ))
}

/// The trailing run of `#tag` words of an anchor description — the user's
/// local tags, kept across syncs.
pub fn local_tags(description: &str) -> Vec<String> {
    let mut tags: Vec<String> = description
        .split_whitespace()
        .rev()
        .take_while(|w| w.starts_with('#') && w.len() > 1)
        .map(str::to_string)
        .collect();
    tags.reverse();
    tags
}

/// The owned part of an anchor description: `[KEY](url) title`.
pub fn owned_description(issue: &RemoteIssue) -> String {
    format!(
        "[{}]({}) {}",
        issue.key,
        issue.url,
        clean_title(&issue.title)
    )
}

/// Build the anchor task line for `issue`. `local` is the existing line's
/// task (kept: indentation, local tags, scheduled/start/created,
/// recurrence, block id, unknown fields); `status` and its `done` /
/// `cancelled` date are decided by the caller.
pub fn anchor_line(
    issue: &RemoteIssue,
    local: Option<(&str, &Task)>,
    status: Status,
    status_date: Option<NaiveDate>,
    format: &dyn TaskFormat,
) -> String {
    let mut task = local.map(|(_, t)| t.clone()).unwrap_or_default();
    let tags = local
        .map(|(_, t)| local_tags(&t.description))
        .unwrap_or_default();
    let mut description = owned_description(issue);
    for t in &tags {
        if !description.ends_with(t.as_str()) {
            description.push(' ');
            description.push_str(t);
        }
    }
    task.tags = crate::task::emoji::extract_tags(&description);
    task.description = description;
    task.priority = issue.priority;
    task.due = issue.due;
    task.id = Some(issue.key.clone());
    task.indent_level = 0;
    if task.status != status || local.is_none() {
        task.status = status;
        task.done = None;
        task.cancelled = None;
        match status {
            Status::Done => task.done = status_date,
            Status::Cancelled => task.cancelled = status_date,
            _ => {}
        }
    }
    let ws = local
        .map(|(line, _)| &line[..line.len() - line.trim_start().len()])
        .unwrap_or("");
    format!("{ws}{}", format.serialize_line(&task))
}

/// Parse a line as a task (no source context).
pub fn parse(line: &str, format: &dyn TaskFormat) -> Option<Task> {
    format.parse_line(line, ParseContext::default())
}

/// `Status` from its `as_str` form.
pub fn parse_status(s: &str) -> Option<Status> {
    Some(match s {
        "Open" => Status::Open,
        "InProgress" => Status::InProgress,
        "Done" => Status::Done,
        "Cancelled" => Status::Cancelled,
        _ => return None,
    })
}

/// `ft.source` values, in their fixed order.
pub fn source_entries<'a>(
    source: &'a str,
    issue: &'a RemoteIssue,
    state: &'a str,
    synced: Status,
    conflict: Option<Status>,
) -> Vec<(&'static str, Option<FmValue>)> {
    vec![
        ("name", Some(FmValue::scalar(source))),
        ("key", Some(FmValue::scalar(&issue.key))),
        ("url", Some(FmValue::scalar(&issue.url))),
        ("status", Some(FmValue::scalar(&issue.status_name))),
        ("assignee", issue.assignee.as_deref().map(FmValue::scalar)),
        ("state", Some(FmValue::scalar(state))),
        ("updated", Some(FmValue::scalar(&issue.updated))),
        (
            "synced",
            Some(FmValue::Map(vec![(
                "status".into(),
                FmValue::scalar(synced.as_str()),
            )])),
        ),
        ("conflict", conflict.map(|s| FmValue::scalar(s.as_str()))),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::emoji::EmojiFormat;
    use crate::task::Priority;

    #[test]
    fn neutralise_rules() {
        let md = "# Title\n- [ ] run migration\n  * [x] done thing\n3. [/] half\n---\n```\n- [ ] in code\n# not heading\n```\n<!-- /ft:source:comments -->\nplain [ ] text";
        assert_eq!(
            neutralise(md),
            "### Title\n- ☐ run migration\n  * ☑ done thing\n3. ☑ half\n***\n```\n- [ ] in code\n# not heading\n```\n<!-- /ft-source:comments -->\nplain [ ] text"
        );
    }

    #[test]
    fn neutralised_text_has_no_tasks() {
        let out = neutralise("- [ ] a\n* [x] b\n1) [ ] c");
        assert!(
            out.lines()
                .all(|l| EmojiFormat.parse_line(l, ParseContext::default()).is_none()),
            "{out}"
        );
    }

    #[test]
    fn titles_are_cleaned() {
        assert_eq!(
            clean_title("Fix 📅 parser [[x]]\nnow ⏫"),
            "Fix parser [x] now"
        );
    }

    #[test]
    fn sections_replace_and_respect_removal() {
        let c = format!("a\n{}\nb\n", section(DESCRIPTION, "old"));
        assert_eq!(
            replace_section(&c, DESCRIPTION, "new"),
            Some(format!("a\n{}\nb\n", section(DESCRIPTION, "new")))
        );
        assert_eq!(replace_section("no markers", DESCRIPTION, "x"), None);
    }

    fn issue() -> RemoteIssue {
        RemoteIssue {
            key: "ABC-123".into(),
            url: "https://x.test/browse/ABC-123".into(),
            title: "Fix login".into(),
            priority: Some(Priority::High),
            due: NaiveDate::from_ymd_opt(2026, 10, 30),
            ..Default::default()
        }
    }

    #[test]
    fn anchor_line_new_and_preserving_locals() {
        let i = issue();
        assert_eq!(
            anchor_line(&i, None, Status::InProgress, None, &EmojiFormat),
            "- [/] [ABC-123](https://x.test/browse/ABC-123) Fix login ⏫ 📅 2026-10-30 🆔 ABC-123"
        );
        let old = "  - [/] [ABC-123](https://x.test/browse/ABC-123) Old title #work #wait ⤴ p2m7rd ⏳ 2026-10-25 📅 2026-10-28 🆔 ABC-123 ^blk";
        let t = parse(old, &EmojiFormat).unwrap();
        let mut i2 = i.clone();
        i2.title = "Fix login on Safari".into();
        i2.due = None;
        assert_eq!(
            anchor_line(&i2, Some((old, &t)), Status::InProgress, None, &EmojiFormat),
            "  - [/] [ABC-123](https://x.test/browse/ABC-123) Fix login on Safari #work #wait ⤴ p2m7rd ⏫ ⏳ 2026-10-25 🆔 ABC-123 ^blk"
        );
        let done = anchor_line(
            &i2,
            Some((old, &t)),
            Status::Done,
            NaiveDate::from_ymd_opt(2026, 10, 20),
            &EmojiFormat,
        );
        assert!(
            done.contains("- [x] ") && done.contains("✅ 2026-10-20"),
            "{done}"
        );
    }

    #[test]
    fn comments_last_n() {
        let mut i = issue();
        for (n, d) in [
            ("a", "2026-10-01"),
            ("b", "2026-10-02"),
            ("c", "2026-10-03"),
        ] {
            i.comments.push(super::super::Comment {
                author: n.into(),
                created: format!("{d}T00:00:00Z"),
                body: format!("# from {n}"),
            });
        }
        assert_eq!(
            comments_body(&i, 2),
            "## Comments\n\n### b — 2026-10-02\n\n### from b\n\n### c — 2026-10-03\n\n### from c"
        );
        assert_eq!(
            comments_body(&RemoteIssue::default(), 5),
            "## Comments\n\n_No comments._"
        );
    }
}

//! Promote: turn a local subtask of a remote anchor into a real remote
//! issue, then restructure the vault so the task *is* that issue's note —
//! its block (subtasks, notes) moves under the new anchor, the original
//! lines go, and references to its block ids follow.

use std::path::{Path, PathBuf};

use super::sync::{self, SyncCtx};
use super::{note, FetchResult, IssueRef, NewIssue, RemoteIssue};
use crate::config::SourceCfg;
use crate::frontmatter;
use crate::scan::Scan;
use crate::task::format::TaskFormat;
use crate::task::links::{self, LinkCtx};
use crate::task::ops::{self, FileEdit, MovePlan, MoveSource, MovedBlock};
use crate::task::Task;

/// The remote parent a promoted task will hang under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromoteTarget {
    pub source: String,
    pub parent: IssueRef,
}

/// The issue-note identity of an anchor task, if `task` is one.
fn anchor_of(scan: &Scan, task: &Task) -> Option<frontmatter::FtSource> {
    let f = scan.files.iter().find(|f| f.rel == task.source_file)?;
    let fm = f.frontmatter.as_deref()?;
    let src = frontmatter::ft_source_in(fm)?;
    (frontmatter::ft_tasks_anchor_in(fm)? == src.key
        && task.id.as_deref() == Some(src.key.as_str()))
    .then_some(src)
}

/// Check promote's local preconditions (network ones come later).
pub fn check(
    scan: &Scan,
    task: &Task,
    sources: &std::collections::BTreeMap<String, SourceCfg>,
) -> Result<PromoteTarget, String> {
    if anchor_of(scan, task).is_some() {
        return Err("this task already is a remote issue".into());
    }
    let parent = crate::task::hierarchy::parent_key(task)
        .and_then(|k| {
            scan.tasks
                .iter()
                .find(|t| t.source_file == k.0 && t.source_line == k.1)
        })
        .and_then(|p| anchor_of(scan, p))
        .ok_or("promote needs a parent that is a remote issue — move or link it under one first")?;
    let cfg = sources
        .get(&parent.name)
        .ok_or_else(|| format!("source `{}` is not configured", parent.name))?;
    if !cfg.push {
        return Err(format!("{} is read-only (push = false)", parent.name));
    }
    Ok(PromoteTarget {
        source: parent.name,
        parent: IssueRef {
            key: parent.key,
            url: parent.url,
        },
    })
}

/// The remote title for a task: its description without wikilinks,
/// markdown links (their text is kept), or hashtags.
pub fn clean_summary(description: &str) -> String {
    let mut out = String::new();
    let mut rest = description;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("![[").or_else(|| rest.strip_prefix("[[")) {
            rest = r.find("]]").map_or("", |i| &r[i + 2..]);
            continue;
        }
        if rest.starts_with('[') {
            if let Some(close) = rest.find("](") {
                if let Some(end) = rest[close..].find(')') {
                    out.push_str(&rest[1..close]);
                    rest = &rest[close + end + 1..];
                    continue;
                }
            }
        }
        let c = rest.chars().next().expect("non-empty");
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out.split_whitespace()
        .filter(|w| !ops::is_tag_word(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `NewIssue` for a task.
pub fn new_issue(task: &Task) -> NewIssue {
    NewIssue {
        title: clean_summary(&task.description),
        due: task.due,
        priority: task.priority,
    }
}

/// What promote did locally.
#[derive(Debug, Clone)]
pub struct Promoted {
    /// Vault-relative path of the new issue note.
    pub note: PathBuf,
    /// Lines that moved under the new anchor (subtasks and notes).
    pub moved_lines: usize,
    pub edits: Vec<FileEdit>,
}

/// Plan the local side of a promote: the new issue note (anchor carrying
/// the task's block id, the task's block nested under it), the task's lines
/// removed from their file, and block-id references retargeted.
#[allow(clippy::too_many_arguments)]
pub fn plan_local(
    vault_root: &Path,
    templates_dir: &Path,
    scan: &Scan,
    source: &str,
    cfg: &SourceCfg,
    format: &dyn TaskFormat,
    today: chrono::NaiveDate,
    task: &Task,
    created: &RemoteIssue,
) -> Result<Promoted, String> {
    let sctx = SyncCtx {
        vault_root,
        templates_dir,
        scan,
        source,
        cfg,
        format,
        today,
    };
    let fetched = FetchResult {
        active: vec![created.clone()],
        ..Default::default()
    };
    let plan = sync::plan_sync(&sctx, &fetched);
    let mut note_edit = plan
        .edits
        .into_iter()
        .find(|e| e.original.is_empty())
        .ok_or_else(|| format!("{} already has a note — run ft tasks sync", created.key))?;

    // The task's block in its source file, guarded against drift.
    let src_abs = vault_root.join(&task.source_file);
    let src_original = std::fs::read_to_string(&src_abs).map_err(|e| e.to_string())?;
    let mut src_lines: Vec<String> = src_original.split('\n').map(str::to_string).collect();
    let idx = task.source_line - 1;
    let current = src_lines
        .get(idx)
        .and_then(|l| note::parse(l, format))
        .ok_or("the task's line changed on disk — rescan and retry")?;
    if format.serialize_line(&current) != format.serialize_line(task) {
        return Err("the task's line changed on disk — rescan and retry".into());
    }
    let end = ops::block_end(&src_lines, idx, current.indent_level);
    let block: Vec<String> = src_lines[idx + 1..=end].to_vec();

    // New note: anchor takes the block id, the block nests under it.
    let mut note_lines: Vec<String> = note_edit.new.split('\n').map(str::to_string).collect();
    let a = note_lines
        .iter()
        .position(|l| {
            note::parse(l, format).is_some_and(|t| t.id.as_deref() == Some(created.key.as_str()))
        })
        .ok_or("new note has no anchor line")?;
    if let Some(id) = &task.block_link {
        note_lines[a] = format!("{} ^{id}", note_lines[a].trim_end());
    }
    if !block.is_empty() {
        let head_indent = block
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| ops::leading_ws(l).len())
            .min()
            .unwrap_or(0);
        let src_unit = ops::detect_indent_unit(&src_lines);
        let dst_unit = ops::detect_indent_unit(&note_lines);
        let child_ws = format!("{}{}", ops::leading_ws(&note_lines[a]), dst_unit.as_str());
        let moved = ops::reindent_block(&block, head_indent, src_unit, dst_unit, &child_ws);
        note_lines.splice(a + 1..a + 1, moved);
    }
    note_edit.new = note_lines.join("\n");
    src_lines.drain(idx..=end);

    // A move from the task's block to the new anchor: `plan_retarget`
    // rewrites block-id references to follow it.
    let mut move_plan = MovePlan {
        edits: vec![
            FileEdit {
                path: src_abs.clone(),
                original: src_original,
                new: src_lines.join("\n"),
            },
            note_edit.clone(),
        ],
        blocks: vec![MovedBlock {
            source: MoveSource {
                path: src_abs,
                line: task.source_line,
                expected: None,
            },
            end_line: end + 1,
            task_description: task.description.clone(),
            dest: Some((note_edit.path.clone(), a + 1)),
        }],
    };
    let note_rel = note_edit
        .path
        .strip_prefix(vault_root)
        .unwrap_or(&note_edit.path)
        .to_path_buf();
    let lctx = LinkCtx::new(vault_root, scan).including(&note_rel);
    links::plan_retarget(&mut move_plan, &lctx).map_err(|e| e.to_string())?;
    Ok(Promoted {
        note: note_rel,
        moved_lines: block.len(),
        edits: move_plan.edits,
    })
}

/// Write every edit, but only if each file still holds its planned
/// original; otherwise nothing is written.
pub fn apply_guarded(edits: &[FileEdit]) -> Result<(), String> {
    for e in edits {
        let current = std::fs::read_to_string(&e.path).unwrap_or_default();
        if current != e.original {
            return Err(format!("{} changed meanwhile — retry", e.path.display()));
        }
    }
    for e in edits.iter().filter(|e| e.original != e.new) {
        crate::fs::write_atomic(&e.path, &e.new).map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::emoji::EmojiFormat;

    #[test]
    fn summaries_are_cleaned() {
        assert_eq!(
            clean_summary("write [[ABC-123]] regression [test](https://x.test) for #work Safari"),
            "write regression test for Safari"
        );
    }

    fn cfg() -> SourceCfg {
        toml::from_str("kind = \"jira\"\nemail = \"fake-user@example.test\"\nbase_url = \"https://x.test\"\ncomments = 0\n").unwrap()
    }

    fn anchor_note(key: &str) -> String {
        format!(
            "---\nft:\n  tasks:\n    anchor: {key}\n  source:\n    name: jira\n    key: {key}\n    url: https://x.test/browse/{key}\n    state: active\n    synced:\n      status: InProgress\n---\n# {key} Story\n\n- [/] [{key}](https://x.test/browse/{key}) Story 🆔 {key}\n"
        )
    }

    fn setup(files: &[(&str, &str)]) -> (tempfile::TempDir, Scan) {
        let dir = tempfile::tempdir().unwrap();
        for (rel, body) in files {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        let s = crate::scan::scan_vault(dir.path(), &[]);
        (dir, s)
    }

    fn task<'a>(s: &'a Scan, rel: &str, line: usize) -> &'a Task {
        s.tasks
            .iter()
            .find(|t| t.source_file == Path::new(rel) && t.source_line == line)
            .unwrap()
    }

    #[test]
    fn preconditions() {
        let (_d, s) = setup(&[
            ("jira/ABC-123.md", &anchor_note("ABC-123")),
            (
                "j.md",
                "- [ ] linked [[ABC-123]]\n- [ ] P\n  - [ ] local child\n",
            ),
        ]);
        let sources = [("jira".to_string(), cfg())].into_iter().collect();
        let t = check(&s, task(&s, "j.md", 1), &sources).unwrap();
        assert_eq!(t.parent.key, "ABC-123");
        assert!(check(&s, task(&s, "j.md", 3), &sources)
            .unwrap_err()
            .contains("needs a parent that is a remote issue"));
        assert!(check(&s, task(&s, "jira/ABC-123.md", 15), &sources)
            .unwrap_err()
            .contains("already is a remote issue"));
        let mut ro = cfg();
        ro.push = false;
        let sources = [("jira".to_string(), ro)].into_iter().collect();
        assert!(check(&s, task(&s, "j.md", 1), &sources)
            .unwrap_err()
            .contains("read-only"));
    }

    #[test]
    fn local_restructuring() {
        let (d, s) = setup(&[
            ("jira/ABC-123.md", &anchor_note("ABC-123")),
            (
                "journal/2026-10-21.md",
                "# Day\n- [ ] write regression test [[ABC-123]] 📅 2026-10-28 ^wr1\n  - [ ] cover Safari\n    context note\n- [ ] other\n",
            ),
            ("notes.md", "see [[2026-10-21#^wr1]]\n"),
        ]);
        let created = RemoteIssue {
            key: "ABC-124".into(),
            url: "https://x.test/browse/ABC-124".into(),
            title: "write regression test".into(),
            status_name: "To Do".into(),
            parent: Some("ABC-123".into()),
            due: chrono::NaiveDate::from_ymd_opt(2026, 10, 28),
            updated: "2026-10-21T10:00:00.000+0000".into(),
            ..Default::default()
        };
        let t = task(&s, "journal/2026-10-21.md", 2).clone();
        let p = plan_local(
            d.path(),
            &d.path().join("templates-ft"),
            &s,
            "jira",
            &cfg(),
            &EmojiFormat,
            chrono::NaiveDate::from_ymd_opt(2026, 10, 21).unwrap(),
            &t,
            &created,
        )
        .unwrap();
        assert_eq!(p.note, Path::new("jira/ABC-124.md"));
        assert_eq!(p.moved_lines, 2);
        apply_guarded(&p.edits).unwrap();
        let read = |r: &str| std::fs::read_to_string(d.path().join(r)).unwrap();
        assert_eq!(read("journal/2026-10-21.md"), "# Day\n- [ ] other\n");
        let n = read("jira/ABC-124.md");
        assert!(
            n.contains("- [ ] [ABC-124](https://x.test/browse/ABC-124) write regression test 📅 2026-10-28 🆔 ABC-124 ^wr1\n  - [ ] cover Safari\n    context note\n"),
            "{n}"
        );
        assert!(n.contains("    parent: \"[[ABC-123]]\""));
        assert_eq!(read("notes.md"), "see [[ABC-124#^wr1]]\n");
        // The vault now sees the promoted issue under the story.
        let s2 = crate::scan::scan_vault(d.path(), &[]);
        let anchor = s2
            .tasks
            .iter()
            .find(|t| t.id.as_deref() == Some("ABC-124"))
            .unwrap();
        assert_eq!(
            crate::task::hierarchy::parent_key(anchor).unwrap().0,
            Path::new("jira/ABC-123.md")
        );
        assert!(s2
            .tasks
            .iter()
            .any(|t| t.description == "cover Safari"
                && t.source_file == Path::new("jira/ABC-124.md")));
    }

    #[test]
    fn stale_line_is_refused() {
        let (d, s) = setup(&[
            ("jira/ABC-123.md", &anchor_note("ABC-123")),
            ("j.md", "- [ ] t [[ABC-123]]\n"),
        ]);
        let t = task(&s, "j.md", 1).clone();
        std::fs::write(d.path().join("j.md"), "- [ ] changed\n").unwrap();
        let created = RemoteIssue {
            key: "ABC-9".into(),
            url: "u".into(),
            ..Default::default()
        };
        let err = plan_local(
            d.path(),
            d.path(),
            &s,
            "jira",
            &cfg(),
            &EmojiFormat,
            chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            &t,
            &created,
        )
        .unwrap_err();
        assert!(err.contains("changed on disk"));
    }
}

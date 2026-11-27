//! Where a task lives and what its note is about: the enclosing headings,
//! the links it mentions, and its note's tags and dates. Printed next to
//! each task by `ft tasks list --format json|ndjson`, so a sync tool can
//! place a task without reading the vault itself.
//!
//! Kept out of [`Task`] on purpose: it is derived from the scan and the
//! graph, not from the task line, and the rows writes print stay as they
//! are.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde::Serialize;

use crate::graph::{Graph, NodeKind, NoteId};
use crate::scan::{ParsedFile, Scan};
use crate::task::Task;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TaskContext {
    /// Headings enclosing the task, outermost first; empty above the
    /// first heading.
    pub headings: Vec<HeadingRef>,
    /// Wikilink targets in the task's paragraph, the query DSL's
    /// `mentions`; deduplicated, in document order.
    pub mentions: Vec<Mention>,
    /// The subset of [`Self::mentions`] written on the task's own line.
    pub line_mentions: Vec<Mention>,
    pub note: NoteMeta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HeadingRef {
    pub text: String,
    pub level: u8,
    /// 1-indexed line of the heading.
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Mention {
    /// The note's title, or the raw link text when it resolves to no
    /// note. Never the link's alias.
    pub target: String,
    /// Vault-relative path of the resolved note; `None` when unresolved.
    /// Two notes may share a title, a path tells them apart.
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct NoteMeta {
    /// Frontmatter `tags:` / `tag:`; not the inline `#tags` of the body.
    pub tags: Vec<String>,
    /// The date the note is about: the first `YYYY-MM-DD` in the file
    /// name, else frontmatter `date_created`.
    pub date: Option<NaiveDate>,
    /// File modification time, RFC 3339 UTC to the second.
    pub modified: Option<String>,
}

/// Builds [`TaskContext`]s for the tasks of one scan and its graph.
pub struct ContextIndex<'a> {
    files: HashMap<&'a Path, &'a ParsedFile>,
    graph: &'a Graph,
}

impl<'a> ContextIndex<'a> {
    pub fn new(scan: &'a Scan, graph: &'a Graph) -> Self {
        let files = scan.files.iter().map(|f| (f.rel.as_path(), f)).collect();
        Self { files, graph }
    }

    pub fn for_task(&self, task: &Task) -> TaskContext {
        let file = self.files.get(task.source_file.as_path()).copied();
        let (mentions, line_mentions) = self.mentions(task);
        TaskContext {
            headings: file
                .map(|f| heading_path(&f.headings, task.source_line))
                .unwrap_or_default(),
            mentions,
            line_mentions,
            note: note_meta(&task.source_file, file),
        }
    }

    fn mentions(&self, task: &Task) -> (Vec<Mention>, Vec<Mention>) {
        let Some(id) = self.graph.task_by_loc(&task.source_file, task.source_line) else {
            return Default::default();
        };
        let mut all = Vec::new();
        let mut own = Vec::new();
        for (target, link) in self.graph.task_mention_links(id) {
            let Some(m) = self.mention(target) else {
                continue;
            };
            if link.line == task.source_line && !own.contains(&m) {
                own.push(m.clone());
            }
            if !all.contains(&m) {
                all.push(m);
            }
        }
        (all, own)
    }

    /// A link to a heading counts as a mention of the heading's note.
    fn mention(&self, target: NoteId) -> Option<Mention> {
        let note = |path: &Path| {
            let id = self.graph.note_by_path(path)?;
            match self.graph.node(id) {
                NodeKind::Note(n) => Some(Mention {
                    target: n.title.clone(),
                    path: Some(n.path.clone()),
                }),
                _ => None,
            }
        };
        match self.graph.node(target) {
            NodeKind::Note(n) => Some(Mention {
                target: n.title.clone(),
                path: Some(n.path.clone()),
            }),
            NodeKind::Ghost(g) => Some(Mention {
                target: g.raw.clone(),
                path: None,
            }),
            NodeKind::Heading(h) => note(&h.source_file),
            _ => None,
        }
    }
}

/// The headings enclosing `line`: a heading closes at the next heading of
/// the same or a higher level (fewer `#`s).
fn heading_path(headings: &[crate::markdown::Heading], line: usize) -> Vec<HeadingRef> {
    let mut stack: Vec<&crate::markdown::Heading> = Vec::new();
    for h in headings.iter().take_while(|h| h.line < line) {
        while stack.last().is_some_and(|top| top.level >= h.level) {
            stack.pop();
        }
        stack.push(h);
    }
    stack
        .into_iter()
        .map(|h| HeadingRef {
            text: h.text.clone(),
            level: h.level,
            line: h.line,
        })
        .collect()
}

fn note_meta(rel: &Path, file: Option<&ParsedFile>) -> NoteMeta {
    let fm = file.and_then(|f| f.frontmatter.as_deref());
    let stem = rel.file_stem().map(|s| s.to_string_lossy());
    NoteMeta {
        tags: fm.map(crate::frontmatter::tags_in).unwrap_or_default(),
        date: stem
            .and_then(|s| crate::dates::first_iso_date(&s))
            .or_else(|| fm.and_then(crate::frontmatter::date_created_in)),
        modified: file
            .map(|f| DateTime::<Utc>::from(f.mtime).to_rfc3339_opts(SecondsFormat::Secs, true)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::Heading;

    fn h(text: &str, level: u8, line: usize) -> Heading {
        Heading {
            text: text.into(),
            level,
            line,
        }
    }

    fn texts(path: Vec<HeadingRef>) -> Vec<String> {
        path.into_iter().map(|h| h.text).collect()
    }

    #[test]
    fn heading_path_nests_and_closes_sections() {
        let hs = [
            h("A", 1, 1),
            h("B", 2, 3),
            h("C", 3, 5),
            h("D", 2, 8),
            h("E", 1, 12),
        ];
        assert_eq!(texts(heading_path(&hs, 6)), ["A", "B", "C"]);
        assert_eq!(texts(heading_path(&hs, 9)), ["A", "D"]);
        assert_eq!(texts(heading_path(&hs, 13)), ["E"]);
    }

    #[test]
    fn heading_path_keeps_skipped_levels_and_is_empty_above_the_first() {
        let hs = [h("Top", 1, 3), h("Deep", 3, 5)];
        assert_eq!(texts(heading_path(&hs, 2)), Vec::<String>::new());
        assert_eq!(texts(heading_path(&hs, 6)), ["Top", "Deep"]);
        assert_eq!(texts(heading_path(&[], 6)), Vec::<String>::new());
    }

    #[test]
    fn note_date_prefers_the_file_name() {
        let meta = note_meta(Path::new("daily/2026-11-20.md"), None);
        assert_eq!(meta.date, NaiveDate::from_ymd_opt(2026, 11, 20));
        assert_eq!(meta.modified, None);
        assert_eq!(note_meta(Path::new("2026-W47.md"), None).date, None);
    }
}

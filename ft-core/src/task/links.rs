//! Logical (cross-file) task parents expressed as wikilinks: a block
//! reference to a task (`[[note#^id]]`, `[[#^id]]`) or a link to an anchor
//! note (`[[note]]` whose frontmatter names its anchor task via
//! `ft.tasks.anchor`). Resolved once per scan into [`Task::parent_link`].

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{Task, TaskKey};
use crate::graph::resolve::{resolve_wiki, Resolution, ScanNoteIndex};
use crate::graph::LinkForm;
use crate::scan::Scan;

/// A task's parent link: the first qualifying wikilink on its line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParentLink {
    /// The link's raw text, e.g. `[[proj#^vo1]]`.
    pub raw: String,
    /// Raw texts of later qualifying links on the same line (ignored).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<String>,
    pub state: ParentLinkState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParentLinkState {
    /// Points at this task; it is the effective parent.
    Resolved(TaskKey),
    /// The link names a parent that doesn't exist (missing note, missing
    /// block id, anchor note without its anchor task).
    Dangling,
    /// The task is physically nested; indentation wins over the link.
    Ignored,
    /// Following the link would make the task its own ancestor.
    Cycle,
}

impl ParentLinkState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ParentLinkState::Resolved(_) => "resolved",
            ParentLinkState::Dangling => "dangling",
            ParentLinkState::Ignored => "ignored",
            ParentLinkState::Cycle => "cycle",
        }
    }
}

/// What a block id in a note marks.
#[derive(Clone, Copy)]
enum Block {
    Task(usize),
    Other,
}

/// How one link on a task line classifies.
enum Candidate {
    Resolved(usize),
    Dangling,
}

/// Vault-wide lookups for classifying parent-link candidates, built from a
/// scan without touching the filesystem.
pub(crate) struct ParentIndex {
    pub(crate) notes: ScanNoteIndex,
    /// (file idx, block id) → what it marks. First occurrence wins.
    blocks: HashMap<(usize, String), Block>,
    /// file idx → the anchor task (`None` when `ft.tasks.anchor` is set but
    /// no task carries that 🆔).
    anchors: HashMap<usize, Option<usize>>,
}

impl ParentIndex {
    pub(crate) fn build(scan: &Scan) -> Self {
        let notes = ScanNoteIndex::new(scan.files.iter().map(|f| f.rel.as_path()));
        let file_of: HashMap<&std::path::Path, usize> = scan
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| (f.rel.as_path(), i))
            .collect();
        let task_at: HashMap<(usize, usize), usize> = scan
            .tasks
            .iter()
            .enumerate()
            .filter_map(|(ti, t)| {
                Some(((*file_of.get(t.source_file.as_path())?, t.source_line), ti))
            })
            .collect();

        let mut blocks = HashMap::new();
        let mut anchors = HashMap::new();
        for (fi, f) in scan.files.iter().enumerate() {
            for (id, line) in &f.block_ids {
                let block = match task_at.get(&(fi, *line)) {
                    Some(&ti) if scan.tasks[ti].block_link.as_deref() == Some(id) => {
                        Block::Task(ti)
                    }
                    _ => Block::Other,
                };
                blocks.entry((fi, id.clone())).or_insert(block);
            }
            if let Some(anchor) = f
                .frontmatter
                .as_deref()
                .and_then(crate::frontmatter::ft_tasks_anchor_in)
            {
                let task = scan.tasks.iter().position(|t| {
                    t.source_file == f.rel && t.id.as_deref() == Some(anchor.as_str())
                });
                anchors.insert(fi, task);
            }
        }
        Self {
            notes,
            blocks,
            anchors,
        }
    }

    /// Classify a wikilink written in file `from` (index into the scan's
    /// files). `None` means it is an ordinary link, not a parent candidate.
    fn classify(&self, from: usize, target: &str, anchor: Option<&str>) -> Option<Candidate> {
        let file = if target.trim().is_empty() {
            Some(from)
        } else {
            match resolve_wiki(target, &self.notes) {
                Resolution::Resolved(i) => Some(i),
                _ => None,
            }
        };
        match anchor {
            Some(a) => {
                let id = a.strip_prefix('^')?;
                let Some(file) = file else {
                    return Some(Candidate::Dangling);
                };
                match self.blocks.get(&(file, id.to_string())) {
                    Some(Block::Task(t)) => Some(Candidate::Resolved(*t)),
                    Some(Block::Other) => None,
                    None => Some(Candidate::Dangling),
                }
            }
            None => match self.anchors.get(&file?)? {
                Some(t) => Some(Candidate::Resolved(*t)),
                None => Some(Candidate::Dangling),
            },
        }
    }
}

/// Resolve every task's parent link in place. Runs once at the end of
/// [`crate::scan::scan_vault`]; linear in tasks plus task-line links, no I/O.
pub fn resolve_parent_links(scan: &mut Scan) {
    let index = ParentIndex::build(scan);
    let mut links_by_line: HashMap<
        (&std::path::Path, usize),
        Vec<(usize, &crate::graph::parser::RawLink)>,
    > = HashMap::new();
    for (fi, f) in scan.files.iter().enumerate() {
        for l in f.links.iter().chain(&f.self_block_refs) {
            if l.form == LinkForm::WikiLink {
                links_by_line
                    .entry((f.rel.as_path(), l.line))
                    .or_default()
                    .push((fi, l));
            }
        }
    }

    let mut resolved: Vec<Option<ParentLink>> = Vec::with_capacity(scan.tasks.len());
    for list in links_by_line.values_mut() {
        list.sort_by_key(|(_, l)| l.byte_range.start);
    }
    for task in &scan.tasks {
        let Some(links) = links_by_line.get(&(task.source_file.as_path(), task.source_line)) else {
            resolved.push(None);
            continue;
        };
        let mut first: Option<(String, Candidate)> = None;
        let mut extra = Vec::new();
        for (fi, l) in links {
            let Some(c) = index.classify(*fi, &l.target_text, l.anchor.as_deref()) else {
                continue;
            };
            if first.is_none() {
                first = Some((l.raw_text.clone(), c));
            } else {
                extra.push(l.raw_text.clone());
            }
        }
        resolved.push(first.map(|(raw, c)| {
            let state = match (task.parent, c) {
                (Some(_), _) => ParentLinkState::Ignored,
                (None, Candidate::Resolved(t)) => {
                    let p = &scan.tasks[t];
                    ParentLinkState::Resolved((p.source_file.clone(), p.source_line))
                }
                (None, Candidate::Dangling) => ParentLinkState::Dangling,
            };
            ParentLink { raw, extra, state }
        }));
    }
    for (task, link) in scan.tasks.iter_mut().zip(resolved) {
        task.parent_link = link;
    }
    break_cycles(&mut scan.tasks);
}

/// Mark one link per effective-parent cycle as [`ParentLinkState::Cycle`]:
/// the link-parented member with the greatest key.
fn break_cycles(tasks: &mut [Task]) {
    let index: HashMap<TaskKey, usize> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| ((t.source_file.clone(), t.source_line), i))
        .collect();
    let parent_of = |tasks: &[Task], i: usize| -> Option<usize> {
        super::hierarchy::parent_key(&tasks[i]).and_then(|k| index.get(&k).copied())
    };
    // 0 = unvisited, 1 = on the current walk, 2 = done.
    let mut color = vec![0u8; tasks.len()];
    for start in 0..tasks.len() {
        if color[start] != 0 {
            continue;
        }
        let mut path = Vec::new();
        let mut cur = Some(start);
        while let Some(i) = cur {
            match color[i] {
                0 => {
                    color[i] = 1;
                    path.push(i);
                    cur = parent_of(tasks, i);
                }
                1 => {
                    let pos = path.iter().position(|&p| p == i).expect("on current walk");
                    let breaker = path[pos..]
                        .iter()
                        .copied()
                        .filter(|&m| tasks[m].parent.is_none())
                        .max_by(|&a, &b| {
                            (&tasks[a].source_file, tasks[a].source_line)
                                .cmp(&(&tasks[b].source_file, tasks[b].source_line))
                        })
                        .expect("a cycle contains at least one link edge");
                    if let Some(link) = tasks[breaker].parent_link.as_mut() {
                        link.state = ParentLinkState::Cycle;
                    }
                    break;
                }
                _ => break,
            }
        }
        for i in path {
            color[i] = 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn scan(files: &[(&str, &str)]) -> (tempfile::TempDir, Scan) {
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
            .unwrap_or_else(|| panic!("no task at {rel}:{line}"))
    }

    fn state(s: &Scan, rel: &str, line: usize) -> Option<ParentLinkState> {
        task(s, rel, line)
            .parent_link
            .as_ref()
            .map(|l| l.state.clone())
    }

    fn resolved(rel: &str, line: usize) -> Option<ParentLinkState> {
        Some(ParentLinkState::Resolved((PathBuf::from(rel), line)))
    }

    #[test]
    fn block_reference_across_files() {
        let (_d, s) = scan(&[
            ("proj.md", "- [ ] Vendor onboarding ^vo1\n"),
            (
                "journal/2026-10-21.md",
                "# Day\n- [ ] reproduce on staging [[proj#^vo1]]\n",
            ),
        ]);
        assert_eq!(
            state(&s, "journal/2026-10-21.md", 2),
            resolved("proj.md", 1)
        );
        let link = task(&s, "journal/2026-10-21.md", 2)
            .parent_link
            .clone()
            .unwrap();
        assert_eq!(link.raw, "[[proj#^vo1]]");
    }

    #[test]
    fn same_file_block_reference_and_embed() {
        let (_d, s) = scan(&[(
            "a.md",
            "- [ ] A ^a1\n\ntext\n\n- [ ] B [[#^a1]]\n- [ ] C ![[a#^a1]]\n",
        )]);
        assert_eq!(state(&s, "a.md", 5), resolved("a.md", 1));
        assert_eq!(state(&s, "a.md", 6), resolved("a.md", 1));
    }

    #[test]
    fn anchor_note() {
        let (_d, s) = scan(&[
            (
                "jira/ABC-123.md",
                "---\nft:\n  tasks:\n    anchor: ABC-123\n---\n- [ ] Fix login 🆔 ABC-123\n",
            ),
            (
                "j.md",
                "- [ ] write test [[ABC-123]]\n- [ ] alias [[ABC-123|the bug]]\n",
            ),
        ]);
        assert_eq!(state(&s, "j.md", 1), resolved("jira/ABC-123.md", 6));
        assert_eq!(state(&s, "j.md", 2), resolved("jira/ABC-123.md", 6));
    }

    #[test]
    fn non_candidates() {
        let (_d, s) = scan(&[
            ("Claude.md", "some note\n"),
            ("notes.md", "a paragraph ^p1\n## Head\n"),
            (
                "j.md",
                "- [ ] plain [[Claude]]\n- [ ] para [[notes#^p1]]\n- [ ] heading [[notes#Head]]\n- [ ] ghost [[Nope]]\n- [ ] md [x](notes.md)\n",
            ),
        ]);
        for line in 1..=5 {
            assert_eq!(state(&s, "j.md", line), None, "line {line}");
        }
    }

    #[test]
    fn dangling_variants() {
        let (_d, s) = scan(&[
            ("proj.md", "- [ ] P ^p1\n"),
            ("anchor.md", "---\nft:\n  tasks:\n    anchor: X-1\n---\n- [ ] no id here\n"),
            (
                "j.md",
                "- [ ] missing id [[proj#^gone]]\n- [ ] missing note [[ghost#^p1]]\n- [ ] anchor without task [[anchor]]\n",
            ),
        ]);
        for line in 1..=3 {
            assert_eq!(
                state(&s, "j.md", line),
                Some(ParentLinkState::Dangling),
                "line {line}"
            );
        }
    }

    #[test]
    fn first_candidate_wins_rest_are_extra() {
        let (_d, s) = scan(&[
            ("a.md", "- [ ] A ^x\n"),
            ("b.md", "- [ ] B ^y\n"),
            ("j.md", "- [ ] two [[Claude]] [[a#^x]] and [[b#^y]]\n"),
        ]);
        let link = task(&s, "j.md", 1).parent_link.clone().unwrap();
        assert_eq!(link.state, resolved("a.md", 1).unwrap());
        assert_eq!(link.extra, vec!["[[b#^y]]".to_string()]);
    }

    #[test]
    fn physically_nested_link_is_ignored() {
        let (_d, s) = scan(&[
            ("proj.md", "- [ ] P ^p1\n"),
            ("j.md", "- [ ] parent\n  - [ ] child [[proj#^p1]]\n"),
        ]);
        assert_eq!(state(&s, "j.md", 2), Some(ParentLinkState::Ignored));
        let child = task(&s, "j.md", 2);
        assert_eq!(
            super::super::hierarchy::parent_key(child),
            Some((PathBuf::from("j.md"), 1))
        );
    }

    #[test]
    fn two_cycle_breaks_at_greatest_key() {
        let (_d, s) = scan(&[
            ("a.md", "- [ ] A [[b#^b1]] ^a1\n"),
            ("b.md", "- [ ] B [[a#^a1]] ^b1\n"),
        ]);
        assert_eq!(state(&s, "a.md", 1), resolved("b.md", 1));
        assert_eq!(state(&s, "b.md", 1), Some(ParentLinkState::Cycle));
    }

    #[test]
    fn cycle_through_physical_edge_breaks_on_a_link() {
        // c.md:2 is physically under c.md:1 and links nothing; c.md:1 links
        // a.md:1, which links back to c.md:2 → cycle a → c2 → c1 → a.
        let (_d, s) = scan(&[
            ("a.md", "- [ ] A [[c#^c2]] ^a1\n"),
            ("c.md", "- [ ] C1 [[a#^a1]]\n  - [ ] C2 ^c2\n"),
        ]);
        // Greatest link-parented key among {a.md:1, c.md:1} is c.md:1.
        assert_eq!(state(&s, "c.md", 1), Some(ParentLinkState::Cycle));
        assert_eq!(state(&s, "a.md", 1), resolved("c.md", 2));
    }

    #[test]
    fn self_link_is_a_cycle() {
        let (_d, s) = scan(&[("a.md", "- [ ] A [[#^a1]] ^a1\n")]);
        assert_eq!(state(&s, "a.md", 1), Some(ParentLinkState::Cycle));
    }

    #[test]
    fn three_cycle_leaves_two_links_resolved() {
        let (_d, s) = scan(&[
            ("a.md", "- [ ] A [[b#^b1]] ^a1\n"),
            ("b.md", "- [ ] B [[c#^c1]] ^b1\n"),
            ("c.md", "- [ ] C [[a#^a1]] ^c1\n"),
        ]);
        assert_eq!(state(&s, "a.md", 1), resolved("b.md", 1));
        assert_eq!(state(&s, "b.md", 1), resolved("c.md", 1));
        assert_eq!(state(&s, "c.md", 1), Some(ParentLinkState::Cycle));
    }

    #[test]
    fn duplicate_block_id_first_occurrence_wins() {
        let (_d, s) = scan(&[
            ("p.md", "- [ ] first ^d\n- [ ] second ^d\n"),
            ("j.md", "- [ ] x [[p#^d]]\n"),
        ]);
        assert_eq!(state(&s, "j.md", 1), resolved("p.md", 1));
    }

    /// Gated perf budget: the post-pass stays a small fraction of a scan.
    #[test]
    fn post_pass_is_cheap_relative_to_scan() {
        if std::env::var("FT_PERF_TESTS").as_deref() != Ok("1") {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        for i in 0..2000usize {
            let parent = (i + 1) % 2000;
            let body = format!(
                "# Note {i}\n\nSome prose [[note-{parent}]].\n\n- [ ] task a{i} ^a{i}\n  - [ ] sub\n- [ ] linked [[note-{parent}#^a{parent}]]\n- [ ] plain [[note-{parent}]]\n"
            );
            std::fs::write(dir.path().join(format!("note-{i}.md")), body).unwrap();
        }
        let start = std::time::Instant::now();
        let mut s = crate::scan::scan_vault(dir.path(), &[]);
        let scan_time = start.elapsed();
        let start = std::time::Instant::now();
        resolve_parent_links(&mut s);
        let pass_time = start.elapsed();
        assert!(
            pass_time * 10 < scan_time,
            "post-pass {pass_time:?} vs scan {scan_time:?}"
        );
        assert!(s
            .tasks
            .iter()
            .filter(|t| t.description.starts_with("linked"))
            .all(|t| matches!(
                t.parent_link.as_ref().map(|l| &l.state),
                Some(ParentLinkState::Resolved(_))
            )));
    }
}

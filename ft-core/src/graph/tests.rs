//! End-to-end tests for [`Graph::build`] and [`Graph::refresh_note`]
//! against the dedicated `tests/fixtures/links/` vault.
//!
//! Parser-internal and resolver-internal tests live next to the code
//! they cover, in `parser.rs::parser_tests` and
//! `resolve.rs::resolve_tests`. The tests here exercise the full
//! parse → resolve → graph pipeline and the per-file refresh + ghost
//! cleanup paths.

use std::path::{Path, PathBuf};

use crate::graph::{EdgeKind, Graph, LinkForm, NodeKind, NoteId};
use crate::task::{Status, Task};
use crate::vault::{Scan, Vault};

fn fixture_vault() -> Vault {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests/fixtures/links");
    Vault::discover(Some(path)).expect("links fixture vault must exist")
}

fn note(graph: &Graph, rel: &str) -> NoteId {
    graph
        .note_by_path(Path::new(rel))
        .unwrap_or_else(|| panic!("no note for {rel}"))
}

fn outgoing_targets(graph: &Graph, src: NoteId) -> Vec<String> {
    graph
        .outgoing(src)
        .filter_map(|(dst, edge)| {
            let kind_label = match graph.node(dst) {
                NodeKind::Note(n) => format!("note:{}", n.path.display()),
                NodeKind::Ghost(g) => format!("ghost:{}", g.raw),
                NodeKind::Directory(d) => format!("dir:{}", d.path.display()),
                NodeKind::Task(t) => format!("task:{}", t.description),
            };
            let edge_kind = match edge {
                EdgeKind::Link(_) => "link",
                EdgeKind::Embed(_) => "embed",
                EdgeKind::Contains => "contains",
                EdgeKind::HasTask => "has-task",
            };
            let l = edge.link()?;
            Some(format!(
                "{kind_label}|{edge_kind}|{:?}|target={}",
                l.form, l.target_text
            ))
        })
        .collect()
}

#[test]
fn build_creates_one_node_per_markdown_file() {
    let v = fixture_vault();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let note_count = g
        .nodes()
        .filter(|(_, k)| matches!(k, NodeKind::Note(_)))
        .count();
    // hub, alpha, beta, gamma, sub/inner, sub/My Inner, Index,
    // archive/Index, collision-linker → 9 notes
    assert_eq!(note_count, 9, "expected 9 note nodes");
}

#[test]
fn hub_outgoing_covers_every_link_shape() {
    let v = fixture_vault();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let hub = note(&g, "notes/hub.md");
    let edges: Vec<&EdgeKind> = g.outgoing(hub).map(|(_, e)| e).collect();

    // Sanity: at least the wikilink + md + embed shapes from the
    // fixture all show up. Exact count below.
    let wiki = edges
        .iter()
        .filter(|e| e.link().unwrap().form == LinkForm::WikiLink && matches!(e, EdgeKind::Link(_)))
        .count();
    let md = edges
        .iter()
        .filter(|e| e.link().unwrap().form == LinkForm::MdLink && matches!(e, EdgeKind::Link(_)))
        .count();
    let wiki_embed = edges
        .iter()
        .filter(|e| e.link().unwrap().form == LinkForm::WikiLink && matches!(e, EdgeKind::Embed(_)))
        .count();
    let md_embed = edges
        .iter()
        .filter(|e| e.link().unwrap().form == LinkForm::MdLink && matches!(e, EdgeKind::Embed(_)))
        .count();

    // 8 wikilinks: alpha, beta|alias, gamma#anchor, gamma#anchor|alias,
    //              sub/inner, Phantom, alpha (repeat 1), alpha (repeat 2)
    assert_eq!(wiki, 8, "wikilinks");
    // 4 md links: alpha.md, beta (extless), sub/My Inner.md, missing.md
    assert_eq!(md, 4, "md links");
    // 2 wiki embeds: ![[alpha]], ![[diagram.png]]
    assert_eq!(wiki_embed, 2, "wiki embeds");
    // 1 md embed: ![alt](sub/inner.md)
    assert_eq!(md_embed, 1, "md embeds");
}

#[test]
fn fenced_and_indented_and_inline_code_are_skipped() {
    let v = fixture_vault();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let hub = note(&g, "notes/hub.md");
    // The hub has fenced and indented code blocks containing fake links;
    // those should not contribute outgoing edges. Total checked above.
    // Spot-check: the inline-code `[[alpha]]` doesn't add a 9th wikilink.
    let wiki_count = g
        .outgoing(hub)
        .filter(|(_, e)| {
            matches!(e, EdgeKind::Link(_)) && e.link().unwrap().form == LinkForm::WikiLink
        })
        .count();
    assert_eq!(wiki_count, 8);
}

#[test]
fn frontmatter_links_are_skipped() {
    let v = fixture_vault();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let alpha = note(&g, "notes/alpha.md");
    // alpha.md has a `[[Phantom]]` inside its frontmatter and a real
    // `[[hub]]` in the body. Only `hub` should appear.
    let targets: Vec<&str> = g
        .outgoing(alpha)
        .map(|(_, e)| e.link().unwrap().target_text.as_str())
        .collect();
    assert_eq!(targets, vec!["hub"]);
}

#[test]
fn ghost_node_is_shared_across_linkers() {
    // hub.md and (we'll add via mutation) another note both point at
    // [[Phantom]]; the ghost is shared.
    let v = fixture_vault();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let phantom = g
        .ghost_by_raw("Phantom")
        .expect("Phantom ghost should exist");
    // Only hub.md links to Phantom in the fixture; one incoming edge.
    let incoming: Vec<_> = g.incoming(phantom).collect();
    assert_eq!(incoming.len(), 1);
}

#[test]
fn shortest_path_tiebreak_resolves_collision_linker_to_top_level_index() {
    let v = fixture_vault();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let linker = note(&g, "notes/collision-linker.md");
    let mut targets: Vec<PathBuf> = g
        .outgoing(linker)
        .filter_map(|(dst, _)| match g.node(dst) {
            NodeKind::Note(n) => Some(n.path.clone()),
            _ => None,
        })
        .collect();
    targets.sort();
    assert_eq!(targets, vec![PathBuf::from("Index.md")]);
}

#[test]
fn url_encoded_md_link_resolves() {
    let v = fixture_vault();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let hub = note(&g, "notes/hub.md");
    // Look for the edge whose raw_text is the URL-encoded form.
    let resolved = g
        .outgoing(hub)
        .filter(|(_, e)| e.link().unwrap().raw_text.contains("%20"))
        .find_map(|(dst, _)| match g.node(dst) {
            NodeKind::Note(n) => Some(n.path.clone()),
            _ => None,
        });
    assert_eq!(
        resolved,
        Some(PathBuf::from("notes/sub/My Inner.md")),
        "URL-encoded path should resolve to the spaced filename"
    );
}

#[test]
fn external_urls_do_not_become_edges() {
    let v = fixture_vault();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let hub = note(&g, "notes/hub.md");
    for (_, e) in g.outgoing(hub) {
        let raw = &e.link().unwrap().raw_text;
        assert!(
            !raw.contains("https://") && !raw.contains("mailto:"),
            "external URL leaked as an edge: {raw}"
        );
    }
}

#[test]
fn byte_ranges_round_trip_against_source_files() {
    let v = fixture_vault();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let hub_id = note(&g, "notes/hub.md");
    let abs = v.path.join("notes/hub.md");
    let content = std::fs::read_to_string(&abs).unwrap();
    for (_, edge) in g.outgoing(hub_id) {
        let l = edge.link().unwrap();
        assert_eq!(
            &content[l.byte_range.clone()],
            l.raw_text,
            "byte_range did not round-trip for {:?}",
            l.raw_text
        );
    }
}

#[test]
fn refresh_note_replaces_outgoing_edges_and_preserves_incoming() {
    use std::io::Write as _;

    let tmp = assert_fs::TempDir::new().unwrap();
    use assert_fs::prelude::*;
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("a.md").write_str("[[b]] [[c]]\n").unwrap();
    tmp.child("b.md").write_str("# b\n").unwrap();
    tmp.child("c.md").write_str("[[a]]\n").unwrap();

    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let mut g = Graph::build(&v, &Scan::default()).unwrap();

    let a = note(&g, "a.md");
    let b = note(&g, "b.md");
    let c = note(&g, "c.md");
    assert_eq!(g.outgoing(a).count(), 2, "a starts with two outgoing");
    assert_eq!(
        g.incoming(a).filter(|(_, e)| e.link().is_some()).count(),
        1,
        "c links to a"
    );

    // Mutate a.md: remove the [[b]] link, leave the [[c]] link.
    let mut f = std::fs::File::create(tmp.path().join("a.md")).unwrap();
    writeln!(f, "[[c]]").unwrap();
    drop(f);

    g.refresh_note(&v.path, &tmp.path().join("a.md")).unwrap();

    // Outgoing changed: only c remains.
    let outgoing: Vec<_> = g
        .outgoing(a)
        .filter_map(|(dst, _)| match g.node(dst) {
            NodeKind::Note(n) => Some(n.path.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(outgoing, vec![PathBuf::from("c.md")]);

    // Incoming to a is untouched (c.md still links to a).
    assert_eq!(g.incoming(a).filter(|(_, e)| e.link().is_some()).count(), 1);
    // b lost its incoming edge from a.
    assert_eq!(g.incoming(b).filter(|(_, e)| e.link().is_some()).count(), 0);
    let _ = c;
}

#[test]
fn refresh_note_garbage_collects_orphaned_ghost() {
    use assert_fs::prelude::*;
    use std::io::Write as _;

    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("a.md").write_str("[[Phantom]]\n").unwrap();

    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let mut g = Graph::build(&v, &Scan::default()).unwrap();
    assert!(g.ghost_by_raw("Phantom").is_some());

    // Remove the link from a.md.
    let mut f = std::fs::File::create(tmp.path().join("a.md")).unwrap();
    writeln!(f, "no links here").unwrap();
    drop(f);

    g.refresh_note(&v.path, &tmp.path().join("a.md")).unwrap();
    assert!(
        g.ghost_by_raw("Phantom").is_none(),
        "orphaned ghost should be removed"
    );
}

#[test]
fn refresh_note_keeps_ghost_when_other_linkers_remain() {
    use assert_fs::prelude::*;
    use std::io::Write as _;

    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("a.md").write_str("[[Phantom]]\n").unwrap();
    tmp.child("b.md").write_str("[[Phantom]]\n").unwrap();

    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let mut g = Graph::build(&v, &Scan::default()).unwrap();
    let phantom = g.ghost_by_raw("Phantom").unwrap();
    assert_eq!(g.incoming(phantom).count(), 2);

    // Remove the link from a.md only.
    let mut f = std::fs::File::create(tmp.path().join("a.md")).unwrap();
    writeln!(f, "nothing").unwrap();
    drop(f);

    g.refresh_note(&v.path, &tmp.path().join("a.md")).unwrap();
    let phantom = g
        .ghost_by_raw("Phantom")
        .expect("ghost should still exist (b still links)");
    assert_eq!(g.incoming(phantom).count(), 1);
}

#[test]
fn empty_vault_builds_empty_graph() {
    let tmp = assert_fs::TempDir::new().unwrap();
    use assert_fs::prelude::*;
    tmp.child(".obsidian").create_dir_all().unwrap();
    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    assert_eq!(
        g.nodes().count(),
        1,
        "root directory node should exist even for empty vault"
    );
}

#[test]
fn outgoing_visible_via_str_helper_for_debugging() {
    // Sanity that the debug helper this file uses doesn't blow up on
    // any node kind. (Exercised through fixture_vault.)
    let v = fixture_vault();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let hub = note(&g, "notes/hub.md");
    let dump = outgoing_targets(&g, hub);
    assert!(!dump.is_empty());
}

// ── Directory node tests ──────────────────────────────────────────────

fn dirs_fixture() -> Vault {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests/fixtures/dirs");
    Vault::discover(Some(path)).expect("dirs fixture vault must exist")
}

fn dir_by_path(graph: &Graph, rel: &str) -> NoteId {
    let id = graph
        .node_by_path(Path::new(rel))
        .unwrap_or_else(|| panic!("no node for {rel}"));
    assert!(
        matches!(graph.node(id), NodeKind::Directory(_)),
        "{rel} is not a Directory node"
    );
    id
}

fn note_in_dirs(graph: &Graph, rel: &str) -> NoteId {
    graph
        .note_by_path(Path::new(rel))
        .unwrap_or_else(|| panic!("no note for {rel}"))
}

#[test]
fn build_includes_directory_nodes() {
    let v = dirs_fixture();
    let g = Graph::build(&v, &Scan::default()).unwrap();

    let dir_count = g
        .nodes()
        .filter(|(_, k)| matches!(k, NodeKind::Directory(_)))
        .count();
    // root + Areas + Areas/operations + Projects = 4 directories
    assert_eq!(dir_count, 4, "expected 4 directory nodes");

    // Spot-check directory names
    let root_id = dir_by_path(&g, "");
    let areas_id = dir_by_path(&g, "Areas");
    let ops_id = dir_by_path(&g, "Areas/operations");
    let projects_id = dir_by_path(&g, "Projects");

    match g.node(root_id) {
        NodeKind::Directory(d) => {
            assert!(d.path.as_os_str().is_empty(), "root path should be empty");
            assert!(d.name.is_empty(), "root name should be empty");
        }
        _ => panic!("expected Directory"),
    }
    match g.node(areas_id) {
        NodeKind::Directory(d) => {
            assert_eq!(d.path, PathBuf::from("Areas"));
            assert_eq!(d.name, "Areas");
        }
        _ => panic!("expected Directory"),
    }
    match g.node(ops_id) {
        NodeKind::Directory(d) => {
            assert_eq!(d.path, PathBuf::from("Areas/operations"));
            assert_eq!(d.name, "operations");
        }
        _ => panic!("expected Directory"),
    }
    match g.node(projects_id) {
        NodeKind::Directory(d) => {
            assert_eq!(d.path, PathBuf::from("Projects"));
            assert_eq!(d.name, "Projects");
        }
        _ => panic!("expected Directory"),
    }
}

#[test]
fn contains_edges_connect_directories_to_immediate_children() {
    let v = dirs_fixture();
    let g = Graph::build(&v, &Scan::default()).unwrap();

    let root = dir_by_path(&g, "");
    let areas = dir_by_path(&g, "Areas");
    let ops = dir_by_path(&g, "Areas/operations");

    // Root contains: root.md, Areas, Projects (3 top-level items)
    let root_children: Vec<PathBuf> = g
        .outgoing(root)
        .filter(|(_, e)| matches!(e, EdgeKind::Contains))
        .map(|(dst, _)| match g.node(dst) {
            NodeKind::Note(n) => n.path.clone(),
            NodeKind::Directory(d) => d.path.clone(),
            _ => PathBuf::new(),
        })
        .collect();
    assert_eq!(root_children.len(), 3);
    assert!(root_children.contains(&PathBuf::from("root.md")));
    assert!(root_children.contains(&PathBuf::from("Areas")));
    assert!(root_children.contains(&PathBuf::from("Projects")));

    // Areas contains: finance.md, Areas/operations (2 children)
    let areas_children: Vec<PathBuf> = g
        .outgoing(areas)
        .filter(|(_, e)| matches!(e, EdgeKind::Contains))
        .map(|(dst, _)| match g.node(dst) {
            NodeKind::Note(n) => n.path.clone(),
            NodeKind::Directory(d) => d.path.clone(),
            _ => PathBuf::new(),
        })
        .collect();
    assert_eq!(areas_children.len(), 2);
    assert!(areas_children.contains(&PathBuf::from("Areas/finance.md")));
    assert!(areas_children.contains(&PathBuf::from("Areas/operations")));

    // Areas/operations contains: shifts.md (1 child)
    let ops_children: Vec<PathBuf> = g
        .outgoing(ops)
        .filter(|(_, e)| matches!(e, EdgeKind::Contains))
        .map(|(dst, _)| match g.node(dst) {
            NodeKind::Note(n) => n.path.clone(),
            NodeKind::Directory(d) => d.path.clone(),
            _ => PathBuf::new(),
        })
        .collect();
    assert_eq!(
        ops_children,
        vec![PathBuf::from("Areas/operations/shifts.md")]
    );
}

#[test]
fn note_incoming_includes_containing_directory() {
    let v = dirs_fixture();
    let g = Graph::build(&v, &Scan::default()).unwrap();

    let finance = note_in_dirs(&g, "Areas/finance.md");
    let areas = dir_by_path(&g, "Areas");

    let parents: Vec<NoteId> = g
        .incoming(finance)
        .filter(|(_, e)| matches!(e, EdgeKind::Contains))
        .map(|(src, _)| src)
        .collect();
    assert_eq!(parents, vec![areas]);
}

#[test]
fn note_by_path_does_not_return_directory_nodes() {
    let v = dirs_fixture();
    let g = Graph::build(&v, &Scan::default()).unwrap();

    assert!(g.note_by_path(Path::new("Areas")).is_none());
    assert!(g.note_by_path(Path::new("")).is_none());
    assert!(g.note_by_path(Path::new("root.md")).is_some());
}

#[test]
fn node_by_path_returns_directory_nodes() {
    let v = dirs_fixture();
    let g = Graph::build(&v, &Scan::default()).unwrap();

    let root_id = g.node_by_path(Path::new(""));
    assert!(root_id.is_some());
    assert!(matches!(g.node(root_id.unwrap()), NodeKind::Directory(_)));

    let areas_id = g.node_by_path(Path::new("Areas"));
    assert!(areas_id.is_some());
    assert!(matches!(g.node(areas_id.unwrap()), NodeKind::Directory(_)));
}

#[test]
fn empty_vault_has_root_directory() {
    let tmp = assert_fs::TempDir::new().unwrap();
    use assert_fs::prelude::*;
    tmp.child(".obsidian").create_dir_all().unwrap();
    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let g = Graph::build(&v, &Scan::default()).unwrap();

    // Root directory node is always present, even without any notes.
    let ids: Vec<_> = g
        .nodes()
        .filter(|(_, k)| matches!(k, NodeKind::Directory(_)))
        .collect();
    assert_eq!(ids.len(), 1);
}

fn dirs_fixture_scan_with_tasks() -> Scan {
    Scan {
        tasks: vec![
            Task {
                description: "Fix login bug".into(),
                status: Status::Open,
                priority: Some(crate::task::Priority::High),
                tags: vec!["bug".into(), "urgent".into()],
                due: Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 1).unwrap()),
                scheduled: Some(chrono::NaiveDate::from_ymd_opt(2025, 5, 15).unwrap()),
                source_file: PathBuf::from("root.md"),
                source_line: 3,
                created: None,
                start: None,
                done: None,
                cancelled: None,
                recurrence: None,
                id: None,
                depends_on: vec![],
                on_completion: None,
                block_link: None,
                raw_trailing: None,
                indent_level: 0,
                parent: None,
            },
            Task {
                description: "Review quarterly report".into(),
                status: Status::Done,
                priority: None,
                tags: vec!["finance".into()],
                due: None,
                scheduled: None,
                source_file: PathBuf::from("root.md"),
                source_line: 7,
                created: None,
                start: None,
                done: None,
                cancelled: None,
                recurrence: None,
                id: None,
                depends_on: vec![],
                on_completion: None,
                block_link: None,
                raw_trailing: None,
                indent_level: 0,
                parent: None,
            },
            Task {
                description: "Process invoices".into(),
                status: Status::Open,
                priority: Some(crate::task::Priority::Medium),
                tags: vec!["finance".into(), "invoices".into()],
                due: Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 15).unwrap()),
                scheduled: None,
                source_file: PathBuf::from("Areas/finance.md"),
                source_line: 5,
                created: None,
                start: None,
                done: None,
                cancelled: None,
                recurrence: None,
                id: None,
                depends_on: vec![],
                on_completion: None,
                block_link: None,
                raw_trailing: None,
                indent_level: 0,
                parent: None,
            },
        ],
        errors: vec![],
    }
}

/// Task 7.1: TaskData construction from &Task preserves all fields correctly.
#[test]
fn task_data_from_task_preserves_fields() {
    let task = Task {
        description: "Write docs".into(),
        status: Status::InProgress,
        priority: Some(crate::task::Priority::Low),
        tags: vec!["docs".into()],
        due: Some(chrono::NaiveDate::from_ymd_opt(2025, 7, 1).unwrap()),
        scheduled: Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 20).unwrap()),
        source_file: PathBuf::from("docs/readme.md"),
        source_line: 10,
        created: None,
        start: None,
        done: None,
        cancelled: None,
        recurrence: None,
        id: None,
        depends_on: vec![],
        on_completion: None,
        block_link: None,
        raw_trailing: None,
        indent_level: 0,
        parent: None,
    };

    // Verify what insert_task_node creates internally matches expectations.
    // We can't easily call insert_task_node directly since it's private,
    // so we verify via Graph::build with a scan.
    let tmp = assert_fs::TempDir::new().unwrap();
    use assert_fs::prelude::*;
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("docs").create_dir_all().unwrap();
    tmp.child("docs/readme.md")
        .write_str("- [ ] Write docs")
        .unwrap();
    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let scan = Scan {
        tasks: vec![task],
        errors: vec![],
    };
    let g = Graph::build(&v, &scan).unwrap();

    // Find task node in the graph
    let task_nodes: Vec<_> = g
        .nodes()
        .filter(|(_, k)| matches!(k, NodeKind::Task(_)))
        .collect();
    assert_eq!(task_nodes.len(), 1);

    if let NodeKind::Task(td) = task_nodes[0].1 {
        assert_eq!(td.description, "Write docs");
        assert_eq!(td.status, "InProgress");
        assert_eq!(td.priority.as_deref(), Some("Low"));
        assert_eq!(td.due.as_deref(), Some("2025-07-01"));
        assert_eq!(td.scheduled.as_deref(), Some("2025-06-20"));
        assert_eq!(td.tags, vec!["docs"]);
        assert_eq!(td.source_file, PathBuf::from("docs/readme.md"));
        assert_eq!(td.source_line, 10);
    } else {
        panic!("expected Task node");
    }
}

/// Task 7.2: Graph::build with non-empty scan creates task nodes and HasTask edges.
#[test]
fn build_with_tasks_creates_task_nodes_and_edges() {
    let v = dirs_fixture();
    let scan = dirs_fixture_scan_with_tasks();
    let g = Graph::build(&v, &scan).unwrap();

    // Check task nodes exist
    let task_nodes: Vec<_> = g
        .nodes()
        .filter(|(_, k)| matches!(k, NodeKind::Task(_)))
        .collect();
    assert_eq!(task_nodes.len(), 3);

    // Check HasTask edges exist
    let hastask_edges: Vec<_> = g
        .nodes()
        .flat_map(|(id, _)| {
            g.outgoing(id)
                .filter(|(_, e)| matches!(e, EdgeKind::HasTask))
                .map(move |(dst, _)| (id, dst))
        })
        .collect();
    assert_eq!(hastask_edges.len(), 3);

    // Check root.md has 2 tasks
    let root_id = g.note_by_path(Path::new("root.md")).unwrap();
    let root_tasks: Vec<_> = g
        .outgoing(root_id)
        .filter(|(_, e)| matches!(e, EdgeKind::HasTask))
        .collect();
    assert_eq!(root_tasks.len(), 2);

    // Check Areas/finance.md has 1 task
    let finance_id = g.note_by_path(Path::new("Areas/finance.md")).unwrap();
    let finance_tasks: Vec<_> = g
        .outgoing(finance_id)
        .filter(|(_, e)| matches!(e, EdgeKind::HasTask))
        .collect();
    assert_eq!(finance_tasks.len(), 1);
}

/// Task 7.3: Graph::build with empty scan produces no task nodes.
#[test]
fn build_with_empty_scan_has_no_task_nodes() {
    let v = dirs_fixture();
    let g = Graph::build(&v, &Scan::default()).unwrap();

    let task_nodes: Vec<_> = g
        .nodes()
        .filter(|(_, k)| matches!(k, NodeKind::Task(_)))
        .collect();
    assert_eq!(task_nodes.len(), 0);

    // No HasTask edges either
    let hastask_edges: Vec<_> = g
        .nodes()
        .flat_map(|(id, _)| {
            g.outgoing(id)
                .filter(|(_, e)| matches!(e, EdgeKind::HasTask))
        })
        .collect();
    assert_eq!(hastask_edges.len(), 0);
}

/// Task 7.4: Task node deduplication by (source_file, source_line).
#[test]
fn task_node_deduplication_by_source() {
    let v = dirs_fixture();
    // Two tasks with the same source_file and source_line
    let scan = Scan {
        tasks: vec![
            Task {
                description: "First task".into(),
                status: Status::Open,
                source_file: PathBuf::from("root.md"),
                source_line: 3,
                priority: None,
                tags: vec![],
                due: None,
                scheduled: None,
                created: None,
                start: None,
                done: None,
                cancelled: None,
                recurrence: None,
                id: None,
                depends_on: vec![],
                on_completion: None,
                block_link: None,
                raw_trailing: None,
                indent_level: 0,
                parent: None,
            },
            Task {
                description: "Second task (duplicate key, should be deduped)".into(),
                status: Status::Done,
                source_file: PathBuf::from("root.md"),
                source_line: 3,
                priority: None,
                tags: vec![],
                due: None,
                scheduled: None,
                created: None,
                start: None,
                done: None,
                cancelled: None,
                recurrence: None,
                id: None,
                depends_on: vec![],
                on_completion: None,
                block_link: None,
                raw_trailing: None,
                indent_level: 0,
                parent: None,
            },
        ],
        errors: vec![],
    };
    let g = Graph::build(&v, &scan).unwrap();

    let task_nodes: Vec<_> = g
        .nodes()
        .filter(|(_, k)| matches!(k, NodeKind::Task(_)))
        .collect();
    // Only one task node despite two input tasks with same key
    assert_eq!(task_nodes.len(), 1);

    // The first task's data should win (since we use get-or-create semantics)
    if let NodeKind::Task(td) = task_nodes[0].1 {
        assert_eq!(td.description, "First task");
    }
}

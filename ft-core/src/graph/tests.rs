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
                NodeKind::Paragraph(p) => {
                    format!("paragraph:{}:{}", p.source_file.display(), p.line_start)
                }
                NodeKind::Heading(h) => {
                    format!("heading:{}:{}", h.source_file.display(), h.line)
                }
            };
            let edge_kind = match edge {
                EdgeKind::NoteLink(_) => "note-link",
                EdgeKind::HeadingLink(_) => "heading-link",
                EdgeKind::ParagraphLink(_) => "paragraph-link",
                EdgeKind::Contains => "contains",
                EdgeKind::HasTask => "has-task",
                EdgeKind::Subtask => "subtask",
                EdgeKind::LinksInto => "links-into",
                EdgeKind::OwnsParagraph => "owns-paragraph",
                EdgeKind::OwnsHeading => "owns-heading",
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
    // Note-level links: one NoteLink per occurrence (the note level is the
    // multiset of all link occurrences in the note).
    let edges: Vec<&EdgeKind> = g
        .outgoing(hub)
        .filter(|(_, e)| matches!(e, EdgeKind::NoteLink(_)))
        .map(|(_, e)| e)
        .collect();

    // Sanity: at least the wikilink + md + embed shapes from the
    // fixture all show up. Exact count below.
    let wiki = edges
        .iter()
        .filter(|e| {
            let l = e.link().unwrap();
            l.form == LinkForm::WikiLink && !l.is_embed
        })
        .count();
    let md = edges
        .iter()
        .filter(|e| {
            let l = e.link().unwrap();
            l.form == LinkForm::MdLink && !l.is_embed
        })
        .count();
    let wiki_embed = edges
        .iter()
        .filter(|e| {
            let l = e.link().unwrap();
            l.form == LinkForm::WikiLink && l.is_embed
        })
        .count();
    let md_embed = edges
        .iter()
        .filter(|e| {
            let l = e.link().unwrap();
            l.form == LinkForm::MdLink && l.is_embed
        })
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
    // (Excludes wiki-form embeds, which are NoteLink with is_embed = true.)
    let wiki_count = g
        .outgoing(hub)
        .filter(|(_, e)| matches!(e, EdgeKind::NoteLink(_)))
        .filter(|(_, e)| {
            let l = e.link().unwrap();
            l.form == LinkForm::WikiLink && !l.is_embed
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
        .filter_map(|(_, e)| e.link().map(|l| l.target_text.as_str()))
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
    // Only hub.md links to Phantom in the fixture; one Link edge from
    // hub plus a ParagraphLink edge from hub's owning paragraph.
    let link_incoming: Vec<_> = g
        .incoming(phantom)
        .filter(|(_, e)| matches!(e, EdgeKind::NoteLink(_)))
        .collect();
    assert_eq!(link_incoming.len(), 1);
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
        .filter(|(_, e)| e.link().is_some_and(|l| l.raw_text.contains("%20")))
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
        let Some(l) = e.link() else { continue };
        let raw = &l.raw_text;
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
        let Some(l) = edge.link() else { continue };
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
    assert_eq!(
        g.outgoing(a)
            .filter(|(_, e)| matches!(e, EdgeKind::NoteLink(_)))
            .count(),
        2,
        "a starts with two note-level link edges"
    );
    assert_eq!(
        g.incoming(a)
            .filter(|(_, e)| matches!(e, EdgeKind::NoteLink(_)))
            .count(),
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
    assert_eq!(
        g.incoming(a)
            .filter(|(_, e)| matches!(e, EdgeKind::NoteLink(_)))
            .count(),
        1
    );
    // b lost its incoming edge from a.
    assert_eq!(
        g.incoming(b)
            .filter(|(_, e)| matches!(e, EdgeKind::NoteLink(_)))
            .count(),
        0
    );
    let _ = c;
}

#[test]
fn heading_nodes_created_with_owns_heading_tree() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(
            root.join("note.md"),
            "# A\nintro\n\n## B\nbody\n\n### C\nmore\n",
        )
        .unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let n = note(&g, "note.md");

    // Three headings, each owned by the nearest shallower heading.
    let a = g
        .heading_by_loc(Path::new("note.md"), 1)
        .expect("A heading");
    let b = g
        .heading_by_loc(Path::new("note.md"), 4)
        .expect("B heading");
    let c = g
        .heading_by_loc(Path::new("note.md"), 7)
        .expect("C heading");

    // OwnsHeading: note -> A, A -> B, B -> C.
    assert!(has_owns_heading(&g, n, a));
    assert!(has_owns_heading(&g, a, b));
    assert!(has_owns_heading(&g, b, c));
    assert!(
        !has_owns_heading(&g, n, b),
        "B is not a direct child of the note"
    );

    // Levels + text.
    assert_eq!(heading_text(&g, a), "A");
    assert_eq!(heading_level(&g, a), 1);
    assert_eq!(heading_text(&g, b), "B");
    assert_eq!(heading_level(&g, b), 2);
    assert_eq!(heading_text(&g, c), "C");
    assert_eq!(heading_level(&g, c), 3);
}

#[test]
fn heading_sibling_closes_prior_section() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("note.md"), "# A\n## B\n## C\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let n = note(&g, "note.md");
    let a = g.heading_by_loc(Path::new("note.md"), 1).unwrap();
    let b = g.heading_by_loc(Path::new("note.md"), 2).unwrap();
    let c = g.heading_by_loc(Path::new("note.md"), 3).unwrap();
    // A owns both B and C (sibling headings); B does not own C.
    assert!(has_owns_heading(&g, n, a));
    assert!(has_owns_heading(&g, a, b));
    assert!(has_owns_heading(&g, a, c));
    assert!(!has_owns_heading(&g, b, c));
}

#[test]
fn heading_in_fenced_code_is_not_a_heading() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("note.md"), "```\n# Fake\n```\n# Real\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    assert!(g.heading_by_loc(Path::new("note.md"), 2).is_none());
    assert!(g.heading_by_loc(Path::new("note.md"), 4).is_some());
}

#[test]
fn paragraph_ownership_uses_nearest_container() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("note.md"), "intro\n\n# A\nbody\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let n = note(&g, "note.md");
    let a = g.heading_by_loc(Path::new("note.md"), 3).unwrap();
    let paras = g.note_paragraphs(n);
    assert_eq!(paras.len(), 2, "intro + heading-body");
    // Intro (line 1) owned by the note; the paragraph starting at the
    // `# A` heading line (3) owned by A.
    let intro = g.paragraph_by_loc(Path::new("note.md"), 1).unwrap();
    let body = g.paragraph_by_loc(Path::new("note.md"), 3).unwrap();
    assert!(has_owns_paragraph(&g, n, intro));
    assert!(!has_owns_paragraph(&g, n, body), "body owned by heading A");
    assert!(has_owns_paragraph(&g, a, body));
    assert!(!has_owns_paragraph(&g, a, intro));
}

#[test]
fn paragraph_at_heading_line_owned_by_that_heading() {
    // Fork A2: the heading line begins a paragraph; that paragraph is
    // owned by the heading whose line it begins at.
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("note.md"), "# A\n## B\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let a = g.heading_by_loc(Path::new("note.md"), 1).unwrap();
    let b = g.heading_by_loc(Path::new("note.md"), 2).unwrap();
    // The paragraph beginning at line 2 (## B) is owned by B, not A.
    let p2 = g.paragraph_by_loc(Path::new("note.md"), 2).unwrap();
    assert!(has_owns_paragraph(&g, b, p2));
    assert!(!has_owns_paragraph(&g, a, p2));
}

#[test]
fn paragraph_ownership_is_exclusive() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("note.md"), "# A\nbody\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let body = g.paragraph_by_loc(Path::new("note.md"), 1).unwrap();
    let incoming_owners: Vec<_> = g
        .incoming(body)
        .filter(|(_, e)| matches!(e, EdgeKind::OwnsParagraph))
        .collect();
    assert_eq!(
        incoming_owners.len(),
        1,
        "exactly one OwnsParagraph edge per paragraph"
    );
}

#[test]
fn note_headings_and_all_headings() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("note.md"), "# A\n## B\n### C\n# D\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let n = note(&g, "note.md");
    // Direct children: A and D (both top-level).
    let direct = g.note_headings(n);
    assert_eq!(direct.len(), 2, "A and D are direct children");
    // Full subtree: A, B, C, D.
    let all = g.all_headings(n);
    assert_eq!(all.len(), 4, "A, B, C, D in subtree");
}

#[test]
fn note_paragraphs_recurses_through_nested_headings() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("note.md"), "intro\n# A\np1\n## B\np2\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let n = note(&g, "note.md");
    let paras = g.note_paragraphs(n);
    assert_eq!(paras.len(), 3, "intro + p1 + p2 across nested headings");
}

#[test]
fn refresh_note_updates_heading_count_and_cleans_index() {
    use assert_fs::prelude::*;
    use std::io::Write as _;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("note.md").write_str("# A\nbody\n").unwrap();
    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let mut g = Graph::build(&v, &Scan::default()).unwrap();
    assert!(g.heading_by_loc(Path::new("note.md"), 1).is_some());
    // Edit: replace # A with # B and add ## C.
    let mut f = std::fs::File::create(tmp.path().join("note.md")).unwrap();
    writeln!(f, "# B\nbody\n\n## C\n").unwrap();
    drop(f);
    g.refresh_note(&v.path, &tmp.path().join("note.md"))
        .unwrap();
    assert!(
        g.heading_by_loc(Path::new("note.md"), 1).is_some(),
        "B at line 1"
    );
    assert!(
        g.heading_by_loc(Path::new("note.md"), 4).is_some(),
        "C at line 4"
    );
}

#[test]
fn stable_key_round_trips_for_heading() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("note.md"), "# A\n").unwrap();
    });
    let g1 = Graph::build(&v, &Scan::default()).unwrap();
    let h = g1.heading_by_loc(Path::new("note.md"), 1).unwrap();
    let key = g1.stable_key(h);
    let g2 = Graph::build(&v, &Scan::default()).unwrap();
    assert_eq!(g2.id_for_key(&key), Some(h));
}

// ── heading test helpers ───────────────────────────────────────

fn heading_text(g: &Graph, id: NoteId) -> String {
    match g.node(id) {
        NodeKind::Heading(h) => h.text.clone(),
        _ => panic!("not a heading"),
    }
}

fn heading_level(g: &Graph, id: NoteId) -> u8 {
    match g.node(id) {
        NodeKind::Heading(h) => h.level,
        _ => panic!("not a heading"),
    }
}

fn has_owns_heading(g: &Graph, parent: NoteId, child: NoteId) -> bool {
    g.outgoing(parent)
        .any(|(dst, e)| dst == child && matches!(e, EdgeKind::OwnsHeading))
}

fn has_owns_paragraph(g: &Graph, parent: NoteId, child: NoteId) -> bool {
    g.outgoing(parent)
        .any(|(dst, e)| dst == child && matches!(e, EdgeKind::OwnsParagraph))
}

#[test]
fn link_in_paragraph_body_produces_note_link_and_paragraph_link() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("a.md"), "body [[b]] here\n").unwrap();
        std::fs::write(root.join("b.md"), "# b\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let a = note(&g, "a.md");
    let b = note(&g, "b.md");

    // Note-level: one NoteLink a -> b.
    let note_links: Vec<_> = g
        .outgoing(a)
        .filter(|(_, e)| matches!(e, EdgeKind::NoteLink(_)))
        .collect();
    assert_eq!(note_links.len(), 1);
    assert_eq!(note_links[0].0, b);

    // Paragraph-level: one ParagraphLink from a's paragraph to b.
    let paras = g.note_paragraphs(a);
    assert_eq!(paras.len(), 1);
    let p_links: Vec<_> = g
        .outgoing(paras[0])
        .filter(|(_, e)| matches!(e, EdgeKind::ParagraphLink(_)))
        .collect();
    assert_eq!(p_links.len(), 1);
    assert_eq!(p_links[0].0, b);

    // No HeadingLink (the link is not on a heading line).
    let h_links = g
        .outgoing(a)
        .filter(|(_, e)| matches!(e, EdgeKind::HeadingLink(_)))
        .count();
    assert_eq!(h_links, 0);
}

#[test]
fn link_on_heading_line_produces_all_three_levels() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("a.md"), "# See [[b]]\n").unwrap();
        std::fs::write(root.join("b.md"), "# b\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let a = note(&g, "a.md");
    let b = note(&g, "b.md");
    let heading = g.heading_by_loc(Path::new("a.md"), 1).unwrap();
    let para = g.paragraph_by_loc(Path::new("a.md"), 1).unwrap();

    // NoteLink: a -> b.
    assert!(g
        .outgoing(a)
        .any(|(dst, e)| dst == b && matches!(e, EdgeKind::NoteLink(_))));
    // HeadingLink: heading -> b.
    assert!(g
        .outgoing(heading)
        .any(|(dst, e)| dst == b && matches!(e, EdgeKind::HeadingLink(_))));
    // ParagraphLink: paragraph -> b (the heading line begins a paragraph).
    assert!(g
        .outgoing(para)
        .any(|(dst, e)| dst == b && matches!(e, EdgeKind::ParagraphLink(_))));
}

#[test]
fn embed_links_carry_is_embed_flag() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("a.md"), "![[b]]\n").unwrap();
        std::fs::write(root.join("b.md"), "# b\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let a = note(&g, "a.md");
    let note_link = g
        .outgoing(a)
        .find(|(_, e)| matches!(e, EdgeKind::NoteLink(_)))
        .unwrap();
    assert!(note_link.1.link().unwrap().is_embed, "![[b]] is an embed");
    // No separate Embed edge kind exists in the unified model.
    let _ = note_link; // (EdgeKind::Embed no longer exists; nothing to assert against.)
}

#[test]
fn markdown_link_in_paragraph_produces_paragraph_link_with_full_data() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("a.md"), "see [b](b.md) here\n").unwrap();
        std::fs::write(root.join("b.md"), "# b\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let a = note(&g, "a.md");
    let b = note(&g, "b.md");
    let para = g.note_paragraphs(a).pop().unwrap();
    let p_link = g
        .outgoing(para)
        .find(|(_, e)| matches!(e, EdgeKind::ParagraphLink(_)))
        .unwrap();
    assert_eq!(p_link.0, b);
    let l = p_link.1.link().unwrap();
    assert_eq!(l.form, LinkForm::MdLink, "markdown form at paragraph level");
    assert_eq!(l.raw_text, "[b](b.md)");
    let _ = l;
}

#[test]
fn ghost_kept_alive_by_heading_link_is_garbage_collected() {
    use assert_fs::prelude::*;
    use std::io::Write as _;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    // A heading whose line links to [[Phantom]]; the ghost's only incoming
    // edge at the heading level is the HeadingLink.
    tmp.child("a.md")
        .write_str("# Heading [[Phantom]]\n")
        .unwrap();
    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let mut g = Graph::build(&v, &Scan::default()).unwrap();
    assert!(g.ghost_by_raw("Phantom").is_some());
    // Rewrite a.md to drop the link.
    let mut f = std::fs::File::create(tmp.path().join("a.md")).unwrap();
    writeln!(f, "# Heading\n").unwrap();
    drop(f);
    g.refresh_note(&v.path, &tmp.path().join("a.md")).unwrap();
    assert!(
        g.ghost_by_raw("Phantom").is_none(),
        "heading-link-only ghost is GC'd across all levels"
    );
}

#[test]
fn resolvable_anchor_targets_heading_node() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("a.md"), "see [[b#Section]]\n").unwrap();
        std::fs::write(root.join("b.md"), "# Top\n\n## Section\nbody\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let a = note(&g, "a.md");
    let b = note(&g, "b.md");
    let section = g
        .resolve_anchor(b, "Section")
        .expect("anchor resolves to heading Section");
    // The ParagraphLink from a's paragraph targets the heading, not b.
    let para = g.note_paragraphs(a).pop().unwrap();
    let p_link_target = g
        .outgoing(para)
        .find(|(_, e)| matches!(e, EdgeKind::ParagraphLink(_)))
        .unwrap()
        .0;
    assert_eq!(
        p_link_target, section,
        "anchored ParagraphLink targets heading"
    );
    assert_ne!(p_link_target, b);
}

#[test]
fn unresolvable_anchor_targets_note() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("a.md"), "see [[b#Nope]]\n").unwrap();
        std::fs::write(root.join("b.md"), "# Top\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let a = note(&g, "a.md");
    let b = note(&g, "b.md");
    let para = g.note_paragraphs(a).pop().unwrap();
    let p_link = g
        .outgoing(para)
        .find(|(_, e)| matches!(e, EdgeKind::ParagraphLink(_)))
        .unwrap();
    assert_eq!(p_link.0, b, "unresolvable anchor falls back to the note");
    assert_eq!(
        p_link.1.link().unwrap().anchor.as_deref(),
        Some("Nope"),
        "anchor retained as metadata"
    );
}

#[test]
fn anchor_on_unresolved_note_targets_ghost() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("a.md"), "see [[Missing#Bar]]\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let a = note(&g, "a.md");
    let ghost = g.ghost_by_raw("Missing").expect("ghost materialized");
    let para = g.note_paragraphs(a).pop().unwrap();
    let p_link = g
        .outgoing(para)
        .find(|(_, e)| matches!(e, EdgeKind::ParagraphLink(_)))
        .unwrap();
    assert_eq!(p_link.0, ghost, "anchored link to ghost targets the ghost");
    assert_eq!(p_link.1.link().unwrap().anchor.as_deref(), Some("Bar"));
}

#[test]
fn note_link_ignores_anchor_for_target() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("a.md"), "see [[b#Section]]\n").unwrap();
        std::fs::write(root.join("b.md"), "# Top\n\n## Section\nbody\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let a = note(&g, "a.md");
    let b = note(&g, "b.md");
    // NoteLink targets the note b (not the heading), even with an anchor.
    let note_link = g
        .outgoing(a)
        .find(|(_, e)| matches!(e, EdgeKind::NoteLink(_)))
        .unwrap();
    assert_eq!(note_link.0, b, "NoteLink ignores anchor for target");
}

#[test]
fn mentions_of_includes_heading_targeted_links() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("a.md"), "see [[b#Section]]\n").unwrap();
        std::fs::write(root.join("b.md"), "# Top\n\n## Section\nbody\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let b = note(&g, "b.md");
    // mentions_of(b) yields a's paragraph link (anchored to b's heading).
    let a_para = g.paragraph_by_loc(Path::new("a.md"), 1).unwrap();
    let mentions: Vec<NoteId> = g.mentions_of(b).iter().map(|(src, _)| *src).collect();
    assert!(
        mentions.contains(&a_para),
        "a's paragraph mentions b via the anchored link"
    );
}

#[test]
fn anchor_match_normalizes_case_whitespace_trailing_hashes() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::write(root.join("a.md"), "see [[b#My Section]]\n").unwrap();
        // Heading text "My Section" (trailing ### stripped by extract_headings).
        std::fs::write(root.join("b.md"), "# My Section ###\nbody\n").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let b = note(&g, "b.md");
    assert!(
        g.resolve_anchor(b, "my section").is_some(),
        "case-insensitive"
    );
    assert!(
        g.resolve_anchor(b, "  My   Section  ").is_some(),
        "whitespace-collapsed"
    );
    assert!(g.resolve_anchor(b, "Nope").is_none());
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
    // Two Link incoming edges (a.md, b.md). Paragraph nodes also link
    // via ParagraphLink — filter to Link-form edges for this assertion.
    let link_in = |g: &Graph, id: NoteId| {
        g.incoming(id)
            .filter(|(_, e)| matches!(e, EdgeKind::NoteLink(_)))
            .count()
    };
    assert_eq!(link_in(&g, phantom), 2);

    // Remove the link from a.md only.
    let mut f = std::fs::File::create(tmp.path().join("a.md")).unwrap();
    writeln!(f, "nothing").unwrap();
    drop(f);

    g.refresh_note(&v.path, &tmp.path().join("a.md")).unwrap();
    let phantom = g
        .ghost_by_raw("Phantom")
        .expect("ghost should still exist (b still links)");
    assert_eq!(link_in(&g, phantom), 1);
}

// ── Paragraph node tests ──────────────────────────────────────────────

#[test]
fn paragraph_nodes_inserted_for_each_paragraph_in_note() {
    use assert_fs::prelude::*;

    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("a.md")
        .write_str("first paragraph\n\nsecond paragraph\n")
        .unwrap();

    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let a = g.note_by_path(Path::new("a.md")).unwrap();

    let owned: Vec<NoteId> = g
        .outgoing(a)
        .filter(|(_, e)| matches!(e, EdgeKind::OwnsParagraph))
        .map(|(p, _)| p)
        .collect();
    assert_eq!(owned.len(), 2, "two paragraphs → two OwnsParagraph edges");
    for p_id in &owned {
        assert!(matches!(g.node(*p_id), NodeKind::Paragraph(_)));
    }
}

#[test]
fn paragraph_link_edges_resolve_to_target_note() {
    use assert_fs::prelude::*;

    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("a.md").write_str("links to [[b]]\n").unwrap();
    tmp.child("b.md").write_str("hello\n").unwrap();

    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let a = g.note_by_path(Path::new("a.md")).unwrap();
    let b = g.note_by_path(Path::new("b.md")).unwrap();

    let paragraph = g
        .outgoing(a)
        .find(|(_, e)| matches!(e, EdgeKind::OwnsParagraph))
        .map(|(p, _)| p)
        .expect("a.md owns one paragraph");
    let targets: Vec<NoteId> = g
        .outgoing(paragraph)
        .filter(|(_, e)| matches!(e, EdgeKind::ParagraphLink(_)))
        .map(|(t, _)| t)
        .collect();
    assert_eq!(targets, vec![b], "paragraph links to b via ParagraphLink");
}

#[test]
fn paragraph_link_to_unresolved_target_creates_ghost() {
    use assert_fs::prelude::*;

    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("a.md").write_str("see [[Phantom]]\n").unwrap();

    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let phantom = g.ghost_by_raw("Phantom").expect("ghost exists");
    let paragraph_link_in: usize = g
        .incoming(phantom)
        .filter(|(_, e)| matches!(e, EdgeKind::ParagraphLink(_)))
        .count();
    assert_eq!(paragraph_link_in, 1);
}

#[test]
fn paragraph_by_loc_lookup_returns_correct_id() {
    use assert_fs::prelude::*;

    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("a.md")
        .write_str("first\n\nsecond paragraph here\n")
        .unwrap();

    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let g = Graph::build(&v, &Scan::default()).unwrap();

    let p1 = g.paragraph_by_loc(Path::new("a.md"), 1).unwrap();
    assert!(matches!(g.node(p1), NodeKind::Paragraph(p) if p.line_start == 1));
    let p2 = g.paragraph_by_loc(Path::new("a.md"), 3).unwrap();
    assert!(matches!(g.node(p2), NodeKind::Paragraph(p) if p.line_start == 3));
    assert!(g.paragraph_by_loc(Path::new("a.md"), 2).is_none());
}

#[test]
fn refresh_note_updates_paragraph_count() {
    use assert_fs::prelude::*;
    use std::io::Write as _;

    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("a.md").write_str("only paragraph\n").unwrap();

    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let mut g = Graph::build(&v, &Scan::default()).unwrap();
    let a = g.note_by_path(Path::new("a.md")).unwrap();

    let count = |g: &Graph| {
        g.outgoing(a)
            .filter(|(_, e)| matches!(e, EdgeKind::OwnsParagraph))
            .count()
    };
    assert_eq!(count(&g), 1);

    // Add a second paragraph.
    let mut f = std::fs::File::create(tmp.path().join("a.md")).unwrap();
    writeln!(f, "first\n\nsecond paragraph").unwrap();
    drop(f);

    g.refresh_note(&v.path, &tmp.path().join("a.md")).unwrap();
    assert_eq!(count(&g), 2, "refresh should reinsert paragraphs");
}

#[test]
fn refresh_note_clears_stale_paragraph_index_entries() {
    use assert_fs::prelude::*;
    use std::io::Write as _;

    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("a.md")
        .write_str("first\n\nsecond\n\nthird\n")
        .unwrap();

    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let mut g = Graph::build(&v, &Scan::default()).unwrap();

    // Original line_start for the third paragraph is 5; after rewrite
    // we drop the third, so paragraph_by_loc(a.md, 5) should be gone.
    assert!(g.paragraph_by_loc(Path::new("a.md"), 5).is_some());

    let mut f = std::fs::File::create(tmp.path().join("a.md")).unwrap();
    writeln!(f, "first\n\nsecond").unwrap();
    drop(f);

    g.refresh_note(&v.path, &tmp.path().join("a.md")).unwrap();
    assert!(g.paragraph_by_loc(Path::new("a.md"), 5).is_none());
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

/// Build a vault in a fresh temp dir from a closure that creates files
/// and directories under `path`. Common scaffolding for the empty-dir
/// tests below. `.obsidian/` is created automatically so `Vault::discover`
/// resolves to the temp dir.
fn temp_vault(setup: impl FnOnce(&Path)) -> (Vault, assert_fs::TempDir) {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    setup(tmp.path());
    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    (v, tmp)
}

fn contains_children(graph: &Graph, parent: NoteId) -> Vec<PathBuf> {
    graph
        .outgoing(parent)
        .filter(|(_, e)| matches!(e, EdgeKind::Contains))
        .map(|(dst, _)| match graph.node(dst) {
            NodeKind::Note(n) => n.path.clone(),
            NodeKind::Directory(d) => d.path.clone(),
            _ => PathBuf::new(),
        })
        .collect()
}

#[test]
fn empty_directory_appears_as_node() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::create_dir_all(root.join("Empty")).unwrap();
        std::fs::write(root.join("root.md"), "# root").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();

    let empty_id = g
        .node_by_path(Path::new("Empty"))
        .expect("empty dir must appear as a node");
    assert!(matches!(g.node(empty_id), NodeKind::Directory(_)));

    let root = dir_by_path(&g, "");
    let kids = contains_children(&g, root);
    assert!(
        kids.contains(&PathBuf::from("Empty")),
        "root must Contains the empty dir; got {kids:?}"
    );
}

#[test]
fn attachment_only_directory_appears_as_node() {
    let (v, _tmp) = temp_vault(|root| {
        let media = root.join("media");
        std::fs::create_dir_all(&media).unwrap();
        // a non-markdown file — the dir has no notes but exists
        std::fs::write(media.join("diagram.png"), b"\x89PNG").unwrap();
        std::fs::write(root.join("root.md"), "# root").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();

    let media_id = g
        .node_by_path(Path::new("media"))
        .expect("attachment-only dir must appear as a node");
    assert!(matches!(g.node(media_id), NodeKind::Directory(_)));

    let root = dir_by_path(&g, "");
    assert!(contains_children(&g, root).contains(&PathBuf::from("media")));
}

#[test]
fn default_ignored_dirs_stay_excluded_even_as_empty_dirs() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::create_dir_all(root.join("attachments")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("root.md"), "# root").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();

    assert!(
        g.node_by_path(Path::new("attachments")).is_none(),
        "default-ignored attachments/ dir must not appear"
    );
    assert!(
        g.node_by_path(Path::new(".git")).is_none(),
        ".git/ must not appear"
    );
    assert!(
        g.node_by_path(Path::new(".obsidian")).is_none(),
        ".obsidian/ must not appear"
    );
}

#[test]
fn nested_empty_directories_chain_to_root() {
    let (v, _tmp) = temp_vault(|root| {
        std::fs::create_dir_all(root.join("a/b/c")).unwrap();
        std::fs::write(root.join("root.md"), "# root").unwrap();
    });
    let g = Graph::build(&v, &Scan::default()).unwrap();

    let a = g.node_by_path(Path::new("a")).expect("a must exist");
    let b = g.node_by_path(Path::new("a/b")).expect("a/b must exist");
    let c = g
        .node_by_path(Path::new("a/b/c"))
        .expect("a/b/c must exist");
    let root = dir_by_path(&g, "");

    assert!(contains_children(&g, root).contains(&PathBuf::from("a")));
    assert!(contains_children(&g, a).contains(&PathBuf::from("a/b")));
    assert!(contains_children(&g, b).contains(&PathBuf::from("a/b/c")));
    // c is a leaf with no contained children
    assert!(contains_children(&g, c).is_empty());
}

#[test]
fn refresh_note_wires_missing_parent_dir_chain() {
    let (v, tmp) = temp_vault(|root| {
        std::fs::write(root.join("root.md"), "# root").unwrap();
    });
    let mut g = Graph::build(&v, &Scan::default()).unwrap();

    // No `fresh/` dir yet at build time.
    assert!(g.node_by_path(Path::new("fresh")).is_none());

    // Create a note under a brand-new dir chain, then refresh.
    std::fs::create_dir_all(tmp.path().join("fresh/sub")).unwrap();
    let new_note = tmp.path().join("fresh/sub/note.md");
    std::fs::write(&new_note, "# new").unwrap();
    g.refresh_note(tmp.path(), &new_note).unwrap();

    let root = dir_by_path(&g, "");
    let fresh = g
        .node_by_path(Path::new("fresh"))
        .expect("fresh dir must be created by refresh_note");
    let sub = g
        .node_by_path(Path::new("fresh/sub"))
        .expect("fresh/sub dir must be created by refresh_note");
    g.note_by_path(Path::new("fresh/sub/note.md"))
        .expect("note must be inserted");

    assert!(contains_children(&g, root).contains(&PathBuf::from("fresh")));
    assert!(contains_children(&g, fresh).contains(&PathBuf::from("fresh/sub")));
    assert!(contains_children(&g, sub).contains(&PathBuf::from("fresh/sub/note.md")));

    // Idempotent: a second refresh of the same note must not duplicate edges.
    g.refresh_note(tmp.path(), &new_note).unwrap();
    let fresh_kids = contains_children(&g, fresh);
    let sub_kids = contains_children(&g, sub);
    assert_eq!(
        fresh_kids
            .iter()
            .filter(|p| p.as_path() == Path::new("fresh/sub"))
            .count(),
        1
    );
    assert_eq!(
        sub_kids
            .iter()
            .filter(|p| p.as_path() == Path::new("fresh/sub/note.md"))
            .count(),
        1
    );
}

fn dirs_fixture_scan_with_tasks() -> Scan {
    Scan {
        tasks: vec![
            Task {
                description: "Fix login bug".into(),
                priority: Some(crate::task::Priority::High),
                tags: vec!["bug".into(), "urgent".into()],
                due: Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 1).unwrap()),
                scheduled: Some(chrono::NaiveDate::from_ymd_opt(2025, 5, 15).unwrap()),
                source_file: PathBuf::from("root.md"),
                source_line: 3,
                ..Default::default()
            },
            Task {
                description: "Review quarterly report".into(),
                status: Status::Done,
                tags: vec!["finance".into()],
                source_file: PathBuf::from("root.md"),
                source_line: 7,
                ..Default::default()
            },
            Task {
                description: "Process invoices".into(),
                priority: Some(crate::task::Priority::Medium),
                tags: vec!["finance".into(), "invoices".into()],
                due: Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 15).unwrap()),
                source_file: PathBuf::from("Areas/finance.md"),
                source_line: 5,
                ..Default::default()
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
        ..Default::default()
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

/// Subtask edges connect a parent task to each of its indented children,
/// parent → child, intra-file.
#[test]
fn build_creates_subtask_edges_from_parent_pointers() {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("house.md")
        .write_str("- [ ] house\n  - [ ] walls\n  - [ ] pipes\n")
        .unwrap();
    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();

    let parent = Task {
        description: "house".into(),
        source_file: PathBuf::from("house.md"),
        source_line: 1,
        ..Default::default()
    };
    let walls = Task {
        description: "walls".into(),
        source_file: PathBuf::from("house.md"),
        source_line: 2,
        parent: Some(1),
        ..Default::default()
    };
    let pipes = Task {
        description: "pipes".into(),
        source_file: PathBuf::from("house.md"),
        source_line: 3,
        parent: Some(1),
        ..Default::default()
    };
    let scan = Scan {
        tasks: vec![parent, walls, pipes],
        errors: vec![],
    };
    let g = Graph::build(&v, &scan).unwrap();

    // The parent task node has two outgoing Subtask edges.
    let parent_id = g.task_by_loc(Path::new("house.md"), 1).unwrap();
    let subtasks: Vec<_> = g
        .outgoing(parent_id)
        .filter(|(_, e)| matches!(e, EdgeKind::Subtask))
        .collect();
    assert_eq!(subtasks.len(), 2);

    // A leaf has none.
    let walls_id = g.task_by_loc(Path::new("house.md"), 2).unwrap();
    assert_eq!(
        g.outgoing(walls_id)
            .filter(|(_, e)| matches!(e, EdgeKind::Subtask))
            .count(),
        0
    );
}

/// HasTask edges reach top-level tasks only; subtasks are reachable via
/// the Subtask edge chain, never via a direct HasTask from the note.
/// (graph-task-interaction §D1.)
#[test]
fn hastask_edges_skip_subtasks() {
    use assert_fs::prelude::*;
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("house.md")
        .write_str("- [ ] house\n  - [ ] walls\n  - [ ] pipes\n")
        .unwrap();
    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();

    let parent = Task {
        description: "house".into(),
        source_file: PathBuf::from("house.md"),
        source_line: 1,
        ..Default::default()
    };
    let walls = Task {
        description: "walls".into(),
        source_file: PathBuf::from("house.md"),
        source_line: 2,
        parent: Some(1),
        ..Default::default()
    };
    let pipes = Task {
        description: "pipes".into(),
        source_file: PathBuf::from("house.md"),
        source_line: 3,
        parent: Some(1),
        ..Default::default()
    };
    let scan = Scan {
        tasks: vec![parent, walls, pipes],
        errors: vec![],
    };
    let g = Graph::build(&v, &scan).unwrap();

    let note_id = g.note_by_path(Path::new("house.md")).unwrap();
    // Exactly one HasTask edge: to the top-level task only.
    let hastask: Vec<_> = g
        .outgoing(note_id)
        .filter(|(_, e)| matches!(e, EdgeKind::HasTask))
        .collect();
    assert_eq!(
        hastask.len(),
        1,
        "only the top-level task gets a HasTask edge"
    );

    // Subtasks receive NO direct HasTask edge from the note.
    for line in [2, 3] {
        let subtask_id = g.task_by_loc(Path::new("house.md"), line).unwrap();
        let direct_hastask = g
            .incoming(subtask_id)
            .filter(|(_, e)| matches!(e, EdgeKind::HasTask))
            .count();
        assert_eq!(
            direct_hastask, 0,
            "subtask at line {line} must not have a direct HasTask edge"
        );
        // ...but it IS reachable via a Subtask edge from the parent.
        let via_subtask = g
            .incoming(subtask_id)
            .filter(|(_, e)| matches!(e, EdgeKind::Subtask))
            .count();
        assert_eq!(
            via_subtask, 1,
            "subtask at line {line} must be reachable via a Subtask edge"
        );
    }
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
                source_file: PathBuf::from("root.md"),
                source_line: 3,
                ..Default::default()
            },
            Task {
                description: "Second task (duplicate key, should be deduped)".into(),
                status: Status::Done,
                source_file: PathBuf::from("root.md"),
                source_line: 3,
                ..Default::default()
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

/// 3.2: Task whose source_file does not match any note: task node exists,
/// no HasTask edge terminates at it, and node where kind = Task returns it.
#[test]
fn task_with_no_matching_note() {
    let tmp = assert_fs::TempDir::new().unwrap();
    use assert_fs::prelude::*;
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("root.md").write_str("- [ ] Real task\n").unwrap();
    let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
    let scan = Scan {
        tasks: vec![Task {
            description: "Orphan task".into(),
            source_file: PathBuf::from("nonexistent.md"),
            source_line: 1,
            ..Default::default()
        }],
        errors: vec![],
    };
    let g = Graph::build(&v, &scan).unwrap();

    // (a) The task node exists
    let task_nodes: Vec<_> = g
        .nodes()
        .filter(|(_, k)| matches!(k, NodeKind::Task(_)))
        .collect();
    assert_eq!(task_nodes.len(), 1);
    if let NodeKind::Task(td) = task_nodes[0].1 {
        assert_eq!(td.description, "Orphan task");
        assert_eq!(td.source_file, PathBuf::from("nonexistent.md"));
    }

    // (b) No HasTask edge terminates at this task node
    let task_id = task_nodes[0].0;
    let incoming_has_task = g
        .incoming(task_id)
        .any(|(_, e)| matches!(e, EdgeKind::HasTask));
    assert!(
        !incoming_has_task,
        "orphan task should have no incoming HasTask edge"
    );

    // (c) node where kind = Task returns it
    use crate::graph::query::parse;
    let q = parse("node where kind = Task;").unwrap();
    let results = q.select(&g);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], task_id);
}

// ── LinksInto edge tests ────────────────────────────────────────────

use assert_fs::prelude::*;

fn make_links_vault(files: &[(&str, &str)]) -> (assert_fs::TempDir, Vault) {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    for (rel, content) in files {
        dir.child(rel).write_str(content).unwrap();
    }
    let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();
    (dir, vault)
}

/// Collect (src_path, dst_kind, dst_path_or_label) for outgoing LinksInto edges.
fn links_into_edges(graph: &Graph) -> Vec<(PathBuf, String, String)> {
    let mut results: Vec<(PathBuf, String, String)> = Vec::new();
    for (id, node) in graph.nodes() {
        let NodeKind::Note(nd) = node else { continue };
        for (dst, edge) in graph.outgoing(id) {
            if !matches!(edge, EdgeKind::LinksInto) {
                continue;
            }
            let dst_label = match graph.node(dst) {
                NodeKind::Directory(d) => {
                    if d.path.as_os_str().is_empty() {
                        "<root>".to_string()
                    } else {
                        d.path.to_string_lossy().into_owned()
                    }
                }
                other => format!("{:?}", other),
            };
            results.push((nd.path.clone(), "LinksInto".into(), dst_label));
        }
    }
    results.sort();
    results
}

/// 4.1: Note linking to a note in a subdirectory produces a LinksInto edge.
#[test]
fn links_into_subdirectory() {
    let (_dir, v) = make_links_vault(&[("a/b/foo.md", "[[Bar]]"), ("c/d/e/Bar.md", "")]);
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let edges = links_into_edges(&g);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].0, PathBuf::from("a/b/foo.md"));
    assert_eq!(edges[0].2, "c/d/e");
}

/// 4.2: Note linking to a root-level note produces a LinksInto edge
/// to the root Directory node.
#[test]
fn links_into_root_level_target() {
    let (_dir, v) = make_links_vault(&[("a/foo.md", "[[Index]]"), ("Index.md", "")]);
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let edges = links_into_edges(&g);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].0, PathBuf::from("a/foo.md"));
    assert_eq!(edges[0].2, "<root>");
}

/// 4.3: Embed link produces a LinksInto edge.
#[test]
fn links_into_from_embed() {
    let (_dir, v) = make_links_vault(&[("a/foo.md", "![[pic]]"), ("images/pic.md", "")]);
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let edges = links_into_edges(&g);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].0, PathBuf::from("a/foo.md"));
    assert_eq!(edges[0].2, "images");
}

/// 4.4: Multiple links to notes in the same folder produce exactly one
/// LinksInto edge (deduplication).
#[test]
fn links_into_deduplicates_same_folder() {
    let (_dir, v) =
        make_links_vault(&[("a/foo.md", "[[X]]\n[[Y]]"), ("d/X.md", ""), ("d/Y.md", "")]);
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let edges = links_into_edges(&g);
    assert_eq!(
        edges.len(),
        1,
        "should be exactly one LinksInto edge to folder d"
    );
    assert_eq!(edges[0].0, PathBuf::from("a/foo.md"));
    assert_eq!(edges[0].2, "d");
}

/// 4.5: Links to notes in different folders produce separate LinksInto edges.
#[test]
fn links_into_different_folders() {
    let (_dir, v) = make_links_vault(&[
        ("a/foo.md", "[[X]]\n[[Y]]"),
        ("d1/X.md", ""),
        ("d2/Y.md", ""),
    ]);
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let edges = links_into_edges(&g);
    assert_eq!(edges.len(), 2);
    let dirs: Vec<&str> = edges.iter().map(|e| e.2.as_str()).collect();
    assert!(dirs.contains(&"d1"));
    assert!(dirs.contains(&"d2"));
}

/// 4.6: Unresolved (ghost) links produce no LinksInto edges.
#[test]
fn links_into_excludes_ghosts() {
    let (_dir, v) = make_links_vault(&[("a/foo.md", "[[Phantom]]")]);
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let edges = links_into_edges(&g);
    assert!(
        edges.is_empty(),
        "ghost links should produce no LinksInto edges"
    );
}

/// 4.7: Mix of resolved and unresolved — resolved produces LinksInto, ghost does not.
#[test]
fn links_into_mixed_resolved_and_ghost() {
    let (_dir, v) = make_links_vault(&[("a/foo.md", "[[Real]]\n[[Phantom]]"), ("d/Real.md", "")]);
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let edges = links_into_edges(&g);
    // Only one LinksInto edge (from Real), not from Phantom.
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].2, "d");
}

/// 4.8: Note linking to a sibling in its own folder still produces a LinksInto edge.
#[test]
fn links_into_self_folder() {
    let (_dir, v) = make_links_vault(&[("a/b/foo.md", "[[Baz]]"), ("a/b/Baz.md", "")]);
    let g = Graph::build(&v, &Scan::default()).unwrap();
    let edges = links_into_edges(&g);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].2, "a/b");
}

/// 4.9: refresh_note recomputes LinksInto edges correctly.
#[test]
fn links_into_refresh_note() {
    let (dir, v) = make_links_vault(&[("a/foo.md", "[[X]]"), ("d1/X.md", ""), ("d2/Y.md", "")]);
    let mut g = Graph::build(&v, &Scan::default()).unwrap();

    // Initially: one LinksInto edge to d1.
    let initial = links_into_edges(&g);
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].2, "d1");

    // Edit foo.md to also link to d2/Y.
    dir.child("a/foo.md").write_str("[[X]]\n[[Y]]").unwrap();
    g.refresh_note(&v.path, &PathBuf::from("a/foo.md")).unwrap();

    let updated = links_into_edges(&g);
    assert_eq!(
        updated.len(),
        2,
        "should now have LinksInto edges to both d1 and d2"
    );
    let dirs: Vec<&str> = updated.iter().map(|e| e.2.as_str()).collect();
    assert!(dirs.contains(&"d1"));
    assert!(dirs.contains(&"d2"));

    // Remove all links from foo.md.
    dir.child("a/foo.md").write_str("").unwrap();
    g.refresh_note(&v.path, &PathBuf::from("a/foo.md")).unwrap();

    let cleared = links_into_edges(&g);
    assert!(
        cleared.is_empty(),
        "no links should mean no LinksInto edges"
    );
}

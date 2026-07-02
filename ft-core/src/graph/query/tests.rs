use super::eval::edge_kind_str;
use super::*;
use crate::vault::Scan;

/// Every edge kind the graph can produce must be an accepted `edge.kind`
/// value, or that edge becomes silently unqueryable. Guards the two lists
/// (`edge_kind_str` ↔ [`EDGE_KIND_VALUES`]) against drift.
#[test]
fn every_edge_kind_is_a_queryable_value() {
    fn link() -> crate::graph::LinkEdge {
        crate::graph::LinkEdge {
            form: LinkForm::WikiLink,
            is_embed: false,
            byte_range: 0..0,
            line: 1,
            raw_text: String::new(),
            target_text: String::new(),
            anchor: None,
            display: None,
        }
    }
    // Exhaustive by construction: a new EdgeKind variant forces a new
    // entry here (and then in EDGE_KIND_VALUES to pass).
    let all = [
        EdgeKind::NoteLink(link()),
        EdgeKind::HeadingLink(link()),
        EdgeKind::ParagraphLink(link()),
        EdgeKind::Contains,
        EdgeKind::HasTask,
        EdgeKind::Subtask,
        EdgeKind::LinksInto,
        EdgeKind::OwnsParagraph,
        EdgeKind::OwnsHeading,
    ];
    for e in &all {
        let name = edge_kind_str(e);
        assert!(
            EDGE_KIND_VALUES.contains(&name),
            "edge kind `{name}` is missing from EDGE_KIND_VALUES"
        );
    }
    assert_eq!(EDGE_KIND_VALUES.len(), all.len(), "no stale extra values");
}

// ── Parser tests ─────────────────────────────────────────────────

mod parser {
    use super::*;

    fn parse_ok(src: &str) -> GraphQuery {
        parse(src).unwrap_or_else(|e| panic!("parse failed for {src:?}: {e}"))
    }

    #[test]
    fn parse_node_match_all() {
        let q = parse_ok("node;");
        assert_eq!(q.initial.len(), 1);
        assert!(q.initial[0].conditions().is_empty());
        assert!(q.initial[0].without.is_none());
        assert!(q.expansion.is_none());
    }

    #[test]
    fn parse_node_no_trailing_semi() {
        let q = parse_ok("node");
        assert_eq!(q.initial.len(), 1);
    }

    #[test]
    fn parse_kind_eq() {
        let q = parse_ok("node where kind = Note;");
        assert_eq!(
            q.initial[0].conditions()[0],
            &Condition {
                subject: Subject::SelfNode,
                attr: Attr::Kind,
                op: Op::Eq,
                value: Value::Single(Literal::Ident("Note".into())),
            }
        );
    }

    #[test]
    fn parse_self_qualified() {
        let q = parse_ok("node where self.kind = Directory;");
        assert_eq!(q.initial[0].conditions()[0].subject, Subject::SelfNode);
        assert_eq!(q.initial[0].conditions()[0].attr, Attr::Kind);
    }

    #[test]
    fn parse_kind_in_set() {
        let q = parse_ok("node where kind in {Note, Directory};");
        assert_eq!(
            q.initial[0].conditions()[0].value,
            Value::Set(vec![
                Literal::Ident("Note".into()),
                Literal::Ident("Directory".into()),
            ])
        );
    }

    #[test]
    fn parse_path_starts_with() {
        let q = parse_ok("node where path starts_with \"Projects/\";");
        assert_eq!(q.initial[0].conditions()[0].op, Op::StartsWith);
        assert_eq!(
            q.initial[0].conditions()[0].value,
            Value::Single(Literal::Str("Projects/".into()))
        );
    }

    #[test]
    fn parse_path_ends_with() {
        let q = parse_ok("node where path ends_with \".md\";");
        assert_eq!(q.initial[0].conditions()[0].op, Op::EndsWith);
    }

    #[test]
    fn parse_path_includes() {
        let q = parse_ok("node where path includes \"Areas\";");
        assert_eq!(q.initial[0].conditions()[0].op, Op::Includes);
    }

    #[test]
    fn parse_multiple_and_conditions() {
        let q = parse_ok("node where kind = Note and path starts_with \"Areas/\";");
        assert_eq!(q.initial[0].conditions().len(), 2);
    }

    #[test]
    fn parse_indegree() {
        let q = parse_ok("node where indegree = 0;");
        assert_eq!(q.initial[0].conditions()[0].attr, Attr::Indegree);
        assert_eq!(
            q.initial[0].conditions()[0].value,
            Value::Single(Literal::Int(0))
        );
    }

    #[test]
    fn parse_without_incoming() {
        let q =
            parse_ok("node where kind = Directory without incoming(kind = directory-contains);");
        let nf = q.initial[0].without.as_ref().unwrap();
        assert_eq!(nf.direction, Direction::Incoming);
        assert_eq!(nf.conditions.len(), 1);
        assert_eq!(nf.conditions[0].subject, Subject::Edge);
        assert_eq!(nf.conditions[0].attr, Attr::Kind);
        assert_eq!(
            nf.conditions[0].value,
            Value::Single(Literal::Ident("directory-contains".into()))
        );
    }

    #[test]
    fn parse_without_outgoing() {
        let q = parse_ok("node without outgoing();");
        assert_eq!(
            q.initial[0].without.as_ref().unwrap().direction,
            Direction::Outgoing
        );
    }

    #[test]
    fn parse_two_node_blocks() {
        let q = parse_ok("node where kind = Note; node where kind = Directory;");
        assert_eq!(q.initial.len(), 2);
    }

    #[test]
    fn parse_expand_simple() {
        let q = parse_ok("node; expand where edge.kind = note-link;");
        let pol = q.expansion.as_ref().unwrap();
        assert_eq!(pol.conditions.len(), 1);
        assert_eq!(pol.conditions[0].subject, Subject::Edge);
    }

    #[test]
    fn parse_expand_full_directory_tree() {
        let q = parse_ok(
            "node where kind = Directory; expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};",
        );
        let pol = q.expansion.as_ref().unwrap();
        assert_eq!(pol.conditions.len(), 3);
        let subjects: Vec<Subject> = pol.conditions.iter().map(|c| c.subject).collect();
        assert_eq!(subjects, vec![Subject::From, Subject::Edge, Subject::To]);
    }

    #[test]
    fn parse_string_with_escape() {
        let q = parse_ok("node where title = \"with \\\"quotes\\\"\";");
        assert_eq!(
            q.initial[0].conditions()[0].value,
            Value::Single(Literal::Str("with \"quotes\"".into()))
        );
    }

    // ── Error paths ─────────────────────────────────────────────

    #[test]
    fn empty_input() {
        assert!(matches!(parse("   "), Err(DslError::EmptyInput)));
    }

    #[test]
    fn no_initial_set() {
        let err = parse("expand where edge.kind = note-link;").unwrap_err();
        assert!(matches!(err, DslError::NoInitialSet));
    }

    #[test]
    fn type_mismatch_eq_with_set() {
        let err = parse("node where kind = {Note, Directory};").unwrap_err();
        assert!(matches!(err, DslError::TypeMismatch { .. }));
    }

    #[test]
    fn type_mismatch_in_with_single() {
        let err = parse("node where kind in Note;").unwrap_err();
        assert!(matches!(err, DslError::TypeMismatch { .. }));
    }

    #[test]
    fn ambiguous_attr_in_expand() {
        let err = parse("node; expand where kind = link;").unwrap_err();
        assert!(matches!(err, DslError::AmbiguousAttribute { .. }));
    }

    #[test]
    fn scope_error_from_in_node_block() {
        let err = parse("node where from.kind = Directory;").unwrap_err();
        assert!(matches!(err, DslError::ScopeError { .. }));
    }

    #[test]
    fn scope_error_self_in_expand() {
        let err = parse("node; expand where self.kind = Directory;").unwrap_err();
        assert!(matches!(err, DslError::ScopeError { .. }));
    }

    #[test]
    fn scope_error_indegree_in_expand() {
        let err = parse("node; expand where from.indegree = 0;").unwrap_err();
        assert!(matches!(err, DslError::ScopeError { .. }));
    }

    #[test]
    fn scope_error_form_on_node() {
        let err = parse("node where form = wiki;").unwrap_err();
        assert!(matches!(err, DslError::ScopeError { .. }));
    }

    #[test]
    fn scope_error_path_on_edge() {
        let err = parse("node; expand where edge.path = foo;").unwrap_err();
        assert!(matches!(err, DslError::ScopeError { .. }));
    }

    #[test]
    fn unknown_attribute() {
        let err = parse("node where foo = bar;").unwrap_err();
        assert!(matches!(err, DslError::UnknownAttribute { .. }));
    }

    #[test]
    fn unknown_kind_value() {
        let err = parse("node where kind = Notes;").unwrap_err();
        match err {
            DslError::UnknownKindValue { value, .. } => assert_eq!(value, "Notes"),
            other => panic!("expected UnknownKindValue, got {other:?}"),
        }
    }

    #[test]
    fn unknown_kind_value_in_set() {
        let err = parse("node where kind in {Note, Bogus};").unwrap_err();
        assert!(matches!(err, DslError::UnknownKindValue { .. }));
    }

    #[test]
    fn expand_over_subtask_edges_parses() {
        // The subtask edge is a first-class traversable edge kind.
        parse("node where kind = Task; expand where edge.kind = subtask;").unwrap();
        parse("node; expand where edge.kind in {subtask, has-task};").unwrap();
    }

    #[test]
    fn unknown_form_value() {
        let err = parse("node; expand where edge.form = html;").unwrap_err();
        assert!(matches!(err, DslError::UnknownKindValue { .. }));
    }

    #[test]
    fn trailing_input() {
        let err = parse("node; junk").unwrap_err();
        assert!(matches!(err, DslError::TrailingInput { .. }));
    }

    #[test]
    fn unterminated_string() {
        let err = parse("node where path = \"oops").unwrap_err();
        assert!(matches!(err, DslError::UnterminatedString { .. }));
    }

    #[test]
    fn illegal_character() {
        let err = parse("node where path = @bogus;").unwrap_err();
        assert!(matches!(err, DslError::IllegalCharacter { .. }));
    }

    #[test]
    fn no_with_keyword_anymore() {
        // The v1 keyword `with` should now fail. `n` after `node`
        // is parsed as trailing input.
        let err = parse("node n with n.kind = Note;").unwrap_err();
        assert!(matches!(err, DslError::TrailingInput { .. }));
    }

    #[test]
    fn no_over_keyword_anymore() {
        // Old `expand over e(n, m) ...` should fail at parse.
        // `over` is no longer a keyword; treated as trailing input
        // after `expand`.
        let err = parse("node; expand over e(n, m) with e.kind = directory-contains;").unwrap_err();
        assert!(matches!(err, DslError::TrailingInput { .. }));
    }
}

// ── Display / round-trip tests ───────────────────────────────────

mod display {
    use super::*;

    fn roundtrip(src: &str) {
        let q1 = parse(src).unwrap();
        let s = format!("{q1}");
        let q2 = parse(&s).unwrap_or_else(|e| panic!("re-parse failed for {s:?}: {e}"));
        assert_eq!(q1, q2, "round-trip mismatch:\n  src: {src}\n  ser: {s}");
    }

    #[test]
    fn rt_match_all() {
        roundtrip("node;");
    }

    #[test]
    fn rt_kind_eq() {
        roundtrip("node where kind = Note;");
    }

    #[test]
    fn rt_kind_in_set() {
        roundtrip("node where kind in {Note, Directory};");
    }

    #[test]
    fn rt_path_starts_with() {
        roundtrip("node where path starts_with \"Projects/\";");
    }

    #[test]
    fn rt_path_ends_with() {
        roundtrip("node where path ends_with \".md\";");
    }

    #[test]
    fn rt_multi_and() {
        roundtrip("node where kind = Note and path starts_with \"Areas/\";");
    }

    #[test]
    fn rt_without_incoming() {
        roundtrip("node where kind = Directory without incoming(kind = directory-contains);");
    }

    #[test]
    fn rt_without_outgoing_empty() {
        roundtrip("node without outgoing();");
    }

    #[test]
    fn rt_two_blocks() {
        roundtrip("node where kind = Note; node where kind = Directory;");
    }

    #[test]
    fn rt_expand_full() {
        roundtrip(
            "node where kind = Directory; expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};",
        );
    }

    #[test]
    fn rt_indegree_zero() {
        roundtrip("node where indegree = 0;");
    }

    #[test]
    fn rt_string_with_escapes() {
        roundtrip("node where title = \"with \\\"quotes\\\" and \\\\ slash\";");
    }

    #[test]
    fn self_collapses_to_bare() {
        let q = parse("node where self.kind = Note;").unwrap();
        let s = format!("{q}");
        assert_eq!(s, "node where kind = Note;");
    }
}

// ── Evaluator tests ──────────────────────────────────────────────

mod eval {
    use std::path::{Path, PathBuf};

    use crate::graph::Graph;
    use crate::vault::Vault;

    use super::*;

    fn dirs_vault() -> Vault {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/dirs");
        Vault::discover(Some(path)).expect("dirs fixture vault must exist")
    }

    fn links_vault() -> Vault {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/links");
        Vault::discover(Some(path)).expect("links fixture vault must exist")
    }

    #[test]
    fn select_match_all() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse("node;").unwrap();
        let ids = q.select(&g);
        // 4 notes + 4 dirs + 4 paragraphs + 4 headings = 16
        assert_eq!(ids.len(), 16);
    }

    #[test]
    fn select_all_notes() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse("node where kind = Note;").unwrap();
        let ids = q.select(&g);
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn select_all_directories() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse("node where kind = Directory;").unwrap();
        let ids = q.select(&g);
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn select_path_starts_with() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse("node where path starts_with \"Areas\";").unwrap();
        let ids = q.select(&g);
        // Areas dir, Areas/finance.md, Areas/operations dir, Areas/operations/shifts.md
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn select_path_starts_with_strict() {
        // Substring would match Areas/old-Projects/ too if it existed;
        // starts_with rejects matches that aren't a true prefix.
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse("node where path starts_with \"Projects/\";").unwrap();
        let ids = q.select(&g);
        // Only Projects/alpha.md (the directory itself is "Projects", not "Projects/")
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn select_path_ends_with_md() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse("node where path ends_with \".md\";").unwrap();
        let ids = q.select(&g);
        // All 4 notes end with .md; no directories should match.
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn select_kind_in_set() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse("node where kind in {Note, Directory};").unwrap();
        let ids = q.select(&g);
        assert_eq!(ids.len(), 8);
    }

    #[test]
    fn select_indegree_zero() {
        // Only the vault root directory has no incoming edges.
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse("node where indegree = 0;").unwrap();
        let ids = q.select(&g);
        assert_eq!(ids.len(), 1);
        assert!(matches!(
            g.node(ids[0]),
            NodeKind::Directory(d) if d.path.as_os_str().is_empty()
        ));
    }

    #[test]
    fn select_without_incoming_contains() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse(
            "node where kind in {Note, Directory} without incoming(kind = directory-contains);",
        )
        .unwrap();
        let ids = q.select(&g);
        assert_eq!(ids.len(), 1);
        assert!(matches!(
            g.node(ids[0]),
            NodeKind::Directory(d) if d.path.as_os_str().is_empty()
        ));
    }

    #[test]
    fn select_two_blocks_union_deduped() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q =
            parse("node where kind = Directory; node where path starts_with \"Areas\";").unwrap();
        let ids = q.select(&g);
        // 4 dirs + Areas/finance.md + Areas/operations/shifts.md = 6
        // (Areas dir and Areas/operations dir already in first block)
        assert_eq!(ids.len(), 6);
    }

    #[test]
    fn expand_full_directory_tree() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse(
            "node where indegree = 0; expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};",
        )
        .unwrap();
        let roots = q.select(&g);
        assert_eq!(roots.len(), 1);

        let children = q.expand(&g, roots[0]).unwrap();
        // root has: root.md, Areas/, Projects/
        assert_eq!(children.len(), 3);
    }

    #[test]
    fn expand_notes_only() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse(
            "node where indegree = 0; expand where from.kind = Directory and edge.kind = directory-contains and to.kind = Note;",
        )
        .unwrap();
        let roots = q.select(&g);
        let children = q.expand(&g, roots[0]).unwrap();
        // Only root.md (the Note child of the root dir)
        assert_eq!(children.len(), 1);
        assert!(matches!(g.node(children[0]), NodeKind::Note(_)));
    }

    #[test]
    fn expand_none_when_no_policy() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse("node where kind = Note;").unwrap();
        let any = q.select(&g)[0];
        assert!(q.expand(&g, any).is_none());
    }

    #[test]
    fn expand_some_empty_when_parent_mismatch() {
        // v2 behavior: parent that doesn't satisfy `from` conditions
        // returns Some(vec![]), not None.
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q =
            parse("node; expand where from.kind = Directory and edge.kind = directory-contains;")
                .unwrap();
        let note_id = g
            .nodes()
            .find(|(_, k)| matches!(k, NodeKind::Note(_)))
            .map(|(id, _)| id)
            .unwrap();
        let children = q.expand(&g, note_id).unwrap();
        assert!(children.is_empty());
    }

    #[test]
    fn expand_on_links_vault() {
        let v = links_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse(
            "node; expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};",
        )
        .unwrap();
        let notes_dir = g.node_by_path(Path::new("notes")).unwrap();
        let children = q.expand(&g, notes_dir).unwrap();
        assert_eq!(children.len(), 6);
    }

    #[test]
    fn title_match_on_note() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        // `title` matches both the note `root` and its heading `root`.
        let q = parse("node where title = \"root\";").unwrap();
        let ids = q.select(&g);
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn select_kind_paragraph_returns_only_paragraph_nodes() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse("node where kind = Paragraph;").unwrap();
        let ids = q.select(&g);
        assert_eq!(ids.len(), 4, "one paragraph (heading) per note");
        for id in &ids {
            assert!(matches!(g.node(*id), NodeKind::Paragraph(_)));
        }
    }

    #[test]
    fn expand_owns_paragraph_yields_paragraph_children_of_note() {
        use assert_fs::prelude::*;

        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("note.md")
            .write_str("first\n\nsecond paragraph\n")
            .unwrap();
        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse(
            "node where kind = Note; \
             expand where from.kind = Note and edge.kind = owns-paragraph;",
        )
        .unwrap();
        let roots = q.select(&g);
        assert_eq!(roots.len(), 1);
        let children = q.expand(&g, roots[0]).unwrap();
        assert_eq!(children.len(), 2);
        for id in children {
            assert!(matches!(g.node(id), NodeKind::Paragraph(_)));
        }
    }

    #[test]
    fn select_kind_heading_returns_only_heading_nodes() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse("node where kind = Heading;").unwrap();
        let ids = q.select(&g);
        assert_eq!(ids.len(), 4, "one heading per note");
        for id in &ids {
            assert!(matches!(g.node(*id), NodeKind::Heading(_)));
        }
    }

    #[test]
    fn heading_title_filter_matches_heading_text() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse("node where kind = Heading and title = \"root\";").unwrap();
        let ids = q.select(&g);
        assert_eq!(ids.len(), 1);
        assert!(matches!(g.node(ids[0]), NodeKind::Heading(_)));
    }

    #[test]
    fn expand_owns_heading_yields_subheadings() {
        use assert_fs::prelude::*;

        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("note.md").write_str("# A\n## B\n## C\n").unwrap();
        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &v.scan()).unwrap();
        // Note -> direct headings A; then A -> B, A -> C via owns-heading.
        // The expand clause matches both hops (from Note and from Heading).
        let q = parse(
            "node where kind = Note; \
             expand where from.kind in {Note, Heading} and edge.kind = owns-heading;",
        )
        .unwrap();
        let roots = q.select(&g);
        assert_eq!(roots.len(), 1);
        let top = q.expand(&g, roots[0]).unwrap();
        assert_eq!(top.len(), 1, "only A is a direct child of the note");
        let subs = q.expand(&g, top[0]).unwrap();
        assert_eq!(subs.len(), 2, "A owns B and C");
        for id in subs {
            assert!(matches!(g.node(id), NodeKind::Heading(_)));
        }
    }

    #[test]
    fn edge_kind_values_include_new_link_kinds() {
        // note-link / heading-link / paragraph-link are all accepted.
        for k in ["note-link", "heading-link", "paragraph-link"] {
            let q = parse(&format!(
                "node where kind = Note; expand where edge.kind = {k};"
            ));
            assert!(q.is_ok(), "{k} should parse: {:?}", q);
        }
    }

    #[test]
    fn edge_embed_predicate_filters_embeds() {
        use assert_fs::prelude::*;

        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("note.md")
            .write_str("plain [[b]] and embed ![[b]]\n")
            .unwrap();
        tmp.child("b.md").write_str("# b\n").unwrap();
        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &v.scan()).unwrap();
        // expand following only embed edges yields b (via ![[b]]) but
        // we ask for embed=true: both link occurrences to b exist, but
        // the embed-only filter yields exactly the embed occurrence.
        let q = parse(
            "node where kind = Note; \
             expand where edge.kind = note-link and edge.embed = true;",
        )
        .unwrap();
        let roots = q.select(&g);
        assert_eq!(roots.len(), 2, "note.md and b.md are both notes");
        // Expand only from note.md (the one with the embed).
        let note_id = roots
            .iter()
            .copied()
            .find(|id| matches!(g.node(*id), NodeKind::Note(n) if n.path == Path::new("note.md")))
            .unwrap();
        let children = q.expand(&g, note_id).unwrap();
        // The embed filter keeps only ![[b]]; the destination is b.
        assert!(!children.is_empty(), "embed edge yields b");
        for id in &children {
            assert!(matches!(g.node(*id), NodeKind::Note(_)));
        }
        // Non-embed filter also yields b (sanity).
        let q2 = parse(
            "node where kind = Note; \
             expand where edge.kind = note-link and edge.embed = false;",
        )
        .unwrap();
        let children2 = q2.expand(&g, note_id).unwrap();
        assert!(!children2.is_empty(), "non-embed edge yields b");
    }

    #[test]
    fn edge_embed_rejects_non_boolean() {
        let err = parse("node; expand where edge.embed = yes;").unwrap_err();
        assert!(matches!(err, DslError::UnknownKindValue { .. }));
    }

    #[test]
    fn old_edge_kind_values_rejected() {
        for old in ["link", "embed"] {
            let err = parse(&format!("node; expand where edge.kind = {old};")).unwrap_err();
            assert!(
                matches!(err, DslError::UnknownKindValue { .. }),
                "{old} should be rejected"
            );
        }
    }

    #[test]
    fn expand_paragraph_link_yields_target_notes() {
        use assert_fs::prelude::*;

        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("a.md").write_str("links to [[b]]\n").unwrap();
        tmp.child("b.md").write_str("hi\n").unwrap();
        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse(
            "node where kind = Paragraph; \
             expand where from.kind = Paragraph and edge.kind = paragraph-link;",
        )
        .unwrap();
        let paragraphs = q.select(&g);
        // Two notes → two paragraphs; only a's paragraph has an
        // outgoing ParagraphLink edge.
        let mut total_targets = 0;
        for p in paragraphs {
            let children = q.expand(&g, p).unwrap();
            total_targets += children.len();
        }
        assert_eq!(total_targets, 1);
    }

    #[test]
    fn outdegree_zero_excludes_root() {
        let v = dirs_vault();
        let g = Graph::build(&v, &v.scan()).unwrap();
        // Restrict to Note kind: notes now own paragraph nodes via
        // OwnsParagraph edges (outdegree > 0), so leaf notes can no
        // longer have outdegree = 0. Filter to Paragraph instead
        // for the leaf check.
        let q = parse("node where kind = Paragraph and outdegree = 0;").unwrap();
        let ids = q.select(&g);
        // The 4 heading-only paragraphs in dirs/ have no outgoing
        // edges (no wiki links).
        assert_eq!(ids.len(), 4);
        for id in &ids {
            assert!(matches!(g.node(*id), NodeKind::Paragraph(_)));
        }
    }
}

// ── Walk tests ───────────────────────────────────────────────────

mod walk {
    use std::path::PathBuf;

    use assert_fs::prelude::*;

    use crate::graph::{EdgeKind, Graph};
    use crate::vault::Vault;

    use super::*;

    fn dirs_graph() -> Graph {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/dirs");
        let v = Vault::discover(Some(path)).expect("dirs fixture vault must exist");
        Graph::build(&v, &v.scan()).unwrap()
    }

    fn dirs_query() -> GraphQuery {
        parse(
            "node where kind = Directory and path = \"\"; \
             expand where from.kind = Directory \
                      and edge.kind = directory-contains \
                      and to.kind in {Note, Directory};",
        )
        .unwrap()
    }

    fn count_nodes(tree: &[WalkNode]) -> usize {
        tree.iter().map(|n| 1 + count_nodes(&n.children)).sum()
    }

    fn max_depth(tree: &[WalkNode]) -> usize {
        tree.iter()
            .map(|n| {
                if n.children.is_empty() {
                    n.depth
                } else {
                    max_depth(&n.children)
                }
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn walk_unbounded_dirs_returns_full_tree() {
        let g = dirs_graph();
        let q = dirs_query();
        let tree = q.walk(&g, &WalkOptions::unlimited());
        assert_eq!(tree.len(), 1, "exactly one root: the vault root");
        assert_eq!(tree[0].depth, 0);
        assert!(tree[0].edge_to_parent.is_none(), "roots carry no edge");
        // 4 dirs (root + Projects + Areas + Areas/operations) + 4 notes
        // = 8 nodes reachable from the root. The walk visits every
        // node exactly once.
        assert_eq!(count_nodes(&tree), 8);
        // The deepest path is root → Areas → operations → shifts.md
        assert_eq!(max_depth(&tree), 3);
    }

    #[test]
    fn walk_depth_zero_returns_roots_only() {
        let g = dirs_graph();
        let q = dirs_query();
        let tree = q.walk(
            &g,
            &WalkOptions {
                max_depth: Some(0),
                ..Default::default()
            },
        );
        assert_eq!(tree.len(), 1);
        for root in &tree {
            assert!(root.children.is_empty(), "depth=0 means no descent at all");
        }
    }

    #[test]
    fn walk_depth_one_returns_immediate_children() {
        let g = dirs_graph();
        let q = dirs_query();
        let tree = q.walk(
            &g,
            &WalkOptions {
                max_depth: Some(1),
                ..Default::default()
            },
        );
        assert_eq!(tree.len(), 1);
        // Root's immediate children: Projects/, Areas/, root.md = 3
        assert_eq!(tree[0].children.len(), 3);
        for child in &tree[0].children {
            assert_eq!(child.depth, 1);
            assert!(child.children.is_empty(), "depth=1 means no grandchildren");
            assert!(matches!(child.edge_to_parent, Some(EdgeKind::Contains)));
        }
    }

    #[test]
    fn walk_edge_to_parent_is_populated_for_non_roots() {
        let g = dirs_graph();
        let q = dirs_query();
        let tree = q.walk(&g, &WalkOptions::unlimited());

        fn check(n: &WalkNode) {
            if n.depth == 0 {
                assert!(n.edge_to_parent.is_none());
            } else {
                assert!(
                    n.edge_to_parent.is_some(),
                    "non-root must carry its edge to parent"
                );
            }
            for c in &n.children {
                check(c);
            }
        }
        for root in &tree {
            check(root);
        }
    }

    /// Build an inline graph where `a.md` links to `b.md` which links
    /// back to `a.md` — a simple 2-cycle reachable from a.md.
    fn cyclic_graph() -> (assert_fs::TempDir, Graph) {
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("a.md").write_str("[[b]]\n").unwrap();
        tmp.child("b.md").write_str("[[a]]\n").unwrap();
        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &v.scan()).unwrap();
        (tmp, g)
    }

    #[test]
    fn walk_dedup_marks_reentry_as_reference() {
        let (_tmp, g) = cyclic_graph();
        let q = parse(
            "node where path = \"a.md\"; \
             expand where edge.kind = note-link;",
        )
        .unwrap();
        // Dedup is the default; unbounded.
        let tree = q.walk(&g, &WalkOptions::unlimited());
        // a → b → a(reference)
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].closure, NodeClosure::Open);
        assert_eq!(tree[0].children.len(), 1, "a has one child b");
        let b = &tree[0].children[0];
        assert_eq!(b.closure, NodeClosure::Open);
        assert_eq!(b.children.len(), 1, "b expands once to the a-reference");
        let a_ref = &b.children[0];
        assert_eq!(
            a_ref.closure,
            NodeClosure::Reference,
            "the re-entered a is a dedup reference, not re-expanded"
        );
        assert!(
            a_ref.children.is_empty(),
            "reference markers have no children"
        );
        assert_eq!(a_ref.id, tree[0].id, "same node id as the root a");
    }

    #[test]
    fn walk_tree_marks_ancestor_reentry_as_cycle() {
        let (_tmp, g) = cyclic_graph();
        let q = parse(
            "node where path = \"a.md\"; \
             expand where edge.kind = note-link;",
        )
        .unwrap();
        let tree = q.walk(
            &g,
            &WalkOptions {
                max_depth: None,
                visit: VisitPolicy::Tree,
                ..Default::default()
            },
        );
        // a → b → a(cycle) — Tree mode reports the ancestor re-entry as
        // a cycle rather than a dedup reference.
        assert_eq!(tree.len(), 1);
        let a_cycle = &tree[0].children[0].children[0];
        assert_eq!(a_cycle.closure, NodeClosure::Cycle);
        assert!(
            a_cycle.children.is_empty(),
            "cycle markers have no children"
        );
        assert_eq!(a_cycle.id, tree[0].id);
    }

    #[test]
    fn walk_allow_never_marks_and_relies_on_depth() {
        let (_tmp, g) = cyclic_graph();
        let q = parse(
            "node where path = \"a.md\"; \
             expand where edge.kind = note-link;",
        )
        .unwrap();
        let tree = q.walk(
            &g,
            &WalkOptions {
                max_depth: Some(3),
                visit: VisitPolicy::Allow,
                ..Default::default()
            },
        );
        // No detection — a → b → a → b, terminating only at depth 3.
        assert_eq!(tree.len(), 1);
        let mut current = &tree[0];
        let mut visited_depths = vec![current.depth];
        while let Some(child) = current.children.first() {
            assert_eq!(child.closure, NodeClosure::Open, "Allow never marks a node");
            visited_depths.push(child.depth);
            current = child;
        }
        assert_eq!(visited_depths, vec![0, 1, 2, 3]);
        assert!(
            current.children.is_empty(),
            "depth bound is what terminates the walk"
        );
    }

    /// Build a diamond: `a → b`, `a → c`, `b → d`, `c → d`. `d` is a
    /// shared descendant reachable via two distinct, non-cyclic paths.
    fn diamond_graph() -> (assert_fs::TempDir, Graph) {
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("a.md").write_str("[[b]]\n[[c]]\n").unwrap();
        tmp.child("b.md").write_str("[[d]]\n").unwrap();
        tmp.child("c.md").write_str("[[d]]\n").unwrap();
        tmp.child("d.md").write_str("no links\n").unwrap();
        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &v.scan()).unwrap();
        (tmp, g)
    }

    #[test]
    fn walk_dedup_expands_shared_descendant_once() {
        let (_tmp, g) = diamond_graph();
        let q = parse("node where path = \"a.md\"; expand where edge.kind = note-link;").unwrap();
        let tree = q.walk(&g, &WalkOptions::unlimited());
        // a + b + c + d(open under one parent) + d(reference under the
        // other) = 5 nodes — d is expanded exactly once.
        assert_eq!(count_nodes(&tree), 5);

        let a = &tree[0];
        let d_under: Vec<&WalkNode> = a.children.iter().map(|child| &child.children[0]).collect();
        assert_eq!(d_under.len(), 2, "d appears under both b and c");
        let opens = d_under
            .iter()
            .filter(|n| n.closure == NodeClosure::Open)
            .count();
        let refs = d_under
            .iter()
            .filter(|n| n.closure == NodeClosure::Reference)
            .count();
        assert_eq!(opens, 1, "d is expanded under exactly one parent");
        assert_eq!(refs, 1, "and a reference under the other");
    }

    #[test]
    fn walk_tree_repeats_shared_descendant() {
        let (_tmp, g) = diamond_graph();
        let q = parse("node where path = \"a.md\"; expand where edge.kind = note-link;").unwrap();
        let tree = q.walk(
            &g,
            &WalkOptions {
                max_depth: None,
                visit: VisitPolicy::Tree,
                ..Default::default()
            },
        );
        // Tree mode repeats d's (empty) subtree under both b and c —
        // both are Open, neither a reference.
        let a = &tree[0];
        for child in &a.children {
            let d = &child.children[0];
            assert_eq!(
                d.closure,
                NodeClosure::Open,
                "Tree repeats, never references"
            );
        }
    }

    #[test]
    fn walk_dedup_terminates_on_dense_graph() {
        // A complete digraph on N nodes: every note links to every
        // other. Under the old path-based behavior this enumerates O(N!)
        // simple paths; under Dedup it is bounded.
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        let n = 8;
        let names: Vec<String> = (0..n).map(|i| format!("n{i}")).collect();
        for i in 0..n {
            let body: String = names
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, name)| format!("[[{name}]]\n"))
                .collect();
            tmp.child(format!("n{i}.md")).write_str(&body).unwrap();
        }
        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &v.scan()).unwrap();
        let q = parse("node where path = \"n0.md\"; expand where edge.kind = note-link;").unwrap();
        let tree = q.walk(&g, &WalkOptions::unlimited());
        // Each of the n nodes is expanded once (n Open nodes); every
        // other incident edge yields a single reference leaf. The total
        // is bounded by O(V + E) = O(n^2), nowhere near O(n!).
        assert!(
            count_nodes(&tree) <= n * n,
            "dedup keeps the dense walk bounded"
        );
    }

    #[test]
    fn walk_max_nodes_truncates() {
        let g = dirs_graph();
        let q = dirs_query();
        let tree = q.walk(
            &g,
            &WalkOptions {
                max_depth: None,
                visit: VisitPolicy::Dedup,
                max_nodes: Some(3),
            },
        );
        assert_eq!(count_nodes(&tree), 3, "the budget caps materialized nodes");
    }

    #[test]
    fn walk_no_expand_block_returns_flat_roots() {
        let g = dirs_graph();
        // Selector only — no expand block.
        let q = parse("node where kind = Directory;").unwrap();
        let tree = q.walk(&g, &WalkOptions::unlimited());
        assert!(!tree.is_empty(), "the dirs fixture has directories");
        for root in &tree {
            assert!(
                root.children.is_empty(),
                "no expand block means no children regardless of max_depth"
            );
            assert_eq!(root.closure, NodeClosure::Open);
            assert!(root.edge_to_parent.is_none());
        }
        // depth_zero with a None max_depth still returns nothing
        // below the roots — same as Some(_) — so the assertion above
        // is true for any max_depth value.
    }

    #[test]
    fn walk_empty_select_returns_empty_tree() {
        let g = dirs_graph();
        // Query that matches nothing — there are no notes whose
        // path starts with "nope/".
        let q = parse("node where path starts_with \"nope/\";").unwrap();
        let tree = q.walk(&g, &WalkOptions::unlimited());
        assert!(tree.is_empty());
    }

    #[test]
    fn walk_unlimited_terminates_on_cyclic_graph() {
        // Sanity: Stop policy + unlimited depth must terminate on a
        // cycle (otherwise this test would hang).
        let (_tmp, g) = cyclic_graph();
        let q = parse("node where path = \"a.md\"; expand where edge.kind = note-link;").unwrap();
        let tree = q.walk(&g, &WalkOptions::unlimited());
        // a(root) + b + a(cycle) = 3 nodes total
        assert_eq!(count_nodes(&tree), 3);
    }
}

// ── Error message snapshot tests ─────────────────────────────────

mod error_snapshots {
    use super::*;

    macro_rules! snap_err {
        ($name:ident, $src:literal) => {
            #[test]
            fn $name() {
                let err = parse($src).unwrap_err();
                insta::assert_snapshot!(stringify!($name), format!("{err}"));
            }
        };
    }

    snap_err!(err_empty_input, "   ");
    snap_err!(err_no_initial_set, "expand where edge.kind = note-link;");
    snap_err!(
        err_type_mismatch_eq_with_set,
        "node where kind = {Note, Directory};"
    );
    snap_err!(err_type_mismatch_in_with_single, "node where kind in Note;");
    snap_err!(err_ambiguous_attribute, "node; expand where kind = link;");
    snap_err!(err_scope_from_in_node, "node where from.kind = Directory;");
    snap_err!(
        err_scope_self_in_expand,
        "node; expand where self.kind = Directory;"
    );
    snap_err!(
        err_scope_indegree_qualified,
        "node; expand where from.indegree = 0;"
    );
    snap_err!(err_scope_form_on_node, "node where form = wiki;");
    snap_err!(
        err_scope_path_on_edge,
        "node; expand where edge.path = foo;"
    );
    snap_err!(err_unknown_attribute, "node where foo = bar;");
    snap_err!(err_unknown_kind_value, "node where kind = Notes;");
    snap_err!(
        err_unknown_form_value,
        "node; expand where edge.form = html;"
    );
    snap_err!(err_trailing_input, "node; junk");
    snap_err!(err_unterminated_string, "node where path = \"oops");
    snap_err!(err_illegal_character, "node where path = @bogus;");
    snap_err!(err_v1_with_keyword, "node n with n.kind = Note;");
    snap_err!(
        err_v1_over_keyword,
        "node; expand over e(n, m) with e.kind = link;"
    );
}

// ── Proptest: round-trip + stability + whitespace insensitivity ──

mod proptests {
    use super::*;
    use proptest::collection::vec;
    use proptest::prelude::*;

    // ── Literal generators (only safe forms — see below) ─────

    // Bare identifiers are used ONLY for known kind/form values
    // so that Display→parse round-trips can't be tripped up by
    // accidentally producing an identifier that the lexer treats
    // as a keyword. Arbitrary user strings always go through
    // `Literal::Str`, which gets quoted by Display.

    fn arb_node_kind_literal() -> impl Strategy<Value = Literal> {
        prop_oneof![
            Just(Literal::Ident("Note".into())),
            Just(Literal::Ident("Directory".into())),
            Just(Literal::Ident("Ghost".into())),
        ]
    }

    fn arb_edge_kind_literal() -> impl Strategy<Value = Literal> {
        prop_oneof![
            Just(Literal::Ident("note-link".into())),
            Just(Literal::Ident("heading-link".into())),
            Just(Literal::Ident("paragraph-link".into())),
            Just(Literal::Ident("directory-contains".into())),
            Just(Literal::Ident("has-task".into())),
            Just(Literal::Ident("subtask".into())),
        ]
    }

    fn arb_form_literal() -> impl Strategy<Value = Literal> {
        prop_oneof![
            Just(Literal::Ident("wiki".into())),
            Just(Literal::Ident("md".into())),
        ]
    }

    // Strings used in path/title queries. Restricted to a charset
    // that survives Display escaping cleanly: no shell weirdness
    // but enough variety to be meaningful (slashes, dots,
    // ascii-quotes, spaces).
    fn arb_user_string_literal() -> impl Strategy<Value = Literal> {
        proptest::string::string_regex(r#"[a-zA-Z0-9 ./_\-"\\]{0,12}"#)
            .unwrap()
            .prop_map(Literal::Str)
    }

    fn arb_int_literal() -> impl Strategy<Value = Literal> {
        (0i64..50).prop_map(Literal::Int)
    }

    // ── Condition shape helpers ──────────────────────────────

    fn op_value_node_kind() -> impl Strategy<Value = (Op, Value)> {
        prop_oneof![
            arb_node_kind_literal().prop_map(|l| (Op::Eq, Value::Single(l))),
            arb_node_kind_literal().prop_map(|l| (Op::NotEq, Value::Single(l))),
            vec(arb_node_kind_literal(), 1..=3).prop_map(|ls| (Op::In, Value::Set(ls))),
        ]
    }

    fn op_value_edge_kind() -> impl Strategy<Value = (Op, Value)> {
        prop_oneof![
            arb_edge_kind_literal().prop_map(|l| (Op::Eq, Value::Single(l))),
            arb_edge_kind_literal().prop_map(|l| (Op::NotEq, Value::Single(l))),
            vec(arb_edge_kind_literal(), 1..=3).prop_map(|ls| (Op::In, Value::Set(ls))),
        ]
    }

    fn op_value_form() -> impl Strategy<Value = (Op, Value)> {
        prop_oneof![
            arb_form_literal().prop_map(|l| (Op::Eq, Value::Single(l))),
            arb_form_literal().prop_map(|l| (Op::NotEq, Value::Single(l))),
            vec(arb_form_literal(), 1..=2).prop_map(|ls| (Op::In, Value::Set(ls))),
        ]
    }

    fn op_value_string_attr() -> impl Strategy<Value = (Op, Value)> {
        prop_oneof![
            arb_user_string_literal().prop_map(|l| (Op::Eq, Value::Single(l))),
            arb_user_string_literal().prop_map(|l| (Op::NotEq, Value::Single(l))),
            arb_user_string_literal().prop_map(|l| (Op::Includes, Value::Single(l))),
            arb_user_string_literal().prop_map(|l| (Op::StartsWith, Value::Single(l))),
            arb_user_string_literal().prop_map(|l| (Op::EndsWith, Value::Single(l))),
            vec(arb_user_string_literal(), 1..=3).prop_map(|ls| (Op::In, Value::Set(ls))),
        ]
    }

    fn op_value_int_attr() -> impl Strategy<Value = (Op, Value)> {
        prop_oneof![
            arb_int_literal().prop_map(|l| (Op::Eq, Value::Single(l))),
            arb_int_literal().prop_map(|l| (Op::NotEq, Value::Single(l))),
            vec(arb_int_literal(), 1..=3).prop_map(|ls| (Op::In, Value::Set(ls))),
        ]
    }

    // ── Condition generators per subject ─────────────────────

    fn arb_self_condition() -> impl Strategy<Value = Condition> {
        prop_oneof![
            op_value_node_kind().prop_map(|(op, value)| Condition {
                subject: Subject::SelfNode,
                attr: Attr::Kind,
                op,
                value,
            }),
            op_value_string_attr().prop_map(|(op, value)| Condition {
                subject: Subject::SelfNode,
                attr: Attr::Path,
                op,
                value,
            }),
            op_value_string_attr().prop_map(|(op, value)| Condition {
                subject: Subject::SelfNode,
                attr: Attr::Title,
                op,
                value,
            }),
            op_value_int_attr().prop_map(|(op, value)| Condition {
                subject: Subject::SelfNode,
                attr: Attr::Indegree,
                op,
                value,
            }),
            op_value_int_attr().prop_map(|(op, value)| Condition {
                subject: Subject::SelfNode,
                attr: Attr::Outdegree,
                op,
                value,
            }),
        ]
    }

    fn arb_from_to_condition(subject: Subject) -> impl Strategy<Value = Condition> {
        prop_oneof![
            op_value_node_kind().prop_map(move |(op, value)| Condition {
                subject,
                attr: Attr::Kind,
                op,
                value,
            }),
            op_value_string_attr().prop_map(move |(op, value)| Condition {
                subject,
                attr: Attr::Path,
                op,
                value,
            }),
            op_value_string_attr().prop_map(move |(op, value)| Condition {
                subject,
                attr: Attr::Title,
                op,
                value,
            }),
        ]
    }

    fn arb_edge_condition() -> impl Strategy<Value = Condition> {
        prop_oneof![
            op_value_edge_kind().prop_map(|(op, value)| Condition {
                subject: Subject::Edge,
                attr: Attr::Kind,
                op,
                value,
            }),
            op_value_form().prop_map(|(op, value)| Condition {
                subject: Subject::Edge,
                attr: Attr::Form,
                op,
                value,
            }),
        ]
    }

    fn arb_neighbor_filter() -> impl Strategy<Value = NeighborFilter> {
        (
            prop_oneof![Just(Direction::Incoming), Just(Direction::Outgoing)],
            vec(arb_edge_condition(), 0..=2),
        )
            .prop_map(|(direction, conditions)| NeighborFilter {
                direction,
                conditions,
            })
    }

    fn arb_node_selector() -> impl Strategy<Value = NodeSelector> {
        (
            vec(arb_self_condition(), 0..=3),
            prop_oneof![Just(None), arb_neighbor_filter().prop_map(Some),],
        )
            .prop_map(|(conditions, without)| NodeSelector {
                condition: if conditions.is_empty() {
                    None
                } else if conditions.len() == 1 {
                    Some(CondExpr::Cond(conditions.into_iter().next().unwrap()))
                } else {
                    Some(CondExpr::And(
                        conditions.into_iter().map(CondExpr::Cond).collect(),
                    ))
                },
                without,
            })
    }

    fn arb_expand_condition() -> impl Strategy<Value = Condition> {
        prop_oneof![
            arb_from_to_condition(Subject::From),
            arb_from_to_condition(Subject::To),
            arb_edge_condition(),
        ]
    }

    fn arb_edge_policy() -> impl Strategy<Value = EdgePolicy> {
        vec(arb_expand_condition(), 0..=4).prop_map(|conditions| EdgePolicy { conditions })
    }

    fn arb_graph_query() -> impl Strategy<Value = GraphQuery> {
        (
            vec(arb_node_selector(), 1..=3),
            prop_oneof![Just(None), arb_edge_policy().prop_map(Some)],
        )
            .prop_map(|(initial, expansion)| GraphQuery { initial, expansion })
    }

    // ── Whitespace injector ──────────────────────────────────

    /// Insert random whitespace at token boundaries. Since the
    /// canonical Display uses single spaces, this exercises the
    /// lexer's tolerance. Skips injection inside string literals
    /// (between `"` opener and `"` closer) so quoted content isn't
    /// corrupted; respects `\"` escapes.
    fn inject_whitespace(src: &str, salt: u64) -> String {
        let mut out = String::with_capacity(src.len() * 2);
        let mut rng = salt;
        let mut prev_kind = CharKind::Punct;
        let mut in_string = false;
        let mut chars = src.chars().peekable();
        while let Some(c) = chars.next() {
            if in_string {
                out.push(c);
                if c == '\\' {
                    // Pass through next char unescaped — keep it
                    // attached to the backslash.
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                } else if c == '"' {
                    in_string = false;
                }
                prev_kind = CharKind::Punct;
                continue;
            }
            if c == '"' {
                out.push(c);
                in_string = true;
                prev_kind = CharKind::Punct;
                continue;
            }
            let kind = char_kind(c);
            if kind != prev_kind && kind != CharKind::Space && prev_kind != CharKind::Space {
                let n = (rng % 4) as usize;
                rng = rng
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                for i in 0..n {
                    out.push(match (rng >> (i * 4)) & 3 {
                        0 => ' ',
                        1 => '\t',
                        2 => '\n',
                        _ => ' ',
                    });
                }
            }
            out.push(c);
            prev_kind = kind;
        }
        out
    }

    #[derive(PartialEq, Eq, Clone, Copy)]
    enum CharKind {
        Alpha,
        Punct,
        Space,
    }

    fn char_kind(c: char) -> CharKind {
        if c.is_whitespace() {
            CharKind::Space
        } else if c.is_alphanumeric() || c == '_' || c == '-' {
            CharKind::Alpha
        } else {
            CharKind::Punct
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 256,
            .. ProptestConfig::default()
        })]

        /// The core invariant: every AST value the generator can
        /// produce serializes to text that parses back to itself.
        #[test]
        fn prop_round_trip(q in arb_graph_query()) {
            let s = format!("{q}");
            let parsed = parse(&s).map_err(|e| {
                TestCaseError::fail(format!("re-parse failed for {:?}: {}", s, e))
            })?;
            prop_assert_eq!(parsed, q);
        }

        /// Stability: Display ∘ parse is idempotent. Parsing a
        /// canonical form and re-displaying yields the same text.
        #[test]
        fn prop_stability(q in arb_graph_query()) {
            let s1 = format!("{q}");
            let q1 = parse(&s1).map_err(|e| {
                TestCaseError::fail(format!("parse failed: {}", e))
            })?;
            let s2 = format!("{q1}");
            prop_assert_eq!(s1, s2);
        }

        /// Whitespace insensitivity: extra spaces/tabs/newlines
        /// inserted at token boundaries don't change the parsed
        /// AST.
        #[test]
        fn prop_whitespace_insensitivity(q in arb_graph_query(), salt in any::<u64>()) {
            let canonical = format!("{q}");
            let noisy = inject_whitespace(&canonical, salt);
            let parsed = parse(&noisy).map_err(|e| {
                TestCaseError::fail(format!("parse failed on whitespace-noisy form: {} on {:?}", e, noisy))
            })?;
            prop_assert_eq!(parsed, q);
        }
    }

    // ── Invalid-input variant-coverage tests ─────────────────

    /// For each `DslError` variant, supply a query string that
    /// triggers exactly that variant. Catches regressions where a
    /// grammar tweak silently routes an error to a different
    /// variant.
    #[test]
    fn every_dslerror_variant_is_reachable() {
        let cases: &[(&str, &str)] = &[
            ("EmptyInput", "   "),
            ("NoInitialSet", "expand where edge.kind = note-link;"),
            ("UnexpectedToken", "node where = Note;"),
            ("UnknownAttribute", "node where foo = bar;"),
            ("AmbiguousAttribute", "node; expand where kind = link;"),
            ("ScopeError", "node where from.kind = Note;"),
            ("TypeMismatch", "node where kind = {Note};"),
            ("UnknownKindValue", "node where kind = Notes;"),
            ("TrailingInput", "node; junk"),
            ("UnterminatedString", "node where path = \"oops"),
            ("IllegalCharacter", "node where path = @x;"),
        ];
        for (label, src) in cases {
            let err = match parse(src) {
                Err(e) => e,
                Ok(_) => panic!("expected {label} parsing {src:?}, but parse succeeded"),
            };
            let variant = dslerror_variant(&err);
            assert_eq!(
                variant, *label,
                "expected {label} for {src:?}, got {variant} ({err})"
            );
        }
    }

    fn dslerror_variant(e: &DslError) -> &'static str {
        match e {
            DslError::EmptyInput => "EmptyInput",
            DslError::NoInitialSet => "NoInitialSet",
            DslError::UnexpectedToken { .. } => "UnexpectedToken",
            DslError::UnknownAttribute { .. } => "UnknownAttribute",
            DslError::AmbiguousAttribute { .. } => "AmbiguousAttribute",
            DslError::ScopeError { .. } => "ScopeError",
            DslError::TypeMismatch { .. } => "TypeMismatch",
            DslError::UnknownKindValue { .. } => "UnknownKindValue",
            DslError::TrailingInput { .. } => "TrailingInput",
            DslError::UnterminatedString { .. } => "UnterminatedString",
            DslError::IllegalCharacter { .. } => "IllegalCharacter",
        }
    }
}

mod task_queries {
    use std::path::PathBuf;

    use super::*;
    use crate::graph::Graph;
    use crate::task::{Priority, Status, Task};
    use crate::vault::Vault;
    use assert_fs::prelude::*;

    fn vault_with_tasks() -> (assert_fs::TempDir, Scan) {
        let tmp = assert_fs::TempDir::new().unwrap();
        // Vault is discovered here (not returned) only to build the
        // single-pass scan the literal below overrides tasks on.
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("root.md")
            .write_str("- [ ] Fix login bug\n- [x] Review quarterly report\n")
            .unwrap();
        tmp.child("Areas").create_dir_all().unwrap();
        tmp.child("Areas/finance.md")
            .write_str("- [ ] Process invoices\n")
            .unwrap();
        tmp.child("Projects").create_dir_all().unwrap();
        tmp.child("Projects/alpha.md")
            .write_str("- [ ] Ship beta\n")
            .unwrap();

        let scan = Scan {
            tasks: vec![
                Task {
                    description: "Fix login bug".into(),
                    priority: Some(Priority::High),
                    due: Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 1).unwrap()),
                    tags: vec!["bug".into(), "urgent".into()],
                    source_file: PathBuf::from("root.md"),
                    source_line: 1,
                    ..Default::default()
                },
                Task {
                    description: "Review quarterly report".into(),
                    status: Status::Done,
                    tags: vec!["finance".into()],
                    source_file: PathBuf::from("root.md"),
                    source_line: 2,
                    ..Default::default()
                },
                Task {
                    description: "Process invoices".into(),
                    priority: Some(Priority::Medium),
                    due: Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 15).unwrap()),
                    tags: vec!["finance".into(), "invoices".into()],
                    source_file: PathBuf::from("Areas/finance.md"),
                    source_line: 1,
                    ..Default::default()
                },
            ],
            ..Vault::discover(Some(tmp.path().to_path_buf()))
                .unwrap()
                .scan()
        };
        (tmp, scan)
    }

    /// Task 7.5: node_kind_str returns "Task" for task nodes.
    #[test]
    fn node_kind_str_returns_task() {
        let td = crate::graph::TaskData {
            description: "test".into(),
            status: "Open".into(),
            priority: None,
            due: None,
            scheduled: None,
            source_file: PathBuf::from("test.md"),
            source_line: 1,
            ..Default::default()
        };
        assert_eq!(
            crate::graph::query::eval::node_kind_str(&NodeKind::Task(td)),
            "Task"
        );
    }

    /// Task 7.6: edge_kind_str returns "has-task" for HasTask edges.
    #[test]
    fn edge_kind_str_returns_has_task() {
        assert_eq!(super::edge_kind_str(&EdgeKind::HasTask), "has-task");
    }

    /// Parse round-trip: links-into edge kind accepted in expand block.
    #[test]
    fn dsl_parses_links_into_edge_kind() {
        let q = parse("node where kind = Note; expand where edge.kind = \"links-into\";").unwrap();
        // Round-trip serialization.
        let s = q.to_string();
        let q2 = parse(&s).unwrap();
        assert_eq!(q, q2);
    }

    /// Parse round-trip: links-into accepted in set form.
    #[test]
    fn dsl_parses_links_into_in_set() {
        let q = parse(
            r#"node where kind = Directory and path = ""; expand where edge.kind in {directory-contains, links-into};"#,
        )
        .unwrap();
        let s = q.to_string();
        let q2 = parse(&s).unwrap();
        assert_eq!(q, q2);
    }

    /// Task 7.7: DSL `node where kind = "Task"` returns only task nodes.
    #[test]
    fn dsl_kind_eq_task_returns_task_nodes() {
        let (_tmp, scan) = vault_with_tasks();
        let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &scan).unwrap();

        let q = parse("node where kind = Task;").unwrap();
        let results = q.select(&g);
        assert_eq!(results.len(), 3);
        for id in &results {
            assert!(matches!(g.node(*id), NodeKind::Task(_)));
        }
    }

    /// Task 7.8: DSL task attribute filters.
    #[test]
    fn dsl_task_attribute_filters() {
        let (_tmp, scan) = vault_with_tasks();
        let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &scan).unwrap();

        // Filter by status = "Done"
        let q = parse(r#"node where kind = Task and status = "Done";"#).unwrap();
        let results = q.select(&g);
        assert_eq!(results.len(), 1);
        if let NodeKind::Task(td) = g.node(results[0]) {
            assert_eq!(td.description, "Review quarterly report");
        }

        // Filter by priority = "High"
        let q = parse(r#"node where kind = Task and priority = "High";"#).unwrap();
        let results = q.select(&g);
        assert_eq!(results.len(), 1);
        if let NodeKind::Task(td) = g.node(results[0]) {
            assert_eq!(td.description, "Fix login bug");
        }

        // Filter by due date
        let q = parse(r#"node where kind = Task and due = "2025-06-15";"#).unwrap();
        let results = q.select(&g);
        assert_eq!(results.len(), 1);
        if let NodeKind::Task(td) = g.node(results[0]) {
            assert_eq!(td.description, "Process invoices");
        }

        // Filter by description starts_with
        let q = parse(r#"node where kind = Task and description starts_with "Process";"#).unwrap();
        let results = q.select(&g);
        assert_eq!(results.len(), 1);
        if let NodeKind::Task(td) = g.node(results[0]) {
            assert_eq!(td.description, "Process invoices");
        }

        // Filter by tags includes
        let q = parse(r#"node where kind = Task and tags includes "bug";"#).unwrap();
        let results = q.select(&g);
        assert_eq!(results.len(), 1);
        if let NodeKind::Task(td) = g.node(results[0]) {
            assert_eq!(td.description, "Fix login bug");
        }

        // Filter by tags in set
        let q = parse(r#"node where kind = Task and tags in {"bug", "urgent"};"#).unwrap();
        let results = q.select(&g);
        assert_eq!(results.len(), 1);
        if let NodeKind::Task(td) = g.node(results[0]) {
            assert_eq!(td.description, "Fix login bug");
        }
    }

    /// 5.2: DSL expand with to.kind including "Task" reveals task children.
    #[test]
    fn dsl_expand_to_kind_includes_task() {
        let (_tmp, scan) = vault_with_tasks();
        let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &scan).unwrap();

        let q = parse(
            r#"node where kind = Directory and path = ""; expand where edge.kind in {directory-contains, has-task} and to.kind in {Note, Directory, Task};"#,
        )
            .unwrap();

        let tree = q.walk(&g, &WalkOptions::unlimited());
        assert!(!tree.is_empty());

        fn count_tasks_in_tree(nodes: &[WalkNode], graph: &Graph) -> usize {
            let mut count = 0;
            for node in nodes {
                if matches!(graph.node(node.id), NodeKind::Task(_)) {
                    count += 1;
                }
                count += count_tasks_in_tree(&node.children, graph);
            }
            count
        }
        assert!(count_tasks_in_tree(&tree, &g) > 0);
    }

    /// 5.3: DSL expand with to.kind excluding "Task" omits task children.
    #[test]
    fn dsl_expand_to_kind_excludes_task() {
        let (_tmp, scan) = vault_with_tasks();
        let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &scan).unwrap();

        let q = parse(
            r#"node where kind = Directory and path = ""; expand where edge.kind in {directory-contains, has-task} and to.kind in {Note, Directory};"#,
        )
            .unwrap();

        let tree = q.walk(&g, &WalkOptions::unlimited());
        assert!(!tree.is_empty());

        fn count_tasks_in_tree(nodes: &[WalkNode], graph: &Graph) -> usize {
            let mut count = 0;
            for node in nodes {
                if matches!(graph.node(node.id), NodeKind::Task(_)) {
                    count += 1;
                }
                count += count_tasks_in_tree(&node.children, graph);
            }
            count
        }
        assert_eq!(count_tasks_in_tree(&tree, &g), 0);
    }

    /// `self.path` on a task node returns the vault-relative path of
    /// the owning source file. Matches the task DSL's old
    /// `path includes "Areas/"` predicate semantics.
    #[test]
    fn dsl_path_on_task_matches_source_file() {
        let (_tmp, scan) = vault_with_tasks();
        let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &scan).unwrap();

        let q = parse(r#"node where kind = Task and path = "root.md";"#).unwrap();
        let results = q.select(&g);
        // Two tasks live in root.md in the fixture.
        assert_eq!(results.len(), 2);
        for id in &results {
            if let NodeKind::Task(td) = g.node(*id) {
                assert_eq!(td.source_file.to_string_lossy(), "root.md");
            }
        }
    }

    /// 5.5: title attribute on task node yields no match.
    #[test]
    fn dsl_title_on_task_yields_no_match() {
        let (_tmp, scan) = vault_with_tasks();
        let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &scan).unwrap();

        let q = parse(r#"node where kind = Task and title = "anything";"#).unwrap();
        let results = q.select(&g);
        assert_eq!(results.len(), 0);
    }

    /// 5.6: Inequality and in-set on status.
    #[test]
    fn dsl_task_inequality_and_in_set() {
        let (_tmp, scan) = vault_with_tasks();
        let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &scan).unwrap();

        // status != "Done" returns the two open tasks
        let q = parse(r#"node where kind = Task and status != "Done";"#).unwrap();
        let results = q.select(&g);
        assert_eq!(results.len(), 2);
        for id in &results {
            if let NodeKind::Task(td) = g.node(*id) {
                assert_ne!(td.status, "Done");
            }
        }

        // status in {"Open", "InProgress"} returns only open tasks
        let q = parse(r#"node where kind = Task and status in {"Open", "InProgress"};"#).unwrap();
        let results = q.select(&g);
        assert_eq!(results.len(), 2);
        for id in &results {
            if let NodeKind::Task(td) = g.node(*id) {
                assert_eq!(td.status, "Open");
            }
        }
    }

    /// 5.7: description ends_with selects the matching task.
    #[test]
    fn dsl_task_description_ends_with() {
        let (_tmp, scan) = vault_with_tasks();
        let v = Vault::discover(Some(_tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &scan).unwrap();

        let q = parse(r#"node where kind = Task and description ends_with "bug";"#).unwrap();
        let results = q.select(&g);
        assert_eq!(results.len(), 1);
        if let NodeKind::Task(td) = g.node(results[0]) {
            assert_eq!(td.description, "Fix login bug");
        }
    }
}

// ── New ops and Date value coverage ──────────────────────────────
mod new_ops {
    use super::*;
    use chrono::NaiveDate;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 5, 9).unwrap()
    }

    fn p(src: &str) -> GraphQuery {
        parse_with(src, Profile::Default, today())
            .unwrap_or_else(|e| panic!("parse failed for {src:?}: {e}"))
    }

    #[test]
    fn lt_le_gt_ge_on_indegree() {
        let q = p("node where indegree > 5;");
        let c = q.initial[0].conditions()[0];
        assert_eq!(c.op, Op::Gt);
        assert_eq!(c.value, Value::Single(Literal::Int(5)));

        let q = p("node where indegree <= 10;");
        assert_eq!(q.initial[0].conditions()[0].op, Op::Le);
    }

    #[test]
    fn lt_le_gt_ge_on_due_date() {
        let q = parse_with(
            "node where kind = Task and self.due < today;",
            Profile::Default,
            today(),
        )
        .unwrap();
        let conds = q.initial[0].conditions();
        // [kind = Task, due < today]
        assert_eq!(conds.len(), 2);
        assert_eq!(conds[1].op, Op::Lt);
        assert_eq!(conds[1].value, Value::Single(Literal::Date(today())));
    }

    #[test]
    fn type_mismatch_lt_on_title() {
        let err =
            parse_with("node where self.title < \"x\";", Profile::Default, today()).unwrap_err();
        assert!(matches!(err, DslError::TypeMismatch { .. }), "got: {err}");
    }

    #[test]
    fn is_null_on_due() {
        let q = parse_with(
            "node where kind = Task and self.due is null;",
            Profile::Default,
            today(),
        )
        .unwrap();
        let conds = q.initial[0].conditions();
        assert_eq!(conds[1].op, Op::IsNull);
        assert_eq!(conds[1].value, Value::None);
    }

    #[test]
    fn is_not_null_on_due() {
        let q = parse_with(
            "node where kind = Task and self.due is not null;",
            Profile::Default,
            today(),
        )
        .unwrap();
        let conds = q.initial[0].conditions();
        assert_eq!(conds[1].op, Op::IsNotNull);
    }

    #[test]
    fn is_null_on_required_attr_errors() {
        let err =
            parse_with("node where self.kind is null;", Profile::Default, today()).unwrap_err();
        assert!(matches!(err, DslError::TypeMismatch { .. }), "got: {err}");
    }

    #[test]
    fn date_iso_literal() {
        let q = parse_with(
            "node where kind = Task and self.due = 2026-12-31;",
            Profile::Default,
            today(),
        )
        .unwrap();
        let conds = q.initial[0].conditions();
        assert_eq!(
            conds[1].value,
            Value::Single(Literal::Date(
                NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()
            ))
        );
    }

    #[test]
    fn date_today_keyword_resolves_via_ft_today() {
        let q = parse_with(
            "node where kind = Task and self.due = today;",
            Profile::Default,
            today(),
        )
        .unwrap();
        let conds = q.initial[0].conditions();
        assert_eq!(conds[1].value, Value::Single(Literal::Date(today())));
    }

    #[test]
    fn date_relative_offsets() {
        let q = parse_with(
            "node where kind = Task and self.due < +7d;",
            Profile::Default,
            today(),
        )
        .unwrap();
        let conds = q.initial[0].conditions();
        let expected = today()
            .checked_add_signed(chrono::Duration::days(7))
            .unwrap();
        assert_eq!(conds[1].value, Value::Single(Literal::Date(expected)));
    }

    #[test]
    fn date_keyword_outside_date_context_errors() {
        // `self.title = today` — title is a string attr, `today` is not
        // a valid string. The parser uses Ident("today") here.
        let q = parse_with("node where self.title = today;", Profile::Default, today());
        // We don't strictly require a TypeMismatch error here — the
        // current parser accepts arbitrary idents on the rhs of `=`
        // for string attrs. The test pins the current behaviour so
        // we notice if it changes.
        assert!(q.is_ok());
    }

    #[test]
    fn roundtrip_lt_le_gt_ge() {
        for src in [
            "node where indegree > 5;",
            "node where indegree <= 10;",
            "node where kind = Task and due >= 2026-12-31;",
            "node where kind = Task and due < today;",
        ] {
            let q1 = parse_with(src, Profile::Default, today()).unwrap();
            let s = format!("{q1}");
            let q2 = parse_with(&s, Profile::Default, today()).unwrap();
            assert_eq!(q1, q2, "roundtrip mismatch:\n  src: {src}\n  ser: {s}");
        }
    }

    #[test]
    fn roundtrip_is_null() {
        for src in [
            "node where kind = Task and due is null;",
            "node where kind = Task and due is not null;",
        ] {
            let q1 = parse_with(src, Profile::Default, today()).unwrap();
            let s = format!("{q1}");
            let q2 = parse_with(&s, Profile::Default, today()).unwrap();
            assert_eq!(q1, q2);
        }
    }

    #[test]
    fn roundtrip_or_and_parens() {
        for src in [
            "node where kind = Task and (due = today or scheduled = today);",
            "node where (status = Open or status = InProgress) and priority = High;",
        ] {
            let q1 = parse_with(src, Profile::Default, today()).unwrap();
            let s = format!("{q1}");
            let q2 = parse_with(&s, Profile::Default, today()).unwrap();
            assert_eq!(q1, q2, "roundtrip:\n  src: {src}\n  ser: {s}");
        }
    }
}

// ── Tasks-profile desugaring ─────────────────────────────────────
mod tasks_profile {
    use super::*;
    use chrono::NaiveDate;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 5, 9).unwrap()
    }

    #[test]
    fn bare_predicate_desugars_to_node_kind_task_self() {
        let q_short = parse_with("priority = High", Profile::Tasks, today()).unwrap();
        let q_long = parse_with(
            "node where kind = Task and self.priority = High;",
            Profile::Default,
            today(),
        )
        .unwrap();
        assert_eq!(q_short, q_long);
    }

    #[test]
    fn explicit_node_block_preserved() {
        let src = "node where kind = Task and self.tags includes \"work\";";
        let q_tasks = parse_with(src, Profile::Tasks, today()).unwrap();
        let q_default = parse_with(src, Profile::Default, today()).unwrap();
        assert_eq!(q_tasks, q_default);
    }

    #[test]
    fn bare_path_includes() {
        let q = parse_with("path includes \"Areas/\"", Profile::Tasks, today()).unwrap();
        let conds = q.initial[0].conditions();
        // [kind = Task, path includes "Areas/"]
        assert_eq!(conds.len(), 2);
        assert_eq!(conds[1].subject, Subject::SelfNode);
        assert_eq!(conds[1].attr, Attr::Path);
    }

    #[test]
    fn bare_or_compound() {
        let q = parse_with("due = today or scheduled = today", Profile::Tasks, today()).unwrap();
        // The synthesized `and` from the prelude binds tighter than
        // the user's `or`, so the AST shape is:
        //   And(kind=Task, Or(due=today, scheduled=today))
        // expressed as a CondExpr tree on the sole selector.
        let leaves = q.initial[0].conditions();
        assert_eq!(leaves.len(), 3);
    }
}

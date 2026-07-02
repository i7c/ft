pub(crate) use super::modals::{collect_search_candidates, Candidate};
pub(crate) use super::view::TreeState;
pub(crate) use super::*;

#[cfg(test)]
mod tree_tests {
    use std::path::PathBuf;

    use ft_core::graph::query::parse as parse_query;
    use ft_core::graph::Graph;
    use ft_core::vault::{Scan, Vault};

    use super::*;

    /// Pinned "today" for graph-tab tests, so task relative-date labels in
    /// snapshots don't drift with the wall clock. Matches `fixed_clock`.
    const FT_TEST_TODAY: chrono::NaiveDate = match chrono::NaiveDate::from_ymd_opt(2026, 5, 12) {
        Some(d) => d,
        None => panic!("invalid test date"),
    };

    fn dirs_graph() -> Graph {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dirs");
        let v = Vault::discover(Some(path)).expect("dirs fixture vault must exist");
        Graph::build(&v, &v.scan()).unwrap()
    }

    fn dirs_query() -> GraphQuery {
        parse_query(
            "node where kind = Directory without incoming(kind = directory-contains); expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};",
        )
        .unwrap()
    }

    #[test]
    fn build_from_roots_creates_flat_rows() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);
        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0].depth, 0);
        assert_eq!(state.rows[0].kind_char, 'D');
    }

    #[test]
    fn expand_inserts_children_at_correct_position() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);
        assert_eq!(state.rows.len(), 1);

        let changed = state.expand_at(0, &g, &q, FT_TEST_TODAY);
        assert!(changed);
        assert_eq!(state.rows.len(), 4);
        assert!(state.rows[0].expanded);
        assert_eq!(state.rows[0].depth, 0);
        assert_eq!(state.rows[1].depth, 1);
        assert_eq!(state.rows[2].depth, 1);
        assert_eq!(state.rows[3].depth, 1);
    }

    #[test]
    fn collapse_removes_descendants() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);
        state.expand_at(0, &g, &q, FT_TEST_TODAY);
        assert_eq!(state.rows.len(), 4);

        state.collapse_at(0);
        assert_eq!(state.rows.len(), 1);
        assert!(!state.rows[0].expanded);
    }

    #[test]
    fn expand_toggle_collapses_when_already_expanded() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);

        state.expand_at(0, &g, &q, FT_TEST_TODAY);
        assert_eq!(state.rows.len(), 4);
        assert!(state.rows[0].expanded);

        let changed = state.expand_at(0, &g, &q, FT_TEST_TODAY);
        assert!(changed);
        assert_eq!(state.rows.len(), 1);
        assert!(!state.rows[0].expanded);
    }

    #[test]
    fn expand_then_expand_child() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);

        state.expand_at(0, &g, &q, FT_TEST_TODAY);
        assert_eq!(state.rows.len(), 4);

        let areas_idx = state
            .rows
            .iter()
            .position(|r| r.kind_char == 'D' && r.display == "Areas/")
            .unwrap();

        state.expand_at(areas_idx, &g, &q, FT_TEST_TODAY);
        assert_eq!(state.rows.len(), 6);

        let ops = state
            .rows
            .iter()
            .find(|r| r.display == "operations/")
            .unwrap();
        assert_eq!(ops.depth, 2);
    }

    #[test]
    fn expand_unexpandable_node_returns_false() {
        let g = dirs_graph();
        let q = parse_query("node where kind = Note;").unwrap();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);

        let changed = state.expand_at(0, &g, &q, FT_TEST_TODAY);
        assert!(!changed);
        assert!(!state.rows[0].expandable);
    }

    #[test]
    fn move_selection_wraps_at_bounds() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots: Vec<_> = g
            .nodes()
            .filter(|(_, k)| matches!(k, NodeKind::Note(_)))
            .map(|(id, _)| id)
            .take(3)
            .collect();

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);
        assert_eq!(state.rows.len(), 3);

        assert_eq!(state.move_selection_up(0), 2);
        assert_eq!(state.move_selection_down(2), 0);
        assert_eq!(state.move_selection_down(0), 1);
        assert_eq!(state.move_selection_up(1), 0);
    }

    #[test]
    fn empty_tree_selection_is_zero() {
        let state = TreeState::default();
        assert_eq!(state.move_selection_up(0), 0);
        assert_eq!(state.move_selection_down(0), 0);
    }

    #[test]
    fn cache_is_used_on_repeat_expand() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);

        state.expand_at(0, &g, &q, FT_TEST_TODAY);
        let first_len = state.rows.len();
        state.collapse_at(0);
        state.expand_at(0, &g, &q, FT_TEST_TODAY);
        assert_eq!(state.rows.len(), first_len);
        assert!(state.expansion_cache.contains_key(&state.rows[0].note_id));
    }

    #[test]
    fn build_marks_expandable_false_when_policy_returns_no_children() {
        // Empty vault → root has no Note children under the
        // policy. Expandability is now determined up front by
        // `make_row` asking the query; the row never shows the
        // ▶ arrow at all.
        let tmp = assert_fs::TempDir::new().unwrap();
        use assert_fs::prelude::*;
        tmp.child(".obsidian").create_dir_all().unwrap();

        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &v.scan()).unwrap();

        let q = parse_query(
            "node where indegree = 0; expand where from.kind = Directory and edge.kind = directory-contains and to.kind = Note;",
        ).unwrap();

        let root_id = g.node_by_path(std::path::Path::new("")).unwrap();

        let mut state = TreeState::default();
        state.build_from(&[root_id], &g, &q, FT_TEST_TODAY);

        // Pre-computed: not expandable, so attempting expand is a
        // no-op and `expanded` stays false (nothing was opened).
        assert!(!state.rows[0].expandable);
        let changed = state.expand_at(0, &g, &q, FT_TEST_TODAY);
        assert!(!changed);
        assert!(!state.rows[0].expanded);
        assert_eq!(state.rows.len(), 1);
    }

    #[test]
    fn build_marks_expandable_true_when_policy_returns_children() {
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);
        assert_eq!(state.rows.len(), 1);
        // Root directory has 3 immediate children under the policy →
        // expandable from the start.
        assert!(state.rows[0].expandable);
    }

    #[test]
    fn build_marks_note_rows_unexpandable_under_directory_contains_policy() {
        // Notes have no outgoing directory-contains edges, so the
        // policy yields zero children — rows for notes should not
        // display the ▶ arrow.
        let g = dirs_graph();
        let q = dirs_query();
        let roots = q.select(&g);

        let mut state = TreeState::default();
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);
        state.expand_at(0, &g, &q, FT_TEST_TODAY);

        for row in state.rows().iter().filter(|r| r.kind_char == 'N') {
            assert!(
                !row.expandable,
                "note row {} should be a leaf under the dirs policy",
                row.display
            );
        }
    }

    #[test]
    fn task_nodes_render_with_kind_char_t() {
        use assert_fs::prelude::*;
        use ft_core::task::{Status, Task};

        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("root.md")
            .write_str("- [ ] Task one\n- [x] Task two\n")
            .unwrap();
        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();

        let scan = Scan {
            tasks: vec![
                Task {
                    description: "Task one".into(),
                    source_file: PathBuf::from("root.md"),
                    source_line: 1,
                    ..Default::default()
                },
                Task {
                    description: "Task two".into(),
                    status: Status::Done,
                    source_file: PathBuf::from("root.md"),
                    source_line: 2,
                    ..Default::default()
                },
            ],
            ..v.scan()
        };
        let g = Graph::build(&v, &scan).unwrap();

        // Query for task nodes only
        let q = parse_query("node where kind = Task;").unwrap();
        let mut state = TreeState::default();
        let roots = q.select(&g);
        state.build_from(&roots, &g, &q, FT_TEST_TODAY);

        assert_eq!(state.rows.len(), 2);
        assert_eq!(state.rows[0].kind_char, 'T');
        assert_eq!(state.rows[0].display, "[ ] Task one");
        assert_eq!(state.rows[1].kind_char, 'T');
        assert_eq!(state.rows[1].display, "[x] Task two");
    }
}

#[cfg(test)]
mod view_tests {
    use std::path::PathBuf;

    use assert_fs::prelude::*;
    use ft_core::graph::Graph;
    use ft_core::vault::{Scan, Vault};

    use super::*;

    /// Pinned "today" for graph-tab tests, so task relative-date labels in
    /// snapshots don't drift with the wall clock. Matches `fixed_clock`.
    const FT_TEST_TODAY: chrono::NaiveDate = match chrono::NaiveDate::from_ymd_opt(2026, 5, 12) {
        Some(d) => d,
        None => panic!("invalid test date"),
    };

    fn dirs_graph() -> Graph {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dirs");
        let v = Vault::discover(Some(path)).expect("dirs fixture vault must exist");
        Graph::build(&v, &v.scan()).unwrap()
    }

    fn dirs_query_text() -> &'static str {
        "node where kind = Directory without incoming(kind = directory-contains); expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};"
    }

    fn view_with_query() -> (Graph, ExpandedView) {
        let g = dirs_graph();
        let mut v = ExpandedView {
            query_buf: EditBuffer::from(dirs_query_text()),
            ..Default::default()
        };
        v.apply_query(Some(&g), FT_TEST_TODAY);
        (g, v)
    }

    /// Test helper: vault-relative path → `NodeKey::Directory`.
    fn dir_key(p: &str) -> NodeKey {
        NodeKey::Directory(std::path::PathBuf::from(p))
    }

    #[test]
    fn add_expansion_path_includes_all_prefixes() {
        let mut v = ExpandedView::default();
        let root = dir_key("");
        let areas = dir_key("Areas");
        let ops = dir_key("Areas/operations");
        v.add_expansion_path(vec![root.clone(), areas.clone(), ops.clone()]);
        assert!(v.expanded_paths.contains(&vec![root.clone()]));
        assert!(v
            .expanded_paths
            .contains(&vec![root.clone(), areas.clone()]));
        assert!(v.expanded_paths.contains(&vec![root, areas, ops]));
    }

    #[test]
    fn forget_expansion_subtree_removes_descendants() {
        let root = dir_key("");
        let areas = dir_key("Areas");
        let ops = dir_key("Areas/operations");
        let projects = dir_key("Projects");
        let mut v = ExpandedView::default();
        v.add_expansion_path(vec![root.clone(), areas.clone(), ops.clone()]);
        v.add_expansion_path(vec![root.clone(), projects.clone()]);
        v.forget_expansion_subtree(&[root.clone(), areas.clone()]);
        assert!(!v
            .expanded_paths
            .contains(&vec![root.clone(), areas.clone()]));
        assert!(!v.expanded_paths.contains(&vec![root.clone(), areas, ops]));
        // Untouched siblings stay.
        assert!(v.expanded_paths.contains(&vec![root.clone(), projects]));
        assert!(v.expanded_paths.contains(&vec![root]));
    }

    #[test]
    fn path_to_walks_back_to_root() {
        let (g, v) = view_with_query();
        assert_eq!(v.path_to(0, &g).len(), 1);
    }

    #[test]
    fn restore_expansion_walks_each_path() {
        let (g, mut v) = view_with_query();
        // Expand root then Areas/.
        let root_key = g.stable_key(v.tree.rows()[0].note_id);
        v.tree
            .expand_at(0, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        v.add_expansion_path(vec![root_key.clone()]);
        let areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        let areas_key = g.stable_key(v.tree.rows()[areas_idx].note_id);
        v.tree
            .expand_at(areas_idx, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        v.add_expansion_path(vec![root_key, areas_key]);
        let expected_len = v.tree.len();

        // Now drop and re-derive from spec.
        v.tree = TreeState::default();
        v.restore_expansion(&g, FT_TEST_TODAY);

        assert_eq!(v.tree.len(), expected_len);
        assert!(v.tree.rows()[0].expanded);
        let restored_areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        assert!(v.tree.rows()[restored_areas_idx].expanded);
    }

    #[test]
    fn restore_expansion_truncates_at_missing_node() {
        let (g, mut v) = view_with_query();
        let root_key = g.stable_key(v.tree.rows()[0].note_id);
        v.tree
            .expand_at(0, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        v.add_expansion_path(vec![root_key.clone()]);
        // Inject a fictitious deeper path whose intermediate key
        // doesn't appear as a child of root in the visible tree —
        // restoration should drop it without panicking.
        let stale = NodeKey::Directory(std::path::PathBuf::from("does/not/exist"));
        v.expanded_paths.clear();
        v.expanded_paths.insert(vec![root_key.clone()]);
        v.expanded_paths
            .insert(vec![root_key, stale.clone(), dir_key("Areas/operations")]);
        v.tree = TreeState::default();
        v.restore_expansion(&g, FT_TEST_TODAY);
        // The valid path expanded the root.
        assert!(v.tree.rows()[0].expanded);
        // Verify expanded_paths retained only paths whose keys resolve.
        for path in &v.expanded_paths {
            for key in path {
                assert!(
                    g.id_for_key(key).is_some(),
                    "every restored path key must resolve in the graph"
                );
                assert_ne!(key, &stale, "stale key must have been dropped");
            }
        }
    }

    #[test]
    fn restore_expansion_preserves_selection_when_present() {
        let (g, mut v) = view_with_query();
        // Expand root, then select Areas/.
        v.tree
            .expand_at(0, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        let root_key = g.stable_key(v.tree.rows()[0].note_id);
        v.add_expansion_path(vec![root_key]);
        let areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        v.selected = areas_idx;
        v.refresh_selected_path(&g);

        // Drop derived state and restore.
        v.tree = TreeState::default();
        v.restore_expansion(&g, FT_TEST_TODAY);

        let restored_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        assert_eq!(v.selected, restored_idx);
    }

    #[test]
    fn restore_expansion_falls_back_to_ancestor_when_selection_gone() {
        let (g, mut v) = view_with_query();
        v.tree
            .expand_at(0, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        let root_key = g.stable_key(v.tree.rows()[0].note_id);
        v.add_expansion_path(vec![root_key.clone()]);
        // selected_path = [root, areas, ops]. Restoration only expands
        // root via expanded_paths, so areas isn't expanded → walker
        // stops at areas → selection falls back to that ancestor.
        let areas = g.node_by_path(std::path::Path::new("Areas")).unwrap();
        v.selected_path = Some(vec![
            root_key,
            dir_key("Areas"),
            dir_key("Areas/operations"),
        ]);
        v.tree = TreeState::default();
        v.restore_expansion(&g, FT_TEST_TODAY);

        let areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.note_id == areas)
            .unwrap();
        assert_eq!(v.selected, areas_idx);
    }

    #[test]
    fn restore_expansion_with_no_paths_falls_back_to_row_zero() {
        let (g, mut v) = view_with_query();
        v.selected = 5; // out of bounds for the no-expansion tree
        v.tree = TreeState::default();
        v.restore_expansion(&g, FT_TEST_TODAY);
        assert_eq!(v.selected, 0);
    }

    /// Regression: a freshly-built `Graph` assigns new `NodeIndex`
    /// values (per the `NoteId` doc-comment, IDs aren't stable across
    /// builds). Expansion must survive the rebuild by way of the
    /// path-based `NodeKey`s, so the user-perceived "tree collapses
    /// after delete / rename / git-sync" bug doesn't return.
    #[test]
    fn restore_expansion_survives_full_rebuild() {
        let (g, mut v) = view_with_query();
        // Expand root and Areas/.
        let root_key = g.stable_key(v.tree.rows()[0].note_id);
        v.tree
            .expand_at(0, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        v.add_expansion_path(vec![root_key.clone()]);
        let areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        let areas_key = g.stable_key(v.tree.rows()[areas_idx].note_id);
        v.tree
            .expand_at(areas_idx, &g, v.query.as_ref().unwrap(), FT_TEST_TODAY);
        v.add_expansion_path(vec![root_key, areas_key]);
        v.selected = areas_idx;
        v.refresh_selected_path(&g);
        let expanded_count_before = v.tree.len();

        // Drop the old graph entirely and build a new one against the
        // same vault. NoteIds in the new graph are not guaranteed
        // equal to those in `g`, but `NodeKey`s are.
        drop(g);
        let g2 = dirs_graph();
        v.tree = TreeState::default();
        v.restore_expansion(&g2, FT_TEST_TODAY);

        // Both expansions land in the rebuilt tree.
        assert_eq!(v.tree.len(), expanded_count_before);
        assert!(v.tree.rows()[0].expanded, "root stays expanded");
        let restored_areas_idx = v
            .tree
            .rows()
            .iter()
            .position(|r| r.display == "Areas/")
            .unwrap();
        assert!(
            v.tree.rows()[restored_areas_idx].expanded,
            "Areas/ stays expanded after rebuild"
        );
        // Selection landed back on Areas/.
        assert_eq!(v.selected, restored_areas_idx);
    }

    #[test]
    fn query_snippet_truncates_long_text() {
        let v = ExpandedView {
            query_buf: EditBuffer::from(
                "node where kind = Directory and path = \"\"; expand where ...",
            ),
            ..Default::default()
        };
        let snip = v.query_snippet();
        assert!(snip.chars().count() <= VIEW_LABEL_QUERY_WIDTH);
        assert!(snip.ends_with('…'));
    }

    #[test]
    fn query_snippet_empty_says_empty() {
        let v = ExpandedView::default();
        assert_eq!(v.query_snippet(), "(empty)");
    }

    #[test]
    fn new_graph_tab_has_one_empty_view() {
        let tab = GraphTab::new();
        assert_eq!(tab.views.len(), 1);
        assert_eq!(tab.active, 0);
        assert!(tab.views[0].query_buf.text.is_empty());
    }

    #[test]
    fn add_view_appends_and_switches() {
        let mut tab = GraphTab::new();
        tab.add_view();
        assert_eq!(tab.views.len(), 2);
        assert_eq!(tab.active, 1);
        // Input-mode focus is now expressed via `OpenModal(QueryBar)`
        // posted by the production `Ctrl+N` path; `add_view` itself
        // is a pure state-mutator and no longer sets a flag.
    }

    #[test]
    fn close_last_view_replaces_with_empty() {
        let mut tab = GraphTab::new();
        tab.views[0].query_buf.text = "node where indegree = 0;".into();
        tab.close_view();
        assert_eq!(tab.views.len(), 1);
        assert!(tab.views[0].query_buf.text.is_empty());
    }

    #[test]
    fn close_view_picks_left_neighbor() {
        let mut tab = GraphTab::new();
        tab.add_view();
        tab.add_view();
        assert_eq!(tab.active, 2);
        tab.close_view();
        // After removing index 2 from [_, _, _], new len=2 → active clamps to 1.
        assert_eq!(tab.views.len(), 2);
        assert_eq!(tab.active, 1);
    }

    #[test]
    fn cycle_views_wraps_at_bounds() {
        let mut tab = GraphTab::new();
        tab.add_view();
        tab.add_view();
        // active = 2
        tab.next_view();
        assert_eq!(tab.active, 0);
        tab.prev_view();
        assert_eq!(tab.active, 2);
        tab.prev_view();
        assert_eq!(tab.active, 1);
    }

    #[test]
    fn switch_view_bounds_checked() {
        let mut tab = GraphTab::new();
        tab.add_view();
        tab.switch_view(5);
        assert_eq!(tab.active, 1, "out-of-range switch must be a no-op");
        tab.switch_view(0);
        assert_eq!(tab.active, 0);
    }

    /// Ctrl+P opens the preset picker for the active view; selecting
    /// a preset replaces the active view's query in-place. Updated
    /// for extract-modal-driver §4: the picker is now an `ActiveModal`
    /// variant and commits via `AppRequest::GraphApplyPreset`.
    #[test]
    fn ctrl_p_preset_replaces_active_view_query() {
        use chrono::NaiveDate;
        use ft_core::recents::RecentsLog;
        use std::cell::Cell;
        use std::cell::RefCell;
        use std::sync::Arc;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dirs");
        let vault = Vault::discover(Some(path)).expect("dirs fixture vault must exist");
        let vault = Arc::new(vault);
        let recents = Arc::new(RecentsLog::for_vault(&vault));
        let today = NaiveDate::from_ymd_opt(2026, 5, 29).unwrap();
        let last_refresh = Cell::new(None);
        let pending_request = RefCell::new(None);
        let graph_refresh = Cell::new(false);

        let ctx = TabCtx {
            vault: &vault,
            recents: &recents,
            today,
            last_refresh: &last_refresh,
            pending_request: &pending_request,
            active_modal_name: None,
            host_popup_open: false,
            snapshot: None,
            graph_refresh: &graph_refresh,
        };

        // Build graph so views can resolve queries.
        let scan = vault.scan();
        let graph = Graph::build(&vault, &scan).unwrap();

        let mut tab = GraphTab::new();
        tab.set_graph_for_test(graph);
        tab.views[0].query_buf.text = "node where kind = Note;".to_string();

        // Ctrl+P → tab posts OpenModal(PresetPicker(... for_active_view=true ...)).
        tab.open_preset_picker_for_active_view(&ctx);
        let req = pending_request
            .borrow_mut()
            .take()
            .expect("Ctrl+P must queue an OpenModal request");
        let mut modal = match req {
            AppRequest::OpenModal(m) => match *m {
                ActiveModal::PresetPicker(p) => p,
                other => panic!("expected PresetPicker, got {:?}", other.name()),
            },
            other => panic!("expected OpenModal, got {other:?}"),
        };

        // Feed Enter to the modal: should commit by posting GraphApplyPreset.
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let outcome = modal.handle_event(Event::Key(enter), &ctx);
        assert!(
            matches!(outcome, ModalOutcome::Closed),
            "Enter on a selected row must close the modal"
        );

        // The modal queued GraphApplyPreset(dsl). Apply it via the tab hook.
        let req = pending_request
            .borrow_mut()
            .take()
            .expect("Enter must queue GraphApplyPreset");
        match req {
            AppRequest::GraphApplyPreset(dsl) => tab.graph_apply_preset(dsl),
            other => panic!("expected GraphApplyPreset, got {other:?}"),
        }

        // The active view's query is replaced with the preset DSL.
        // `fs` is the first preset alphabetically, so the picker lands on it.
        assert_eq!(
            tab.views[0].query_buf.text,
            r#"node where path = ""; expand where edge.kind in {directory-contains};"#,
            "active view query should be replaced by the selected preset DSL"
        );
    }

    // ── z (root-on-selected) tests ──────────────────────────────────

    /// Helper: build a graph, apply a query so the tree has the target
    /// node as a row, select it, and return the tab.
    fn tab_with_node_selected(
        files: &[(&str, &str)],
        query_text: &str,
        select_path: &str,
    ) -> GraphTab {
        use std::path::Path;
        let dir = assert_fs::TempDir::new().unwrap();
        dir.child(".obsidian").create_dir_all().unwrap();
        for (rel, content) in files {
            dir.child(rel).write_str(content).unwrap();
        }
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        let scan = vault.scan();
        let graph = Graph::build(&vault, &scan).unwrap();
        let mut v = ExpandedView {
            query_buf: EditBuffer::from(query_text),
            ..Default::default()
        };
        v.apply_query(Some(&graph), FT_TEST_TODAY);
        // Find and select the row matching select_path.
        let target = graph
            .node_by_path(Path::new(select_path))
            .expect("target node must exist");
        let sel = v
            .tree
            .rows()
            .iter()
            .position(|r| r.note_id == target)
            .expect("target row must be in tree");
        v.selected = sel;
        let mut tab = GraphTab::new();
        tab.set_graph_for_test(graph);
        tab.views[0] = v;
        tab
    }

    #[test]
    fn z_on_note_rewrites_query() {
        let mut tab = tab_with_node_selected(
            &[("Areas/finance.md", "[[Projects/alpha]]"), ("Projects/alpha.md", "")],
            "node where kind in {Note} and path = \"Areas/finance.md\"; expand where edge.kind in {directory-contains, note-link};",
            "Areas/finance.md",
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_buf.text,
            "node where kind in {Note} and path = \"Areas/finance.md\"; expand where edge.kind in {directory-contains, note-link};"
        );
    }

    #[test]
    fn z_on_directory_rewrites_query() {
        let mut tab = tab_with_node_selected(
            &[("Areas/finance.md", "")],
            "node where kind in {Directory} and path = \"Areas\"; expand where edge.kind in {directory-contains};",
            "Areas",
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_buf.text,
            "node where kind in {Directory} and path = \"Areas\"; expand where edge.kind in {directory-contains};"
        );
    }

    #[test]
    fn z_on_root_directory_rewrites_query() {
        let mut tab = tab_with_node_selected(
            &[("foo.md", "")],
            "node where kind in {Directory} and path = \"\"; expand where edge.kind in {directory-contains};",
            "",
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_buf.text,
            "node where kind in {Directory} and path = \"\"; expand where edge.kind in {directory-contains};"
        );
    }

    #[test]
    fn z_on_ghost_is_noop() {
        let dir = assert_fs::TempDir::new().unwrap();
        dir.child(".obsidian").create_dir_all().unwrap();
        dir.child("foo.md").write_str("[[Phantom]]").unwrap();
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        let graph = Graph::build(&vault, &vault.scan()).unwrap();
        let mut v = ExpandedView {
            query_buf: EditBuffer::from("node where kind = Ghost;"),
            ..Default::default()
        };
        v.apply_query(Some(&graph), FT_TEST_TODAY);
        v.selected = 0;
        let mut tab = GraphTab::new();
        tab.set_graph_for_test(graph);
        tab.views[0] = v;
        let before = tab.views[0].query_buf.text.clone();
        tab.rewrite_query_for_root();
        assert_eq!(tab.views[0].query_buf.text, before, "ghost should be no-op");
    }

    #[test]
    fn z_on_task_is_noop() {
        use ft_core::task::Task;
        let dir = assert_fs::TempDir::new().unwrap();
        dir.child(".obsidian").create_dir_all().unwrap();
        dir.child("root.md").write_str("- [ ] A task\n").unwrap();
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        let scan = Scan {
            tasks: vec![Task {
                description: "A task".into(),
                source_file: PathBuf::from("root.md"),
                source_line: 1,
                ..Default::default()
            }],
            ..vault.scan()
        };
        let graph = Graph::build(&vault, &scan).unwrap();
        let mut v = ExpandedView {
            query_buf: EditBuffer::from("node where kind = Task;"),
            ..Default::default()
        };
        v.apply_query(Some(&graph), FT_TEST_TODAY);
        v.selected = 0;
        let mut tab = GraphTab::new();
        tab.set_graph_for_test(graph);
        tab.views[0] = v;
        let before = tab.views[0].query_buf.text.clone();
        tab.rewrite_query_for_root();
        assert_eq!(tab.views[0].query_buf.text, before, "task should be no-op");
    }

    #[test]
    fn z_preserves_expand_block() {
        let mut tab = tab_with_node_selected(
            &[("Areas/finance.md", "")],
            "node where kind in {Directory} and path = \"\"; expand where edge.kind in {directory-contains, links-into, note-link};",
            "", // root directory is always in the tree for this query
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_buf.text,
            "node where kind in {Directory} and path = \"\"; expand where edge.kind in {directory-contains, links-into, note-link};"
        );
    }

    #[test]
    fn z_no_expand_block_produces_trailing_semicolon() {
        let mut tab = tab_with_node_selected(
            &[("foo.md", "")],
            "node where kind in {Note} and path = \"foo.md\";",
            "foo.md",
        );
        tab.rewrite_query_for_root();
        assert_eq!(
            tab.views[0].query_buf.text,
            "node where kind in {Note} and path = \"foo.md\";"
        );
    }
}

#[cfg(test)]
mod search_tests {
    use std::path::PathBuf;

    use assert_fs::prelude::*;
    use ft_core::graph::query::parse as parse_query;
    use ft_core::graph::Graph;
    use ft_core::vault::Vault;

    use super::*;
    use crate::tui::widgets::PickerSource;

    /// Pinned "today" for graph-tab tests, so task relative-date labels in
    /// snapshots don't drift with the wall clock. Matches `fixed_clock`.
    const FT_TEST_TODAY: chrono::NaiveDate = match chrono::NaiveDate::from_ymd_opt(2026, 5, 12) {
        Some(d) => d,
        None => panic!("invalid test date"),
    };

    fn dirs_graph() -> Graph {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dirs");
        let v = Vault::discover(Some(path)).expect("dirs fixture vault must exist");
        Graph::build(&v, &v.scan()).unwrap()
    }

    fn dirs_query() -> GraphQuery {
        parse_query(
            "node where kind = Directory and path = \"\"; expand where edge.kind = directory-contains;",
        )
        .unwrap()
    }

    // 7.1
    #[test]
    fn collect_finds_root_and_deeper_with_shortest_paths() {
        let g = dirs_graph();
        let q = dirs_query();
        let candidates = collect_search_candidates(&g, &q, FT_TEST_TODAY);

        let root_id = g.node_by_path(std::path::Path::new("")).unwrap();
        let root = candidates
            .iter()
            .find(|c| c.path == vec![root_id])
            .expect("root candidate present");
        assert_eq!(root.leaf, "/");
        assert!(root.breadcrumb.is_empty());

        let areas_id = g.node_by_path(std::path::Path::new("Areas")).unwrap();
        let areas = candidates
            .iter()
            .find(|c| *c.path.last().unwrap() == areas_id)
            .expect("Areas candidate present");
        assert_eq!(areas.path, vec![root_id, areas_id]);
        assert_eq!(areas.leaf, "Areas/");
        assert_eq!(areas.breadcrumb, "/");

        let ops_id = g
            .node_by_path(std::path::Path::new("Areas/operations"))
            .unwrap();
        let ops = candidates
            .iter()
            .find(|c| *c.path.last().unwrap() == ops_id)
            .expect("Areas/operations candidate present");
        assert_eq!(ops.path, vec![root_id, areas_id, ops_id]);
        assert_eq!(ops.leaf, "operations/");
        assert_eq!(ops.breadcrumb, "/Areas");
    }

    // 7.2
    #[test]
    fn bfs_terminates_on_cycle() {
        // Build a tiny vault with two notes that link to each other:
        // a.md → [[b]], b.md → [[a]]. With an expand policy that
        // follows Link edges and no max_depth, naive traversal would
        // loop. BFS with the visited set must return ≤ 2 candidates.
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("a.md").write_str("[[b]]\n").unwrap();
        tmp.child("b.md").write_str("[[a]]\n").unwrap();
        let vault = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&vault, &vault.scan()).unwrap();
        let q = parse_query(
            "node where kind = Note and path = \"a.md\"; expand where edge.kind = note-link;",
        )
        .unwrap();

        let candidates = collect_search_candidates(&g, &q, FT_TEST_TODAY);
        // a (depth 0) and b (depth 1); BFS must terminate.
        assert_eq!(candidates.len(), 2);
        let depths: Vec<usize> = candidates.iter().map(|c| c.path.len()).collect();
        assert!(depths.contains(&1));
        assert!(depths.contains(&2));
    }

    // 7.3
    #[test]
    fn no_expand_block_yields_only_roots() {
        let g = dirs_graph();
        // Two roots, no expand block.
        let q = parse_query("node where kind = Note;").unwrap();
        let candidates = collect_search_candidates(&g, &q, FT_TEST_TODAY);
        assert!(!candidates.is_empty(), "dirs fixture has at least one note");
        // Every candidate's path has length 1 (it's a root).
        assert!(
            candidates.iter().all(|c| c.path.len() == 1),
            "without expand, every candidate is a root"
        );
        // Exactly equal to `query.select(graph)` length.
        assert_eq!(candidates.len(), q.select(&g).len());
    }

    // 7.4 — the factor-out can't drift since make_row literally calls
    // leaf_display, but we still assert it for every node in the dirs
    // graph so any future divergence (e.g. someone re-inlining) is
    // caught.
    #[test]
    fn leaf_display_matches_make_row_for_every_node() {
        let g = dirs_graph();
        let q = dirs_query();
        for (id, _) in g.nodes() {
            let row = TreeState::make_row(id, 0, &g, &q, FT_TEST_TODAY);
            let (display, kind_char) = leaf_display(&g, id, FT_TEST_TODAY);
            assert_eq!(row.display, display, "display mismatch for {:?}", id);
            assert_eq!(row.kind_char, kind_char, "kind mismatch for {:?}", id);
        }
    }

    /// graph-task-interaction §D6: a Task row shows the status marker,
    /// description, and compact relative due/scheduled + priority when set.
    #[test]
    fn leaf_display_task_shows_dates_and_priority() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        let v = ft_core::vault::Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        // Due 3 days ago (relative to FT_TEST_TODAY = 2026-05-12).
        let scan = ft_core::vault::Scan {
            tasks: vec![ft_core::task::Task {
                description: "Fix login bug".into(),
                status: ft_core::task::Status::Open,
                priority: Some(ft_core::task::Priority::High),
                due: Some(chrono::NaiveDate::from_ymd_opt(2026, 5, 9).unwrap()),
                source_file: PathBuf::from("root.md"),
                source_line: 1,
                ..Default::default()
            }],
            ..v.scan()
        };
        let g = ft_core::graph::Graph::build(&v, &scan).unwrap();
        let task_id = g.task_by_loc(Path::new("root.md"), 1).unwrap();
        let (display, kind) = leaf_display(&g, task_id, FT_TEST_TODAY);
        assert_eq!(kind, 'T');
        assert!(display.starts_with("[ ] Fix login bug"), "got: {display}");
        assert!(
            display.contains("📅 3d ago"),
            "expected relative due: {display}"
        );
        assert!(
            display.contains("⏫"),
            "expected high priority marker: {display}"
        );
    }

    /// A Task with no dates/priority renders just the marker + description.
    #[test]
    fn leaf_display_task_omits_absent_fields() {
        use assert_fs::prelude::*;
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        let v = ft_core::vault::Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let scan = ft_core::vault::Scan {
            tasks: vec![ft_core::task::Task {
                description: "Plain task".into(),
                source_file: PathBuf::from("root.md"),
                source_line: 1,
                ..Default::default()
            }],
            ..v.scan()
        };
        let g = ft_core::graph::Graph::build(&v, &scan).unwrap();
        let task_id = g.task_by_loc(Path::new("root.md"), 1).unwrap();
        let (display, kind) = leaf_display(&g, task_id, FT_TEST_TODAY);
        assert_eq!(kind, 'T');
        assert_eq!(display, "[ ] Plain task", "got: {display}");
    }

    // 7.5
    #[test]
    fn nucleo_ranks_leaf_match_over_unrelated() {
        // Synthesize two candidates with known haystacks; pick the
        // first NoteId from the dirs graph as a stand-in id (we don't
        // actually use it for matching).
        let g = dirs_graph();
        let some_id = g.nodes().next().unwrap().0;
        let mut src = GraphSearchPickerSource {
            candidates: vec![
                Candidate {
                    path: vec![some_id, some_id],
                    leaf: "bar".to_string(),
                    breadcrumb: "foo".to_string(),
                    kind_char: 'D',
                },
                Candidate {
                    path: vec![some_id, some_id],
                    leaf: "quux".to_string(),
                    breadcrumb: "foo".to_string(),
                    kind_char: 'D',
                },
            ],
            matcher: nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT),
            buf: Vec::new(),
        };
        let items = src.query("bar", 10);
        assert!(!items.is_empty(), "matcher must produce at least one row");
        // First (highest-ranked) item is the `bar` candidate, not `quux`.
        assert!(items[0].label.starts_with("bar"));
    }

    // 7.6
    #[test]
    fn jump_to_path_lands_cursor_at_target_with_ancestors_expanded() {
        let g = dirs_graph();
        let root_id = g.node_by_path(std::path::Path::new("")).unwrap();
        let areas_id = g.node_by_path(std::path::Path::new("Areas")).unwrap();
        let ops_id = g
            .node_by_path(std::path::Path::new("Areas/operations"))
            .unwrap();
        let shifts_id = g
            .note_by_path(std::path::Path::new("Areas/operations/shifts.md"))
            .unwrap();

        let mut tab = GraphTab::new();
        tab.set_graph_for_test(g);
        tab.views[0].query_buf.text =
            "node where kind = Directory and path = \"\"; expand where edge.kind = directory-contains;"
                .to_string();
        let snap = tab.snapshot.clone().unwrap();
        tab.views[0].apply_query(Some(&snap.graph), FT_TEST_TODAY);

        let path = vec![root_id, areas_id, ops_id, shifts_id];
        tab.jump_to_path(path.clone());

        let v = &tab.views[0];
        let row = v.tree.rows().get(v.selected).expect("a row is selected");
        assert_eq!(row.note_id, shifts_id, "cursor landed on shifts.md");
        assert_eq!(row.depth, 3, "shifts.md is at depth 3");
        // The view stores paths as build-stable `NodeKey`s; rebuild
        // the expected key path the same way `jump_to_path` does.
        let g_ref = tab.graph().unwrap();
        let key_path: Vec<NodeKey> = path.iter().map(|id| g_ref.stable_key(*id)).collect();
        assert_eq!(v.selected_path.as_deref(), Some(key_path.as_slice()));
        // Ancestors are recorded in expanded_paths (closed under prefixes).
        assert!(v.expanded_paths.contains(&vec![key_path[0].clone()]));
        assert!(v
            .expanded_paths
            .contains(&vec![key_path[0].clone(), key_path[1].clone()]));
        assert!(v.expanded_paths.contains(&vec![
            key_path[0].clone(),
            key_path[1].clone(),
            key_path[2].clone(),
        ]));
        // Target itself is NOT in expanded_paths.
        assert!(!v.expanded_paths.contains(&key_path));
    }
}

#[cfg(test)]
mod nav_tests {
    use std::path::PathBuf;

    use assert_fs::prelude::*;
    use ft_core::graph::Graph;
    use ft_core::vault::Vault;

    use super::*;

    /// Pinned "today" for graph-tab tests, so task relative-date labels in
    /// snapshots don't drift with the wall clock. Matches `fixed_clock`.
    const FT_TEST_TODAY: chrono::NaiveDate = match chrono::NaiveDate::from_ymd_opt(2026, 5, 12) {
        Some(d) => d,
        None => panic!("invalid test date"),
    };

    fn dirs_graph() -> Graph {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dirs");
        let v = Vault::discover(Some(path)).expect("dirs fixture vault must exist");
        Graph::build(&v, &v.scan()).unwrap()
    }

    fn tab_with_query(graph: Graph, query_text: &str) -> GraphTab {
        let mut v = ExpandedView {
            query_buf: EditBuffer::from(query_text),
            ..Default::default()
        };
        v.apply_query(Some(&graph), FT_TEST_TODAY);
        let mut tab = GraphTab::new();
        tab.set_graph_for_test(graph);
        tab.views = vec![v];
        tab
    }

    // ── find_node_path ─────────────────────────────────────────────

    #[test]
    fn find_node_path_reachable_target() {
        let g = dirs_graph();
        let tab = tab_with_query(g, "node where kind = Directory and path = \"\"; expand where edge.kind = directory-contains;");

        let target = tab
            .graph()
            .unwrap()
            .node_by_path(std::path::Path::new("Areas"))
            .unwrap();
        let path = tab.find_node_path(target);
        assert!(path.is_some(), "Areas should be reachable");
        let path = path.unwrap();
        // Path should be: root → Areas
        assert_eq!(path.len(), 2, "path has 2 nodes: root, Areas");
    }

    #[test]
    fn find_node_path_unreachable_target() {
        // Use a query that only selects a specific directory —
        // other directories not connected via the expand policy
        // are unreachable.
        let g = dirs_graph();
        let tab = tab_with_query(g, "node where kind = Directory and path = \"Areas\";");
        // "Projects" is a different directory, not reachable via
        // directory-contains from just the "Areas" root with no expand.
        let target = tab
            .graph()
            .unwrap()
            .node_by_path(std::path::Path::new("Projects"))
            .unwrap();
        let path = tab.find_node_path(target);
        assert!(
            path.is_none(),
            "Projects should be unreachable from Areas-only root"
        );
    }

    #[test]
    fn find_node_path_root_is_target() {
        let g = dirs_graph();
        let tab = tab_with_query(g, "node where kind = Directory and path = \"\"; expand where edge.kind = directory-contains;");
        let root = tab
            .graph()
            .unwrap()
            .node_by_path(std::path::Path::new(""))
            .unwrap();
        let path = tab.find_node_path(root);
        assert!(path.is_some(), "root should be found");
        let path = path.unwrap();
        assert_eq!(path.len(), 1, "root path should be length 1");
        assert_eq!(path[0], root);
    }

    #[test]
    fn find_node_path_shortest_path_wins() {
        // With a link-graph expand policy, a note reachable via
        // multiple paths should return the shortest (BFS).
        let (_dir, vault) = link_vault_for_shortest_path();
        let g = Graph::build(&vault, &vault.scan()).unwrap();
        let tab = tab_with_query(
            g,
            "node where kind = Note and path = \"A.md\"; expand where edge.kind in {links-into, note-link};",
        );

        // A links to C, and A links to D which links to C.
        // The BFS should find the shorter path A→C.
        let c_id = tab
            .graph()
            .unwrap()
            .node_by_path(std::path::Path::new("C.md"))
            .unwrap();
        let path = tab
            .find_node_path(c_id)
            .expect("C should be reachable from A");
        // Path should be A→C (length 2) not A→D→C (length 3).
        assert_eq!(path.len(), 2, "shortest path should be A→C (length 2)");
        // Verify path starts at A and ends at C.
        let a_id = tab
            .graph()
            .unwrap()
            .node_by_path(std::path::Path::new("A.md"))
            .unwrap();
        assert_eq!(path.first(), Some(&a_id));
        assert_eq!(path.last(), Some(&c_id));
    }

    fn link_vault_for_shortest_path() -> (assert_fs::TempDir, Vault) {
        let dir = assert_fs::TempDir::new().unwrap();
        dir.child(".obsidian").create_dir_all().unwrap();
        // A links to C, A links to D, D links to C.
        // Shortest A→C is direct (A→C), not A→D→C.
        dir.child("A.md").write_str("[[C]]\n[[D]]\n").unwrap();
        dir.child("C.md").write_str("").unwrap();
        dir.child("D.md").write_str("[[C]]\n").unwrap();
        let vault = Vault::discover(Some(dir.path().to_path_buf())).unwrap();
        (dir, vault)
    }
}

//! Built-in graph-query presets.
//!
//! User-defined presets in [`GraphCfg::presets`](crate::config::GraphCfg::presets)
//! shadow built-ins of the same name. Resolution lives in the CLI; this module
//! just owns the canonical built-in definitions as DSL strings so they round-
//! trip through the same parser as user queries.

/// Return the DSL string for a built-in preset, or `None` if unknown.
pub fn builtin(name: &str) -> Option<&'static str> {
    Some(match name {
        "fs" => r#"node where path = ""; expand where edge.kind in {directory-contains};"#,
        "ghosts" => "node where kind in {Ghost};",
        "links" => "node where kind in {Note}; expand where edge.kind in {link, embed};",
        "tasks-in-fs" => {
            r#"node where path = ""; expand where edge.kind in {directory-contains, has-task};"#
        }
        "tree" => {
            r#"node where path = ""; expand where edge.kind in {directory-contains, link, embed};"#
        }
        _ => return None,
    })
}

/// Names of all built-in presets, sorted, for help text and shell completions.
pub fn builtin_names() -> &'static [&'static str] {
    &["fs", "ghosts", "links", "tasks-in-fs", "tree"]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::query;
    use crate::graph::Graph;
    use crate::task::{Status, Task};
    use crate::vault::{Scan, Vault};
    use assert_fs::prelude::*;
    use std::path::PathBuf;

    #[test]
    fn every_builtin_parses() {
        for name in builtin_names() {
            let dsl_str = builtin(name).unwrap_or_else(|| panic!("missing preset {name}"));
            query::parse(dsl_str)
                .unwrap_or_else(|e| panic!("preset `{name}` failed to parse: {e}"));
        }
    }

    #[test]
    fn unknown_preset_returns_none() {
        assert!(builtin("nope").is_none());
    }

    /// tasks-in-fs preset includes tasks when walked against a graph with tasks.
    #[test]
    fn tasks_in_fs_preset_includes_tasks() {
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("root.md").write_str("- [ ] A task\n").unwrap();

        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let scan = Scan {
            tasks: vec![Task {
                description: "A task".into(),
                status: Status::Open,
                priority: None,
                tags: vec![],
                due: None,
                scheduled: None,
                source_file: PathBuf::from("root.md"),
                source_line: 1,
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
            }],
            errors: vec![],
        };
        let g = Graph::build(&v, &scan).unwrap();

        let dsl_str = builtin("tasks-in-fs").expect("tasks-in-fs preset exists");
        let q = query::parse(dsl_str).unwrap();
        let tree = q.walk(&g, &query::WalkOptions::unlimited());

        fn count_tasks(nodes: &[query::WalkNode], graph: &Graph) -> usize {
            let mut count = 0;
            for node in nodes {
                if matches!(graph.node(node.id), crate::graph::NodeKind::Task(_)) {
                    count += 1;
                }
                count += count_tasks(&node.children, graph);
            }
            count
        }

        assert!(
            count_tasks(&tree, &g) > 0,
            "tasks-in-fs should include tasks"
        );
    }

    /// tree preset excludes tasks: it expands directory-contains/link/embed
    /// only, so has-task edges (the only way to reach a Task node) are not
    /// followed.
    #[test]
    fn fs_preset_excludes_tasks() {
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("root.md").write_str("- [ ] A task\n").unwrap();

        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let scan = Scan {
            tasks: vec![Task {
                description: "A task".into(),
                status: Status::Open,
                priority: None,
                tags: vec![],
                due: None,
                scheduled: None,
                source_file: PathBuf::from("root.md"),
                source_line: 1,
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
            }],
            errors: vec![],
        };
        let g = Graph::build(&v, &scan).unwrap();

        let dsl_str = builtin("fs").expect("fs preset exists");
        let q = query::parse(dsl_str).unwrap();
        let tree = q.walk(&g, &query::WalkOptions::unlimited());

        fn count_tasks(nodes: &[query::WalkNode], graph: &Graph) -> usize {
            let mut count = 0;
            for node in nodes {
                if matches!(graph.node(node.id), crate::graph::NodeKind::Task(_)) {
                    count += 1;
                }
                count += count_tasks(&node.children, graph);
            }
            count
        }

        assert_eq!(count_tasks(&tree, &g), 0, "fs preset should exclude tasks");
    }

    /// tree preset excludes tasks: it expands directory-contains/link/embed
    /// but not has-task.
    #[test]
    fn tree_preset_excludes_tasks() {
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        tmp.child("root.md").write_str("- [ ] A task\n").unwrap();

        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let scan = Scan {
            tasks: vec![Task {
                description: "A task".into(),
                status: Status::Open,
                priority: None,
                tags: vec![],
                due: None,
                scheduled: None,
                source_file: PathBuf::from("root.md"),
                source_line: 1,
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
            }],
            errors: vec![],
        };
        let g = Graph::build(&v, &scan).unwrap();

        let dsl_str = builtin("tree").expect("tree preset exists");
        let q = query::parse(dsl_str).unwrap();
        let tree = q.walk(&g, &query::WalkOptions::unlimited());

        fn count_tasks(nodes: &[query::WalkNode], graph: &Graph) -> usize {
            let mut count = 0;
            for node in nodes {
                if matches!(graph.node(node.id), crate::graph::NodeKind::Task(_)) {
                    count += 1;
                }
                count += count_tasks(&node.children, graph);
            }
            count
        }

        assert_eq!(
            count_tasks(&tree, &g),
            0,
            "tree preset should exclude tasks"
        );
    }
}

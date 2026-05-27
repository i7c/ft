//! Built-in graph-query presets.
//!
//! User-defined presets in [`GraphCfg::presets`](crate::config::GraphCfg::presets)
//! shadow built-ins of the same name. Resolution lives in the CLI; this module
//! just owns the canonical built-in definitions as DSL strings so they round-
//! trip through the same parser as user queries.

/// Return the DSL string for a built-in preset, or `None` if unknown.
pub fn builtin(name: &str) -> Option<&'static str> {
    Some(match name {
        "dangling" => "node where kind = Ghost;",
        "links" => "node where kind = Note; expand where edge.kind in {link, embed};",
        "orphans" => "node where indegree = 0 and kind = Note;",
        "tree" => {
            r#"node where kind = Directory and path = ""; expand where edge.kind = directory-contains;"#
        }
        "tasks-in-tree" => {
            r#"node where kind = Directory and path = ""; expand where edge.kind in {directory-contains, has-task};"#
        }
        _ => return None,
    })
}

/// Names of all built-in presets, sorted, for help text and shell completions.
pub fn builtin_names() -> &'static [&'static str] {
    &["dangling", "links", "orphans", "tasks-in-tree", "tree"]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::query;

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

    /// Task 7.10: tasks-in-tree preset parses and differs from tree in expand targets.
    #[test]
    fn tasks_in_tree_preset_differs_from_tree() {
        let tasks_dsl = builtin("tasks-in-tree").expect("tasks-in-tree preset exists");
        let tree_dsl = builtin("tree").expect("tree preset exists");

        // Both parse successfully
        let tasks_q = query::parse(tasks_dsl)
            .unwrap_or_else(|e| panic!("tasks-in-tree failed to parse: {e}"));
        let tree_q = query::parse(tree_dsl).unwrap_or_else(|e| panic!("tree failed to parse: {e}"));

        // Both should have an expand block
        assert!(tasks_q.expansion.is_some(), "tasks-in-tree has expand");
        assert!(tree_q.expansion.is_some(), "tree has expand");

        // The DSL strings should differ (tasks-in-tree includes has-task)
        assert_ne!(tasks_dsl, tree_dsl, "presets must differ");
        assert!(
            tasks_dsl.contains("has-task"),
            "tasks-in-tree should reference has-task"
        );
        assert!(
            !tree_dsl.contains("has-task"),
            "tree should not reference has-task"
        );
    }
}

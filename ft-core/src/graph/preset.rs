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
        _ => return None,
    })
}

/// Names of all built-in presets, sorted, for help text and shell completions.
pub fn builtin_names() -> &'static [&'static str] {
    &["dangling", "links", "orphans", "tree"]
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
}

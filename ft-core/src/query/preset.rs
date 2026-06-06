//! Built-in task query presets.
//!
//! These are graph DSL strings parsed under [`Profile::Tasks`](crate::graph::query::Profile::Tasks).
//! User-defined presets in [`Config::presets`](crate::config::Config::presets) shadow
//! built-ins of the same name. Resolution lives in the CLI; this module just owns the
//! canonical built-in definitions as DSL strings so they round-trip through the same
//! parser as user queries.

/// Return the DSL string for a built-in preset, or `None` if unknown.
///
/// The strings here use the unified graph DSL under `Profile::Tasks`
/// semantics — bare attribute references default to `self.<attr>` and a
/// `node where kind = Task and` prelude is implicit.
pub fn builtin(name: &str) -> Option<&'static str> {
    Some(match name {
        "today" => "(status in {Open, InProgress}) and (due = today or scheduled = today)",
        "overdue" => "(status in {Open, InProgress}) and due < today",
        "upcoming" => "(status in {Open, InProgress}) and due > today",
        "done-today" => "status = Done and completed = today",
        "not-done" => "status in {Open, InProgress}",
        _ => return None,
    })
}

/// Names of all built-in presets, sorted, for help text and shell completions.
pub fn builtin_names() -> &'static [&'static str] {
    &["done-today", "not-done", "overdue", "today", "upcoming"]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::query::{parse_with, Profile};
    use chrono::NaiveDate;

    #[test]
    fn every_builtin_parses() {
        let today = NaiveDate::from_ymd_opt(2026, 5, 9).unwrap();
        for name in builtin_names() {
            let dsl_str = builtin(name).unwrap_or_else(|| panic!("missing preset {name}"));
            parse_with(dsl_str, Profile::Tasks, today)
                .unwrap_or_else(|e| panic!("preset `{name}` failed to parse: {e}"));
        }
    }

    #[test]
    fn unknown_preset_returns_none() {
        assert!(builtin("nope").is_none());
    }

    #[test]
    fn today_preset_uses_or_disjunction() {
        // The `today` preset uses `or` between `due = today` and
        // `scheduled = today`. Ensure both branches survive parsing.
        let today = NaiveDate::from_ymd_opt(2026, 5, 9).unwrap();
        let q = parse_with(builtin("today").unwrap(), Profile::Tasks, today).unwrap();
        // Sanity: there is exactly one selector and at least three Condition
        // leaves (status in {...}, due = today, scheduled = today).
        assert_eq!(q.initial.len(), 1);
        assert!(q.initial[0].conditions().len() >= 3);
    }
}

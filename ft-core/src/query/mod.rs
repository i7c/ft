//! Programmatic task query API.
//!
//! - [`preset`] — named built-in / user query strings, parsed under
//!   [`crate::graph::query::Profile::Tasks`].
//! - [`sort`] — sort key compilation and `Vec<&Task>` sort helpers.
//!
//! Task predicate evaluation lives entirely in [`crate::graph::query`]:
//! `ft tasks list` lowers its CLI flags to a DSL fragment and parses
//! everything (positional preset, `--query`, lowered flags) through one
//! parser under [`Profile::Tasks`](crate::graph::query::Profile::Tasks).
//! No separate `Filter` type — the DSL is the single source of truth
//! for what predicates exist.

pub mod preset;
pub mod sort;

pub use sort::{default_sort, parse_sort_key, sort_by_keys, SortKey, SortOrder};

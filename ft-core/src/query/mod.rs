//! Programmatic task query API.
//!
//! - [`Filter`] — flag-based, conjunctive, used by CLI flags directly.
//! - [`preset`] — named built-in / user query strings, parsed under
//!   [`crate::graph::query::Profile::Tasks`].
//! - [`sort`] — sort key compilation and `Vec<&Task>` sort helpers.
//!
//! Task DSL parsing now lives in [`crate::graph::query`] — `ft tasks list`
//! invokes the unified graph DSL parser under
//! [`Profile::Tasks`](crate::graph::query::Profile::Tasks).

pub mod filter;
pub mod preset;
pub mod sort;

pub use filter::Filter;
pub use sort::{default_sort, parse_sort_key, sort_by_keys, SortKey, SortOrder};

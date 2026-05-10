//! Programmatic query API: filter and sort task collections.
//!
//! The DSL parser (Session 4) compiles textual queries into the same
//! [`Filter`] / [`SortKey`] types that flag-based CLI arguments use, so both
//! paths share semantics.

pub mod filter;
pub mod sort;

pub use filter::Filter;
pub use sort::{default_sort, SortKey, SortOrder};

//! The App-owned graph snapshot — single source of graph, task, and
//! search data for every TUI tab and modal (openspec:
//! shared-graph-snapshot).
//!
//! One snapshot is one `vault.scan()` + `Graph::build` + search-index
//! pass. It is immutable once installed: consumers read it through
//! [`crate::tui::tab::TabCtx::snapshot`] and never build graphs or
//! indexes themselves. Mutating flows post `AppRequest::RefreshGraph`;
//! the App's background worker builds a replacement and installs it
//! with a higher generation.

use std::sync::Arc;

use ft_core::graph::Graph;
use ft_core::scan::Scan;
use ft_core::search::SearchIndex;
use ft_core::vault::Vault;

/// One installed snapshot. `scan`, `graph`, and `search`
/// all derive from the same read pass, so task line numbers, graph task
/// nodes, and search hits always agree.
#[derive(Debug)]
pub struct GraphSnapshot {
    /// Monotonic per App; increments with every completed build. Tabs
    /// compare against the generation they last derived view state from
    /// to know when to re-derive (expansion, selection, cursor anchors —
    /// all keyed by `NodeKey` so they survive the swap).
    pub generation: u64,
    /// `Arc` so pickers and other transient UI components can hold an
    /// owned handle to the scan's parse artifacts (paths, headings,
    /// mtimes, frontmatter) without outliving the snapshot borrow.
    pub scan: Arc<Scan>,
    pub graph: Graph,
    /// The paragraph search index, rebuilt with every snapshot so the
    /// Search tab's live queries run against the installed generation.
    pub search: Arc<SearchIndex>,
}

/// Build one snapshot. The single build path shared by the background
/// worker and the synchronous test pump, so tests exercise exactly what
/// production runs. Errors are stringified so the result can ride the
/// `Clone` event channel.
pub fn build_graph_snapshot(vault: &Vault, generation: u64) -> Result<Arc<GraphSnapshot>, String> {
    let scan = vault.scan();
    let graph = Graph::build(&scan).map_err(|e| e.to_string())?;
    let cfg = &vault.config.config;
    let search = SearchIndex::build_with(&scan, &cfg.synth.exclude_prefixes, &cfg.search);
    Ok(Arc::new(GraphSnapshot {
        generation,
        scan: Arc::new(scan),
        graph,
        search: Arc::new(search),
    }))
}

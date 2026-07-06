//! The App-owned graph snapshot — single source of graph and task data
//! for every TUI tab and modal (openspec: shared-graph-snapshot).
//!
//! One snapshot is one `vault.scan()` + `Graph::build` pass. It is
//! immutable once installed: consumers read it through
//! [`crate::tui::tab::TabCtx::snapshot`] and never build graphs
//! themselves. Mutating flows post `AppRequest::RefreshGraph`; the App's
//! background worker builds a replacement and installs it with a higher
//! generation.

use std::sync::Arc;

use ft_core::graph::Graph;
use ft_core::synth::citations::CitationIndex;
use ft_core::vault::{Scan, Vault};

/// One installed graph build. `scan` and `graph` come from the same
/// read pass, so task line numbers and graph task nodes always agree.
#[derive(Debug)]
pub struct GraphSnapshot {
    /// Monotonic per App; increments with every completed build. Tabs
    /// compare against the generation they last derived view state from
    /// to know when to re-derive (expansion, selection, cursor anchors —
    /// all keyed by `NodeKey` so they survive the swap).
    pub generation: u64,
    pub scan: Scan,
    pub graph: Graph,
    /// Which synth notes cite which paragraphs, rebuilt with every
    /// snapshot so feed badges stay generation-consistent with the
    /// graph they annotate.
    pub citations: CitationIndex,
}

/// Build one snapshot. The single build path shared by the background
/// worker and the synchronous test pump, so tests exercise exactly what
/// production runs. Errors are stringified so the result can ride the
/// `Clone` event channel.
pub fn build_graph_snapshot(vault: &Vault, generation: u64) -> Result<Arc<GraphSnapshot>, String> {
    let scan = vault.scan();
    let citations = CitationIndex::build(vault);
    match Graph::build(vault, &scan) {
        Ok(graph) => Ok(Arc::new(GraphSnapshot {
            generation,
            scan,
            graph,
            citations,
        })),
        Err(e) => Err(e.to_string()),
    }
}

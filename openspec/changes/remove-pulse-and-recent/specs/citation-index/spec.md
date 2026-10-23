## REMOVED Requirements

### Requirement: CitationIndex build
**Reason**: `ft_core::synth::citations` is deleted. Its only consumers were
the Recent feed's badges (TUI) and `cited_in` (`ft notes recent --json`); with
the feed removed the index is built into every TUI graph snapshot and read by
nothing.
**Migration**: None. The index is derivative data (rebuilt per snapshot, never
persisted), so reintroducing a future citation surface is a self-contained
change; the behavior is recoverable from this archive.

### Requirement: Three-state lookup
**Reason**: `CitationIndex::lookup` (Cited / CitedStale / Uncited) is deleted
with the module.
**Migration**: None.

### Requirement: Consistency with scaffold dedup
**Reason**: The badge-side mirror of `accrete::filter_missing`'s match rule is
deleted; `filter_missing` itself is unchanged and remains the dedup rule for
scaffold/append.
**Migration**: None.

### Requirement: Build points
**Reason**: No CLI command or TUI tab builds or reads the index any more;
`GraphSnapshot::citations` is removed and the graph-rebuild worker no longer
builds it.
**Migration**: None.

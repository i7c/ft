# Shared Graph Snapshot

## Why

Every TUI tab builds its own private `Graph` by re-scanning the entire vault
synchronously inside key handlers and `on_focus` (~20 `vault.scan()` +
`Graph::build` call sites; `tabs/journal.rs` says it outright: "the App-level
graph belongs to the Graph tab and isn't easily reachable from here").
Consequences: keystroke latency scales with vault size in the worst place,
tabs drift out of sync after mutations, and every new feature re-answers
"where do I get a graph?" by pasting another scan+build block. This is
finding 1 of the 2026-07-02 architecture review — the prerequisites
(single-pass `scan → build`, one `AppRequest` routing table) landed in
sessions B and C.

## What Changes

- `App` owns the one graph: a `RefCell<Option<Arc<GraphSnapshot>>>` slot
  holding the built `Graph`, the `Scan` it came from, and a monotonically
  increasing generation counter.
- A background worker (same pattern as the git-sync job) runs
  `vault.scan()` + `Graph::build` off the UI thread and posts a
  `BgEvent::GraphReady` back to the main loop. Rebuild requests are
  single-flight with a dirty flag: a request arriving mid-build coalesces
  into one follow-up rebuild.
- Tabs read the snapshot through `TabCtx` (cheap `Arc` clone) instead of
  building their own graph. Cross-rebuild UI state (expanded paths,
  selection, cursor anchors) keys off `NodeKey` — which was designed for
  exactly this — and re-derives when the generation changes.
- Mutating flows (task edits, note create/rename/delete/move, editor
  return, git sync completion) post one `AppRequest::RefreshGraph` instead
  of rebuilding inline; until the fresh snapshot arrives, tabs render the
  previous one, and the expected-line guards from session A make stale
  mutations fail safe instead of corrupting.
- Tabs render a lightweight "loading" state when no snapshot exists yet
  (first frames after startup).
- All per-tab `scan()` + `Graph::build` call sites in the TUI are removed;
  a test-only synchronous pump keeps `TestBackend` snapshot tests
  deterministic.

## Capabilities

### New Capabilities

- `tui-shared-graph`: the App-owned graph snapshot — single source of graph
  truth for all tabs, background rebuild lifecycle (request, coalescing,
  delivery, generation), tab access rules, staleness semantics, and loading
  states.

### Modified Capabilities

<!-- none — existing tab specs describe user-visible behavior (tree
     contents, journal entries, review rows), which is unchanged; how the
     graph is obtained is an implementation seam. Eventual-consistency
     timing is covered by the new capability's requirements. -->

## Impact

- **`ft/src/tui/app.rs`** — snapshot slot, rebuild worker + coalescing
  state, `BgEvent::GraphReady` handling, `AppRequest::RefreshGraph` arm in
  `service_simple`, test pump helper.
- **`ft/src/tui/event.rs`** — new `BgEvent` variant.
- **`ft/src/tui/tab.rs`** — `TabCtx` gains the snapshot; possibly a
  `Tab::on_graph_ready` hook (default no-op).
- **Tabs** — `tabs/graph.rs` (largest: ~12 build sites + owned
  `Option<Graph>` field), `tabs/tasks/search.rs`, `tabs/journal.rs`,
  `tabs/review.rs`, `tui/modal.rs` (search picker).
- **Tests** — TUI snapshot tests that mutate-then-assert need one pump call
  inserted; this is the main churn (budgeted in tasks).
- **No `ft-core` API changes** and no CLI changes — the CLI keeps its
  per-invocation `scan → build`.
- Unblocks: file watching, incremental `refresh_note` adoption, and
  moving Journal/Review git computation off-thread (out of scope here).

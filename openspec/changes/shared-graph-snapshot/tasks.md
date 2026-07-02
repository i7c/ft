# Tasks — shared-graph-snapshot

Keep all five build invariants green after every numbered section
(`cargo build --release`, `cargo test --workspace`, clippy `-D warnings`,
`cargo fmt --check`, `ft commands docs --check`). Migration order follows
design.md §7 — each section leaves the app working with a mix of migrated
and unmigrated tabs.

## 1. Snapshot infrastructure

- [x] 1.1 Add `GraphSnapshot { generation, scan, graph }` (in
      `ft/src/tui/` — it is a TUI concern, not ft-core) and the App slot
      `graph_snapshot: RefCell<Option<Arc<GraphSnapshot>>>`
- [x] 1.2 Add `GraphJob { in_flight, dirty, next_generation }` state and
      `App::request_graph_rebuild()` with single-flight + dirty coalescing
- [x] 1.3 Add the worker (`run_graph_job`: owns `Arc<Vault>` + generation,
      runs `vault.scan()` + `Graph::build`, posts one
      `BgEvent::GraphReady(Result<GraphSnapshot, String>)`) mirroring
      `run_sync_job`, including panic containment
- [x] 1.4 Handle `BgEvent::GraphReady` in `handle_background`: install the
      snapshot, bump generation bookkeeping, call the active tab's
      `on_graph_ready`, re-spawn if dirty; on `Err`, keep the old snapshot
      and toast
- [x] 1.5 Add `AppRequest::RefreshGraph` with its one arm in
      `service_simple`; post it from `run()` startup and from
      `apply_sync_result` / `apply_commit_result` and the editor-return
      path
- [x] 1.6 Add `TabCtx::snapshot: Option<Arc<GraphSnapshot>>` (fill at every
      TabCtx construction site) and the `Tab::on_graph_ready` default
      no-op hook
- [x] 1.7 Add `App::pump_graph_rebuild_for_test()` (synchronous scan+build
      delivered through the same GraphReady handler) and make
      `App::for_test*` constructors pump once so read-only tests see a
      ready snapshot
- [x] 1.8 Unit tests for the lifecycle: coalescing (N requests → ≤2
      builds), failed build keeps previous snapshot + toasts, generation
      monotonicity, pump determinism

## 2. GraphTab migration (reference implementation)

- [x] 2.1 Replace `GraphTab.graph: Option<Graph>` reads with the ctx
      snapshot; add `seen_generation` and re-derive views
      (`restore_all_views`) on generation change in `on_focus` /
      `on_graph_ready`
- [x] 2.2 Convert the ~12 mutation-site rebuilds (`scan(); Graph::build`)
      into `RefreshGraph` posts with pending cursor anchors (`NodeKey` /
      `(path, line)`) resolved in `on_graph_ready`
- [x] 2.3 Render the loading placeholder when `ctx.snapshot` is `None`
- [x] 2.4 Update GraphTab tests: insert `pump_graph_rebuild_for_test()` in
      mutate-then-assert flows; add a snapshot test for the loading frame
      and one for cursor-anchor restore after an async rebuild

## 3. Tasks tab migration

- [x] 3.1 Replace `search.rs` `reload()`'s scan+build with snapshot reads:
      tasks from `snapshot.scan.tasks`, query mapping against
      `snapshot.graph`, `seen_generation` re-derive
- [x] 3.2 Post `RefreshGraph` from task mutations (complete/cancel/edit/
      create/quickline) instead of `refresh_after_mutation`'s manual
      reload; keep cursor anchoring via the pending-anchor pattern
- [x] 3.3 Update Tasks-tab tests with the pump; verify the
      stale-guard path end to end (mutation with stale line →
      `LineChanged` toast, no file change)

## 4. Journal + Review tabs migration

- [x] 4.1 Journal: drop the three inline `Graph::build` sites; resolve
      sources against `ctx.snapshot.graph` (git/blame compute unchanged);
      re-derive on generation change in `on_focus`
- [x] 4.2 Review: drop the two inline builds; `compute_link_review`
      against the snapshot graph; reload key posts `RefreshGraph`
- [x] 4.3 Update both tabs' tests with the pump where needed

## 5. Modal + stragglers

- [x] 5.1 Search-picker modal (`modal.rs`) reads `ctx.snapshot` instead of
      building; handle the `None` case with an inline "graph loading"
      message
- [x] 5.2 Sweep: `rg "Graph::build|vault.scan\(\)" ft/src/tui` shows only
      the worker, the pump, and test code; delete any dead per-tab
      graph/scan fields

## 6. Docs + close-out

- [x] 6.1 Update `docs/architecture.md`: concurrency section (graph job
      alongside git jobs), tab recipe (read the snapshot, never build),
      and the modal/table recipes if touched
- [x] 6.2 Update `docs/graph-semantics.md` build-invariants note (TUI
      builds happen on the rebuild worker) and `CLAUDE.md`'s TUI bullet if
      wording changed
- [x] 6.3 Re-run `ft commands docs > docs/keybindings.md` if any
      `CommandDef`/keymap changed; full five-invariant pass; update the
      architecture-review doc's finding 1 with a "fixed in" pointer

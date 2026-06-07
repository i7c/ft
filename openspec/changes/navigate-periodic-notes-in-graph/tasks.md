## 1. GraphTab: BFS and navigation plumbing

- [x] 1.1 Add `find_node_path(&self, graph: &Graph, query: &GraphQuery, target: NoteId) -> Option<Vec<NoteId>>` private method to `GraphTab` — BFS from `query.select(graph)` roots using `query.expand(graph, id)` as successor, returning the shortest path on first hit. Reuse visited-set pattern from `collect_search_candidates`.
- [x] 1.2 Add `AppRequest::GraphNavigatePeriodic(Period)` variant to the `AppRequest` enum in `ft/src/tui/tab.rs`, add a `Display` arm, and add a no-op default for it in the monolithic match arms.
- [x] 1.3 Add `fn graph_navigate_periodic(&mut self, ctx: &TabCtx, period: Period)` to the `Tab` trait with a default no-op body, and add a `Tab::graph_navigate_periodic` dispatch arm in `App::service_request` (same pattern as `graph_jump_to_nodes`).
- [x] 1.4 Implement `graph_navigate_periodic` in `GraphTab`: resolve the periodic note's path via `ft_core::periodic::resolve_periodic_path`, convert to vault-relative, look up `NoteId` in graph via `note_by_path`, call `find_node_path`, and either `jump_to_path` on success or queue a toast on failure.
- [x] 1.5 Add a new `Periodic` variant to `ActiveModal` or convert `PeriodicLeader` to post an `AppRequest` — whichever is cleaner. The `PeriodicLeader` modal's `handle_event` currently calls `run_periodic_open` directly; it needs to instead post `AppRequest::GraphNavigatePeriodic(Period)` through `ctx.pending_request`.

## 2. Wire periodic keybindings to navigation

- [x] 2.1 Replace `run_periodic_open(ctx, Period::Daily)` with `graph_navigate_periodic` call in `GraphTab::dispatch_command` for `graph.today`.
- [x] 2.2 Replace `run_periodic_open(ctx, p)` with `app_request` posting in the `PeriodicLeader` modal handler (or in the dispatch path that resolves the modal outcome, depending on approach chosen in 1.5).
- [x] 2.3 Update the Periodic notes section in the `GRAPH_COMMANDS` descriptions and the `?` help overlay (`Tab::help_sections`) to say "navigate" instead of "open".
- [x] 2.4 Run `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check` and fix any issues.

## 3. Tests

- [x] 3.1 Add a TUI snapshot test in `ft/src/tui/tests.rs` for `t` (today) on a graph view that reaches the daily note — verify the tree cursor lands on the daily note.
- [x] 3.2 Add a TUI snapshot test for `t` when the daily note is not in the current query's results — verify a toast appears and tree is unchanged.
- [x] 3.3 Add a TUI snapshot test for the periodic leader chord (`p` then `d`) navigating to a daily note.
- [x] 3.4 Add a unit test for `find_node_path` in the `GraphTab` test module, verifying: reachable target → Some(path), unreachable target → None, multi-parent → shortest path.

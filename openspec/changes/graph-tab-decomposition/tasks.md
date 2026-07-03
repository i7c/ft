## 1. Routing collapse: `GraphRequest` + `handle_graph_request`

- [x] 1.1 Add `GraphRequest` enum to `ft/src/tui/tab.rs` with one variant
      per current `AppRequest::Graph*` payload (`JumpToNodes`,
      `ApplyPreset`, `FocusQueryBar`, `CommitRename`, `ConfirmRelated`,
      `QueryBarKey`, `ApplyQueryBar`, `MoveConfirmSourceFromTree`,
      `MoveConfirmTargetFromTree`, `MoveConfirmMoveTarget`,
      `MoveExecuteMultiMove`, `NavigatePeriodic`, `ConfirmDelete`,
      `CreateSubdir`, `TaskEdit`, `TaskCommitCreate`), reusing the exact
      field shapes from the corresponding `AppRequest::Graph*` variant
      being replaced.
- [x] 1.2 Add `AppRequest::Graph(GraphRequest)`; remove the sixteen
      `AppRequest::Graph*` variants it replaces.
- [x] 1.3 Update `AppRequest`'s manual `Debug` impl: drop the arms for
      the removed variants, add one arm for `Graph(GraphRequest)`
      (delegate to `GraphRequest`'s own `#[derive(Debug)]`, added in the
      same step).
- [x] 1.4 Add `Tab::handle_graph_request(&mut self, req: GraphRequest,
      ctx: &mut TabCtx)` to the `Tab` trait with a no-op default body;
      remove the sixteen per-action `graph_*` methods it replaces
      (`graph_jump_to_nodes`, `graph_apply_preset`, `graph_focus_query_bar`,
      `graph_commit_rename`, `graph_confirm_related`,
      `graph_query_bar_key`, `graph_apply_query_bar`,
      `graph_move_confirm_source_from_tree`,
      `graph_move_confirm_target_from_tree`,
      `graph_move_confirm_move_target`, `graph_move_execute_multi_move`,
      `graph_navigate_periodic`, `graph_confirm_delete`,
      `graph_create_subdir`, `graph_task_edit`,
      `graph_task_commit_create`).
- [x] 1.5 Implement `GraphTab::handle_graph_request` as a match over
      `GraphRequest`, each arm calling the same private helper the
      removed dedicated method called (verbatim body move, no logic
      changes).
- [x] 1.6 Collapse `App::service_simple`'s sixteen `Graph*` arms
      (`ft/src/tui/app.rs`) into one arm: look up the tab with
      `kind() == TabKind::Graph`, call `handle_graph_request`.
- [x] 1.7 Update every construction site in
      `ft/src/tui/tabs/graph/modals.rs` (and any other modal files) that
      builds a removed `AppRequest::Graph*` variant to build
      `AppRequest::Graph(GraphRequest::*)` instead.
- [x] 1.8 Fix every remaining compile error from the trait/enum
      collapse (exhaustive match arms elsewhere, any other call sites).
- [x] 1.9 Update test assertions across `ft/src/tui/tests/*.rs` (and
      any other test files) matching on removed `AppRequest::Graph*`
      variants to match `AppRequest::Graph(GraphRequest::*)` instead —
      mechanical pattern update only, no test-logic changes.
- [x] 1.10 Run `cargo build --release`, `cargo test --workspace`,
      `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt
      --check`; fix fallout from this step before proceeding.

## 2. File split: `tabs/graph/` per-concern modules

- [x] 2.1 Create `ft/src/tui/tabs/graph/render.rs`; move `GraphTab::render`
      and its private rendering-only helpers out of `mod.rs` verbatim.
      (Landed as an inherent `render_impl` method; `Tab::render` in
      `mod.rs` is a one-line delegate — a single `impl Tab for GraphTab`
      block can't span files.)
- [x] 2.2 Create `ft/src/tui/tabs/graph/mutations.rs`; move the
      rename/move/delete/subdir commit helpers (the bodies reached from
      the `CommitRename`, `Move*`, `ConfirmDelete`, `CreateSubdir` arms
      added in 1.5) out of `mod.rs` verbatim.
- [x] 2.3 Create `ft/src/tui/tabs/graph/tasks.rs`; move the task-popup
      edit/create commit helpers (the bodies reached from the
      `TaskEdit`/`TaskCommitCreate` arms) out of `mod.rs` verbatim.
- [x] 2.4 Create `ft/src/tui/tabs/graph/dispatch.rs` (renamed from the
      `commands.rs` the proposal originally named — that filename was
      already taken by the pre-existing `GRAPH_COMMANDS`/`GRAPH_KEYMAP`
      registry file, a declarative-vs-handler split that is arguably
      cleaner than the original plan); move `dispatch_command` out of
      `mod.rs`, splitting its ~690-line match into five grouped
      `&mut self` helper methods (view/cross-tab, query/navigation,
      notes/task-interaction, mutation flows, periodic/multi-select)
      chained by `_ => self.next_group(...)` fallthrough, called from a
      thin `dispatch_command_impl` entry point. No behavior change —
      each command name still resolves to exactly the same code path it
      did before; internal `return CommandOutcome::Handled` early-returns
      were left untouched since they now return from their own (smaller)
      function.
- [x] 2.5 Leave `GraphTab` struct definition, `new()`, `Tab` impl surface
      (method signatures delegating into the sibling modules), and any
      remaining small view-switch plumbing in `mod.rs`.
- [x] 2.6 Fix visibility (`pub(super)`/`pub(crate)`) and `use` paths
      across the new sibling modules so the split compiles without
      widening any type's visibility beyond `tabs::graph`. (Every
      relocated method marked `pub(super)` uniformly, since most are
      called both from `mod.rs` and from sibling modules like
      `dispatch.rs`.)
- [x] 2.7 Run `cargo build --release`, `cargo test --workspace`,
      `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt
      --check`; confirm zero snapshot diffs (this step is a pure move,
      no snapshot should change). All green; test counts identical
      before/after (969 in the `ft` binary's unit tests).

## 3. Close-out

- [x] 3.1 Run `cargo run --release -q -- commands docs --check` and
      confirm no drift (command names/keymaps are unchanged by this
      refactor). Exit 0, no drift.
- [x] 3.2 Grep the codebase for any remaining reference to a removed
      method/variant name (`graph_commit_rename`, `AppRequest::Graph`
      followed by a capital letter other than the new `Graph(` wrapper,
      etc.) to confirm the old surface is fully gone, not just
      unreachable.
- [x] 3.3 Spot-check `ft/src/tui/tab.rs`'s method count and
      `ft/src/tui/tabs/graph/mod.rs`'s line count against the proposal's
      stated problem (≈25 methods / 2,745 lines) to confirm the
      reduction is real.

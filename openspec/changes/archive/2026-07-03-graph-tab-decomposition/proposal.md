## Why

`ft/src/tui/tabs/graph/mod.rs` has grown to 2,745 lines — the largest file
in the codebase — because every cross-tab action the Graph tab needs to
receive (rename commit, move confirm, task edit, periodic navigation, query
bar keystrokes, …) was added as its own dedicated method on the `Tab` trait
(`tab.rs`) with a matching `AppRequest::Graph*` variant and its own arm in
`App::service_simple`. `Tab` now has ~25 methods, of which ~20 exist for
exactly one implementer (`GraphTab`); every other tab (Tasks, Notes,
Timeblocks, Journal, Review) inherits ~20 no-op defaults it will never use.
`AppRequest` carries ~20 `Graph*` variants and needs a 150-line manual
`Debug` impl to stay printable. `App::service_simple` is a ~130-line match
that is, in practice, a graph-tab dispatcher wearing an App-routing hat.
Inside `GraphTab` itself, `dispatch_command` is a single ~690-line match
covering view management, cross-tab handoffs, task mutation, rename, move,
delete, and query-bar editing.

This pattern repeats every time a new Graph-hosted modal needs to call back
into tab state, so the trait and the file keep growing along the same
fault line. Fixing the routing mechanism once, and splitting the file to
match the per-concern module layout already used by `tabs/tasks/` and
`notes_actions/`, stops the bleeding before the next modal adds method
#26.

This is a structural refactor: no user-visible behavior changes, no key
bindings change, no command names change.

## What Changes

- Replace the ~20 GraphTab-only `Tab::graph_*` hook methods with a single
  `Tab::handle_graph_request(&mut self, req: GraphRequest, ctx: &mut TabCtx)`
  method (default no-op, matching the existing default-no-op convention).
  `GraphRequest` is a new enum carrying the same payloads the removed
  `AppRequest::Graph*` variants carried.
- Collapse the ~20 `AppRequest::Graph*` variants into a single
  `AppRequest::Graph(GraphRequest)` variant. Remove the corresponding
  hand-written `Debug` arms (derive `Debug` on `GraphRequest` instead).
- Collapse the ~20 matching arms in `App::service_simple` into one arm that
  looks up the tab by `TabKind::Graph` and calls `handle_graph_request`
  once. Routing precedence and the deferred/non-deferred split (terminal-
  touching requests still bounce through `service_request`) are unchanged.
- Split `ft/src/tui/tabs/graph/mod.rs` into per-concern modules under
  `ft/src/tui/tabs/graph/` (views, dispatch/commands, mutations
  [rename/move/delete/subdir], task popups, rendering), following the
  existing `tabs/tasks/` layout. `GraphTab` stays a single struct; only its
  method bodies move.
- Break `GraphTab::dispatch_command`'s ~690-line match into per-concern
  helper functions (view commands, mutation commands, navigation commands)
  called from a thinner top-level match.
- No change to `Modal`, `ActiveModal`, modal dispatch precedence, the
  Command/Keymap registry, or the shared graph snapshot mechanism.

## Capabilities

### New Capabilities
- `tui-tab-request-routing`: the single typed `GraphRequest` payload +
  `Tab::handle_graph_request` mechanism that App uses to route Graph-
  targeted, modal-raised requests to the tab that owns `TabKind::Graph`,
  replacing the one-hook-per-action pattern.

### Modified Capabilities
- `tui-modal-driver`: the "State-touching commits route via `AppRequest`"
  requirement currently names specific `AppRequest::GraphMove*` variants
  and `Tab::graph_move_*` hooks; it is updated to describe routing through
  `AppRequest::Graph(GraphRequest::Move*)` and the single
  `handle_graph_request` hook instead, with the same host-side behavior
  (`confirm_target_from_tree`, `confirm_move_target`, `execute_multi_move`
  stay on `GraphTab`, called from the new hook's match arm).

## Impact

- `ft/src/tui/tab.rs` — trait shrinks from ~25 methods to the tab-generic
  set plus one `handle_graph_request` default; `AppRequest` enum shrinks;
  manual `Debug` impl shrinks correspondingly.
- `ft/src/tui/app.rs` — `service_simple` routing table collapses to one
  arm.
- `ft/src/tui/tabs/graph/mod.rs` and new sibling modules under
  `ft/src/tui/tabs/graph/` — file split, no new files outside that
  directory.
- `ft/src/tui/tabs/graph/modals.rs` — modal `handle_event` impls change
  which `AppRequest` variant they construct (payload move into
  `GraphRequest`), not their control flow.
- Test ripple: every existing cross-tab test that asserts on a specific
  `AppRequest::Graph*` variant (`ft/src/tui/tests/graph.rs` and friends)
  needs its assertion updated to match on `AppRequest::Graph(GraphRequest::*)`
  instead — mechanical, per CLAUDE.md's guidance on budgeting test-ripple
  cost for signature changes on core APIs.
- No change to `ft-core`, CLI, config format, or any on-disk format.
- No change to `docs/keybindings.md` content (regenerate only if
  `cargo run --release -q -- commands docs --check` reports drift, which
  is not expected since command names/bindings are unchanged).

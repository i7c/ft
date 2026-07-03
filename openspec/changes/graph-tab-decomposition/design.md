## Context

The TUI's cross-tab routing grew organically: each time a Graph-hosted
modal needed to commit a state change back into `GraphTab` (rename, move,
delete, task edit, periodic-nav, query-bar keystrokes, …), the pattern
used was "add a default-no-op method to `Tab`, add a matching
`AppRequest::Graph*` variant, add an arm to `App::service_simple`." That
precedent is documented in `docs/architecture.md` under "App ↔ Tab
routing" as *the* recipe for adding a new modal action, so every new
Graph-hosted modal reinforces it. Twenty-some iterations later, `Tab` (in
`ft/src/tui/tab.rs`) carries ~25 methods, ~20 of which are meaningful only
for `GraphTab`; `AppRequest` needs a 150-line hand-written `Debug` impl;
`App::service_simple` is a ~130-line match that is really a graph-tab
dispatcher; and `GraphTab` itself (`ft/src/tui/tabs/graph/mod.rs`) is
2,745 lines, the largest file in the codebase, with a single 690-line
`dispatch_command` match.

Other tabs (Tasks, Notes, Timeblocks, Journal, Review) never got this
treatment because they don't host complex multi-step modals the way
Graph does — but they still pay the cost of ~20 inherited no-op trait
methods. The `Modal` trait sidesteps this same trap: modals communicate
outward only via `AppRequest`/`ModalOutcome`, never by growing new methods
on `Modal` itself. `Tab` needs the equivalent discipline for its inbound
side.

This is purely a routing-mechanism and file-layout refactor. It changes no
key bindings, no command names, no on-disk format, and no `ft-core` API.

## Goals / Non-Goals

**Goals:**
- Collapse the ~20 `Tab::graph_*` hook methods into one
  `Tab::handle_graph_request(&mut self, req: GraphRequest, ctx: &mut TabCtx)`
  method, default no-op, so tabs other than Graph carry exactly one extra
  method instead of twenty.
- Collapse the ~20 `AppRequest::Graph*` variants into
  `AppRequest::Graph(GraphRequest)`, restoring `#[derive(Debug)]` on the
  common path (only the genuinely non-`Debug` payloads — `ActiveModal` in
  `OpenModal`/`OpenModalWithToast` — keep a manual arm).
- Collapse `App::service_simple`'s ~20 `Graph*` arms into one arm.
- Split `GraphTab`'s implementation across sibling modules under
  `ft/src/tui/tabs/graph/`, mirroring the `tabs/tasks/` layout
  (`mod.rs` holds the struct + `Tab` impl surface; concerns live in
  named siblings).
- Preserve every existing behavior, snapshot test, and command/keybinding
  exactly. This is a mechanical extraction, not a rewrite.

**Non-Goals:**
- Not changing `Modal`/`ActiveModal`/modal dispatch precedence.
- Not changing the Command/Keymap registry, the shared graph snapshot
  mechanism, or any other tab's structure.
- Not introducing a generic "any tab can receive any typed request" bus.
  `GraphRequest` is still Graph-specific by name — the fix is arity (one
  hook, one enum) not genericity (a downcast-based any-tab message bus
  would be a bigger, riskier change for no present benefit: there is
  exactly one tab, Graph, that other components route typed requests
  into).

## Decisions

### One `GraphRequest` enum, one `Tab::handle_graph_request` hook

`GraphRequest` is a new enum in `ft/src/tui/tab.rs` with one variant per
current `AppRequest::Graph*` payload (`JumpToNodes`, `ApplyPreset`,
`FocusQueryBar`, `CommitRename`, `ConfirmRelated`, `QueryBarKey`,
`ApplyQueryBar`, `MoveConfirmSourceFromTree`, `MoveConfirmTargetFromTree`,
`MoveConfirmMoveTarget`, `MoveExecuteMultiMove`, `NavigatePeriodic`,
`ConfirmDelete`, `CreateSubdir`, `TaskEdit`, `TaskCommitCreate`) — a
straight rename/regroup of the existing payload shapes, not a redesign.
`AppRequest` gets one new variant, `Graph(GraphRequest)`, replacing all
sixteen `Graph*` variants above (`OpenModal`, `OpenModalWithToast`,
`Toast`, `Journal*`, `SyncGit`, `CommitGit`, `OpenInEditor`,
`OpenInObsidian` are untouched — they were never part of the
graph-hook-sprawl problem).

`Tab::handle_graph_request(&mut self, req: GraphRequest, ctx: &mut TabCtx)`
replaces the sixteen `graph_*` methods. Default body is a no-op (matching
the existing convention for hooks other tabs ignore). `GraphTab`
implements it as one match over `GraphRequest` variants, each arm calling
the same private helper method the old dedicated hook called (e.g.
`GraphRequest::CommitRename { .. } => self.commit_rename(...)`) — the
helpers themselves are untouched, only the dispatch surface collapses.

**Alternative considered**: keep per-action methods but move them to a
new `GraphHost` trait implemented only by `GraphTab`, with `App` doing a
runtime downcast (`&mut dyn Any`) from `&mut dyn Tab` to `&mut dyn
GraphHost`. Rejected: downcasting through `dyn Any` in a codebase that has
otherwise avoided `Any`/dynamic typing everywhere else (CLAUDE.md's
"TUI concurrency" and "Modal driver" sections both favor typed enums over
dynamic dispatch) would be a new pattern for no gain over routing-by-
`TabKind`, which the App already does today via `service_simple`'s tab
lookup.

### `App::service_simple` collapses to one arm

The existing tab-lookup-by-`TabKind::Graph` logic (shared across all
sixteen current arms) is factored into one helper the single new arm
calls: find the tab with `kind() == TabKind::Graph`, call
`tab.handle_graph_request(req, ctx)`. Deferred (terminal-touching)
variants (`OpenInEditor`, `OpenInObsidian`, `SyncGit`, `CommitGit`) keep
their existing separate handling in `service_request` — unaffected by
this change since they were never `Graph*` variants.

### File split mirrors `tabs/tasks/`

`ft/src/tui/tabs/graph/mod.rs` (2,745 lines) splits into:
- `mod.rs` — `GraphTab` struct, `Tab` impl surface (thin: delegates to
  helpers in siblings), `new()`, view-switch plumbing kept inline since
  it's small.
- `commands.rs` — `dispatch_command` and its per-concern helper functions
  (views / mutations / navigation), replacing the single 690-line match
  with a thinner top-level match that calls into grouped helper fns.
- `mutations.rs` — rename/move/delete/subdir commit logic (the bodies
  currently reached via `graph_commit_rename`, `graph_move_*`,
  `graph_confirm_delete`, `graph_create_subdir`).
- `tasks.rs` — task-popup edit/create commit logic (`graph_task_edit`,
  `graph_task_commit_create` bodies) — named to avoid clashing with the
  existing `tabs/tasks/` module; lives under `tabs/graph/` so it's
  unambiguous in `use` paths (`tabs::graph::tasks` vs `tabs::tasks`).
- `render.rs` — the `render()` body and its private helpers.

This is the same shape `tabs/tasks/` (`mod.rs`, `quickline.rs`,
`edit_popup.rs`, `search.rs`) and `notes_actions/` already use — no new
pattern, just applying the existing one to the one tab that skipped it.

**Alternative considered**: leave `GraphTab` in one file and only fix the
trait/enum routing. Rejected: the routing fix alone doesn't touch
`dispatch_command`'s 690-line match or the file's total size — the
proposal's stated problem ("largest file in the codebase") would remain
unsolved, and the next Graph feature would still land in one giant match.

## Risks / Trade-offs

- **[Risk] Mechanical rename touches every call site of a removed
  `graph_*` method or `AppRequest::Graph*` variant, across production code
  and tests.** → Mitigation: do the enum/trait collapse first as one
  commit-sized unit (compiler enforces exhaustiveness — every call site
  that doesn't compile is a call site that needs updating, so nothing is
  silently missed), then do the file split as a second, purely-internal-
  to-`graph/` unit. Two units bound the blast radius per step.
- **[Risk] Test ripple.** Tests asserting `matches!(req,
  AppRequest::GraphCommitRename { .. })` etc. (in
  `ft/src/tui/tests/graph.rs` and cross-tab tests) need updating to
  `AppRequest::Graph(GraphRequest::CommitRename { .. })`. → Mitigation:
  purely mechanical pattern update, no test logic changes; budgeted
  explicitly in tasks.md per CLAUDE.md's guidance on signature-change
  test-ripple cost.
- **[Risk] Splitting `dispatch_command`'s match across files risks
  behavior drift if a helper's borrow-checker context changes (e.g. a
  helper that used to close over locals now takes `&mut self` and loses
  access to a `vis` local computed once at the top of the old function).**
  → Mitigation: keep per-concern helpers as `&mut self` methods on
  `GraphTab` (not free functions), so they have the same access to `self`
  fields the inline match arms had; recompute small locals like `vis`
  inside each helper rather than threading them as parameters.
- **[Trade-off] `GraphRequest` is still Graph-specific, not a generic
  any-tab bus.** Accepted per Non-Goals — genericity here would cost more
  (dynamic dispatch or a trait-object registry) than it buys, since Graph
  is the only tab with this shape of inbound cross-tab request today. If
  a second tab ever needs the same treatment, revisit with two data
  points instead of speculating on one.

## Migration Plan

1. Swap the routing mechanism in one atomic step rather than running old
   and new in parallel — an intermediate "both exist" state has no test
   coverage of its own and just doubles the surface temporarily. Single
   commit: add `GraphRequest` + `handle_graph_request`, delete the
   sixteen old methods/variants/arms, fix every resulting compile error
   (the compiler's exhaustiveness checking surfaces every call site that
   needs updating).
2. Update `ft/src/tui/tabs/graph/modals.rs` construction sites (where
   `AppRequest::Graph*` values are built) to build `AppRequest::Graph(
   GraphRequest::*)` instead.
3. Update test assertions across `ft/src/tui/tests/*.rs` to match the new
   shape.
4. Run full build/test/clippy/fmt gate; fix fallout.
5. Split `graph/mod.rs` into the sibling modules described above, moving
   method bodies verbatim (no logic edits) and adjusting `use`/visibility.
6. Re-run the full gate plus `cargo run --release -q -- commands docs
   --check` (expected no-op, since no `CommandDef`/keymap changed).
7. No rollback complexity beyond normal revert: this is a single-binary,
   no-persisted-state, no-migration refactor. If step 5 destabilizes
   review, it can be reverted independently of steps 1-4 (they are
   separable commits) since the file split doesn't depend on call sites
   outside `graph/`.

## Open Questions

- None blocking. The one design choice worth revisiting later — whether
  `GraphRequest` should become a generic per-`TabKind` request bus if a
  second tab grows the same need — is deliberately deferred (see
  Non-Goals) rather than an open question to resolve now.

# Design — shared-graph-snapshot

## Context

Post sessions B and C, the pieces this change needs are in place:
`vault.scan()` is a single parallel read pass whose `Scan::files` feeds
`Graph::build` with zero additional I/O, and all cross-tab requests route
through one table (`App::service_simple`) with typed tab lookup
(`Tab::kind()`). The TUI still builds graphs per tab, synchronously, at
~20 call sites: GraphTab owns an `Option<Graph>` field and rebuilds it
after every mutation; Journal, Review, and Tasks-search tabs and the
search-picker modal each run `scan → build` inline in `on_focus` / key
handlers.

The background-worker pattern to copy is the git-sync job
(`jobs.rs` + `run_sync_job`): worker owns its inputs, posts exactly one
`BgEvent` back into the shared mpsc channel, is never joined. The
existing `Tab::refresh` hook (called on the active tab after sync
completion) and `NodeKey` (build-independent node identity, already used
by GraphTab's `restore_all_views`) are the seams tabs will use to survive
snapshot swaps.

Constraints: no async runtime, no `Mutex<AppState>` — single-threaded
main loop plus fire-and-forget workers. `TestBackend` snapshot tests
must stay deterministic.

## Goals / Non-Goals

**Goals:**

- One graph per App, not one per tab: every TUI consumer reads the same
  snapshot.
- No `vault.scan()` or `Graph::build` on the UI thread (except the
  test pump).
- Mutation flows stay correct under staleness: old snapshot renders
  until the new one lands; guarded ops fail safe.
- Deterministic tests with bounded churn.

**Non-Goals:**

- File watching / inotify (future; this change makes it possible).
- Incremental rebuild via `Graph::refresh_note` (future optimization;
  full rebuild is the v1 policy).
- Moving Journal/Review *git* computation (blame, log scans) off-thread —
  they keep their synchronous compute but stop paying scan+build.
- CLI behavior — unchanged, still per-invocation `scan → build`.

## Decisions

### 1. Snapshot type: `Arc<GraphSnapshot>` carrying graph + scan + generation

```rust
pub struct GraphSnapshot {
    /// Monotonic; increments per completed build. Tabs compare against
    /// their `seen_generation` to know when to re-derive view state.
    pub generation: u64,
    pub scan: Scan,
    pub graph: Graph,
}
```

`App` holds `graph_snapshot: RefCell<Option<Arc<GraphSnapshot>>>`;
`TabCtx` gains `snapshot: Option<Arc<GraphSnapshot>>` (Arc clone per
ctx construction — cheap). The snapshot is immutable once installed;
tabs never mutate a graph, they request a rebuild.

- Carrying the `Scan` gives the Tasks tab its `Vec<Task>` and keeps
  graph and task data from the same read pass (they must agree for
  (path, line) mapping).
- *Alternative — `&RefCell<...>` in TabCtx*: rejected; tabs (Tasks)
  legitimately stash the snapshot across events, and borrows out of a
  shared RefCell across tab calls invite runtime borrow panics.
- *Alternative — per-tab graphs kept, App just caches*: rejected; doesn't
  fix divergence or the "where do I get a graph" question.

### 2. Dedicated rebuild state with single-flight + dirty-flag coalescing

```rust
struct GraphJob { in_flight: bool, dirty: bool, next_generation: u64 }
```

`App::request_graph_rebuild()`: if `in_flight`, set `dirty` and return;
else spawn the worker. On `BgEvent::GraphReady`: install the snapshot,
call the active tab's `on_graph_ready`, and if `dirty`, clear it and
spawn again. Any burst of mutations costs at most one extra rebuild.

- This state is **separate from the git `jobs` slot**: a graph rebuild
  must not make `g s` toast "sync already in progress", and the two have
  different lifecycles (git = one-shot, graph = coalescing). The jobs
  slot's own doc anticipates a `HashMap` upgrade; not needed yet.
- Worker shape mirrors `run_sync_job`: owns `Arc<Vault>` + generation,
  runs `vault.scan()` + `Graph::build`, posts exactly one
  `BgEvent::GraphReady(Result<GraphSnapshot, String>)`, exits. Build
  errors surface as a toast and keep the previous snapshot.

### 3. Rebuild triggers: one request, posted from every mutating flow

New `AppRequest::RefreshGraph`, one arm in `service_simple`, which calls
`request_graph_rebuild()`. Posted by:

- startup (`run()` before the first focus — tabs render a loading state
  until the first snapshot lands);
- every mutating flow that today rebuilds inline (task edit/complete/
  create, note create/rename/delete/move, section move, capture);
- editor return (`dispatch_open_in_editor` after resume — the user
  probably edited something);
- git sync/commit completion (`apply_sync_result` posts it in addition
  to the existing `Tab::refresh` call, which non-graph tabs still use
  for file-content reloads);
- manual reload keys (`R` etc.) on graph-backed tabs.

### 4. Delivery: push to the active tab, pull-by-generation for the rest

- `Tab::on_graph_ready(&mut self, ctx: &mut TabCtx)` — default no-op,
  invoked only on the **active** tab when a snapshot installs. The
  visible tab updates immediately (restore views via `NodeKey`, resolve
  pending cursor anchors).
- Every graph-backed tab records `seen_generation: u64` and re-derives
  its view state in `on_focus` (and lazily in `handle_event`) when
  `ctx.snapshot.generation` differs. Background tabs therefore pay
  re-derivation cost only when the user returns to them.
- *Alternative — push to all tabs on every install*: rejected; wakes
  five tabs to rebuild trees nobody is looking at.

### 5. Staleness semantics: render old, guard writes, anchor async

Between a mutation and `GraphReady`, tabs render the previous snapshot.
This is safe because:

- line-addressed mutations carry the expected `Task` (session A) and
  fail with `LineChanged` + toast rather than editing a shifted line;
- flows that need read-after-write cursor placement (create task, move
  note, rename) store a **pending anchor** (`NodeKey` or `(path, line)`)
  on the tab and resolve it in `on_graph_ready` — the existing
  `restore_task_cursor`/`restore_all_views` logic, made async.

The status bar may show a subtle "graph…" indicator while a rebuild is
in flight (reuse the in-flight cell; lowest-priority slot).

### 6. Test determinism: pump helper + eager first snapshot

- `App::pump_graph_rebuild_for_test()`: if a rebuild is requested or
  in flight, run `scan → build` synchronously on the calling thread and
  deliver it through the same `GraphReady` handler (same code path, no
  thread). Mutate-then-assert tests insert one pump call between the
  mutation and the assertion.
- `App::for_test*` constructors pump once after construction so the
  hundreds of read-only snapshot tests see a ready graph and need **no
  changes** — only tests that mutate and re-assert need the pump.
- Production never calls the pump; the worker thread is the only
  non-test build site.

### 7. Migration order (one tab per PR-sized step, invariants green throughout)

1. Infrastructure: `GraphSnapshot`, App slot + job state, worker,
   `BgEvent::GraphReady`, `AppRequest::RefreshGraph`, TabCtx field,
   `on_graph_ready` hook, pump helper, loading-state convention.
2. GraphTab (reference implementation; deletes its `Option<Graph>` field
   and ~12 build sites; pending-anchor pattern established here).
3. Tasks tab (`tabs/tasks/search.rs` — snapshot replaces its
   `graph`/`tasks` fields).
4. Journal + Review tabs (drop their 5 inline builds; their git compute
   stays synchronous, unchanged).
5. Search-picker modal (`modal.rs`) reads `ctx.snapshot`.
6. Sweep: assert no `Graph::build` remains under `ft/src/tui/` outside
   the worker and tests; update `docs/architecture.md` (concurrency
   section + tab recipe) and `docs/graph-semantics.md` build notes.

## Risks / Trade-offs

- **[Test churn]** Mutate-then-assert TUI tests need a pump call; the
  suite has ~355 tests in `tests.rs` plus per-tab test modules.
  → Mitigation: eager first snapshot in `for_test` constructors confines
  churn to mutation tests (estimated dozens, not hundreds); the pump is
  one line; tasks budget a dedicated step per tab migration for it.
- **[Async cursor restore]** Cursor lands when the rebuild completes,
  not on the keystroke. On small vaults this is a few ms; on large
  vaults it's exactly the latency we're moving off the input path.
  → Mitigation: pending-anchor pattern keeps the *target* deterministic;
  tests pump so assertions are exact.
- **[Coherence window across tabs]** A mutation on tab A is invisible on
  tab B until GraphReady lands (previously B rebuilt on focus and was
  coincidentally fresh).
  → Mitigation: generation check in `on_focus` means B re-derives from
  the *newest installed* snapshot on switch; the window is one rebuild,
  same as the Graph tab's own view after a mutation.
- **[Unbounded rebuild time on huge vaults]** A rebuild can take long;
  dirty-flag coalescing prevents pile-up but the snapshot can lag many
  mutations.
  → Accepted: stale-render + guarded writes is the designed degradation;
  incremental `refresh_note` is the future fix and slots behind the same
  snapshot API.
- **[Memory]** Two snapshots alive transiently (tabs hold old Arcs while
  the new one installs) — roughly 2× graph size for a moment. Accepted.

## Open Questions

- Status-bar rebuild indicator in v1: default **no** (silent rebuilds;
  the loading state covers the only user-visible gap). Flip to yes if
  large-vault lag proves confusing during real use.
- Whether `Tab::refresh` (post-sync file reload hook) can be folded into
  `on_graph_ready` + generation checks once all tabs are migrated —
  revisit in step 6; keep both until then.

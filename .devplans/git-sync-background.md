---
id: 014
name: git-sync-background
title: Git sync: background worker + toast/modal UX
status: finished
created: 2026-05-15
updated: 2026-05-15
---

# Git sync: background worker + toast/modal UX

## Goal

Move the `g s` TUI sync off the main event loop so the user can keep
navigating, editing tasks, and opening notes while `ft_core::git::sync()`
runs. On success, surface the outcome as a status-bar toast (no modal).
On conflict or error, raise the modal that already exists from plan 012.

Concretely:

1. Pressing `g s` spawns a background thread that owns the sync
   subprocess chain (`add → commit → pull → push`) and posts a single
   `SyncCompleted(Result<SyncOutcome, …>)` message back into the
   existing TUI event channel when finished.
2. The main loop stays responsive throughout. A subtle in-flight
   indicator (status-bar segment or footer mark) tells the user a
   sync is running; no modal overlay obscures the work area.
3. When the worker's completion message arrives, the main loop
   maps the outcome to either a toast (clean / synced) or the
   `SyncConflict` modal (merge / rebase conflict) or an error toast.
4. The active tab is refreshed once the worker reports back so
   pulled-in changes show up.

This plan also establishes the project's first documented
**concurrency model** (see `## Concurrency Model` below). `ft` has
been intentionally single-threaded with one producer thread (the
crossterm reader); this is the first plan that adds a *second*
producer pattern. Future plans (file watchers, async indexers,
schedule-driven autosync) will follow the same shape.

## Motivation and Context

Plan 012 shipped `g s` with a deliberate v1 simplification: the sync
runs synchronously on the main thread, the event loop blocks for its
entire duration, and a "Syncing…" modal covers the work area until it
returns. The technical notes in 012 flagged this as v1-only and called
out a v2 "phased progress UI on a background thread" as the natural
follow-up.

Real usage exposed two problems sooner than expected:

1. **The modal is intrusive.** Most syncs are sub-second on a clean
   vault, but a single ~200ms remote round-trip is still long enough
   that the modal flashes — a flicker more annoying than informative.
   On a slow network it locks up the TUI for whole seconds; the user
   can't even read the file they were about to edit while the push
   completes.
2. **The block prevents recovery.** If the sync is taking unexpectedly
   long, the user has no way to inspect *why* without killing ft. With
   a background worker, the rest of the TUI stays live; they can open
   a terminal multiplexer pane, check network, or just keep working.

The fix is small but touches the part of the codebase we've kept
deliberately simple, so it's worth doing carefully and documenting the
pattern. After this plan there will be a concrete recipe for "run a
job off the main thread, post the result back into the event channel,
render the outcome" — applicable to every future async-flavored
feature without needing to introduce tokio or rewire the event loop.

**Why not introduce an async runtime now.** The whole TUI is built
on a blocking `mpsc::Receiver::recv()` loop. Swapping in tokio would
mean rewriting the event loop, the input thread, every tab's
synchronous render path, and the test harness — for a feature whose
total off-thread work is one subprocess chain. `std::thread::spawn` +
the existing `mpsc` channel is exactly the right size of hammer.

**Why post results into the existing event channel rather than a new
one.** The main loop already has a single source of truth for "what
happens next" (`EventStream::next()`). A second receiver would force
`select!`-style multiplexing and add a code path for "no event
arrived but a result did," whose interaction with the 1s tick is
non-obvious. Adding a `Background(BgEvent)` variant to the existing
`Event` enum keeps the loop shape unchanged: one `recv()`, one match.

## Concurrency Model

This section is durable documentation, not just a session preamble —
the acceptance criteria below mirror it into `docs/architecture.md`.

### Current state (pre-plan)

- **One producer thread.** `event.rs::crossterm_loop` reads from
  stdin via `crossterm::event::poll`/`read` on a 1s cadence and
  sends `Event::{Key,Mouse,Resize,Tick}` onto an `mpsc::channel`.
  The thread owns no app state — it's a pure producer.
- **Single-threaded main loop.** `App` lives entirely on the main
  thread, blocking on `EventStream::next()` between frames.
- **`Arc<T>` for read-only sharing.** `Arc<Vault>` and
  `Arc<RecentsLog>` flow into widgets and `TabCtx`. They are
  *not* `Mutex`-protected — they're cloned read handles.
- **`RefCell<Option<T>>` for single-threaded "slot" state.**
  `pending_request`, `toast`, `sync_conflict`. Single-threaded so
  no lock contention; `Option` so they can be drained and replaced
  atomically *for the renderer* (no torn reads possible without
  multiple threads).
- **No async runtime.** No tokio, no `async fn` anywhere. The only
  `Mutex<()>` in the test layer is `EDITOR_ENV_LOCK`, which exists
  solely because process env vars are global state across parallel
  tests.

### The pattern this plan establishes

For any operation that today blocks the main loop and might want to
run in the background later (git sync, file watching, search
indexing, schedule-driven autosync, an HTTP fetch), the canonical
shape is:

1. **Worker thread owns its inputs.** Move `Arc<Vault>`, `PathBuf`,
   `SyncOptions`, etc. into a `move` closure spawned with
   `std::thread::spawn`. No borrows cross the thread boundary —
   the `Send` constraint enforces this at compile time.
2. **Result is posted back via the existing event channel.** The
   worker holds a `Sender<Event>` clone (vending one is a new
   `EventStream::sender()` method). It calls `tx.send(Event::Background(BgEvent::…))`
   exactly once and then drops out.
3. **The main loop matches the new event variant just like a
   keystroke.** No `select!`, no polling, no second receiver. The
   1s tick continues to drive expiry of toasts; the worker can
   take any wall-clock duration without starving redraws.
4. **In-flight state lives on `App`.** A typed `RefCell<Option<JobHandle>>`
   (or a `Mode::*` variant if the UI shows it) tracks "is a job
   running?" so the main loop can refuse re-entrant submissions
   ("sync already in progress" toast) and the renderer can show
   an indicator.
5. **Cancellation is cooperative and opt-in.** v1 jobs don't cancel
   — they're short. When v2 needs it, share an
   `Arc<AtomicBool>::cancel` flag the worker checks between
   phases. Never `thread.kill()`.
6. **Quit handshake.** If `should_quit` flips while a job is in
   flight, the main loop drains pending events for up to 250ms,
   then returns. The OS reaps the orphaned thread. We do **not**
   `join()` indefinitely — a stuck `git push` over a dead network
   should never block ft from quitting. (Documented; not a code
   change in v1 since the worker is short-lived.)
7. **Drop = abandon, not abort.** Dropping the result `Receiver`
   side never harms the worker — it just means the worker's
   `Sender::send` returns `Err` and the worker exits. This is
   correct: by the time we're dropping the receiver, we're tearing
   down the app.

### What this plan does *not* change

- The crossterm input thread is unchanged.
- The synchronous render path is unchanged.
- `Arc<Vault>` / `Arc<RecentsLog>` stay read-only `Arc`s; this
  plan does not introduce `Arc<Mutex<Vault>>` or any other locking
  on shared mutable state. The worker only *reads* the vault path
  out of the `Arc` and `git`-shells out to do work on the
  filesystem — no in-process vault mutation.
- The CLI (`ft git sync`) stays fully synchronous. There is no
  reason for `cmd::git::run` to thread itself; it has nothing else
  to do while waiting. **Only the TUI gets the background
  treatment.**
- `ft_core::git::sync()` itself stays synchronous. Pushing
  threading down into the library would be a leaky abstraction —
  the library is the unit of work, the TUI is the unit of
  coordination.

### Future-proofing notes (not in this plan)

- **`BgEvent` as a sum type.** The new `Event::Background(BgEvent)`
  variant is intentionally an enum so future plans (file watchers,
  fuzzy-index rebuilds) can add variants without rewriring the
  event loop again.
- **Multiple in-flight jobs.** If a future plan needs more than one
  concurrent job (e.g. a fetch while a sync runs), the
  `RefCell<Option<JobHandle>>` becomes a `HashMap<JobId,
  JobHandle>` and `BgEvent` variants gain a `JobId`. Out of scope
  here.

## Acceptance Criteria

### Library — `ft-core` (no changes)

- [ ] `ft_core::git::sync()` remains synchronous. No new `tokio`,
      `rayon`, or async surface in `ft-core`. The worker thread
      lives entirely in the `ft` binary's TUI layer.

### TUI — event channel plumbing

- [ ] `EventStream::sender(&self) -> Sender<Event>` returns a
      cloneable handle to the internal channel. The internal
      `_tx` field changes from `Sender<Event>` to be exposed via
      this accessor (rename / unprefix as needed; the leading
      underscore was a "kept alive but unused" marker, no longer
      true).
- [ ] New `Event::Background(BgEvent)` variant. `BgEvent` is a new
      enum in `event.rs`:
      ```rust
      #[derive(Debug)]
      pub enum BgEvent {
          SyncCompleted(SyncJobResult),
      }

      #[derive(Debug)]
      pub struct SyncJobResult {
          /// `Ok` for any defined outcome (clean, synced, or
          /// conflicted). `Err` for hard failures (no upstream,
          /// push rejected, network errors).
          pub outcome: Result<ft_core::git::SyncOutcome, String>,
          /// Walked at job-submission time so it doesn't depend
          /// on the live App vault path.
          pub repo: std::path::PathBuf,
      }
      ```
      `SyncOutcome` is `Debug + Clone` already from plan 012;
      anyhow's `Error` isn't `Send + 'static`-friendly in this
      shape, so we stringify with `format!("{e:#}")` at the
      worker site. The richer chain is logged via `eprintln!`
      already by the existing CLI; the TUI surface needs only the
      top-line message.

### TUI — job lifecycle

- [ ] New `JobHandle` type in `tui/jobs.rs`:
      ```rust
      pub struct JobHandle {
          pub kind: JobKind,
          /// Wall-clock start, used to render "(syncing for 2s)"
          /// once we exceed a threshold worth showing.
          pub started_at: std::time::Instant,
      }

      pub enum JobKind {
          Sync,
      }
      ```
- [ ] `App` grows `jobs: RefCell<Option<JobHandle>>` (single-slot
      v1 — only one concurrent job kind exists). The existing
      `sync_conflict: RefCell<Option<SyncConflictInfo>>` stays as
      it is.
- [ ] `Mode` changes:
      - Remove `Mode::Syncing` (the blocking-modal variant from
        plan 012). The "sync is happening" indicator moves into a
        status-bar cell, not a mode.
      - Keep `Mode::SyncConflict` exactly as it is — that's still
        a modal-blocking screen.
      - Keep `Mode::GitLeader` — chord entry is unchanged.

### TUI — `g s` dispatch

- [ ] `App::dispatch_sync_git` rewires:
      1. If `jobs.borrow().is_some()` (a sync is already running),
         push a toast `"sync already in progress"` and return.
         Do **not** discover, do **not** spawn.
      2. `discover_repo(&vault.path)`. `None` → red toast
         `"no git repository at or above vault root"`, no spawn.
         (Same as today.)
      3. Build `SyncOptions { strategy: vault.config.config.git.pull_strategy,
                              message }` (TUI always sends `None`).
      4. Clone the channel sender via `EventStream::sender()`.
      5. Set `jobs = Some(JobHandle { kind: JobKind::Sync,
                                       started_at: Instant::now() })`.
      6. `std::thread::spawn(move || run_sync_job(repo, opts, tx))`
         where `run_sync_job` runs `ft_core::git::sync`, maps the
         result into `SyncJobResult`, and sends a single
         `Event::Background(BgEvent::SyncCompleted(_))`. Send
         errors (channel closed) are swallowed — app is gone.
      7. Push a *short* informational toast (`"syncing in
         background…"`) — different from v1's modal. Style:
         `ToastStyle::Info`, the existing variant.
      8. Return immediately; the main loop keeps spinning.
- [ ] Status-bar in-flight indicator. The right cell of the
      status bar (today shows `Mode::label()`) renders an extra
      glyph `"⟳ sync"` (Unicode `U+27F3` clockwise gapped circle
      arrow) in `ToastStyle::Info` color when
      `jobs.borrow().is_some()`. The glyph survives mode changes
      (you can drop into help, search, picker — the indicator
      stays). Why this glyph: it's already in the project's font
      stack on macOS and Linux (no new deps) and renders in a
      single cell. Falls back to ASCII `"~ sync"` if a future
      config flag asks (out of scope here; documented).

### TUI — `SyncCompleted` handler

- [ ] New arm in `App::handle_event` for
      `Event::Background(BgEvent::SyncCompleted(result))`:
      1. Clear `jobs` regardless of outcome.
      2. Refresh the *currently active* tab via the same code path
         `R` uses — even if the user switched tabs mid-sync (a
         legitimate use case now that they aren't blocked).
         Rationale: the pulled changes are on disk; the visible
         tab is the one the user cares about right now.
      3. Render outcome (same mapping as plan 012 session 3):
         - `Ok(Clean { pushed: false })` → green toast
           `"already in sync"`.
         - `Ok(Clean { pushed: true })` → green toast
           `"pushed local commits"`.
         - `Ok(Synced { committed, pulled, pushed })` → green
           toast `"sync ok — committed N, pulled, pushed"`
           (omit clauses that didn't apply).
         - `Ok(MergeConflict { files })` /
           `Ok(RebaseConflict { files })` → enter
           `Mode::SyncConflict`, populate `sync_conflict` with
           the right `SyncConflictKind`. Reuses
           `render_sync_conflict` verbatim from plan 012.
         - `Err(msg)` → red toast `"git sync failed: <msg>"`.

### TUI — re-entrancy and edge cases

- [ ] **Re-entrant `g s` while syncing.** Toast
      `"sync already in progress"`. Pinned by a test.
- [ ] **`g s` mid-conflict.** If `Mode::SyncConflict` is active
      (user hasn't dismissed the modal from a prior failed sync),
      pressing `Esc` first dismisses the modal as today; only
      then does `g s` get processed by the normal handler. No
      new behavior — `Mode::SyncConflict` already short-circuits
      input.
- [ ] **Quit during sync.** Pressing `q` while a sync is in
      flight: the main loop sets `should_quit` and exits its
      `recv()` loop on the next event. The worker thread
      continues to completion (it's a short subprocess chain);
      its `Sender::send` returns `Err` because the receiver
      dropped; nothing else happens. No `join`. Pinned by a
      test that constructs the spawn but doesn't actually run
      git — see test strategy below.
- [ ] **Tab switch mid-sync.** Allowed. The "refresh active tab"
      step on completion uses `self.active` at *completion time*,
      not at submission time.
- [ ] **App teardown drops `EventStream`.** The receiver is
      owned by `EventStream`; dropping it is sufficient. The
      worker's send will fail silently. No leaks.

### Tests

Tests live in `ft/src/tui/tests.rs` alongside the existing
`git_*` tests. Two flavors:

- [ ] **Pure-Rust orchestration tests** (no `git` subprocess
      involved). These verify the event-channel plumbing and
      lifecycle. They construct an `App`, then *manually*
      inject a `BgEvent::SyncCompleted(…)` via a helper that
      drops it into the channel using `EventStream::sender()`.
      No worker thread spawned. Pin:
      - `sync_completed_clean_pushed_renders_toast_and_clears_job`
      - `sync_completed_merge_conflict_enters_conflict_mode`
      - `sync_completed_error_renders_red_toast`
      - `g_s_while_job_in_flight_renders_already_in_progress_toast`
      - `status_bar_shows_sync_indicator_while_job_in_flight`
- [ ] **End-to-end integration test** — one, not many — that
      *does* spawn the worker and walks a real bare-origin
      handshake. It asserts that after `g s`:
      1. `jobs.borrow()` is `Some` immediately.
      2. The event channel eventually yields a
         `BgEvent::SyncCompleted(Ok(Synced { … }))`.
      3. After dispatching that event, `jobs.borrow()` is
         `None` and the toast cell holds the expected string.
      No timing-based asserts beyond a generous 5-second
      receiver timeout. Reuses the `setup_origin_and_clone`
      fixture from plan 012.
- [ ] **No new "TUI runs a real `git` and renders a modal" test.**
      The library and CLI cover the underlying mechanics; the
      TUI's job here is plumbing, and pure-Rust plumbing tests
      cover it without a `git` dep on every test run.
- [ ] **Snapshot updates.** The status bar's right cell gains a
      "syncing" glyph; existing snapshots that show the right
      cell mid-sync need regeneration. Since plan 012's
      `Mode::Syncing` snapshot existed only because of the
      blocking modal — and we're removing that mode — we can
      drop `syncing_modal_80x24.snap`. New snapshot:
      `sync_indicator_in_status_bar_80x24.snap`.

### Documentation

- [ ] `docs/architecture.md`: new top-level section
      `## Concurrency model` summarizing the **Pattern this plan
      establishes** subsection above (worker owns inputs, post
      result via `EventStream::sender()`, no async runtime, etc.).
      Keep it tight — 30–50 lines, with one labeled diagram or
      code block.
- [ ] `docs/config.md`: no changes (the `[git]` block is
      unaffected; `pull_strategy` flows through unchanged).
- [ ] Update `## Future` in `.devplans/git-sync.md` (plan 012) to
      strike "Auto-sync on a schedule" mentioning that the
      background pattern is now in place. (Light edit; the
      autosync plan is still a separate plan.)
- [ ] `?` help overlay: no change — `g s` is still `g s — git
      sync`.
- [ ] `README.md`: the "Git sync" subsection gets one sentence
      added: "From the TUI, `g s` runs the sync in the
      background — keep working while it completes."

## Technical Notes

- **Why the right-side status-bar indicator and not the footer.**
  The footer is per-tab and already crowded; the status bar is
  global and has room. Reusing `Mode::label()`'s cell for an
  orthogonal "background job" indicator means rotating the
  rendering of that cell to render `[mode] [job]` when both are
  present. Concretely: `Mode::label()` returns the current mode
  string; when `jobs.borrow().is_some()` the renderer prepends
  `"⟳ sync · "` to whatever the mode is showing. Net width on
  80-col is fine (mode labels are short).

- **Glyph choice.** `U+27F3` was picked over the more obvious
  `⟳` (`U+27F3`, identical) and `↻` (`U+21BB`) because its
  visual weight matches the existing status-bar typography. No
  external font dep — both glyphs render in the platform default
  monospace fonts we already require for the rest of the TUI
  (the box-drawing borders, the task checkbox glyphs).

- **Why `Result<SyncOutcome, String>` over `Result<SyncOutcome,
  anyhow::Error>` on the wire.** `anyhow::Error` is `Send +
  Sync` so technically crossable, but `Event` is `Clone`-able
  in our enum today (used in `Tick` debug paths). Stringifying
  at the worker boundary preserves the error chain (`{:#}`) and
  keeps `Event` `Clone`. Plus the user-facing rendering for
  errors is always a toast string anyway.

- **Why no progress reporting in v1.** Plan 012 already noted
  that real-time per-phase progress would require a richer event
  channel. Now that we have one, we *could* — `BgEvent::SyncProgress(stage)`
  is one variant away. We're deliberately *not* doing this in v1
  because the "running…" indicator alone solves the UX problem,
  and the porcelain output from `git pull` over a slow network
  is unstructured enough that we'd need a real protocol-aware
  parser to render anything more honest than "still going."
  Earmarked for a v3 if anyone asks.

- **Why one job slot, not a queue.** Two `g s` presses two
  seconds apart should not queue two syncs — the second is
  redundant (the first will pick up whatever the second would
  have pushed). Toast-and-ignore is the right semantic.

- **Race: `g s` immediately after the worker's send but before
  the main loop has processed the result.** The window is
  microseconds. `jobs.borrow().is_some()` is still `true` at
  that point because the handler hasn't run yet, so the
  re-entrancy check correctly rejects. No bug.

- **Race: tab switch happens between the worker's send and the
  main loop's match arm.** We refresh `self.active` *at handler
  time*, so the post-sync refresh hits the tab the user is
  looking at now — not the one they were on when they pressed
  `g s`. This is the right call: if the user has navigated
  away from Notes to Tasks, refreshing Tasks (which may have
  picked up new task files from the pull) is more useful than
  refreshing a hidden Notes tab.

- **Failure mode: worker thread panics.** `std::thread::spawn`
  returns a `JoinHandle`; we drop it (we don't `join`). A
  panic propagates only to the worker's own stack and is
  logged via the default panic hook (we don't install a custom
  one). The result-channel send never happens, so
  `jobs.borrow()` stays `Some` indefinitely. Mitigation: wrap
  the worker's body in `std::panic::catch_unwind` and post an
  `Err("internal panic: …")` result on the way out. Pinned by
  a test that calls `run_sync_job` with a bad-path repo
  designed to panic in the porcelain parser — except the
  porcelain parser doesn't panic on malformed input today
  (returns errors). Lower-priority safety net; we install it
  anyway because the cost is small and the failure mode
  ("indicator stuck forever") is bad.

- **Threading and `EventStream::drain()`.** The post-editor
  `drain(window)` call (plan 011) reads-and-discards events
  for a fixed window. If a `BgEvent::SyncCompleted` happens to
  land in that window — vanishingly unlikely because editor
  handoff is a separate action from `g s` and the user can't
  press both at once — it would be dropped. We accept that:
  the editor case is a foreground full-screen takeover and any
  background event during it can't usefully be rendered anyway.
  The job handle stays set; the next user interaction
  re-renders the indicator; eventually a manual `R` refreshes
  the tab. Not worth special-casing in v1.

- **No `Arc<AppState>` refactor.** It's tempting to wrap mutable
  app state in `Arc<Mutex<…>>` to let the worker write directly.
  Don't: messaging keeps the threading contract narrow ("worker
  produces one message, main loop applies it"), and the
  alternative ("worker mutates shared state, main loop discovers
  it on next tick") has well-known footguns (torn renders,
  ordering bugs, deadlocks). The cost of one extra `Event`
  variant is much lower than the cost of getting locking right.

## Sessions

### Session 1 · 2026-05-15 · done
**Goal:** Wire the background channel. `EventStream::sender()`,
`Event::Background(BgEvent)`, `BgEvent::SyncCompleted(SyncJobResult)`,
`JobHandle` + `App::jobs` slot. Drop `Mode::Syncing`. Rewire
`dispatch_sync_git` to spawn instead of block; add `SyncCompleted`
handler arm that maps outcome → toast or `SyncConflict` mode.
Status-bar indicator. Pure-Rust plumbing tests
(`sync_completed_*`, `g_s_while_job_in_flight_*`,
`status_bar_shows_sync_indicator_*`). Snapshot update for the
status bar; delete `syncing_modal_80x24.snap`. One end-to-end
real-`git` integration test against a bare-origin clone.
Architecture doc gains a `## Concurrency model` section codifying
the pattern. README + plan-012 light edits.
**Outcome:** Background sync ships end-to-end.

`ft/src/tui/event.rs` grew an `Event::Background(BgEvent)` variant
plus `BgEvent::SyncCompleted(SyncJobResult)`. `SyncJobResult.outcome`
is `Result<SyncOutcome, String>` — `anyhow::Error` is stringified
at the worker boundary with `{e:#}` so `Event` stays `Clone`.
`EventStream` exposes `pub fn sender(&self) -> Sender<Event>` —
the internal `_tx` was renamed and unprefixed since it's no longer
"kept alive but unused."

New `ft/src/tui/jobs.rs` (~50 lines) with `JobHandle { kind: JobKind,
started_at: Instant }` and `JobKind::Sync` (single variant for v1).
`JobKind::indicator_label()` returns the short label rendered in
the status bar (`"sync"`).

`Mode::Syncing` removed. `App.jobs: RefCell<Option<JobHandle>>`
replaces it as the in-flight tracker. The right cell of the status
bar composes either `mode: <label>` (default) or `⟳ <job> ·
<label>` (in-flight, dropping the `mode:` prefix so the 22-char
indicator-plus-mode line fits the 16-char right cell at 80 cols).
Tested fit at 80×24 via the new snapshot. Indicator survives
mode changes — pressing `?` or `g` during a sync keeps `⟳ sync`
visible.

`App::dispatch_sync_git` rewired:
1. Re-entrancy guard — if `self.jobs.borrow().is_some()`, push a
   cyan `"sync already in progress"` info toast and return. No
   discovery, no spawn.
2. `discover_repo(&vault.path)`; on `None`, red toast and return
   (same as today; vault state can change between presses, so
   feature-unavailable is a toast not a panic).
3. Clone the sender via `events.sender()`, set the in-flight slot
   *before* spawning (closes the race where a fast worker posts a
   result before the indicator was lit), push a cyan
   `"syncing in background…"` info toast.
4. `std::thread::spawn(move || run_sync_job(repo, opts, tx))`. The
   worker runs `ft_core::git::sync` wrapped in
   `panic::catch_unwind` so a porcelain-parser bug doesn't strand
   the in-flight indicator forever — panics become `Err("internal
   panic in sync worker: …")` results.

`App::handle_event` now short-circuits on
`Event::Background(_)` *before* any mode check, so a completion
that arrives while help / git leader / conflict is open still
clears the in-flight slot and sets the success/error toast (the
user sees the outcome when they dismiss the overlay).
`handle_background` → `apply_sync_result` map outcome to
toast (clean/synced/error) or `Mode::SyncConflict` modal; the
active tab is refreshed at *completion time* (not submission
time) so tab-switching mid-sync routes the post-sync refresh to
the user's current view.

New `ToastStyle::Info` variant (cyan) for the in-flight notice
and the re-entrancy toast. Existing `Success` (green) / `Error`
(red) unchanged.

`render_status_bar`'s argument list crossed the
`clippy::too_many_arguments` threshold at 8 args, so its inputs
moved into a new `Copy` `StatusBarState<'a>` struct passed by
value.

15 new TUI tests in `ft/src/tui/tests.rs`, all green:
- `sync_completed_clean_pushed_renders_toast_and_clears_job`
- `sync_completed_clean_no_push_renders_already_in_sync_toast`
- `sync_completed_synced_renders_compound_toast`
- `sync_completed_merge_conflict_enters_conflict_mode`
- `sync_completed_rebase_conflict_enters_conflict_mode`
- `sync_completed_error_renders_red_toast`
- `sync_completed_while_in_help_mode_still_clears_job`
- `g_s_while_job_in_flight_toasts_already_in_progress`
- `status_bar_shows_sync_indicator_while_job_in_flight`
- `status_bar_no_indicator_when_no_job_in_flight`
- `sync_indicator_persists_across_modes`
- `dispatch_sync_git_with_no_git_repo_toasts_and_does_not_spawn`
- `sync_indicator_in_status_bar_snapshot` (80×24)
- `e2e_background_sync_dirty_tree_dispatches_event_and_renders_toast`
  — the one real-git test: spawns a bare origin + clone, calls
  `app.submit_sync_for_test(&events, None)`, asserts the
  in-flight slot is `Some` immediately, drains `events.next()`
  until the `Background` arrives (15s budget), dispatches it,
  asserts the slot is `None` and the toast is `"sync ok …"`.
- `no_more_syncing_mode_exists` — compile-time guard: a match on
  `Mode` that doesn't list `Syncing`. Reverting the removal would
  break this test, forcing intentional re-introduction.

Test helpers added to `App`: `in_flight_job_for_test`,
`set_in_flight_for_test(kind)`, and `submit_sync_for_test(&events,
message)`. The first two let plumbing tests pretend a job is in
flight without spawning a thread; the third routes through the
real `dispatch_sync_git` for the e2e walk-through.

New snapshot:
`ft/src/tui/snapshots/ft__tui__tests__sync_indicator_in_status_bar_80x24.snap`
showing the welcome screen with `⟳ sync · normal` in the status
bar's right cell. No existing snapshots needed regeneration —
the right cell remains `mode: <label>` when no job is in flight.

`docs/architecture.md` gained a `## Concurrency model` section
documenting the producer/consumer pattern, the `Arc<T>` /
`RefCell<Option<T>>` conventions, and a recipe for adding future
off-thread work. `README.md`'s Git-sync subsection notes the
chord now runs in the background with a `⟳ sync` indicator.
Plan-012's Future section was lightly edited to flag that the
background-worker pattern (which auto-sync needs) is now in
place.

Workspace state: `cargo test --workspace` → 914 tests green (up
from 803 before this plan: +12 plumbing TUI tests, +1 e2e, +1
extra snapshot, +1 compile-guard, plus snapshot acceptance count
adjustments). `cargo clippy --workspace --all-targets -- -D
warnings` clean — the `too_many_arguments` lint fired once and
was resolved by grouping render-status-bar args into the new
`StatusBarState` struct rather than `#[allow]`-ing it. `cargo fmt
--check` clean after one autoformat pass. No new dependencies —
`std::thread`, `std::sync::mpsc`, and `std::panic::catch_unwind`
are all already in use elsewhere in the workspace.

The plan is now complete — `g s` from any tab launches the sync
on a worker thread, the indicator lights up immediately, the TUI
stays responsive (the user can navigate tabs, open files, edit
queries while the sync runs), and the outcome arrives as a toast
on the happy path or a persistent modal on conflict. The
established concurrency pattern (producer thread → shared event
channel → main-loop match arm) is documented for future plans. 

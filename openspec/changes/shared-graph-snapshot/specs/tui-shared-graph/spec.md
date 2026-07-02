# tui-shared-graph

## ADDED Requirements

### Requirement: Single App-owned graph snapshot
The TUI App SHALL own a single graph snapshot — the built `Graph`, the
`Scan` it was built from, and a monotonically increasing generation
number — shared with every tab and modal through `TabCtx`. Tabs and
modals SHALL NOT run `vault.scan()` or `Graph::build` themselves; the
snapshot is the only source of graph and task data in the TUI.

#### Scenario: All tabs read the same snapshot
- **WHEN** the Graph, Tasks, Journal, and Review tabs each render after
  a snapshot with generation N is installed
- **THEN** all four derive their content from that same generation-N
  snapshot, with no per-tab scan or graph build

#### Scenario: No graph builds on the UI thread
- **WHEN** any key event is handled or any tab gains focus in a
  production session
- **THEN** no vault scan or graph build executes on the main thread —
  builds happen only on the background rebuild worker

### Requirement: Background rebuild with coalescing
Graph rebuilds SHALL run on a background worker thread that performs one
`scan → build` pass and posts one completion event back to the main
loop. Rebuild requests SHALL be single-flight: a request arriving while
a build is in flight sets a dirty flag, and exactly one follow-up
rebuild starts when the in-flight build completes, regardless of how
many requests arrived meanwhile.

#### Scenario: Burst of mutations coalesces
- **WHEN** three mutations each post a rebuild request while the first
  rebuild is still running
- **THEN** exactly one additional rebuild runs after the first
  completes, and the final installed snapshot reflects all three
  mutations

#### Scenario: Failed rebuild keeps the previous snapshot
- **WHEN** a background rebuild fails (e.g. the graph build returns an
  error)
- **THEN** the previously installed snapshot remains in place, an error
  toast is shown, and the app keeps running

### Requirement: Mutations trigger rebuild via one request
Every flow that mutates vault content (task create/complete/edit/cancel/
move, note create/rename/delete/move, section move, capture, editor
return, git sync or commit completion) SHALL post a single
`RefreshGraph` request rather than rebuilding a graph inline. The
request SHALL be serviced through the App's single routing table.

#### Scenario: Task edit refreshes the shared snapshot
- **WHEN** the user completes a task from the Tasks tab
- **THEN** a rebuild is requested, and once it completes, the Graph tab
  (without doing any work of its own) reflects the completed task

#### Scenario: Editor return refreshes
- **WHEN** the user returns from an external editor session opened via
  the TUI
- **THEN** a rebuild is requested so external edits appear without a
  manual reload

### Requirement: Stale snapshot renders until replacement arrives
Between a mutation and the completed rebuild, tabs SHALL continue
rendering the previous snapshot. Line-addressed mutations issued against
a stale snapshot SHALL be protected by the expected-task guard: a
mutation whose target line changed on disk fails with an error toast and
leaves the file untouched.

#### Scenario: Interaction stays live during rebuild
- **WHEN** a rebuild is in flight
- **THEN** navigation and rendering keep working against the previous
  snapshot without blocking

#### Scenario: Guarded mutation on stale data fails safe
- **WHEN** the user triggers a task mutation whose line number is stale
  because the file changed after the snapshot was built
- **THEN** the operation fails with a "changed on disk" error toast and
  no file content is corrupted

### Requirement: Tabs re-derive view state on generation change
Tabs SHALL detect a new snapshot by comparing its generation to the
generation they last derived from. The active tab SHALL be notified
immediately when a snapshot installs (resolving any pending cursor
anchor); background tabs SHALL re-derive on their next focus. Expanded
paths, selections, and cursor positions SHALL be keyed by
build-independent `NodeKey`s so they survive the swap.

#### Scenario: Active tab updates on snapshot install
- **WHEN** a rebuild completes while the Graph tab is active with nodes
  expanded and a row selected
- **THEN** the tab re-derives its tree from the new snapshot and
  restores expansion and selection via `NodeKey`s without user input

#### Scenario: Pending cursor anchor resolves after mutation
- **WHEN** the user creates a task and the triggered rebuild completes
- **THEN** the cursor lands on the newly created task's row

#### Scenario: Background tab catches up on focus
- **WHEN** a mutation on the Tasks tab triggers a rebuild that installs
  generation N+1, and the user then switches to the Journal tab (which
  last derived from generation N)
- **THEN** the Journal tab re-derives from generation N+1 on focus

### Requirement: Loading state before the first snapshot
Until the first snapshot is installed after startup, graph-backed tabs
SHALL render a non-blocking loading indication instead of graph content,
and the app SHALL remain responsive to input (including quit and tab
switching).

#### Scenario: First frames show loading
- **WHEN** the TUI starts and the initial background build has not yet
  completed
- **THEN** graph-backed tabs render a loading placeholder and `q` still
  quits immediately

### Requirement: Deterministic test pump
Tests SHALL be able to drive the rebuild lifecycle synchronously: a
test-only pump runs any requested or in-flight rebuild to completion on
the calling thread through the same installation path as the worker, so
`TestBackend` snapshot assertions observe post-rebuild state without
sleeping or polling.

#### Scenario: Mutate-then-assert test is deterministic
- **WHEN** a test dispatches a mutating key, calls the pump, and renders
  a frame
- **THEN** the frame deterministically reflects the post-mutation
  snapshot

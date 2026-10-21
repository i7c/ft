## Why

Phase 3 made Jira and GitHub issues visible in the vault, but only one
way. Today:
- ticking a Jira anchor in ft just leaves it *pending push*;
- a conflict can only be read in the `ft tasks sync` output;
- a local subtask that turns out to need a real ticket has to be
  re-typed in Jira by hand.

Phase 4 (roadmap in `openspec/changes/task-reparent/design.md`) closes
that loop:
- local status changes reach the remote;
- conflicts become resolvable from ft;
- a local subtask can be promoted to a real Jira sub-task (or GitHub
  sub-issue).

Everything stays offline-first:
- pushes are derived from the notes (no outbox);
- conflicts are stored in the notes, so they can be resolved without
  network;
- only promote, which creates a remote object, requires a connection.

## What Changes

- **Status push (every sync).** Each sync pushes the issue notes whose
  anchor status differs from `ft.source.synced.status` and that aren't
  conflicted. This includes TUI startup, interval and `g r` syncs, and
  `ft tasks sync`.
  - **Jira:** ft picks the available transition whose target status
    category matches the local status (Open → To Do, InProgress → In
    Progress, Done → Done). `transitions = { <Status> = "<transition
    name>" }` overrides that. `Cancelled` is pushed only through a
    configured `transitions.Cancelled`.
  - **GitHub:** Done closes with `state_reason=completed`, Cancelled
    closes with `not_planned`, and Open/InProgress reopen.
  - **After a successful push,** the note is refreshed from the remote,
    so `synced.status` and the remote status name update.
  - **Failed pushes** (no matching transition, permission denied, issue
    deleted) stay pending and are retried every sync. The reason is kept
    in the machine-local cache, shown in `ft tasks sync` and `ft tasks
    check`, and toasted once per new failure.
  - **Opting out:** `push = false` per source, or `ft tasks sync
    --no-push`.
- **Conflicts stored in the note.** A sync that finds both sides changed
  writes `ft.source.conflict: <remote status>`. A conflicted issue is
  never pushed, and the next agreeing sync clears the key.
  - **Resolving:** `ft tasks sync --resolve` asks per conflict (local /
    remote / skip), and `--take local|remote <KEY>…` does it from
    scripts. Both only edit local files and work offline.
  - **Take remote** writes the remote status.
  - **Take local** sets `synced.status` to the remote status, which
    turns the local status into an ordinary pending push.
  - The TUI shows a conflict modal after a sync that reports conflicts.
- **Promote.** `ft tasks promote <SEL>` (TUI: `I`) turns a local task
  whose direct effective parent is a remote anchor into a real remote
  issue. It requires network and credentials, and fails fast with
  nothing queued.
  - **Jira:** a Sub-task in the parent's project; or, when the parent is
    an epic, a Task (the project's standard type) with the epic as
    parent. A parent that is itself a sub-task is refused.
  - **GitHub:** a new issue in the parent's repository, added as a
    sub-issue.
  - **Fields:** the summary is the task description with links and
    hashtags removed; due date and priority are carried over; the issue
    is assigned to you. A local status other than Open is pushed right
    after creation.
  - **Locally:** a new issue note `<folder>/<KEY>.md` is created; the
    task's own subtasks move under the new anchor; the original line is
    removed. References to the line's block id are retargeted to the new
    anchor, which takes over the block id.
- **HTTP.** POST support (JSON body) in the source HTTP client.
  `TaskSource` gains `push_status`, `create_child` and `issue_meta`.
- **TUI:**
  - pushes run inside the existing source job;
  - a `ConflictResolve` modal (`l` take local, `r` take remote, `s`
    skip, `Enter` apply);
  - `I` promote, which runs as a background job: network in the worker,
    note edits on the main thread with the usual unchanged-file guard.
- **[ft.nvim]** A `:FtPromote` command on the cursor task (via `ft tasks
  promote … --format json`).

## Capabilities

### New Capabilities

- `task-source-push`: Pending-push detection; the Jira transition choice
  (category match + override, Cancelled only when configured); GitHub
  close/reopen; the refresh after a push; failure handling and reporting;
  opting out.
- `task-source-promote`: Promote preconditions; Jira and GitHub creation
  rules and fields; the local restructuring (new note, children moved,
  line removed, block id taken over); CLI and JSON output.
- `tui-source-push`: Pushes inside the background source job; the
  conflict-resolution modal; the `I` promote background job; toasts.

### Modified Capabilities

- `task-sources`: config keys `push` and `transitions`; HTTP POST;
  `ft tasks sync --no-push | --resolve | --take`; push and promote
  methods on `TaskSource`.
- `task-source-sync`: the `ft.source.conflict` key and its lifecycle;
  pending excludes conflicted issues; `ft tasks check` reports conflicts
  and push failures.
- `tui-keymaps`: Tasks-tab `I` → `tasks.promote`.

## Impact

- **ft-core:**
  - `sources/http.rs` gains `post_json`;
  - `sources/{jira,github}.rs` gain transitions, create, meta and
    sub-issue calls;
  - new `sources/push.rs` (pending set, transition choice, push loop);
  - new `sources/promote.rs` (plan the local side, apply);
  - `sources/sync.rs` handles the conflict key;
  - `sources/run.rs` adds the push step and the failure cache.
- **ft binary:**
  - `cmd/sources.rs` (push in `run_sync`, `--resolve`/`--take`);
  - `cmd/tasks.rs` (`promote`, `sync` flags);
  - TUI `source_sync.rs` (push in the job, conflict list), a new modal
    for conflict resolution, the promote job, Tasks-tab key `I`.
- **No new dependencies.** Network-touching code stays isolated in the
  sources module and is tested against the local stub server; no test
  touches the real remote.
- **Risk of writing to company Jira** is limited:
  - only status transitions and new issues are written, and only for
    notes ft created;
  - every push is visible in sync output;
  - `push = false` disables pushing.

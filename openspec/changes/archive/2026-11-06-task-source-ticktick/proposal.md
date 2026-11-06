## Why

ft already syncs Jira and GitHub issues into the vault as issue notes, but
personal tasks live in TickTick and stay invisible to ft's queries, TUI and
note workflows. TickTick's Open API now offers personal API tokens, so a
TickTick backend fits the existing source model without an OAuth flow.

## What Changes

- New source kind `kind = "ticktick"` for `[sources.<name>]`. It uses the
  TickTick Open API at `https://api.ticktick.com`; `base_url` can override
  this, e.g. `https://api.dida365.com` for Dida365.
- One issue note per TickTick task, in the existing issue-note format. The
  key is `tt-<taskId>` and the note is `<folder>/tt-<taskId>.md`. Subtasks
  (`parentId`) become parents, so `fetch_parents` and `ft.tasks.parent` work
  unchanged.
- A small ft-defined query syntax for TickTick, which has no query language:
  `list:<name>`, `-list:<name>`, `tag:<tag>`, `priority:<none|low|medium|high>`.
  The default query `mine` is empty and means **all open tasks in all
  lists, including the inbox**.
- Status mapping: `0` → Open, `2` → Done, `-1` (Won't Do) → Cancelled. The
  remote status name is the kanban column name when the task has one, so
  `status_map` can map a column such as `Doing` to InProgress.
- Due and completion dates are converted to local dates in the task's own
  time zone. This adds the pure-Rust `chrono-tz` dependency.
- Push: Done completes the task. Cancelled and reopen set `status` through
  the task update endpoint.
- Promote: creates a TickTick task with `parentId`, due date and priority in
  the parent's list.
- Credentials: `FT_<NAME>_TOKEN`, then keychain account `<name>:ticktick`.
  `ft sources login` verifies the token by listing projects.
- HTTP: a TickTick throttle response (HTTP 500 with `exceed_query_limit`) is
  classified as `RateLimited`. Every source's `Remote` error excerpt
  has its own token scrubbed out, because TickTick echoes the token in some
  error bodies.
- `comments` gets a per-kind default: 5 for Jira and GitHub, 0 for TickTick.
  TickTick needs one request per task for comments, and its comments carry no
  author.

## Capabilities

### New Capabilities
- `task-source-ticktick`: TickTick source mapping. Covers API usage, the
  query syntax, keys and URLs, field mapping (status, column, priority,
  time-zone dates, checklist items, parents), status push, and promote.

### Modified Capabilities
- `task-sources`: source configuration gains `kind = "ticktick"` and a
  per-kind `comments` default. Credentials gain the TickTick keychain
  account. The HTTP policy gains `exceed_query_limit` → `RateLimited` and
  token scrubbing of error excerpts. `ft sources login` gains its TickTick
  verification request.
- `task-source-push`: for an issue pushed in this sync, the refreshed
  remote status wins, even when it differs from the target. A recurring
  TickTick task advances on completion, and without this it would
  re-push forever.

## Impact

- **Code:**
  - new `ft-core/src/sources/ticktick.rs`;
  - `ft-core/src/config.rs` (`SourceKind::Ticktick`, `comments` default,
    validation);
  - `ft-core/src/sources/{credentials,run,http}.rs`.
  - `sources/sync.rs`: `SyncCtx::pushed`, so the remote wins for keys
    pushed in this run; its four construction sites (CLI, TUI, promote,
    tests) pass it. Otherwise the planners and the TUI are unchanged.
- **Dependencies:** `chrono-tz` (pure Rust; keeps `scripts/check-no-cpp.sh`
  clean).
- **Docs:** `docs/config.md` sources section.
- **Tests:** stub-server tests for the backend; config and credential unit
  tests. No real-account tests by default.

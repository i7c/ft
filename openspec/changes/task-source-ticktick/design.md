## Context

`ft-core/src/sources/` turns remote issues into issue notes through one
trait, `TaskSource` (`fetch_query`, `fetch_keys`, `push_status`,
`create_child`). Only the backends know about a remote. The sync, push and
promote planners and the TUI are backend-neutral. Adding a backend touches:

- `SourceKind` and `validate` in `config.rs`;
- the keychain account in `credentials.rs`;
- `build_source` / `verify_token` in `run.rs`;
- one new backend module.

Sources:
- the TickTick Open API reference at developer.ticktick.com, archived at
  `~/.local/share/agents/sources/2026-11-06-ticktick-api/01-ticktick-openapi-docs.md`;
- read-only probes against the user's account (2026-11-06; 3 lists, 13 open
  tasks).

What the probes verified, beyond the docs:

- A personal API token works as `Bearer`, at `api.ticktick.com`.
- `GET /project` returns `id, name, kind, sortOrder, viewMode`. It returned
  no `closed` and no `permission` fields for this account, and it does not
  list the inbox.
- `GET /project/inbox/data` works: 200, `{tasks, columns}`, with no
  `project` key.
- Tasks carry undocumented `modifiedTime`, `createdTime`, `etimestamp`,
  `columnId`, `columnName`, `childIds`, `isFloating` and `progress`.
  `repeatFlag` is often present but empty.
- An all-day due date is stored as local midnight in UTC: for
  `America/Sao_Paulo`, 2026-11-05 is `2026-11-05T03:00:00.000+0000`.
- `GET /project/{p}/task/{t}` returns 404 `resource_not_found` when `p` is
  the wrong project, including `inbox`. It returns 200 with `status: 2` for
  a completed task.
- `POST /task/undone` with `taskIds` cannot be used to look tasks up by id:
  it filters by date range and returned nothing for an undated task.
- `POST /task/filter {"status":[0]}` without `projectIds` returned all 13
  open tasks.
- A 401 body echoes the submitted token verbatim
  (`"Invalid access token: <token>"`).

Not verified (would need writes or a larger account):

- status changes through the update endpoint;
- `parentId` on create;
- completing a recurring task via the API;
- whether `exceed_query_limit` arrives as HTTP 500. That is
  community-reported: about 200 requests/min per IP.

## Goals / Non-Goals

**Goals:**
- TickTick tasks sync as issue notes with the existing format and planners.
- Push and promote work, as they do for the other sources.
- The default scope is all open tasks; queries narrow it.
- No new interactive auth flow: `ft sources login` accepts a personal API
  token.

**Non-Goals:**
- OAuth authorization-code flow. A user can still paste an OAuth access
  token, because it is also a `Bearer` token.
- Syncing anything other than status upstream: title, due date, tags and
  checklist edits stay one-way, as for Jira and GitHub.
- Habits, focus records, countdowns, and NOTE-kind lists.
- Tags → hashtags on the anchor line. Hashtags are a local field under the
  sync rules, and mixing remote tags in would fight the user's own.
- A per-list note layout. One note per task was chosen.

## Decisions

**Scope via per-list `/data`, not `/task/filter`.** `/project/{id}/data`
returns every undone task of a list without a cap. `/task/filter` and
`/task/search` cap at 200 results and have no pagination, so a large account
would silently lose tasks. A sync costs:

- 1 request for the project list;
- 1 per selected list, plus the inbox;
- 1 per out-of-scope known note;
- comments, if enabled.

Lists are fetched once per run and cached, so overlapping queries cost
nothing extra.

**ft-owned query syntax.** TickTick has no query language, but `queries` is
a map of strings shared with Jira and GitHub. A tiny term syntax (`list:`,
`-list:`, `tag:`, `priority:`) covers the useful cuts and keeps the config
shape uniform. Unknown terms and unknown list names are hard errors, so a
typo can't silently empty the scope and push every note out of scope.
Rejected alternative: TOML tables per query, which would need a
kind-dependent `queries` type.

**Key `tt-<id>`.** Task ids are stable 24-hex ids that are valid as `🆔` and
file stem. The prefix makes them recognisable in queries and avoids clashing
with other ids. Rejected alternative: title slugs, which are unstable under
rename and would duplicate notes.

**The project id travels in the URL.** Lookups need `projectId`, and
`FtSource` has no spare field. As with GitHub, the issue URL carries what
the lookup needs: the web-app link encodes the project id and the task id.
For parents, which are fetched by key without a URL, the backend keeps a
`key → projectId` map, seeded from list data and from each child's
`projectId`; subtasks share their parent's list. A task that moved lists is
found again by the scan while it is open. If it moved and was then
completed, it 404s and is reported missing, which is the same outcome as
for a deleted task.

**Status name from the kanban column.** TickTick has no in-progress status,
but kanban lists have columns (`columnName`, verified). Using the column as
the status name for open tasks lets `status_map = { Doing = "InProgress" }`
express it with the existing mechanism. Push never moves columns: InProgress
and Open are equivalent for push, as on GitHub.

**Dates in the task's time zone (`chrono-tz`).** All-day dates are local
midnight expressed in UTC (verified), so taking the UTC date gives the wrong
day east of UTC. Converting with the task's `timeZone` is the only correct
reading. `chrono-tz` is pure Rust, so the no-C++ check stays green. When the
zone is missing or unknown, the UTC date is used. Promote writes due dates
as all-day dates in the user's zone (`/preference`).

**Push sends the whole fetched task.** The update endpoint's partial-update
behaviour is undocumented. Push already fetches the task to check its
current status, so it sends that object back with only `status` changed.
That cannot reset fields by omission. Done uses the dedicated complete
endpoint.

**`comments` default per kind.** Comments cost one request per task, and
TickTick's comments carry no author, so the default is 0 for TickTick and 5
elsewhere. `SourceCfg::comments` becomes `Option<usize>` with an
`effective_comments()` accessor. There are three call sites in `sync.rs`,
plus config tests.

**Token scrubbing in `Http`.** TickTick echoes tokens in error bodies
(verified on 401). `Unauthorized` already drops the body, but `Remote`
keeps a 300-character excerpt. `Http` holds the auth header, so it replaces
the raw token in any excerpt with `***`. Doing this for every source costs
nothing and closes the class of leak.

**Throttle classification.** A 500 whose body contains `exceed_query_limit`
becomes `RateLimited`. The TUI and CLI already treat that as a failed sync,
which is retried on the next interval, and not as an error with a body
dump.

## Risks / Trade-offs

- [Status update via `POST /task/{id}` might ignore `status`] → An
  implementation task verifies it against a scratch task (with the user's
  consent). If it is ignored, Cancelled and reopen pushes become
  `NotPushable` with a clear reason, and Done still works.
- [`parentId` on create might be ignored] → Promote falls back to a
  follow-up update, then fails cleanly. Verified in the same scratch-task
  test.
- [Recurring tasks: completing one advances it rather than closing it] →
  The spec accepts the remote's answer on refresh. The anchor reopens with
  the next due date, and no conflict is raised. Verify the API's behaviour
  in the scratch test.
- [The web-app URL format is unverified] → The URL needs no special form
  for lookups, only parsing back. Pick the format the web app uses
  (`https://ticktick.com/webapp/#p/<projectId>/tasks/<taskId>`), confirm in
  a browser, and keep parsing tolerant.
- [Large accounts and the ~200 req/min limit] → The request count is about
  the number of lists plus out-of-scope notes, not the number of tasks. With
  comments off there is no per-task request. Throttling surfaces as
  `RateLimited`, not as corrupt state.
- [Default "all open tasks" can flood a vault with notes] → This was the
  user's choice. Document narrowing via `list:` / `-list:` and the note
  folder.
- [Undocumented fields (`modifiedTime`, `columnName`) could disappear] → The
  mapping falls back to `etag` and `open`. Neither is needed for
  correctness.

## Migration Plan

Additive. Existing configs are unaffected. The `comments` field keeps its
TOML shape; only the absent-value default becomes per-kind. Rollback means
removing the source section.

## Open Questions

- Should `tag:` also accept `-tag:` exclusion? Left out until a need shows
  up.
- Should tasks in a closed list be rechecked or reported missing? Today they
  are only rechecked if a note already exists, via the URL lookup.

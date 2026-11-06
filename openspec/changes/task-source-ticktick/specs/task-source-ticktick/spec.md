## ADDED Requirements

### Requirement: TickTick API usage

The TickTick source SHALL use the TickTick Open API under `<base>/open/v1`. `<base>` is `https://api.ticktick.com`, or `base_url` when set (e.g. `https://api.dida365.com`). Every request SHALL authenticate with `Authorization: Bearer <token>`, where the token is a personal API token or an OAuth access token.

**Queries:** a query SHALL be resolved as follows:
1. list the projects (`GET /project`) and skip those with `closed: true`;
2. select the lists the query names, or all lists plus the inbox when it names none;
3. fetch each selected list's undone tasks (`GET /project/{id}/data`). The inbox is fetched as project id `inbox`;
4. apply the remaining filters client-side.

Each list SHALL be fetched at most once per sync run, however many queries name it. The capped endpoints `/task/filter` and `/task/search` (200 results, no pagination) SHALL NOT be used for queries.

**`fetch_keys`:** a key SHALL first be looked up among the tasks already fetched this run. Otherwise it SHALL be fetched with `GET /project/{projectId}/task/{taskId}`. The project id comes from the `IssueRef` URL or, for a parent, from its child's `projectId`. A 404 means absent. Completed and abandoned tasks are fetched the same way.

#### Scenario: Default scope
- **WHEN** the source has no `queries` configured
- **THEN** sync covers every open task in every open list and in the inbox

#### Scenario: Known task completed in TickTick
- **WHEN** a note `tt-abc` is active and its task was completed in the TickTick app
- **THEN** the task is absent from the list data, it is fetched by key from its project, and the note's anchor becomes Done with the completion date

### Requirement: TickTick query syntax

A TickTick query string SHALL be a whitespace-separated list of terms. A term value containing spaces SHALL be double-quoted (`list:"Side projects"`).

| Term | Meaning |
|---|---|
| `list:<name>` | Include this list (repeatable; union). `inbox` names the inbox. |
| `-list:<name>` | Exclude this list from the selection. |
| `tag:<tag>` | Keep tasks carrying this tag (repeatable; all must match). |
| `priority:<none\|low\|medium\|high>` | Keep tasks with this priority (repeatable; any may match). |

List names SHALL match case-insensitively against project names, or exactly against project ids. A name matching no list SHALL fail the sync of that source with `unknown TickTick list "<name>"`. An unknown term SHALL fail the sync with `unknown TickTick query term "<term>"`. The empty query SHALL select all open tasks in all lists and the inbox.

#### Scenario: Work list, urgent only
- **WHEN** `queries = { work = "list:Work tag:urgent" }`
- **THEN** only open tasks in the list `Work` tagged `urgent` are in scope

#### Scenario: Everything except shopping
- **WHEN** `queries = { mine = "-list:Shopping" }`
- **THEN** open tasks in every list and the inbox except `Shopping` are in scope

#### Scenario: Typo in a list name
- **WHEN** a query names `list:Wrok` and no such list exists
- **THEN** the source reports `error: unknown TickTick list "Wrok"`, and no file is modified for that source

### Requirement: TickTick keys and URLs

The issue key SHALL be `tt-<taskId>`, with the task id lower-cased; TickTick ids are 24 hexadecimal characters. The note file SHALL be `<folder>/<key>.md`. The issue URL SHALL be the TickTick web-app link to the task, and it SHALL encode both the project id and the task id, so that the task can be fetched from the URL alone. A parent's key is `tt-<parentId>`.

#### Scenario: Key and note path
- **WHEN** task `6aec746604183a21c6cffe42` is synced by source `ticktick`
- **THEN** its key is `tt-6aec746604183a21c6cffe42` and its note is `ticktick/tt-6aec746604183a21c6cffe42.md`

### Requirement: TickTick field mapping

The source SHALL map each TickTick task to a `RemoteIssue` as follows:

| `RemoteIssue` | TickTick |
|---|---|
| `url` | web-app task link (project id + task id) |
| `title` | `title` |
| status name | `completed` for `status = 2`, `wont-do` for `status = -1`; for `status = 0`, the task's `columnName` when present, else `open` |
| `Status` | `status_map` if it maps the status name; else `0` → Open, `2` → Done, `-1` → Cancelled |
| priority | `5` → High, `3` → Medium, `1` → Low, `0` → none |
| due | the date of `dueDate` in the task's `timeZone` (UTC when the zone is absent or unknown) |
| resolution date | the date of `completedTime` in the task's `timeZone` |
| assignee | `assigneeUsername`, when present |
| parent | `tt-<parentId>` when `parentId` is non-empty |
| updated | `modifiedTime`, verbatim, or `etag` when `modifiedTime` is absent |
| description | `content`. For `kind = "CHECKLIST"`: `desc`, then one `- [ ] <title>` / `- [x] <title>` line per checklist item, in `sortOrder` (the planner neutralises these to `☐` / `☑`) |
| comments | from `GET /project/{p}/task/{t}/comments`, only when `comments > 0`. The author is empty (TickTick comments carry none), `created` is `createdTime`, the body is `title` |

Recurrence (`repeatFlag`), reminders, start dates and tags SHALL NOT be mapped to the anchor line. The anchor line keeps its local fields under the existing sync rules.

#### Scenario: All-day due date in a western time zone
- **WHEN** a task has `isAllDay: true`, `dueDate: "2026-11-05T03:00:00.000+0000"` and `timeZone: "America/Sao_Paulo"`
- **THEN** its due date is 2026-11-05, not the UTC date of a different day

#### Scenario: Kanban column as status
- **WHEN** an open task is in column `Doing` and `status_map = { Doing = "InProgress" }`
- **THEN** its status name is `Doing` and its mapped `Status` is InProgress

#### Scenario: Won't do
- **WHEN** a task has `status: -1`
- **THEN** its status name is `wont-do` and its mapped `Status` is Cancelled

### Requirement: TickTick status push

For a pending TickTick task with target status `S`, the push SHALL first fetch the task (`GET /project/{p}/task/{t}`). It SHALL report `AlreadyThere` when the mapped remote status equals `S`, or when both are Open/InProgress. Otherwise it SHALL:
- **Done:** call `POST /project/{p}/task/{t}/complete`;
- **Cancelled:** send the fetched task back unchanged except `status = -1` (`POST /task/{t}`);
- **Open / InProgress:** send the fetched task back unchanged except `status = 0`.

The update SHALL send the complete fetched task object, so that fields the update endpoint might reset when omitted keep their remote values. A 404 SHALL make the push `NotPushable("task not found")`. A 403 SHALL make it `NotPushable("forbidden (403)")`.

#### Scenario: Tick reaches TickTick
- **WHEN** the user ticks the anchor of `tt-abc` and the next sync runs online
- **THEN** ft calls the complete endpoint for `tt-abc`, and the refreshed note has `synced.status: Done`

#### Scenario: Completing a recurring task
- **WHEN** the pushed task is recurring and TickTick advances it to its next occurrence instead of completing it
- **THEN** the refresh records the remote's new status and due date, and no conflict is raised

### Requirement: TickTick promote

Promote under a TickTick anchor SHALL create a task with `POST /task`. The task SHALL get:
- `projectId`: the parent's project;
- `parentId`: the parent's task id;
- `title`: the cleaned task description, as in the other sources;
- `priority`: the task's priority mapped back (Highest/High → 5, Medium → 3, Low/Lowest → 1);
- `dueDate`: when the task has a due date, an all-day date in the user's time zone (`POST /preference` → `timeZone`).

If the create response does not carry the requested `parentId`, the source SHALL set it with a follow-up update. If that update fails too, the promote SHALL fail with `TickTick did not accept the parent for <key>`. The new task is unassigned. TickTick tasks are personal unless the list is shared.

#### Scenario: Promote a dated subtask
- **WHEN** the local task `buy cable 📅 2026-11-10 🔼` under anchor `tt-abc` is promoted
- **THEN** TickTick gets a task `buy cable` with `parentId` of `tt-abc`'s task, priority 3 and due 2026-11-10 in the user's time zone, and the vault gets its issue note

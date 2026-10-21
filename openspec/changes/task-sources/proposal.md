## Why

Jira tickets assigned to the user (and, for side projects, GitHub issues)
are invisible in ft, so their real workload lives in two places.
Phases 1–2 of the task revamp (roadmap in
`openspec/changes/task-reparent/design.md`) prepared the vault side:
- tasks can be reparented;
- a task can have a parent in another file;
- an *anchor note* (`ft.tasks.anchor`) can stand in for a whole issue.

Phase 3 fills those anchor notes from remote sources. Each assigned issue
becomes a regular markdown note in the vault. The user can capture local
subtasks under it, either by nesting them in the issue note or by linking
them from the daily note with `[[ABC-123]]`.

All of this works offline:
- syncing only refreshes notes and never sits in the scan path;
- a machine without network or credentials stays fully functional;
- since the vault is git-synced across several machines, all sync state
  lives in the notes themselves.

## What Changes

- **Sources.** A `[sources.<name>]` config section (vault config, so
  every machine shares it), with `kind = "jira" | "github"`, named
  queries (JQL or GitHub search), a folder, an optional note template,
  and a sync interval. A `TaskSource` trait in ft-core with Jira Cloud
  and GitHub implementations. The new dependencies are blocking HTTPS
  via `ureq` (no async runtime) and credential storage via `keyring`.
- **Credentials.**
  - Jira: `ft sources login <name>` stores an Atlassian API token in the
    OS keychain; `FT_<NAME>_TOKEN` overrides it.
  - GitHub: ft reads `gh auth token`, falling back to the keychain.
  - A source without credentials is skipped silently, with no error.
- **One note per issue,** e.g. `jira/ABC-123.md` or
  `github/owner-repo-42.md`. The note is found again by its frontmatter,
  not its path, so the user may move it anywhere.
  - **Frontmatter:** `ft.tasks.anchor: <KEY>`, plus
    `ft.tasks.parent: "[[EPIC-1]]"` when the issue has a parent, plus an
    `ft.source.*` block (source, key, URL, remote status, assignee, state,
    remote `updated` timestamp, and `synced.status`).
  - **Body:** one anchor task line, `- [/] [ABC-123](url) Fix login ⏫ 📅
    2026-10-30 🆔 ABC-123`, and ft-managed `Description` and `Comments`
    sections between HTML-comment markers. Everything else in the note
    belongs to the user.
- **Sync rules.**
  - **Fields the remote owns:** title, due date, priority and status.
    They are overwritten from the remote. Local `⏳`, `🛫`, `#tags` and
    all subtasks are never touched.
  - **Pending local status:** if the anchor's status changed locally
    since the last sync (it differs from `ft.source.synced.status`), it
    is kept and reported as *pending push*; Phase 4 pushes it. If both
    sides changed, it is reported as a *conflict*.
  - **Dropping out of the queries:** ft fetches the issue's real status.
    If the remote closed it, the anchor is marked done. If it is still
    open but no longer matches a query (e.g. reassigned), the anchor is
    cancelled and the note marked `state: out-of-scope`.
  - **Parents and epics** are fetched as `state: context` notes, so the
    Jira hierarchy shows up in the vault through `ft.tasks.parent`.
  - **Determinism:** the output is byte-identical on every machine.
    There are no local timestamps, and ordering is stable.
  - **Untrusted text:** remote text is neutralised so remote checklists
    can't turn into vault tasks.
- **Guard on remote-owned fields.** `ops::update_task_line` refuses to
  change the title, due date or priority of a remote anchor task, with
  the hint `owned by jira — change it in Jira`. This covers `ft tasks
  edit`, `ft do`, and the TUI and graph edit and quick-mutation keys.
- **Hierarchy.** The Phase 2 resolver also treats an anchor note's
  `ft.tasks.parent` frontmatter as its anchor task's parent link.
- **CLI.**
  - `ft tasks sync [--source NAME] [--dry-run] [--format json]` exits 0
    even when a source is skipped (offline or no credentials), and
    reports per-source status.
  - `ft sources login <name>`, `ft sources logout <name>` and `ft
    sources list` (shows credential presence and last local sync time).
- **TUI.**
  - Sync runs once at startup and then every `interval_minutes`, driven
    by `Event::Tick`, as a background job like git sync. Workers only
    fetch over the network; planning and writing happen on the main
    thread through plan/apply.
  - `g r` syncs now. The status bar shows `⟳ sources`.
  - A toast appears only when something changed or failed (`jira: 2
    new · 5 updated · 1 pending push`).
- **DSL.** New task attributes: `source`, `key` and `remote_status`. A
  built-in preset `remote` (open anchor tasks from any source).
- **[ft.nvim]** A `:FtSync` command over `ft tasks sync --format json`.

## Capabilities

### New Capabilities

- `task-sources`: Source config; the `TaskSource` trait and the
  `RemoteIssue` model; credentials (keychain, env, `gh`); HTTP policy
  (timeouts, HTTPS-only, offline classification); `ft sources
  login/logout/list`; `ft tasks sync` surface and exit semantics.
- `task-source-sync`: The issue-note format; the pure sync planner
  (create, update, pending, conflict, out-of-scope, context parents);
  remote-owned fields and the edit guard; managed sections and text
  neutralisation; determinism; DSL attributes and preset.
- `task-source-jira`: Jira Cloud mapping (search API, status categories
  and the status map, priority, due date, parent and epic, description
  in Atlassian Document Format converted to markdown, comments).
- `task-source-github`: GitHub mapping (issue search, open/closed and
  state reason, sub-issue parent, markdown body, comments, keys).
- `tui-source-sync`: Startup and interval sync, the `g r` leader key,
  the job indicator, toasts, and main-thread apply.

### Modified Capabilities

- `task-logical-parents`: anchor notes may declare `ft.tasks.parent`,
  which resolves as their anchor task's parent link.

## Impact

- **New dependencies:** `ureq` (rustls TLS, JSON) and `keyring`, both in
  ft-core. This is the first network code and the first credential
  storage in ft.
- **ft-core:**
  - `sources/` (`mod.rs` for config and the trait, `http.rs`,
    `credentials.rs`, `jira.rs`, `adf.rs`, `github.rs`, `note.rs` for
    format and parse, `sync.rs` for the planner);
  - `frontmatter.rs` gains a general `ft.*` block upsert;
  - `config.rs` gains `sources`;
  - `task/ops.rs` gains the owned-field guard;
  - `task/links.rs` resolves `ft.tasks.parent`;
  - `scan.rs` / `graph` expose remote info to the DSL;
  - `query/preset.rs` gains the `remote` preset.
- **ft binary:** `cmd/sources.rs` (new), `cmd/tasks.rs` (`sync`), TUI
  `event.rs` / `jobs.rs` / `app.rs` (`BgEvent::SourcesFetched`,
  `JobKind::Sources`, tick-driven scheduling, the `g r` leader key).
- **Vault content:** new folders of issue notes, committed through git
  sync like any other notes.
- **Tests:** mapping tests on recorded JSON fixtures (no live network),
  planner tests against temporary vaults, an HTTP-layer test against a
  local `TcpListener` stub, and TUI tests with an injected fake source.
- **Performance:** scan and query cost are unchanged apart from reading
  `ft.source` frontmatter that the scan already captures. Sync cost is
  network-bound and runs off the UI thread.

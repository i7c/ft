## Context

- **Where Phases 1–2 left things.** Phases 1–2 delivered reparenting,
  cross-file parent links, and *anchor notes*: a note whose `ft.tasks.anchor`
  names a task (`🆔`) that stands for the note. `[[note]]` links to such a
  note make children of that task. Phase 3 materialises remote issues as
  anchor notes.
- **What the codebase offers today** (from a survey of the code):
  - **Background work:** one mpsc `EventStream` carries `Key`, `Tick`
    (every 1 s) and `Background(BgEvent)`. Git sync is the reference
    background job: `App.jobs: RefCell<Option<JobHandle>>`, a
    single-slot design (`jobs.rs`); the worker posts
    `BgEvent::SyncCompleted`; `handle_background` applies the result on
    the main thread. `Tick` currently only triggers a redraw.
  - **Config:** Figment layering, vault `.ft/config.toml` over user
    config, every struct `deny_unknown_fields`, maps of structs
    (`capture_presets`).
  - **No network, credentials or YAML library:** no HTTP client, no
    credential storage, no YAML writer. The frontmatter is hand-parsed.
    The only frontmatter writer is `synth::callout::upsert_synth_frontmatter`,
    a line-based upsert of one `ft.*` sub-block.
  - **Templates:** MiniJinja (`notes::template::render`, with
    `TemplateContext.vars`).
  - **Edit choke point:** `ops::update_task_line` for edits;
    `complete_task` writes on its own. Status isn't remote-owned, so
    that's fine.
  - **Shelling out:** external commands are run with
    `std::process::Command`, as in `git.rs`.
- **The user's environment and choices:**
  - Jira Cloud at `example.atlassian.net`.
  - The vault is git-synced to several machines, and not all of them
    can reach Jira.
  - The user picked: `ureq` + `keyring`; keep local status as *pending*
    until Phase 4; distinguish closed from reassigned; sync description
    and comments into the note.

## Goals / Non-Goals

**Goals:**
- Assigned Jira and GitHub issues show up as anchor tasks in ft (CLI,
  TUI, query language), with their epic/parent chain, ready to take
  local subtasks.
- Zero cost when offline. No machine depends on credentials.
- Sync output is deterministic and merge-friendly across machines.
- Remote data never corrupts the vault (no phantom tasks, no clobbered
  user text).

**Non-Goals (Phase 4 or later):**
- Pushing status to the remote, promoting a local subtask to a remote
  issue, resolving conflicts interactively.
- Editing remote titles, due dates or priorities from ft.
- Server or Data Center Jira, GitLab, Linear.
- Webhooks or live push. Polling is enough.
- Syncing attachments, worklogs, labels or components.

## Decisions

### D1. Read off-thread, write on the main thread

```
worker thread:  vault.scan() → credentials → fetch_all → plan_sync → SyncPlan (FileEdits: original + new)
main thread:    for each edit: write only if the file still equals `original` → request_graph_refresh
```

The worker does every read (scan, network, note contents) and plans; it
never writes. The main thread applies only edits whose file is still
byte-identical to what was planned. Anything edited in the TUI (or in
Obsidian) during the sync is skipped and picked up by the next one.

This keeps the TUI concurrency rule (workers own their inputs, post a
`BgEvent`, and exit). The UI never blocks on the scan. Planning against
a fresh scan, rather than the possibly stale shared snapshot, means a
note created moments earlier can't be duplicated. The CLI runs the same
steps inline.

`FetchResult` is pure data, so planner tests never need network.

### D2. Module layout in ft-core

```
sources/mod.rs          SourceCfg, TaskSource trait, RemoteIssue, SourceError, FetchResult, fetch_all()
sources/http.rs         ureq agent (timeouts, UA), error classification, Retry-After, pagination helpers
sources/credentials.rs  env → gh → keyring resolution; login/logout
sources/jira.rs         JQL search + key fetch, field mapping, status map
sources/adf.rs          ADF → markdown
sources/github.rs       search + key fetch + sub-issue parent + comments, key normalisation
sources/note.rs         issue-note render (new) and parse (existing → IssueNote view)
sources/sync.rs         plan_sync: create/update/scope/status rules, report
```

The scan stays unaware of sources. Issue notes are ordinary notes; their
`ft.source` frontmatter is read from `ParsedFile::frontmatter` when
needed.

### D3. The issue note as the only sync state

Everything another machine needs is in the note:
- `ft.source.synced.status` is the base for pending and conflict
  detection;
- `ft.source.state` records active, context or out-of-scope;
- `ft.source.updated` is the remote's own timestamp.

Remote timestamps are identical on every machine, and nothing local is
written, so two machines produce the same bytes (git sees no conflict).
The only machine-local file is `.ft/cache/sources.json`, which holds last
attempt times for interval scheduling and `ft sources list`. It is
informational, and losing it is harmless.

Considered: a vault-wide `sources.lock.json` index. Rejected, because it
would be a merge hotspot and would duplicate data the notes already
carry.

### D4. Finding issue notes

The planner indexes `scan.files` by `(ft.source.name, ft.source.key)`
from the captured frontmatter, with no extra reads. A note found there
is updated wherever it lives. When no note exists, a new one goes to
`<folder>/<key>.md`.

If that path already exists (for example, an unrelated note with the
same name), the new note becomes `<key> (<source>).md`, and the report
says so. If two notes claim the same key, the planner updates the
lexicographically first one and reports a duplicate warning.

### D5. Editing notes surgically

- **Frontmatter.** `frontmatter::upsert_ft_keys(content, sub, updates)`
  generalises the idea behind `upsert_synth_frontmatter`, setting or removing individual keys:
  - it rewrites only the named `ft.<sub>` blocks, with keys in a fixed
    order;
  - it creates the `---` fence or the `ft:` map if missing;
  - it preserves all other lines;
  - values are quoted when YAML needs it (`[[…]]`, a leading `@`,
    `: `, `#`).

  `upsert_synth_frontmatter` stays as it is: its keep-targets-when-absent
  contract differs, and rewiring it gains nothing.
- **Anchor line.** It is located by `🆔 == key`, parsed with the vault
  format, and its owned fields are replaced (D6). The result is
  serialized with the line's original leading whitespace. Missing anchor
  line (the user deleted it): it is re-inserted after the first heading,
  or at the top of the body.
- **Managed sections.** They are located by marker pair and their inner
  text replaced. Missing markers mean the section isn't re-added
  (respecting the user's deletion).

### D6. Owned versus local fields on the anchor line

- **Owned:** the title part of the description, priority, due date, and
  the status (via the D7 rules).
- **Description layout:** the description is rebuilt as `[KEY](url)
  <title>` followed by the line's existing hashtags. That keeps `#tags`
  local even though they live inside the description.
- **Local:** everything else in `Task` (scheduled, start, created,
  recurrence, block id, `raw_trailing`) is carried over from the parsed
  line.
- **Edit guard:** `update_task_line` compares the description with
  hashtags stripped, the due date and the priority before and after
  `mutate`. When any of them changed and the file is an issue note for
  that task, it returns `OwnedByRemote`. It reads the frontmatter from
  the content it already has in memory, so no new parameter ripples to
  callers.

### D7. Status rules (pending and conflict) before Phase 4

Three-way merge on status only, with base `synced.status`, *local* the
line and *remote* the mapped status (spec table). Phase 4 adds the push
step for "pending". Its "conflict" resolution reuses the same
classification, so this change defines the vocabulary once.

The out-of-scope `Cancelled` write sets `synced.status = Cancelled`, so
it is never mistaken for a local change to push.

### D8. Neutralising remote text

Remote markdown passes through `sources::note::neutralise`:
- task-shaped lines become `☐` / `☑` items;
- headings are demoted to at least `###`;
- a bare `---` becomes `***`, so a thematic break can't look like a
  frontmatter fence;
- managed-marker comments are stripped.

Wikilinks in remote text are left alone. They are rare, and are just
links. Remote text is inserted after template rendering, never passed
through MiniJinja, so there is no template injection.

### D9. HTTP and credentials

- **Client:** a single `ureq::Agent` with rustls, timeouts (connect 5 s,
  overall 20 s) and `User-Agent: ft/<ver>`.
- **Error classification:** transport errors whose cause is DNS,
  connection or timeout become `Offline`; 401/403 become
  `Unauthorized`; 429 retries once after `Retry-After` (capped at
  60 s).
- **Credential order:** `FT_<NAME>_TOKEN`, then `gh auth token` (GitHub
  sources only; run with a 5 s timeout and empty stdin), then
  `keyring::Entry::new("ft", "<name>:<account>")`.
- **Secret hygiene:** tokens are held in a `Secret(String)` newtype
  whose `Debug` / `Display` impls redact the value.

### D10. TUI scheduling

The App gains:
- a `Schedule` (`HashMap<String, Instant>` of next due times). It
  isn't seeded from `.ft/cache/sources.json`, because the startup sync
  always runs and resets every source's next due time;
- a `Tick` arm that, when the job slot is free and any source is due,
  spawns the fetch worker for the due sources.

The startup sync is the first tick after the first `GraphReady`. `g r`
marks every source due and requests an immediate tick. `JobKind::Sources`
is added beside `Sync` / `Commit`, with the label `sources`. A tick that
finds the slot busy just tries again on the next tick.

### D11. DSL plumbing

`TaskData` is at the `large_enum_variant` limit (Phase 2 had to shrink
`parent_link`). Remote info therefore lives in a side table,
`Graph::task_remote: HashMap<NoteId, RemoteData { source, key,
remote_status }>`. It is filled at build time for anchor tasks of issue
notes (frontmatter is already in `ParsedFile`). Evaluation of `source` /
`key` / `remote_status` reads that table.

### D12. Hierarchy from frontmatter

`resolve_parent_links` gains one input: for anchor notes with
`ft.tasks.parent`, the anchor task receives a candidate built from the
frontmatter value. It is classified by the same function as line links,
so cycles, dangling and ignored behave identically. A parent link on the
anchor line wins, and the frontmatter value is then recorded as extra.

## Risks / Trade-offs

- **Remote API drift.** Jira's search endpoint moved in 2025 to
  token-paginated `/search/jql`. Mapping code is isolated per source and
  tested on recorded fixtures. Task 3.1 verifies the live endpoint once
  with a real token, the only manual step.
- **New dependencies** (`ureq`, `rustls`, `keyring`) grow the binary and
  build time. They are scoped to ft-core and justified as the first
  network feature. `keyring` falls back to an error on platforms without
  a store; env vars still work there.
- **Remote text in the vault** (description and comments) makes notes
  larger and git history noisier. It is bounded by `comments` (default
  5), and `description = false` / `comments = 0` turn the sections off
  per source.
- **Rewriting notes on another machine's schedule** can collide with a
  user editing the same note in Obsidian. All edits go through
  `write_atomic` after a fresh read, and edits only touch owned regions.
  A concurrent Obsidian edit to the anchor line between read and write
  is the residual race. It's the same class as every other ft mutation,
  and git sync shows it.
- **"Pending push" piles up until Phase 4.** It's visible in every sync
  summary, and harmless.
- **The single job slot** delays source sync while git sync runs. That's
  acceptable at minute-scale intervals.

## Roadmap notes (Phase 4)

- **Push:** for each issue note where `local status != synced.status`,
  transition the remote. Jira needs a configurable `transitions` map
  from target `Status` to transition name. GitHub closes with
  `state_reason` (Done → completed, Cancelled → not_planned) or reopens.
  On success, set `synced.status`.
- **Conflicts:** `ft tasks sync --resolve` and a TUI modal pick local or
  remote per issue.
- **Promote:** `ft tasks promote <sel>` creates a remote sub-issue,
  writes its issue note (with `ft.tasks.parent` pointing at the source
  issue), and moves the line's subtree under the new anchor
  (`plan_move` + `plan_retarget`).
- **Offline:** pending pushes need no outbox. They are derived from the
  notes on whichever machine next has credentials and network.

## Open Questions

None blocking.

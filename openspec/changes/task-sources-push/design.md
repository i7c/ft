## Context

Phase 3 (`task-sources`) syncs remote issues into issue notes. The
planner already classifies each status three ways:
- *write remote* when the user didn't touch it (local == base);
- *pending push* when only the local side changed (remote == base);
- *conflict* when both sides changed.

Conflicts, however, only reach the report: they aren't stored, and
nothing is written to the remote. The source layer is fetch-only:
`TaskSource { fetch_query, fetch_keys }` over an `Http` client that only
has `get_json`.

What Phase 3 established and this phase keeps:
- the vault is the source of truth, and all sync state lives in notes;
- no outbox;
- a background worker does the reads (scan, network, planning), and the
  main thread writes only unchanged files;
- machine-local data goes in `.ft/cache/sources.json`.

The user's decisions for this phase:
- push on every sync, background included;
- Jira transitions chosen by category match with an override;
- cancels are pushed only through a configured transition;
- promote requires a connection;
- promote creates a sub-task under the parent (a Task under an epic);
- the promoted line becomes the new issue note;
- conflicts are resolvable from the CLI and a TUI modal;
- failed pushes stay pending.

## Goals / Non-Goals

**Goals:**
- Local status changes reach Jira/GitHub automatically, safely and
  observably.
- Conflicts are persistent, visible offline, and resolvable offline.
- One-step promotion of a local subtask to a real remote issue, with the
  vault restructured to match.

**Non-Goals:**
- Pushing title, due date, priority or other fields (they stay
  remote-owned).
- Comments or worklogs.
- Creating top-level issues that have no remote parent.
- Bulk promote.
- Promote while offline (queuing was rejected because of duplicate-issue
  races across machines).

## Decisions

### D1. Push before fetch, in the same worker run

```
worker: scan → pending = notes where anchor ≠ synced.status ∧ ¬conflict ∧ state ∈ {active, context}
        for each pending: source.push_status(ref, target, transitions) → PushOutcome
        fetch_all(...)                       # pushed issues come back fresh
        plan_sync(...)                       # writes synced.status = new remote status
main:   apply unchanged files; record push failures in cache; toast
```

Pushing first means the fetch that follows already sees the new remote
state. The planner then records it through the normal *write remote*
path. No extra "mark synced" edit is needed, and the note edit stays a
single, guarded write.

Considered: push after planning and then patch `synced.status`. Rejected,
because it needs a second write to the same note, and it would record a
status the remote never confirmed.

Before pushing, `push_status` re-reads the remote status. If it already
equals the target, the result is `AlreadyThere`, which makes pushes from
several machines idempotent.

### D2. Jira transitions

1. `GET /rest/api/3/issue/{key}/transitions` returns
   `[{id, name, to: {name, statusCategory: {key}}}]`.
2. Choose per the spec: configured name, else a unique category match,
   with a preference for the category's default name.
3. `POST /rest/api/3/issue/{key}/transitions` with
   `{"transition": {"id": …}}`.

No resolution field is sent. Workflows that require a resolution screen
fail with Jira's 400 message, which is reported as a push failure.

### D3. GitHub state changes

`PATCH /repos/{o}/{r}/issues/{n}` with `{"state": "closed",
"state_reason": "completed" | "not_planned"}` or `{"state": "open"}`. The
repository and number come from the note's URL (`IssueRef.url`), as in
the Phase 3 lookups.

### D4. Conflicts as note state

`ft.source.conflict: <Status>` is written by `plan_sync` whenever the
rules yield a conflict, and removed when they don't. It is deterministic
from the remote state and the note, so the multi-machine guarantee
holds.

Resolution (`resolve_conflict`) is a pure note edit:
- *take remote* writes the remote status and syncs it;
- *take local* re-bases `synced.status` onto the remote status, so the
  local status becomes an ordinary pending push.

Both work offline. The TUI modal and the CLI `--resolve` / `--take` share
the planner.

### D5. Push failures live in the machine-local cache

`AttemptLog` gains `push_failures: {source: {key: reason}}`.
- **Why there:** reasons can differ per machine (permissions,
  connectivity), and persisting them in notes would make machines fight
  over note content.
- **Readers:** `ft tasks check` and the toast logic. A toast appears only
  when a key's reason is new or has changed, so the TUI doesn't nag every
  10 minutes.
- **Clearing:** a key's entry is cleared on success, and when the note is
  no longer pending.

### D6. Promote = remote create + three guarded local steps

**Worker (network):**
1. `issue_meta(parent)` returns the parent's project key and hierarchy
   level (Jira: `GET /rest/api/3/issue/{key}?fields=issuetype,project`).
2. `create_child`:
   - **Jira:** the sub-task type comes from
     `GET /rest/api/3/issue/createmeta/{project}/issuetypes` (with
     `subtask: true`); the epic case uses the standard `Task` type.
     Then `POST /rest/api/3/issue` with `{project, parent, summary,
     issuetype, assignee: {accountId from /myself}, duedate, priority}`.
   - **GitHub:** `POST /repos/{o}/{r}/issues` (title, `assignees: [me]`),
     then `POST /repos/{o}/{r}/issues/{parent}/sub_issues` with
     `{sub_issue_id}`.
3. Push the initial status if it isn't Open.
4. Fetch the new issue by key, giving a full `RemoteIssue`.

**Main thread (vault): one guarded plan, applied all-or-nothing:**
1. **New note:** produced by `plan_sync` over a `FetchResult` that has
   the new issue as active. It is the same code path as a regular sync,
   so the output is byte-identical to what the next sync would produce.
   If the promoted line has a block id, it goes on the anchor line.
2. **The block moves:** the promoted line's whole block (its subtasks
   and any loose note lines) is re-indented under the new anchor
   (`ops::reindent_block`), and the line plus block are removed from
   their file. Moving the whole block avoids orphaning the loose note
   lines that per-child `plan_move` calls would leave behind.
3. **References follow:** the edit is exactly a move of one block from
   the old file to the new anchor's position, so a synthetic `MovePlan`
   (one `MovedBlock` with `dest`) goes through the existing
   `plan_retarget`. Every `[[…#^id]]` to the line or its subtasks is
   rewritten, including same-file refs inside the block.
   `LinkCtx::including(new note)` gives the short `[[ABC-124#^id]]`
   form.
4. **Apply:** `apply_guarded` writes only if every file still holds its
   planned original; otherwise nothing is written.

The steps aren't one atomic transaction, because the remote issue
already exists by then. If a local step fails, the command reports the
created key, and the next regular sync still creates the note, since the
issue is assigned to the user and in scope.

### D7. TUI

- **Pushes** run inside `SourcesJob::run` (D1); `SourcesJobResult`
  gains per-source push results.
- **Conflict modal:** a new `ActiveModal::ConflictResolve` with rows
  `{note path, key, title, local, remote, choice}`, opened by
  `apply_sources_result` when conflicts are new in this session (or the
  sync was manual).
- **Promote job:** `JobKind::Promote` (indicator `promote`) and
  `BgEvent::PromoteDone`, mirroring the sources job. The worker does
  steps 1–4 of D6's remote part; the main thread applies D6's local
  steps.

### D8. Safety

- **Scope of writes:** only status transitions and issue creation, and
  only on issues tied to notes ft created (the source and key come from
  `ft.source`).
- **Visibility:** every write appears in the sync or promote output.
- **Opt-out:** `push = false` gives a read-only source.
- **Tests:** all assert method, path and body against the local stub;
  none touches a real remote.

## Risks / Trade-offs

- **Unexpected automatic writes to company Jira.** Mitigated by
  status-only pushes, visible summaries, the `push = false` opt-out, and
  the rule that only notes whose anchor the user changed are pushed.
- **Workflow quirks** (required fields on transitions, multiple Done
  states). They surface as push failures with Jira's message; overrides
  per status fix them.
- **Promote is a multi-step write without atomicity.** The remote is
  created first, and local failures are reported with the key; the next
  sync heals the missing note.
- **More network round-trips per sync** (a transition list and a POST
  per pending push). Pending pushes are rare, so this is negligible.

## Open Questions

None blocking.

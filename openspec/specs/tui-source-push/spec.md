# tui-source-push Specification

## Purpose
Push and promote in the TUI: pushes inside the background source job, the
conflict-resolution modal, the `I` promote background job, and toasts.
Created by change task-sources-push.

## Requirements

### Requirement: Pushes inside the source job

The background source job SHALL run the push step (worker thread, network) before fetching, for each due source with `push` enabled. Push results SHALL travel back in `SourcesJobResult`. The toast summary SHALL include `N pushed` when there were pushes, plus push failures whose reason is new compared with the machine-local cache.

#### Scenario: Tick in the TUI reaches Jira
- **WHEN** the user presses `x` on a Jira anchor and the next interval sync runs online
- **THEN** the toast reads `jira: 0 new · 1 updated · 1 pushed`

### Requirement: Conflict resolution modal

When a sync's result contains conflicts, the TUI SHALL open the `ConflictResolve` modal once. It lists each conflicted issue as `KEY  title  local=<status>  remote=<status>  → <choice>`.
- `j`/`k` move between rows.
- `l` chooses *take local*, `r` chooses *take remote*, `s` chooses *skip* (the default).
- `Enter` applies the choices through the same local resolution as `ft tasks sync --take`, then refreshes the graph.
- `Esc` closes the modal without changes.

The modal SHALL NOT reopen for conflicts already shown in this TUI session, unless they come from a manual `g r`.

#### Scenario: Take remote
- **WHEN** the modal lists `ABC-123 local=Done remote=Cancelled`, and the user presses `r` and then `Enter`
- **THEN** the anchor becomes `[-] … ❌ <date>`, `synced.status` becomes `Cancelled`, and the conflict key is removed

### Requirement: Promote from the Tasks tab

`tasks.promote` (`I`) SHALL promote the cursor task. Without a selected task it SHALL toast `select a task first`. When the cursor task has no remote-anchor parent, it SHALL toast the precondition message without any network call.

Otherwise it SHALL run as a background job (`JobKind::Promote`, status-bar indicator `promote`):
- **Worker:** creates and fetches the remote issue.
- **Main thread:** plans and applies the local restructuring with the unchanged-file guard, raises a graph refresh, puts the cursor on the new anchor, and toasts `promoted to ABC-124`.

Failures SHALL toast the error, and SHALL include the created key when the remote side succeeded.

#### Scenario: Promote while browsing
- **WHEN** the user presses `I` on a linked subtask of a Jira story while online
- **THEN** the UI stays responsive, and when the job finishes the task appears as anchor `ABC-124` under the story, with its former children nested below

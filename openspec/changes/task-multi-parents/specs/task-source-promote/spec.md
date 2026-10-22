## MODIFIED Requirements

### Requirement: Promote preconditions

`ft tasks promote <SELECTOR> [--parent <KEY>]` SHALL accept a single task. The task:
- must not already be an issue anchor;
- must have at least one *eligible* parent: an effective parent that is the anchor task of an issue note belonging to a configured source with `push` enabled;
- must belong to a source with resolvable credentials, and the remote must be reachable.

The remote parent SHALL be chosen as follows:
- With `--parent KEY`, it must be one of the eligible parents, else the command fails with `KEY is not a ticket parent of this task`.
- Without `--parent`, exactly one eligible parent is used. With more than one, promote fails with `several ticket parents (<KEY>, <KEY>…) — pass --parent KEY` and exits 2. The `--json-errors` object carries a `candidates` array of `{"key", "path", "line", "description"}`.

Promote SHALL fail fast, writing nothing and queuing nothing, with:
- `promote needs a parent that is a remote issue — move or link it under one first` when no parent is eligible;
- `<source>: offline — promote needs a connection` when the remote can't be reached;
- the credential hint from `ft tasks sync` when there are no credentials;
- `<KEY> is a sub-task; Jira can't nest sub-tasks` when the chosen parent is a Jira sub-task.

#### Scenario: Parent is a local task
- **WHEN** the user promotes a task whose only parent is an ordinary local task
- **THEN** the command fails with the parent message and no file or remote changes

#### Scenario: One ticket among several parents
- **WHEN** the task has `⤴ k3x9qa,ABC-123`, where `k3x9qa` is local and `ABC-123` is a Jira anchor
- **THEN** promote uses `ABC-123` without asking

#### Scenario: Two ticket parents
- **WHEN** the task has `⤴ ABC-123,ABC-200` (both Jira anchors) and no `--parent`
- **THEN** promote exits 2 listing both keys, and nothing is created

#### Scenario: Offline
- **WHEN** the machine is offline
- **THEN** promote fails with `jira: offline — promote needs a connection` and nothing is queued

### Requirement: Local restructuring after promote

After the remote issue `NEW-1` is created and fetched, under chosen parent `KEY`, promote SHALL plan and apply one set of local edits:
1. **New note:** create the issue note for `NEW-1` with the issue-note format (state `active`, `ft.tasks.parent: KEY`) in the source's folder.
2. **References:** the new anchor line SHALL carry the promoted line's `⤴` ids except `KEY`. If the promoted task was physically nested under a local task other than `KEY`'s anchor, that task's `🆔` (assigned if missing, in the same plan) SHALL be added to the new anchor line's `⤴`, so the relation survives the move.
3. **Children:** move the promoted task's direct children (the whole block below the line) under the new anchor line as children, via `plan_move`.
4. **Remove the line:** delete the promoted task's own line from its file.
5. **Block id:** if the promoted line had a block id, the new anchor line takes it, and every reference to `<old file>#^<id>` is retargeted to the new note, as with a cross-file move.
6. **Task id:** if the promoted line had a `🆔`, references to it in other tasks' `⤴` fields SHALL be rewritten to `NEW-1` in the same plan, so its children by reference follow it.

All edits SHALL be guarded against stale lines. If the local apply fails after the remote issue was created, promote SHALL report the created key and the failure (`created NEW-1 but couldn't update the vault: …`), so the next sync can still create the note.

#### Scenario: Promote a multi-parent daily-note task with subtasks
- **WHEN** `journal/2026-10-21.md` has `- [ ] write regression test ⤴ ABC-123,p2m7rd 🆔 wr1abc` with child `  - [ ] cover Safari`, and `notes.md` has `- [ ] doc it ⤴ wr1abc`
- **AND** the user promotes it and Jira creates `ABC-124` under `ABC-123`
- **THEN** `jira/ABC-124.md` exists with `ft.tasks.parent: ABC-123`, the anchor `- [ ] [ABC-124](…) write regression test ⤴ p2m7rd … 🆔 ABC-124`, and the child `- [ ] cover Safari` nested under it. The journal line and its child are gone from the journal, and `notes.md` now reads `- [ ] doc it ⤴ ABC-124`

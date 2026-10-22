# task-source-promote Specification

## Purpose
Promoting a local subtask to a real remote issue (Jira sub-task or GitHub
sub-issue): preconditions, creation rules and fields, the local
restructuring (new issue note, children moved, line removed, block id taken
over), and CLI/JSON output. Created by change task-sources-push.

## Requirements

### Requirement: Promote preconditions

`ft tasks promote <SELECTOR>` SHALL accept a single task. The task:
- must not already be an issue anchor;
- must have an effective parent that is the anchor task of an issue note;
- must belong to a source with `push` enabled and resolvable credentials, and the remote must be reachable.

Promote SHALL fail fast, writing nothing and queuing nothing, with:
- `promote needs a parent that is a remote issue — move or link it under one first` when the parent isn't an anchor;
- `<source>: offline — promote needs a connection` when the remote can't be reached;
- the credential hint from `ft tasks sync` when there are no credentials;
- `<KEY> is a sub-task; Jira can't nest sub-tasks` when the parent is a Jira sub-task.

#### Scenario: Parent is a local task
- **WHEN** the user promotes a task whose parent is an ordinary local task
- **THEN** the command fails with the parent message and no file or remote changes

#### Scenario: Offline
- **WHEN** the machine is offline
- **THEN** promote fails with `jira: offline — promote needs a connection` and nothing is queued

### Requirement: Creating the remote issue

**Jira:** the source SHALL read the parent's issue type and project, then:
- for a standard-level parent (hierarchy level 0), create an issue with the project's sub-task issue type, `parent = <parent key>`;
- for an epic-level parent (hierarchy level 1), create an issue with the project's standard `Task` type (or, if absent, the first standard type), `parent = <epic key>`;
- for a sub-task parent, refuse.

**GitHub:** the source SHALL create an issue in the parent's repository, then add it as a sub-issue of the parent.

For both, the created issue SHALL take:
- **summary/title:** the task description with wikilinks, markdown links, hashtags and the block id removed;
- **due date and priority:** from the task, Jira only;
- **assignee:** the authenticated user.

If the local status isn't Open, the status push SHALL be applied immediately after creation.

#### Scenario: Sub-task under a story
- **WHEN** a task `write regression test 📅 2026-10-28` linked under the anchor of story `ABC-123` is promoted
- **THEN** Jira gets a sub-task of `ABC-123` with summary `write regression test`, due 2026-10-28, assigned to the user

#### Scenario: Task under an epic
- **WHEN** the parent anchor is epic `EPIC-7`
- **THEN** Jira gets a Task with parent `EPIC-7`

### Requirement: Local restructuring after promote

After the remote issue `NEW-1` is created and fetched, promote SHALL plan and apply one set of local edits:
1. **New note:** create the issue note for `NEW-1` with the Phase 3 format (state `active`, `ft.tasks.parent` pointing at the parent's note) in the source's folder.
2. **Children:** move the promoted task's direct children (the whole block below the line) under the new anchor line as children, via `plan_move`.
3. **Remove the line:** delete the promoted task's own line from its file.
4. **Block id:** if the promoted line had a block id, the new anchor line takes it, and every reference to `<old file>#^<id>` is retargeted to the new note, as with a cross-file move.

All edits SHALL be guarded against stale lines. If the local apply fails after the remote issue was created, promote SHALL report the created key and the failure (`created NEW-1 but couldn't update the vault: …`), so the next sync can still create the note.

#### Scenario: Promote a linked daily-note task with subtasks
- **WHEN** `journal/2026-10-21.md` has `- [ ] write regression test [[ABC-123]] ^wr1` with child `  - [ ] cover Safari`, and `notes.md` links `[[2026-10-21#^wr1]]`
- **AND** the user promotes it and Jira creates `ABC-124`
- **THEN** `jira/ABC-124.md` exists with anchor `- [ ] [ABC-124](…) write regression test … 🆔 ABC-124 ^wr1` and the child `- [ ] cover Safari` nested under it; the journal line and its child are gone from the journal; `notes.md` now links `[[ABC-124#^wr1]]`

### Requirement: Promote output

The command SHALL print `Promoted to ABC-124 (<url>) → jira/ABC-124.md`. With `--format json` it SHALL print `{"key", "url", "note": <vault-relative path>, "moved_children": N}`.

#### Scenario: JSON for editor clients
- **WHEN** ft.nvim runs `ft tasks promote journal.md:3 --format json`
- **THEN** stdout is the JSON object, and the client can open `note`

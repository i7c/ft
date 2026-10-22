# task-source-push Specification

## Purpose
Pushing local status changes back to remote sources: pending-push detection
from notes, Jira transition selection, GitHub close/reopen, the refresh
after a push, failure handling and reporting, and opting out. Created by
change task-sources-push.

## Requirements

### Requirement: Pending pushes are derived from notes

An issue note SHALL be a *pending push* when all of these hold:
- its source has `push` enabled (the default);
- its anchor line's status differs from `ft.source.synced.status`;
- it has no `ft.source.conflict` key;
- its `ft.source.state` is `active` or `context`.

No other record of pending work SHALL exist; any machine with credentials derives the same set from the vault.

#### Scenario: Ticked anchor is pending
- **WHEN** the anchor of `jira/ABC-123.md` is `[x]`, `synced.status` is `InProgress`, and there's no conflict key
- **THEN** `ABC-123` is a pending push with target status Done

#### Scenario: Out-of-scope notes are not pushed
- **WHEN** an out-of-scope note's anchor differs from `synced.status`
- **THEN** it is not pushed

### Requirement: Push step in every sync

Every sync of a source with `push` enabled SHALL push before fetching. This covers the TUI startup, interval and `g r` syncs, and `ft tasks sync` without `--no-push`. For each pending issue, the push SHALL:
1. **Check the remote first.** Skip the push if the remote is already in the target status.
2. **Push.** Apply the transition (Jira) or state change (GitHub).
3. **Record success.** Mark the issue as pushed. The fetch that follows refreshes its note, so `synced.status` becomes the new remote status and the remote status name updates.

A push SHALL never modify fields other than status. The `ft tasks sync` summary SHALL report `N pushed`, and failures per issue.

#### Scenario: Tick reaches Jira
- **WHEN** the user ticks `ABC-123` (In Review) and the next background sync runs online
- **THEN** ft applies the Done-category transition to `ABC-123`, the refreshed note has `synced.status: Done` and the remote status name (e.g. `Done`), and the summary says `1 pushed`

#### Scenario: Already done remotely
- **WHEN** `ABC-123` is pending Done but was resolved in Jira meanwhile
- **THEN** no transition is attempted and the refresh records `synced.status: Done`

### Requirement: Jira transition selection

For a pending Jira issue with target status `S`, the source SHALL list the issue's available transitions and choose one as follows:
- **Configured override:** if `transitions.<S>` is configured, use the transition with exactly that name; if it isn't available, the push fails with `no transition "<name>" available from <status>`.
- **Category match:** otherwise, for `S` in `Open`, `InProgress` or `Done`, use the unique transition whose target status category is `new`, `indeterminate` or `done` respectively. When several match, prefer the one whose target status name equals the category's default name (`To Do`, `In Progress`, `Done`); if there is still no single winner, the push fails as ambiguous, listing the candidates.
- **Cancelled:** for `S = Cancelled` without a configured transition, the push fails with `cancel isn't pushed — set transitions.Cancelled`.

#### Scenario: Category match
- **WHEN** `ABC-123` has transitions `Start Progress` (→ In Progress, indeterminate) and `Resolve` (→ Done, done), and the target is Done
- **THEN** `Resolve` is applied

#### Scenario: Override
- **WHEN** `transitions = { Done = "Ship it" }` and `Ship it` is available
- **THEN** `Ship it` is applied

#### Scenario: Cancel without configuration
- **WHEN** the user cancels a Jira anchor locally and no `transitions.Cancelled` exists
- **THEN** nothing is sent, the issue stays pending, and the reason is reported

### Requirement: GitHub state push

For GitHub issues, the push SHALL:
- **Done:** close the issue with `state_reason=completed`;
- **Cancelled:** close it with `state_reason=not_planned`;
- **Open / InProgress:** reopen a closed issue. An already open issue needs no push for either.

#### Scenario: Cancel closes as not planned
- **WHEN** a GitHub anchor is cancelled locally and pushed
- **THEN** the issue is closed with `state_reason=not_planned`

### Requirement: Push failures

A push that fails remotely, or that has no applicable transition, SHALL leave the note unchanged and the issue pending, and SHALL be retried on every later sync. The failure reason SHALL be stored per `(source, key)` in the machine-local `.ft/cache/sources.json`, and cleared on success. It SHALL be shown:
- in `ft tasks sync` output (`  push failed ABC-123: <reason>`);
- in `ft tasks check` (kind `push-failed`);
- in a TUI toast only when the reason is new or has changed.

`Offline` and `Unauthorized` push errors SHALL end the push step for that source without recording per-issue failures.

#### Scenario: Permission denied
- **WHEN** a transition returns 403 for `ABC-123`
- **THEN** `ABC-123` stays pending, `ft tasks check` lists `push-failed ABC-123: forbidden (403)`, and the next background sync retries without toasting again

### Requirement: Opting out

`push = false` on a source SHALL disable both the push step and promote for that source. `ft tasks sync --no-push` SHALL skip the push step for that run.

#### Scenario: Read-only source
- **WHEN** `[sources.jira] push = false` and an anchor is ticked
- **THEN** syncs keep reporting it as pending push and never call the remote to change it

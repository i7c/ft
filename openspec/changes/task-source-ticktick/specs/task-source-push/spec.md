## MODIFIED Requirements

### Requirement: Push step in every sync

Every sync of a source with `push` enabled SHALL push before fetching. This covers the TUI startup, interval and `g r` syncs, and `ft tasks sync` without `--no-push`. For each pending issue, the push SHALL:
1. **Check the remote first.** Skip the push if the remote is already in the target status.
2. **Push.** Apply the transition (Jira), state change (GitHub) or task completion / status update (TickTick).
3. **Record success.** Mark the issue as pushed. The fetch that follows refreshes its note, so `synced.status` becomes the new remote status and the remote status name updates. For an issue pushed in this sync, the refreshed remote status SHALL win over the local one, even when it differs from the pushed target (e.g. a recurring task that advanced instead of closing). The issue SHALL be neither pending nor in conflict afterwards.

A push SHALL never modify fields other than status. The `ft tasks sync` summary SHALL report `N pushed`, and failures per issue.

#### Scenario: Tick reaches Jira
- **WHEN** the user ticks `ABC-123` (In Review) and the next background sync runs online
- **THEN** ft applies the Done-category transition to `ABC-123`, the refreshed note has `synced.status: Done` and the remote status name (e.g. `Done`), and the summary says `1 pushed`

#### Scenario: Already done remotely
- **WHEN** `ABC-123` is pending Done but was resolved in Jira meanwhile
- **THEN** no transition is attempted and the refresh records `synced.status: Done`

#### Scenario: Remote answers a push differently
- **WHEN** a pushed Done comes back from the fetch as Open, because the remote advanced a recurring task to its next occurrence
- **THEN** the anchor line becomes Open with the remote's new due date, `synced.status` is Open, and the next sync pushes nothing

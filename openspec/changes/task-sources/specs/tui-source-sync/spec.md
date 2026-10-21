## ADDED Requirements

### Requirement: Background source sync job

The TUI SHALL run source sync as a background job:
- **Fetch:** a worker thread resolves credentials and fetches all remote data for the due sources, then posts `BgEvent::SourcesFetched` and exits. The worker never writes files.
- **Apply:** on the main thread, the App plans the sync against the vault (reading notes fresh), applies it through `write_atomic`, records the last sync time, and raises a graph refresh.
- **Slot:** the job uses `JobKind::Sources` in the existing single job slot. While another job (git sync or commit) is in flight, a due source sync waits for a later tick.
- **Indicator:** the status bar shows `⟳ sources` while the job runs.

#### Scenario: Sync while browsing
- **WHEN** a background source sync is running and the user keeps navigating the Tasks tab
- **THEN** input stays responsive, and when the result arrives the notes are written and the tab re-derives from the refreshed snapshot

### Requirement: When sync runs

A source SHALL be due:
- once at TUI start, after the first graph snapshot is installed;
- whenever `interval_minutes` (when it is above 0) has elapsed since that source's last attempt, checked on `Event::Tick`;
- on demand through `app.sync-sources`, bound to `r` in the git leader (`g r`).

Offline and no-credentials outcomes count as attempts, so the interval also throttles retries.

#### Scenario: Offline laptop
- **WHEN** the TUI runs with no network for an hour with `interval_minutes = 10`
- **THEN** about 6 attempts happen, each classified `offline`, with no toast and no file change; the status bar briefly shows the job indicator only

### Requirement: Sync feedback

After applying, the App SHALL show a toast only when something happened:
- **Changes (success toast):** listed per source, e.g. `jira: 2 new · 5 updated · 1 pending push`.
- **Conflicts:** included in that toast, with `ft tasks sync` named as the place to see details.
- **Errors (error toast):** `unauthorized` and `error` outcomes, e.g. `gh: unauthorized — run ft sources login gh`.

`offline`, `no-credentials` and no-change outcomes SHALL produce no toast. The `g r` on-demand sync SHALL always toast a summary, including `no changes` and `offline`.

#### Scenario: Manual sync while offline
- **WHEN** the user presses `g r` without network
- **THEN** a toast reads `sources: offline — nothing synced`

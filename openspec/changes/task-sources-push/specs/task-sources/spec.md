## ADDED Requirements

### Requirement: Push configuration

`SourceCfg` SHALL accept:
- `push` (bool, default `true`);
- `transitions`: a map from `Open` / `InProgress` / `Done` / `Cancelled` to a transition name. It is valid for Jira only; on a GitHub source, config validation fails with `sources.<name>.transitions is only valid for jira`.

#### Scenario: Cancel transition configured
- **WHEN** the config has `[sources.jira.transitions]\nCancelled = "Won't Do"`
- **THEN** config loads, and cancel pushes use `Won't Do`

### Requirement: Write operations on `TaskSource`

`TaskSource` SHALL gain:
- `push_status(&self, issue: &IssueRef, target: Status, transitions: &BTreeMap<Status, String>) -> Result<PushOutcome, SourceError>`, where `PushOutcome` is `Pushed`, `AlreadyThere`, or `NotPushable(reason)`;
- `create_child(&self, parent: &IssueRef, fields: &NewIssue) -> Result<RemoteIssue, SourceError>`, where `NewIssue` carries the title, due date, priority and initial status.

The HTTP client SHALL gain `post_json(url, body)`, with the same timeouts, classification and secret handling as `get_json`. GitHub state changes use `PATCH` through an equivalent `patch_json`.

#### Scenario: Stubbed writes in tests
- **WHEN** tests run push and promote against the local stub server
- **THEN** request method, path and JSON body are asserted, and no real remote is contacted

### Requirement: `ft tasks sync` push and resolve flags

`ft tasks sync` SHALL accept:
- `--no-push`: skip the push step;
- `--resolve`: interactive per-conflict resolution;
- `--take local|remote <KEY>...`: non-interactive resolution of the given keys.

`--resolve` and `--take` SHALL resolve stored conflicts locally and exit without fetching, so they work offline. The text summary SHALL add `· N pushed` when pushes happened, and print `  push failed <KEY>: <reason>` lines. The JSON summary SHALL add `pushed` (a count) and `push_failures` (an array of `{key, reason}`).

#### Scenario: Scripted resolution offline
- **WHEN** offline, the user runs `ft tasks sync --take remote ABC-123`
- **THEN** the conflict is resolved in the note, and no network is attempted

# task-sources Specification

## Purpose
Remote task sources (Jira, GitHub, TickTick): source configuration, the `TaskSource`
trait and `RemoteIssue` model, credentials (keychain, env, `gh`), HTTP
policy and offline classification, `ft sources login/logout/list`, and the
`ft tasks sync` surface and exit semantics. Created by change task-sources.
## Requirements
### Requirement: Source configuration

`Config` SHALL accept a `sources` map of named sections (`[sources.<name>]`), each `deny_unknown_fields`:

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `kind` | `"jira"` \| `"github"` \| `"ticktick"` | required | Backend |
| `base_url` | string | required for jira; github: `https://api.github.com`; ticktick: `https://api.ticktick.com` | e.g. `https://example.atlassian.net`, `https://api.dida365.com`. Must be `https://` (plain `http://` is accepted only for `localhost`) |
| `email` | string | required for jira | Atlassian account email (basic auth user) |
| `queries` | map name → string | jira: `{ mine = "assignee = currentUser() AND statusCategory != Done" }`; github: `{ mine = "is:issue is:open assignee:@me" }`; ticktick: `{ mine = "" }` (all open tasks) | JQL / GitHub search / TickTick query syntax; the union of results is "in scope" |
| `folder` | string | the source name | Vault-relative folder for new issue notes |
| `template` | string | none | Note template (in `templates_dir`) for the user part of new issue notes |
| `interval_minutes` | int | 10 | TUI background sync interval; `0` disables interval sync |
| `fetch_parents` | bool | true | Also fetch each in-scope issue's parent chain |
| `comments` | int | jira, github: 5; ticktick: 0 | Most recent comments kept in the note; `0` disables the section |
| `description` | bool | true | Keep the managed `Description` section |
| `status_map` | map remote status name → `Open`\|`InProgress`\|`Done`\|`Cancelled` | `{}` | Overrides the default category mapping |
| `delete_missing` | bool | jira, github: false; ticktick: true | Delete untouched notes of issues the remote no longer returns at all; see task-source-sync |

The section SHALL normally live in the vault config (`.ft/config.toml`) so every machine syncing the vault shares it. User config may override it, following the existing layered merge.

#### Scenario: Minimal Jira source
- **WHEN** the vault config contains `[sources.jira]\nkind = "jira"\nbase_url = "https://example.atlassian.net"\nemail = "me@example.test"`
- **THEN** config loads, with query `mine` defaulted, folder `jira`, interval 10, comments 5

#### Scenario: Minimal TickTick source
- **WHEN** the vault config contains `[sources.ticktick]\nkind = "ticktick"`
- **THEN** config loads, with base URL `https://api.ticktick.com`, the empty `mine` query, folder `ticktick`, interval 10 and comments 0

#### Scenario: Unknown key rejected
- **WHEN** a source section contains `intervall = 5`
- **THEN** config loading fails, naming the unknown key

#### Scenario: Insecure URL rejected
- **WHEN** `base_url = "http://jira.example.com"`
- **THEN** config validation fails with a message requiring `https://`

### Requirement: `TaskSource` trait and `RemoteIssue`

ft-core SHALL define a `TaskSource` trait with these methods:
- `fetch_query(&self, query: &str) -> Result<Vec<RemoteIssue>, SourceError>`;
- `fetch_keys(&self, refs: &[IssueRef]) -> Result<Vec<RemoteIssue>, SourceError>`, used for out-of-scope and parent lookups. An `IssueRef` is a key plus the note's `ft.source.url`, which lets backends whose keys aren't reversible (GitHub) locate the issue. A missing issue is simply absent from the result.

`RemoteIssue` SHALL carry:
- `key`, `url` and `title`;
- the remote status name, plus the mapped `Status`;
- priority (`Option<Priority>`) and due date (`Option<NaiveDate>`);
- the resolution or close date (`Option<NaiveDate>`);
- the assignee display name;
- `parent: Option<String>` (a key);
- the remote `updated` timestamp (an RFC 3339 string);
- the description as markdown;
- comments, each with author, RFC 3339 created timestamp and markdown body, oldest first.

Implementations SHALL be blocking and SHALL NOT spawn threads.

#### Scenario: Fake source in tests
- **WHEN** a test implements `TaskSource` over in-memory issues
- **THEN** the sync planner and the TUI flow run end to end without network

### Requirement: HTTP policy and offline classification

All source HTTP SHALL go through one blocking client:
- connect timeout 5 s, overall request timeout 20 s;
- a `User-Agent` of `ft/<version>`;
- pagination handled by the source.

Errors SHALL be classified into `SourceError` variants:
- `Offline` for DNS failure, connection refused, unreachable host or timeout;
- `Unauthorized` for 401/403;
- `RateLimited` for 429, honouring `Retry-After` up to 60 s once, then failing. A 500 response whose body contains `exceed_query_limit` (TickTick's throttle) SHALL also be `RateLimited`, without a retry;
- `Remote` for other non-2xx responses, carrying the status code and a body excerpt;
- `Parse` for unexpected JSON.

Tokens SHALL never appear in logs, error messages or `--format json` output. The client SHALL replace every occurrence of its own token in a `Remote` excerpt with `***`, because some remotes echo the submitted token in error bodies.

#### Scenario: No network
- **WHEN** the machine is offline and a sync runs
- **THEN** each source reports `offline`, no file is modified, and the error carries no token

#### Scenario: TickTick throttled
- **WHEN** TickTick answers HTTP 500 with `exceed_query_limit`
- **THEN** the source fails with `RateLimited`, and no file is modified for that source

#### Scenario: Token echoed in an error body
- **WHEN** a remote answers HTTP 400 with a body containing the request's token
- **THEN** the `Remote` excerpt shows `***` in its place

### Requirement: Credentials

Credentials SHALL resolve in this order:
1. the environment variable `FT_<NAME>_TOKEN` (the source name upper-cased, `-` → `_`);
2. `[credentials.<name>] token_command` (an argv list, run without a shell, 30 s limit): the first line of its standard output, when it exits successfully. The `[credentials]` table SHALL be accepted only from the user config; a vault config that has it SHALL fail to load with `[credentials] is only allowed in the user config`. Entries for sources the vault doesn't define SHALL be ignored. With `token_command` set, `ft sources login <name>` SHALL verify the command's token and store nothing;
3. for `kind = "github"`, the output of `gh auth token` when `gh` is installed and logged in;
4. the OS keychain entry with service `ft` and account `<name>:<email>` (jira), `<name>:github` or `<name>:ticktick`, via `keyring`.

A source with no credentials SHALL be reported as `no-credentials` and skipped, without error. This is the normal state on machines that cannot reach Jira.

#### Scenario: Machine without Jira access
- **WHEN** no token is configured for `jira` and the user runs `ft tasks sync`
- **THEN** the output reports `jira: skipped (no credentials — run ft sources login jira)`, the exit code is 0, and no file is modified

#### Scenario: Token from a password manager
- **WHEN** the user config sets `[credentials.ticktick]\ntoken_command = ["pass", "show", "ft/ticktick"]` and no env token is set
- **THEN** the first line of `pass show ft/ticktick` is used as the token, and `ft sources list` shows `command`

#### Scenario: Command in the vault config
- **WHEN** the vault config has a `[credentials]` table
- **THEN** config loading fails with `[credentials] is only allowed in the user config`

#### Scenario: Credentials for another vault's source
- **WHEN** the user config has `[credentials.ticktick]` and the current vault defines no `ticktick` source
- **THEN** config loads normally and the entry is ignored

#### Scenario: TickTick token from the keychain
- **WHEN** source `ticktick` has no env token and the keychain holds `ft` / `ticktick:ticktick`
- **THEN** that token is used and `ft sources list` shows `keychain`

### Requirement: `ft sources` commands

The CLI SHALL provide:
- `ft sources login <name>`: prompt for the token without echoing it, verify it with one authenticated request, then store it in the keychain. The request is the current-user endpoint for Jira and GitHub, and the project list (`GET /open/v1/project`) for TickTick. A failed verification stores nothing.
- `ft sources logout <name>`: delete the keychain entry.
- `ft sources list [--format json]`: per configured source, show name, kind, base URL, where the credentials come from (`env` / `command` / `gh` / `keychain` / `none`), and the last local sync time.

Last sync times are informational, machine-local, and kept in `.ft/cache/sources.json`.

#### Scenario: Login verifies the token
- **WHEN** the user runs `ft sources login jira` and pastes an invalid token
- **THEN** the command fails with `token rejected by https://example.atlassian.net (401)` and the keychain is unchanged

#### Scenario: TickTick login
- **WHEN** the user runs `ft sources login ticktick` and pastes a valid personal API token
- **THEN** the project list is fetched once, the token is stored under `ticktick:ticktick`, and the command reports the number of lists it can see

### Requirement: `ft tasks sync`

`ft tasks sync [--source NAME]... [--dry-run] [--format text|json]` SHALL, for each selected source (all by default):
1. resolve credentials;
2. fetch all queries and, if enabled, the parent chains and out-of-scope keys;
3. plan the note changes and apply them, unless `--dry-run`, which prints a unified diff.

The text output SHALL be one line per source, e.g. `jira: 2 new · 5 updated · 1 pending push · 0 conflicts`, or `jira: skipped (offline)`. JSON SHALL be an array of `{"source", "status": "ok"|"offline"|"no-credentials"|"unauthorized"|"error", "created", "updated", "pending", "conflicts", "out_of_scope", "message"}`.

The exit code SHALL be 0 when every source is `ok`, `offline` or `no-credentials`, and 1 when any source is `unauthorized` or `error`.

#### Scenario: Partial failure
- **WHEN** `jira` syncs fine and `gh` returns 401
- **THEN** the Jira notes are written, the output reports `gh: unauthorized`, and the exit code is 1

### Requirement: Push configuration

`SourceCfg` SHALL accept:
- `push` (bool, default `true`);
- `transitions`: a map from `Open` / `InProgress` / `Done` / `Cancelled` to a transition name. It is valid for Jira only; on any other source, config validation fails with `sources.<name>.transitions is only valid for jira`.

#### Scenario: Cancel transition configured
- **WHEN** the config has `[sources.jira.transitions]\nCancelled = "Won't Do"`
- **THEN** config loads, and cancel pushes use `Won't Do`

#### Scenario: Transitions on TickTick rejected
- **WHEN** a `kind = "ticktick"` source has `[sources.ticktick.transitions]`
- **THEN** config validation fails with `sources.ticktick.transitions is only valid for jira`

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


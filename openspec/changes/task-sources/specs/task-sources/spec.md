## ADDED Requirements

### Requirement: Source configuration

`Config` SHALL accept a `sources` map of named sections (`[sources.<name>]`), each `deny_unknown_fields`:

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `kind` | `"jira"` \| `"github"` | required | Backend |
| `base_url` | string | required for jira | e.g. `https://example.atlassian.net`. Must be `https://` (plain `http://` is accepted only for `localhost`) |
| `email` | string | required for jira | Atlassian account email (basic auth user) |
| `queries` | map name → string | jira: `{ mine = "assignee = currentUser() AND statusCategory != Done" }`; github: `{ mine = "is:issue is:open assignee:@me" }` | JQL / GitHub search queries; the union of results is "in scope" |
| `folder` | string | the source name | Vault-relative folder for new issue notes |
| `template` | string | none | Note template (in `templates_dir`) for the user part of new issue notes |
| `interval_minutes` | int | 10 | TUI background sync interval; `0` disables interval sync |
| `fetch_parents` | bool | true | Also fetch each in-scope issue's parent chain |
| `comments` | int | 5 | Most recent comments kept in the note; `0` disables the section |
| `description` | bool | true | Keep the managed `Description` section |
| `status_map` | map remote status name → `Open`\|`InProgress`\|`Done`\|`Cancelled` | `{}` | Overrides the default category mapping |

The section SHALL normally live in the vault config (`.ft/config.toml`) so every machine syncing the vault shares it. User config may override it, following the existing layered merge.

#### Scenario: Minimal Jira source
- **WHEN** the vault config contains `[sources.jira]\nkind = "jira"\nbase_url = "https://example.atlassian.net"\nemail = "me@example.test"`
- **THEN** config loads, with query `mine` defaulted, folder `jira`, interval 10, comments 5

#### Scenario: Unknown key rejected
- **WHEN** a source section contains `intervall = 5`
- **THEN** config loading fails, naming the unknown key

#### Scenario: Insecure URL rejected
- **WHEN** `base_url = "http://jira.example.com"`
- **THEN** config validation fails with a message requiring `https://`

### Requirement: `TaskSource` trait and `RemoteIssue`

ft-core SHALL define a `TaskSource` trait with these methods:
- `fetch_query(&self, query: &str) -> Result<Vec<RemoteIssue>, SourceError>`;
- `fetch_keys(&self, keys: &[String]) -> Result<Vec<RemoteIssue>, SourceError>`, used for out-of-scope and parent lookups; a missing key is simply absent from the result.

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
- `RateLimited` for 429, honouring `Retry-After` up to 60 s once, then failing;
- `Remote` for other non-2xx responses, carrying the status code and a body excerpt;
- `Parse` for unexpected JSON.

Tokens SHALL never appear in logs, error messages or `--format json` output.

#### Scenario: No network
- **WHEN** the machine is offline and a sync runs
- **THEN** each source reports `offline`, no file is modified, and the error carries no token

### Requirement: Credentials

Credentials SHALL resolve in this order:
1. the environment variable `FT_<NAME>_TOKEN` (the source name upper-cased, `-` → `_`);
2. for `kind = "github"`, the output of `gh auth token` when `gh` is installed and logged in;
3. the OS keychain entry with service `ft` and account `<name>:<email or "github">`, via `keyring`.

A source with no credentials SHALL be reported as `no-credentials` and skipped, without error. This is the normal state on machines that cannot reach Jira.

#### Scenario: Machine without Jira access
- **WHEN** no token is configured for `jira` and the user runs `ft tasks sync`
- **THEN** the output reports `jira: skipped (no credentials — run ft sources login jira)`, the exit code is 0, and no file is modified

### Requirement: `ft sources` commands

The CLI SHALL provide:
- `ft sources login <name>`: prompt for the token without echoing it, verify it with one authenticated request (for Jira, the current-user endpoint), then store it in the keychain. A failed verification stores nothing.
- `ft sources logout <name>`: delete the keychain entry.
- `ft sources list [--format json]`: per configured source, show name, kind, base URL, where the credentials come from (`env` / `gh` / `keychain` / `none`), and the last local sync time.

Last sync times are informational, machine-local, and kept in `.ft/cache/sources.json`.

#### Scenario: Login verifies the token
- **WHEN** the user runs `ft sources login jira` and pastes an invalid token
- **THEN** the command fails with `token rejected by https://example.atlassian.net (401)` and the keychain is unchanged

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

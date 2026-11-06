## MODIFIED Requirements

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
2. for `kind = "github"`, the output of `gh auth token` when `gh` is installed and logged in;
3. the OS keychain entry with service `ft` and account `<name>:<email>` (jira), `<name>:github` or `<name>:ticktick`, via `keyring`.

A source with no credentials SHALL be reported as `no-credentials` and skipped, without error. This is the normal state on machines that cannot reach Jira.

#### Scenario: Machine without Jira access
- **WHEN** no token is configured for `jira` and the user runs `ft tasks sync`
- **THEN** the output reports `jira: skipped (no credentials — run ft sources login jira)`, the exit code is 0, and no file is modified

#### Scenario: TickTick token from the keychain
- **WHEN** source `ticktick` has no env token and the keychain holds `ft` / `ticktick:ticktick`
- **THEN** that token is used and `ft sources list` shows `keychain`

### Requirement: `ft sources` commands

The CLI SHALL provide:
- `ft sources login <name>`: prompt for the token without echoing it, verify it with one authenticated request, then store it in the keychain. The request is the current-user endpoint for Jira and GitHub, and the project list (`GET /open/v1/project`) for TickTick. A failed verification stores nothing.
- `ft sources logout <name>`: delete the keychain entry.
- `ft sources list [--format json]`: per configured source, show name, kind, base URL, where the credentials come from (`env` / `gh` / `keychain` / `none`), and the last local sync time.

Last sync times are informational, machine-local, and kept in `.ft/cache/sources.json`.

#### Scenario: Login verifies the token
- **WHEN** the user runs `ft sources login jira` and pastes an invalid token
- **THEN** the command fails with `token rejected by https://example.atlassian.net (401)` and the keychain is unchanged

#### Scenario: TickTick login
- **WHEN** the user runs `ft sources login ticktick` and pastes a valid personal API token
- **THEN** the project list is fetched once, the token is stored under `ticktick:ticktick`, and the command reports the number of lists it can see

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

# task-source-github Specification

## Purpose
GitHub source mapping: issue search, open/closed state and state reason,
sub-issue parents, markdown bodies, comments, and issue keys. Created by
change task-sources.

## Requirements

### Requirement: GitHub search and fetch

The GitHub source SHALL use the GitHub REST API at `https://api.github.com` (or `base_url` when set, for GitHub Enterprise), authenticating with a bearer token:
- **Queries:** run through the issue search endpoint, following `Link: rel="next"` pagination. Pull requests SHALL be excluded unless the query names them explicitly.
- **`fetch_keys`:** fetch each issue individually (`GET /repos/{owner}/{repo}/issues/{n}`). A 404 means absent.
- **Comments:** fetched per issue, most recent `comments` only.

#### Scenario: Default query
- **WHEN** the source uses the default `mine` query
- **THEN** it returns the open issues assigned to the token's user, across all repositories it can see

### Requirement: GitHub keys and mapping

The issue key SHALL be `<owner>-<repo>-<number>`, lower-cased, with any character outside `[a-z0-9-]` replaced by `-`. That form is valid as both `🆔` and file stem. The note file SHALL be `<folder>/<key>.md`.

| `RemoteIssue` | GitHub |
|---|---|
| `url` | `html_url` |
| `title` | `title` |
| status name | `open`, or `closed:<state_reason>` |
| `Status` | `status_map` if configured; else `open` → Open, `closed` with `state_reason = not_planned` → Cancelled, other `closed` → Done |
| priority, due | none (GitHub has no such fields) |
| resolution date | the date part of `closed_at` |
| assignee | the first assignee's `login` |
| parent | the parent issue from the sub-issues API, when present (its key normalised the same way); otherwise none |
| updated | `updated_at`, verbatim |
| description | `body` (markdown, neutralised by the planner) |
| comments | `user.login`, `created_at`, `body` |

#### Scenario: Closed as not planned
- **WHEN** an issue is closed with `state_reason = not_planned`
- **THEN** its mapped `Status` is Cancelled

#### Scenario: Key normalisation
- **WHEN** the issue is `My-Org/Some.Repo#42`
- **THEN** its key is `my-org-some-repo-42` and its note is `github/my-org-some-repo-42.md`

# task-source-jira Specification

## Purpose
Jira Cloud source mapping: search API, status categories and the status map,
priority, due date, parent and epic, Atlassian Document Format descriptions
converted to markdown, and comments. Created by change task-sources.

## Requirements

### Requirement: Jira Cloud search and fetch

The Jira source SHALL use the Jira Cloud REST API v3 with basic auth (`email:token`):
- **Queries:** issue search with JQL, through the current token-paginated search endpoint (`/rest/api/3/search/jql`, following `nextPageToken`). Requests SHALL ask for exactly the fields mapped below, not `*all`.
- **`fetch_keys`:** the same endpoint with `key in (…)`, batched 100 keys per request.
- **Credential check:** `ft sources login` verifies the token with `/rest/api/3/myself`.

#### Scenario: Pagination
- **WHEN** a query matches 230 issues
- **THEN** the source follows `nextPageToken` until it is absent and returns all 230

### Requirement: Jira field mapping

| `RemoteIssue` | Jira |
|---|---|
| `key` | `key` |
| `url` | `<base_url>/browse/<key>` |
| `title` | `fields.summary` |
| status name | `fields.status.name` |
| `Status` | `status_map[name]` if configured; else by `fields.status.statusCategory.key`: `new` → Open, `indeterminate` → InProgress, `done` → Done, except that `done` maps to Cancelled when `fields.resolution.name` is `Won't Do`, `Won't Fix`, `Duplicate`, `Cannot Reproduce` or `Declined`, or when the status name is `Canceled`, `Cancelled`, `Abandoned`, `Won't Do`, `Won't Fix`, `Rejected` or `Declined` |
| priority | `fields.priority.name`: `Highest`/`High`/`Medium`/`Low`/`Lowest` map one-to-one; other names map to none |
| due | `fields.duedate` |
| resolution date | the date part of `fields.resolutiondate` |
| assignee | `fields.assignee.displayName` |
| parent | `fields.parent.key` (covers subtask → story and story → epic in team-managed and company-managed projects) |
| updated | `fields.updated`, verbatim |
| description | `fields.description` (Atlassian Document Format) converted to markdown |
| comments | `fields.comment.comments[]`: `author.displayName`, `created`, `body` (ADF → markdown) |

#### Scenario: Won't Do maps to cancelled
- **WHEN** an issue is in category `done` with resolution `Won't Do`
- **THEN** its mapped `Status` is Cancelled

#### Scenario: Status map override
- **WHEN** `status_map = { "Blocked" = "Open" }` and an issue's status is `Blocked` (category `indeterminate`)
- **THEN** its mapped `Status` is Open

### Requirement: ADF to markdown

`sources::adf::to_markdown` SHALL convert Atlassian Document Format to markdown for these node types:
- `paragraph`, `heading`, `bulletList` / `orderedList` / `listItem`, `codeBlock` (with language), `blockquote`, `rule`, `hardBreak`;
- `text` with the marks `strong`, `em`, `code`, `strike` and `link`;
- `mention` (rendered as `@Display Name`), `emoji` (its text), `inlineCard` / `blockCard` (as the URL);
- `taskList` / `taskItem` (as `☐` / `☑` items, never as markdown checkboxes);
- `table` (as a markdown table, with cell text flattened).

Unknown nodes SHALL render their text content, and SHALL never fail the sync.

#### Scenario: Mixed document
- **WHEN** a description has a heading, a bullet list with a bold item, a code block, a mention and a task item
- **THEN** the markdown is `### Heading`, `- **item**`, a fenced code block, `@Jane Doe`, and `- ☐ task text`

## ADDED Requirements

### Requirement: Issue note format

An *issue note* is identified by its frontmatter, not its path. Its `ft:` block SHALL contain:

```yaml
ft:
  tasks:
    anchor: ABC-123            # the anchor task's 🆔
    parent: "[[EPIC-7]]"       # only when the issue has a parent
  source:
    name: jira                 # the [sources.<name>] that owns it
    key: ABC-123
    url: https://example.atlassian.net/browse/ABC-123
    status: In Review          # remote status name, verbatim
    assignee: Michael Weisser
    state: active              # active | context | out-of-scope
    updated: 2026-10-21T09:12:44.000+0000   # remote's own timestamp
    synced:
      status: InProgress       # the local Status ft last wrote
```

The body SHALL contain exactly one anchor task line carrying `🆔 <key>`, in the form

`- [<status char>] [<key>](<url>) <title> [<priority emoji>] [📅 <due>] [✅/❌ <date>] 🆔 <key>`

plus two managed sections, each delimited by HTML comments: `<!-- ft:source:description -->` … `<!-- /ft:source:description -->` (heading `## Description`), and `<!-- ft:source:comments -->` … `<!-- /ft:source:comments -->` (heading `## Comments`). Everything else in the note (other text, local subtasks, the user part of the template) belongs to the user, and sync SHALL NOT modify it.

Issue keys that aren't valid `🆔` / file-stem tokens SHALL be normalised by the source (see `task-source-github`).

#### Scenario: New Jira issue note
- **WHEN** an in-scope issue `ABC-123` (status `In Review` → InProgress, priority High, due 2026-10-30, parent `EPIC-7`) has no note yet
- **THEN** `jira/ABC-123.md` is created with the frontmatter above, a `# ABC-123 Fix login` heading, the anchor line `- [/] [ABC-123](https://example.atlassian.net/browse/ABC-123) Fix login ⏫ 📅 2026-10-30 🆔 ABC-123`, the rendered template body (if configured), and the two managed sections

#### Scenario: Note moved by the user
- **WHEN** the user moves `jira/ABC-123.md` to `projects/login/ABC-123.md`
- **THEN** the next sync updates the note in its new place and creates no duplicate

### Requirement: Remote-owned and local fields

On every sync of an existing issue note, the planner SHALL overwrite these remote-owned fields from the remote issue:
- on the anchor line: the title part of the description (the `[key](url)` prefix and the title text), the priority, the due date (cleared when the remote has none), and the status, subject to the pending/conflict rules;
- in the frontmatter: all `ft.source.*` keys except `state` (set by the scope rules) and `synced`, plus `ft.tasks.parent`;
- the contents of both managed sections.

The planner SHALL preserve these local fields on the anchor line: hashtags in the description (re-appended after the title in their original order), `⏳` scheduled, `🛫` start, `➕` created, recurrence, the block id, and any unknown trailing fields. Lines outside the anchor line and the managed sections SHALL be left byte-identical.

#### Scenario: Title and due change remotely
- **WHEN** the remote title becomes `Fix login on Safari`, the due date is removed, and the local anchor line has `#work ⏳ 2026-10-25`
- **THEN** the anchor line becomes `- [/] [ABC-123](…) Fix login on Safari #work ⏳ 2026-10-25 🆔 ABC-123` and nothing else in the note changes

### Requirement: Status sync with pending and conflict detection

Let *base* be `ft.source.synced.status`, *local* the anchor line's status, and *remote* the mapped remote status. The planner SHALL apply these rules in order:
- `local == base`: write *remote* to the line and set `synced.status = remote`.
- `local != base` and `remote == base`: keep *local*, leave `synced.status` unchanged, and report the issue as **pending push** (Phase 4 pushes it).
- `local != base`, `remote != base` and `remote == local`: both sides agree, so set `synced.status = remote`.
- `local != base`, `remote != base` and `remote != local`: keep *local*, leave `synced.status` unchanged, and report a **conflict** (`local=<x> remote=<y>`).

Writing `Done` or `Cancelled` SHALL add `✅` / `❌` with the remote resolution date (or the sync date when the remote has none). Leaving those states SHALL remove the date.

#### Scenario: Ticked locally before Phase 4
- **WHEN** base and remote are `InProgress` and the user ticked the anchor `[x]` locally
- **THEN** the line stays `[x]` and sync reports `1 pending push`

#### Scenario: Both changed
- **WHEN** base is `InProgress`, local is `Done`, and remote is `Cancelled`
- **THEN** the line stays `Done`, and sync reports `1 conflict` naming `ABC-123 local=Done remote=Cancelled`

### Requirement: Scope states

Each issue note's `ft.source.state` SHALL follow these rules:
- **active:** the issue is returned by at least one of the source's queries.
- **context:** fetched only as an ancestor of an active issue, via `fetch_parents`. Status and other fields sync normally.
- **out-of-scope:** previously `active` or `context`, but neither now. The planner SHALL fetch such issues by key:
  - if the remote status maps to `Done` or `Cancelled`, write it per the status rules and keep `state: out-of-scope`;
  - otherwise set the anchor to `Cancelled` (and `synced.status = Cancelled`).
  
  Issues the remote no longer returns at all (deleted, or no permission) SHALL be treated the same as still open.
- An `out-of-scope` issue that becomes active again SHALL return to `active`, and its status syncs from the remote, with `Cancelled` treated as the base.

Notes never fetched again SHALL never be deleted.

#### Scenario: Reassigned issue
- **WHEN** `ABC-123` is no longer returned by `mine` but is still `In Progress` remotely
- **THEN** its anchor becomes `[-] … ❌ <today>`, `state: out-of-scope`, and sync reports `1 out of scope`

#### Scenario: Closed issue
- **WHEN** `ABC-123` drops out because it was resolved remotely on 2026-10-20
- **THEN** its anchor becomes `[x] … ✅ 2026-10-20`

### Requirement: Parent hierarchy from the source

When an issue has a parent key, its note SHALL carry `ft.tasks.parent: "[[<parent note stem>]]"`, and (with `fetch_parents`) the parent SHALL exist as an issue note itself, created with `state: context` if it isn't in scope. The parent chain SHALL be followed up to 5 levels, so the effective hierarchy mirrors subtask → story → epic.

#### Scenario: Epic shows in the tree
- **WHEN** `ABC-123` has parent `EPIC-7` and the user runs `ft tasks list --tree remote`
- **THEN** the `EPIC-7` anchor appears with `ABC-123`'s anchor nested under it, and any local subtasks under `ABC-123` beneath that

### Requirement: Managed section content and neutralisation

The `Description` section SHALL hold the issue description as markdown. The `Comments` section SHALL hold the last `comments` comments, oldest first, each formatted as `### <author> — <YYYY-MM-DD>` followed by its body.

Before writing remote text, the planner SHALL neutralise it:
- Lines that would parse as tasks (`- [ ]`, `* [x]`, numbered, any status char) SHALL have their checkbox replaced by `☐` (open) or `☑` (any other status).
- Headings SHALL be demoted so none is shallower than `###`.
- Lines that are exactly `---` SHALL be replaced by `***`.
- HTML comments matching the managed markers SHALL be removed.

If a note's markers are missing (the user deleted a section), the planner SHALL NOT re-add that section.

#### Scenario: Remote checklist doesn't become a task
- **WHEN** an issue description contains a checklist item `- [ ] run migration`
- **THEN** the note's Description section contains `- ☐ run migration`, and `ft tasks list` shows no `run migration` task

### Requirement: Deterministic, merge-friendly output

For the same remote data and the same note, the planner SHALL produce byte-identical output on any machine:
- no local timestamps are written (only remote timestamps and dates);
- frontmatter keys are emitted in a fixed order;
- comments are ordered by `(created, author)`;
- an unchanged issue produces no edit.

Frontmatter edits SHALL be made through a general `frontmatter::upsert_ft_keys` that sets or removes individual keys under `ft.tasks` and `ft.source` and preserves every other line of the frontmatter.

#### Scenario: Two machines sync the same issue
- **WHEN** machine A and machine B each run `ft tasks sync` against the same remote state from the same commit
- **THEN** both produce identical file contents, and git sync sees no conflict

#### Scenario: Nothing changed
- **WHEN** a sync runs and no remote issue changed since the previous sync
- **THEN** no file is written and the summary reports `0 new · 0 updated`

### Requirement: Remote-owned field guard on edits

`ops::update_task_line` SHALL fail with `UpdateError::OwnedByRemote { field, source }` when the task line is the anchor task of an issue note (its file's `ft.tasks.anchor` equals the task's `🆔`, and `ft.source.name` is set) and the mutation changes a remote-owned field: the description excluding hashtags, the due date, or the priority. Status changes and every other field SHALL remain allowed.

The CLI SHALL render the error as `<field> is owned by <source> — change it in <source>`, and the TUI SHALL show it as an error toast.

#### Scenario: Bump due on a Jira anchor in the TUI
- **WHEN** the cursor is on a Jira anchor and the user presses `]`
- **THEN** nothing is written and the toast reads `due is owned by jira — change it in jira`

#### Scenario: Retag is allowed
- **WHEN** the user retags a Jira anchor from `#computer` to `#wait`
- **THEN** the hashtag changes and the line is written

### Requirement: Remote attributes in the DSL

The query DSL SHALL support these task attributes:
- `source` (Str, optional): the owning source name;
- `key` (Str, optional): the issue key;
- `remote_status` (Str, optional): the remote status name.

They SHALL be set only on anchor tasks of issue notes. The built-in task preset `remote` SHALL be `source is not null and status in {Open, InProgress}`.

#### Scenario: Remote work view
- **WHEN** the user runs `ft tasks list remote --tree`
- **THEN** every open anchor task is listed, with its local subtasks (physical and linked) nested under it

#### Scenario: By remote status
- **WHEN** the user queries `remote_status = "In Review"`
- **THEN** only anchors whose remote status is `In Review` are listed

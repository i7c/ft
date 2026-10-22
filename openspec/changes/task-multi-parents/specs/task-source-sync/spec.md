## MODIFIED Requirements

### Requirement: Issue note format

An *issue note* is identified by its frontmatter, not its path. Its `ft:` block SHALL contain:

```yaml
ft:
  tasks:
    anchor: ABC-123            # the anchor task's 🆔
    parent: EPIC-7             # the parent issue's 🆔, only when the issue has a parent
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

`- [<status char>] [<key>](<url>) <title> [⤴ <local ids>] [<priority emoji>] [📅 <due>] [✅/❌ <date>] 🆔 <key>`

plus two managed sections, each delimited by HTML comments: `<!-- ft:source:description -->` … `<!-- /ft:source:description -->` (heading `## Description`), and `<!-- ft:source:comments -->` … `<!-- /ft:source:comments -->` (heading `## Comments`). Everything else in the note (other text, local subtasks, the user part of the template) belongs to the user, and sync SHALL NOT modify it.

Issue keys that aren't valid `🆔` / file-stem tokens SHALL be normalised by the source (see `task-source-github`).

#### Scenario: New Jira issue note
- **WHEN** an in-scope issue `ABC-123` (status `In Review` → InProgress, priority High, due 2026-10-30, parent `EPIC-7`) has no note yet
- **THEN** `jira/ABC-123.md` is created with the frontmatter above (`parent: EPIC-7`), a `# ABC-123 Fix login` heading, the anchor line `- [/] [ABC-123](https://example.atlassian.net/browse/ABC-123) Fix login ⏫ 📅 2026-10-30 🆔 ABC-123`, the rendered template body (if configured), and the two managed sections

#### Scenario: Note moved by the user
- **WHEN** the user moves `jira/ABC-123.md` to `projects/login/ABC-123.md`
- **THEN** the next sync updates the note in its new place and creates no duplicate

### Requirement: Remote-owned and local fields

On every sync of an existing issue note, the planner SHALL overwrite these remote-owned fields from the remote issue:
- on the anchor line: the title part of the description (the `[key](url)` prefix and the title text), the priority, the due date (cleared when the remote has none), and the status, subject to the pending/conflict rules;
- in the frontmatter: all `ft.source.*` keys except `state` (set by the scope rules) and `synced`, plus `ft.tasks.parent`;
- the contents of both managed sections.

The planner SHALL preserve these local fields on the anchor line: hashtags in the description (re-appended after the title in their original order), the `⤴` parent references, `⏳` scheduled, `🛫` start, `➕` created, recurrence, the block id, and any unknown trailing fields. `⤴` references SHALL never be pushed to the remote. Lines outside the anchor line and the managed sections SHALL be left byte-identical.

#### Scenario: Title and due change remotely
- **WHEN** the remote title becomes `Fix login on Safari`, the due date is removed, and the local anchor line has `#work ⤴ p2m7rd ⏳ 2026-10-25`
- **THEN** the anchor line becomes `- [/] [ABC-123](…) Fix login on Safari #work ⤴ p2m7rd ⏳ 2026-10-25 🆔 ABC-123` and nothing else in the note changes

### Requirement: Parent hierarchy from the source

When an issue has a parent key, its note SHALL carry `ft.tasks.parent: <parent key>` as a plain scalar (the parent anchor's `🆔`), and (with `fetch_parents`) the parent SHALL exist as an issue note itself, created with `state: context` if it isn't in scope. The parent chain SHALL be followed up to 5 levels, so the effective hierarchy mirrors subtask → story → epic. Notes written by earlier versions with a wikilink value SHALL be rewritten to the plain form on their next sync.

#### Scenario: Epic shows in the tree
- **WHEN** `ABC-123` has parent `EPIC-7` and the user runs `ft tasks list --tree remote`
- **THEN** the `EPIC-7` anchor appears with `ABC-123`'s anchor nested under it, and any local subtasks under `ABC-123` beneath that

#### Scenario: Old wikilink value rewritten
- **WHEN** an existing note has `parent: "[[EPIC-7]]"` and sync runs
- **THEN** the frontmatter becomes `parent: EPIC-7`

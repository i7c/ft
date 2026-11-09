# tasks-edit-for-sync Specification

## Purpose
The CLI surface a two-way sync tool drives: more fields on `ft tasks edit`,
guarded writes with machine-readable results on `edit`, `complete` and
`cancel`, bulk id assignment, and path exclusion. Created by change
tasks-edit-for-sync.

## ADDED Requirements

### Requirement: More fields on `ft tasks edit`

`ft tasks edit` SHALL accept `--title <TEXT>`, `--start <DATE|none>`,
`--status <open|in-progress>`, `--id <ID>` and `--depends-on <IDS|none>`
next to the existing field flags, and at least one field flag SHALL be given.

- `--title` SHALL replace the description's non-tag words and keep its tag
  words, appended after the title.
- `--tags` SHALL remove every tag word from the description and append the
  given tags; `none` removes them all.
- `--status` SHALL set the status and clear the done and cancelled dates.
- `--id` SHALL be refused when the task has a different id, or when another
  task in the vault has that id.
- `--depends-on` SHALL replace the `⛔` set; `none` clears it.

#### Scenario: Tags are written
- **WHEN** `a.md:1` is `- [ ] Buy milk #home 📅 2026-11-10` and the user runs `ft tasks edit a.md:1 --tags "work errand"`
- **THEN** the line is `- [ ] Buy milk #work #errand 📅 2026-11-10`

#### Scenario: Title keeps the tags
- **WHEN** the same line gets `--title "Buy oat milk"`
- **THEN** the line is `- [ ] Buy oat milk #home 📅 2026-11-10`

#### Scenario: Reopen
- **WHEN** `- [x] Task ✅ 2026-11-01` gets `--status open`
- **THEN** the line is `- [ ] Task`

#### Scenario: Id taken
- **WHEN** another task has `🆔 abc123` and the user runs `--id abc123`
- **THEN** the command fails and no file changes

### Requirement: Guarded, machine-readable writes

`ft tasks edit`, `complete` and `cancel` SHALL accept `--expect <JSON>`, a
task object as `ft tasks list --format json` prints it. When the line on disk
does not serialize to the same line as the expected task, the write SHALL be
refused. They SHALL accept `--format text|json` (default text). JSON output
SHALL be the written task in the `list` row shape; for `complete` it SHALL be
`{"completed": <task>, "next": <task>|null}`.

A write refused because the line changed on disk SHALL exit with code 3.

#### Scenario: Stale expectation
- **WHEN** a script lists a task, the line is edited in Obsidian, and the script runs `ft tasks edit a.md:1 --expect '<row>' --due 2026-12-01`
- **THEN** the command exits 3 and the file is unchanged

#### Scenario: JSON result
- **WHEN** the user runs `ft tasks edit a.md:1 --due 2026-12-01 --format json`
- **THEN** stdout is one JSON object whose `due` is `2026-12-01` and whose `source_file` is `a.md`

#### Scenario: Recurring completion
- **WHEN** a recurring task is completed with `--format json`
- **THEN** `completed` has status `Done` and `next` is the new open instance with its next dates

### Requirement: `ft tasks assign-ids`

`ft tasks assign-ids [PRESET_OR_QUERY]` SHALL give a fresh `🆔`, unique in
the vault, to every matching task without one. It SHALL take `--query`,
`--status`, `--path`, `--exclude-path` and `--dry-run`, and `--format
text|json`; JSON is the list of `{source_file, source_line, id}` assigned. A
file whose task line changed since the scan SHALL be skipped and reported,
and the command SHALL then exit 3 after applying the other files.

#### Scenario: Ids for open tasks
- **WHEN** a vault has two open tasks without ids and one with `🆔 x1`, and the user runs `ft tasks assign-ids --status open`
- **THEN** the two tasks get distinct new ids and `x1` is unchanged

### Requirement: `--exclude-path`

`ft tasks list` and `ft tasks assign-ids` SHALL accept `--exclude-path
<PREFIX>`, repeatable, and leave out every task whose vault-relative path
starts with one of the prefixes.

#### Scenario: Exclude a folder
- **WHEN** tasks exist in `ticktick/a.md` and `inbox.md` and the user runs `ft tasks list --exclude-path ticktick/`
- **THEN** only the `inbox.md` task is listed

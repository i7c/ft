## Context

whatnow's vault adapter runs `ft tasks list --format json` to pull, and one
`ft` write per pushed task. It keeps no state of its own between calls; it
finds a task by its `🆔` in a fresh `list`, then writes it by `<file>:<line>`
with `--expect` set to that row, so the write either lands on exactly that
task or is refused.

## Decisions

**`--expect` takes the whole `list` row.** It is deserialized as a `Task`
and handed to the existing `check_expected` guard, which compares the two
serialized lines. No new comparison logic, and the caller passes back what it
was given. Context fields (path, line, indent, parent) don't take part, as
for every guarded op today.

**Exit code 3 for a changed line.** `LineChanged` (from `--expect`, or from
the scan-to-write guard every op already has) maps to 3. A sync re-reads and
merges on 3 and backs off on anything else. 2 stays "ambiguous choice".

**`--status` on edit takes only `open` and `in-progress`.** Done and
cancelled already have `complete` (which writes a recurring task's next
instance) and `cancel`; a second path to done would skip the recurrence.
Setting open or in-progress clears `✅` and `❌`.

**Tags stay in the description.** `--tags` removes every tag word from the
description and appends the new ones at the end. `--title` replaces the
non-tag words and keeps the tags, appended at the end. Both use
`ops::is_tag_word`, so "a tag" means what the parser means. A line with a tag
mid-sentence is normalized only when one of these flags touches it.

**`--id` sets an id only on a task without one** (or with the same one), and
refuses an id another task already holds. Replacing an id would orphan every
`⛔` and `⤴` that names it.

**`--depends-on` replaces the whole `⛔` set** (comma- or space-separated,
`none` clears). It doesn't check that the ids exist; `ft tasks check` reports
dangling ones, as today.

**`assign-ids` is a plan/apply pair in ft-core.** It selects like `list`
(preset or query, `--status`, `--path`, `--exclude-path`), skips tasks that
have an id, draws ids with `links::new_task_id` against every id in the scan,
and applies per file through `write_atomic`, each line checked against the
scanned task. A file whose line changed is skipped and reported, and the rest
apply. `--dry-run` prints the plan.

**`--exclude-path` is a prefix of the vault-relative path**, applied after
the query, because the DSL has no negation. `--path` stays a substring
include.

## Non-goals

- Moving the `🆔` off the done line when a recurring task completes.
- Changing a task's physical parent (indentation); `link`/`unlink` cover
  logical parents already.
- A `--format json` on `create`, `move`, `indent` or `outdent`.

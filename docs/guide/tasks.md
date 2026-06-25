# Tasks

`ft` reads and writes the [Obsidian Tasks plugin emoji
format](https://publish.obsidian.md/tasks/). The CLI gives you
list / create / complete / move. The TUI Tasks tab gives you a live
triage view with single-key mutations. Both surfaces share the same
parser, the same query DSL, and the same atomic write path — so
anything you can do in one, you can do in the other.

The exact emoji vocabulary (`📅` for due, `⏫` for high priority, etc.)
is in [docs/task-format.md](../task-format.md). The query language is
the unified graph DSL, documented in
[docs/graph-query-dsl.md](../graph-query-dsl.md); task queries run it
under `Profile::Tasks` so you can keep typing the short form
(`priority = High`, `due < today`). This chapter is the workflow view.

## The shape of a task

```
- [ ] Pay rent ⏫ 🔁 every month on the 1st 📅 2026-04-01
```

Each task is one markdown list item. The `[ ]` marker is the status
(`[ ]` open, `[x]` done, `[/]` in-progress, `[-]` cancelled). After
the description come emoji-prefixed fields in any order on input, but
fields are re-emitted in canonical order on any rewrite. Indented
`- [ ]` lines underneath are subtasks.

## Listing tasks

The CLI's `ft tasks list` is the workhorse. With no arguments it
shows every task in the vault — not what you want unless your vault
is tiny. Filter with a preset, a query, or flags.

```sh
# Built-in presets
ft tasks list today        # due today or scheduled today, not done
ft tasks list overdue      # due before today, not done
ft tasks list upcoming     # due after today, not done
ft tasks list done-today
ft tasks list not-done     # everything still actionable

# User presets from your config
ft tasks list backlog

# DSL query inline (unified graph DSL, Profile::Tasks)
ft tasks list --query 'priority = High and status in {Open, InProgress}'
ft tasks list --query 'due < today'
ft tasks list --query 'tags includes "work" and not (status = Done)'

# Flag filters (compose as AND with each other and with --query)
ft tasks list --status open --priority high --tag work
ft tasks list --due-before 2026-06-01 --no-due
```

Flag filters compose with `--query` as additional `and` clauses, so
this:

```sh
ft tasks list --query 'priority = High' --status open --tag work
```

…is the same as `'priority = High and status = Open and tags includes
"work"'`. (Enum values like `Open` / `High` are case-insensitive on
the parser side; the canonical form capitalises.)

### Sort and limit

Sort and limit are CLI flags, not part of the query string:

```sh
ft tasks list --query 'status in {Open, InProgress} and due < today' \
    --sort due --limit 10
```

`--sort` accepts comma-separated keys with optional `:reverse` /
`:desc` per key:

```sh
ft tasks list today --sort priority,due:reverse
ft tasks list today --sort priority --sort due:desc   # repeated form, same result
```

Default sort (nothing specified): `due asc, priority desc, path asc`.

### Group and format

```sh
ft tasks list today --group-by priority
ft tasks list overdue --format markdown        # the original task lines
ft tasks list --query 'tags includes "work"' --format ndjson   # one task per line, JSON
ft tasks list --format json    # one big array
```

`--group-by` (priority, tag, folder, path, due) only affects the
`table` format — JSON / NDJSON / markdown are flat by design.

### Subtasks (`--tree`)

```sh
ft tasks list --query 'priority = high' --tree
ft tasks list overdue --tree --format json     # nested "subtasks" arrays
```

`--tree` shows every matching task with its subtasks nested underneath.
Subtasks are pulled in **even when they don't match the filter**, all the
way down — so `--query 'priority = high' --tree` surfaces a high-priority
parent's full checklist, not just the high-priority lines. A task that
matches both on its own and as someone's subtask is shown once, nested
under its parent. `table` indents with a `↳` marker, `markdown` emits a
valid nested list, `json` nests a `subtasks` array on each node, and
`ndjson` streams the expanded set in pre-order. `--tree` is mutually
exclusive with `--group-by`.

In the TUI, the same relationship is interactive: select a task and press
`→`/`l` to expand its subtasks one level (`←`/`h` to collapse). Press `s`
to create a subtask under the selected task — it opens the same quickline
as `c` (and `Ctrl+E` still expands to the full form); the only difference
is that the new task is written indented under the selected one, and the
parent auto-expands so you see it. Subtasks are joined to their parent in
the graph by a `subtask` edge (parent → child), so graph queries can
traverse the hierarchy too.

Colour is on by default when stdout is a TTY. `NO_COLOR=1`,
`--no-color`, and any redirection turn it off.

## Creating tasks

`ft tasks create` writes a new task. It picks the target file in this
order: `--file PATH` → today's daily note → `default_task_location` →
error.

When the target is the daily note and it doesn't exist yet, it's created
from your `[periodic_notes.daily].template` first — the same file
`ft notes today` would produce — so the first task of the day doesn't
leave behind a bare, template-less note. (Explicit `--file` paths are
written as-is and not auto-templated.)

```sh
# Drop into today's daily note
ft tasks create "Call dentist" --due tomorrow --priority high

# Specific file, under a heading
ft tasks create "Read paper" \
    --file Areas/research.md --under-heading "Reading queue"

# Recurring, with tags and an id
ft tasks create "Standup notes" --due today \
    --recurrence "every weekday" --tag work --id stnd
```

Date flags (`--due`, `--scheduled`, `--start`, `--on` on `complete`)
accept ISO dates (`2026-05-10`), keywords (`today` / `tomorrow` /
`yesterday`), relative shifts (`+3d`, `-1w`), and chrono-english
phrases (`next monday`, `in 2 weeks`). Anywhere `today` resolves,
`FT_TODAY=YYYY-MM-DD` overrides it — useful for tests and
reproducible scripts.

### Where in the file a task lands

When you don't pass an explicit position flag, `ft` resolves a target
*section* in this order:

1. The target note's `ft-tasks-section:` frontmatter, e.g.

   ```yaml
   ---
   ft-tasks-section: Tasks
   ---
   ```

   Put this in your daily-note template and every daily note will collect
   new tasks under its `## Tasks` heading. Any fixed-path note can set its
   own.
2. The global `[tasks] default_section` config key (see the config guide).
3. Otherwise, plain append at file end.

A resolved section behaves like `--under-heading`: the heading is created
at file end if the note doesn't have it yet.

Other useful flags:

- `--under-heading "<text>"` — append at the end of that section,
  creating the heading if missing. Overrides the resolution above.
- `--at-line N` — insert at a specific 1-indexed line.
- `--append` — force appending at file end, overriding any configured
  default section.
- `--edit` — after writing, open `$EDITOR` on the new task line.
- `--force` — insert even when an exact-duplicate task already exists
  (same description + dates).
- `--id <X>`, `--depends-on <id>` (repeatable) — Tasks-plugin id and
  dependency fields.

## Completing tasks

```sh
# By task id
ft tasks complete xyz123

# By file + line
ft tasks complete journal/2026-05-10.md:42

# By fuzzy substring (matches the description)
ft tasks complete dentist

# Without a selector — interactive picker over every open task
ft tasks complete
```

`--on DATE` sets the completion date (defaults to today). When the
selector matches multiple tasks, `ft` opens an interactive picker on a
TTY, or errors with a candidate list under `--yes` / non-interactive
stdin.

Recurring tasks write a new instance above the completed line whose
dates shift by the same delta as the primary date (the first of
`due` / `scheduled` / `start`). The exact recurrence rules
`ft` understands are listed in
[docs/task-format.md](../task-format.md#recurrence-whitelist).

## Moving tasks

`ft tasks move` relocates one task or a batch of them, including any
subtasks, into another file (and optionally under a heading). Same
selector forms as `complete`, plus a `--query` form for bulk moves.

```sh
# Single move
ft tasks move stale-id --to inbox/triage.md
ft tasks move "old plan" --to inbox/triage.md#Triage

# Bulk move, with a preview before writing
ft tasks move --query 'tags includes "legacy" and status in {Open, InProgress}' \
              --to inbox/triage.md#Triage --dry-run
```

Behavior worth knowing:

- `--dry-run` prints a unified diff of every affected file. Always
  use it for the first run of a bulk move.
- Bulk moves (more than one task) require an interactive confirmation
  by default; pass `--yes` to skip it. On a non-TTY stdin, `--yes` is
  required.
- The plan is computed pure (no writes), then each non-trivial file is
  rewritten atomically (tempfile + rename). Same-file edits apply in
  descending byte order so earlier rewrites don't invalidate later
  offsets.

## The Tasks tab in the TUI

`ft tui` opens a tabbed interface; press `2` to jump to the Tasks tab
(or `Tab` to cycle). The tab shows a sidebar with built-in and user
views (`Today`, `Overdue`, `Upcoming`, `Done today`, each of your
presets), plus a main pane with the current view's tasks.

Common chords inside the Tasks tab:

| Key       | Action                                          |
|-----------|-------------------------------------------------|
| `j` / `k` / `↑` / `↓` | move the cursor                     |
| `/`       | open the query bar to edit the current view    |
| `R`       | reload from disk                                |
| `Enter`   | open the selected task's file at its line in `$EDITOR` |
| `x`       | complete the selected task (today's date)       |
| `X`       | cancel the selected task                        |
| `t`       | set due to today                                |
| `]` / `[` | bump due ±1 day                                 |
| `}` / `{` | bump scheduled ±1 day                           |
| `p` / `P` | cycle priority forward / backward               |
| `e`       | open the edit popup for the focused task        |
| `c`       | quickline new-task entry                        |
| `C`       | new-task form (multi-field popup)               |

Sidebar:

| Key      | Action                                |
|----------|---------------------------------------|
| `↑` / `↓` | move between views                   |
| `Enter`  | switch to the focused view            |

The `?` overlay at any time shows every binding for the current tab
and the global App-level chords. The generated reference is at
[docs/keybindings.md](../keybindings.md).

Every mutation chord calls the same `ft-core` op the CLI does, so the
state on disk is identical regardless of which surface you used.

## Scripting patterns

Empty result sets exit with code `1` by default — convenient for
shell guards. Pass `--allow-empty` to treat empty as success when
you'd rather not have `set -e` blow up:

```sh
ft tasks list overdue --allow-empty --format ndjson \
  | jq -r '.description'
```

For machine-readable errors, set `--json-errors` at the top level:

```sh
ft --json-errors tasks list today --format ndjson
```

Errors print to stderr as a single JSON object (`{"error": "...",
"chain": [...]}`); stdout still gets the (possibly empty) result body.

See [scripting.md](scripting.md) for the broader scripting story
(headless command dispatch, exit codes, the `--json-errors` flag, and
piping conventions).

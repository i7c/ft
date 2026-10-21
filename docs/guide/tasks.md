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

1. The target note's `ft.tasks.section` frontmatter key, e.g.

   ```yaml
   ---
   ft:
     tasks:
       section: Tasks
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

## Reparenting: moving tasks under other tasks

`--under` makes the moved tasks the last children of another task, in
the same file or a different one. `--sibling` puts them right after
that task's subtree, at its level. The target selector takes the same
forms as the positional one, but has to match exactly one task.

```sh
ft tasks move inbox.md:4 --under "vendor onboarding"
ft tasks move inbox.md:4 --under proj.md:12 --sibling

# One level in / out
ft tasks indent proj.md:7     # becomes the last child of the task above it
ft tasks outdent proj.md:7    # becomes its parent's next sibling
```

- Moved blocks are re-indented to fit their new place. Nested lines
  keep their relative depth, converted between tabs and spaces when the
  two files indent differently.
- Indent only works under the task directly above, at the same level.
  Outdent puts the task after its parent's whole subtree, so the tasks
  below it stay where they are.
- Moving a task under itself or one of its own subtasks is refused.
- `--format json` prints one `{description, from, to}` object per moved
  block, giving its old and new `path` / `line`, for editor
  integrations.

## Linked parents

A task can have a parent in another file without moving there. Put a
wikilink to the parent in the task's description:

```markdown
<!-- projects/vendor.md -->
- [ ] Vendor onboarding ^vo1

<!-- journal/2026-10-21.md -->
- [ ] reproduce on staging [[vendor#^vo1]] 📅 2026-10-23
```

Only two kinds of link make a parent. Every other link is just a link.

- **A block reference to a task**: `[[note#^id]]`, or `[[#^id]]` within
  the same file. The `^id` is Obsidian's block id at the end of the
  parent's line.
- **A link to an anchor note**: `[[note]]`, where the note's frontmatter
  names the task that stands for it, and that task carries the matching
  `🆔`:

  ```markdown
  ---
  ft:
    tasks:
      anchor: ABC-123
  ---
  - [ ] Fix login 🆔 ABC-123
  ```

The first such link on the line is the parent; any later ones are
reported by `ft tasks check`. If a task is also indented under another
task, the indentation wins. Linked children appear under their parent
everywhere: `ft tasks list --tree`, the TUI tree (marked `↳`), the graph,
and the query attributes `parent`, `parent_status`, `parent_link` and
`children`.

```sh
ft tasks link journal/2026-10-21.md:3 --under "Vendor onboarding"   # adds ^id to the parent if needed
ft tasks unlink journal/2026-10-21.md:3
ft tasks create "write regression test" --link-parent ABC-123        # lands in today's note
ft tasks check                                                       # dangling / cycle / ignored / extra; exit 1 on issues
ft tasks list orphaned                                               # open tasks under a done or cancelled parent
```

ft keeps links consistent as you restructure:

- Nesting a task physically (`--under`, indent, TUI paste) removes its
  now-redundant parent link.
- Moving a task that carries a block id to another file rewrites every
  `[[…#^id]]` that pointed at it. `--dry-run` shows those edits, and the
  move's output counts them.

## Remote sources

ft can pull the Jira tickets (or GitHub issues) assigned to you into the
vault, one note per issue. The note is a normal markdown note, so you add
local subtasks to a ticket like to any other task. They work offline,
show in Obsidian, and are tracked by git.

```sh
# once per machine that should sync (others simply skip)
ft sources login jira          # paste an Atlassian API token; verified, stored in the keychain
ft tasks sync                  # also runs in the TUI at startup and every 10 min; `g r` syncs now
ft tasks list remote --tree    # your open tickets, epics above them, your subtasks below
```

Configure sources in `.ft/config.toml` (see
[config.md](../config.md#sources)). Each issue becomes a note like
`jira/ABC-123.md`:

```markdown
---
ft:
  tasks:
    anchor: ABC-123
    parent: "[[EPIC-7]]"
  source:
    name: jira
    key: ABC-123
    status: In Review
    state: active
    …
---
# ABC-123 Fix login

- [/] [ABC-123](https://…/browse/ABC-123) Fix login ⏫ 📅 2026-10-30 🆔 ABC-123
  - [ ] reproduce on staging        ← your subtasks: nest them here…

(Description and Comments sections, kept up to date by sync)
```

…or link to the ticket from anywhere, e.g. from today's note:
`- [ ] write regression test [[ABC-123]]`, or
`ft tasks create "…" --link-parent ABC-123` (TUI: `S` on the ticket).

What sync does:

- **Remote-owned fields:** Jira owns the title, due date, priority and
  status. Changing title, due or priority in ft is refused (`due is owned
  by jira — change it in jira`). Your `#tags`, `⏳`/`🛫` dates, other
  notes, and subtasks are never touched.
- **Local status changes are kept:** if you tick a ticket locally, sync
  leaves it and reports it as *pending push* (pushing back to Jira is a
  later feature). If the status changed on both sides, sync keeps yours
  and reports a conflict.
- **Tickets that leave your queries:** if closed remotely, they're marked
  done locally. If still open but no longer yours, they're cancelled
  locally and the note is marked `state: out-of-scope`. Notes are never
  deleted.
- **Parents and epics** are synced as `state: context` notes, so the tree
  shows epic → story → your subtasks.
- **Checklists in descriptions** are shown as `☐` / `☑`. They never become
  tasks in your vault.
- **Offline and multi-machine:** without network or credentials, sync is
  skipped silently and everything else works. All sync state lives in
  the notes, and every machine produces the same bytes, so git sync
  doesn't conflict.

`ft tasks sync --dry-run` shows the diff, `--format json` gives a
per-source summary, and `ft sources list` shows where each source's
credentials come from.

## The Tasks tab in the TUI

`ft tui` opens a tabbed interface; press `2` to jump to the Tasks tab
(or `Tab` to cycle). The tab shows a sidebar with built-in and user
views (`Today`, `Overdue`, `Upcoming`, `Done today`, each of your
presets), plus a main pane with the current view's tasks.

Common chords inside the Tasks tab (priority moved from `p` / `P` to
`+` / `-` when cut-and-paste took `p` / `P`):

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
| `+` / `-` | cycle priority up / down                         |
| `M`       | move the selected task to another file / heading |
| `d`       | mark / unmark the selected task for moving (cut) |
| `p` / `P` | move the marked tasks under / after the selected task |
| `Esc`     | clear the marked tasks                           |
| `>` / `<` | indent / outdent the selected task              |
| `L`       | link the marked tasks under the selected task (no move) |
| `U`       | remove the selected task's parent link           |
| `S`       | new task in today's note, linked under the selected task |
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

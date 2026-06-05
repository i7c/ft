# Capture and templates

Two features cover "add structured content to a note in one motion":

- **Append-with-template** renders a template into an *existing* note
  — at end-of-file or after a named section. CLI:
  `ft notes append`. TUI: `a` (Notes tab) / `A` (Graph tab).
- **Quick capture** is a config-driven preset that fires a
  pre-templated entry from a single keystroke. TUI: `Q` from any tab
  with capture support.

Both share the same MiniJinja template engine, the same template
folder, and the same variable surface. The exhaustive reference is in
[docs/append-and-capture.md](../append-and-capture.md); this chapter
shows the everyday shape.

## Templates folder

By default `ft` looks in `<vault>/templates-ft/`. Override with
`[notes].templates_dir`:

```toml
[notes]
templates_dir = "templates-ft"   # default; set to anything you prefer
```

A template is just a Markdown file rendered through MiniJinja. Inside,
you have:

| Variable | Type     | Source                                                      |
|----------|----------|-------------------------------------------------------------|
| `title`  | string   | target file's basename without `.md` (override with `--title`) |
| `today`  | date     | `FT_TODAY` if set, else local date                           |
| `now`    | datetime | `FT_TODAY` 00:00 if set, else local now                      |
| `vars`   | map      | every `--var KEY=VAL` from the CLI                           |

Filters: `date(format=)`, `parse_date(format=)`, `add_days(n)`,
`add_weeks(n)`, `add_months(n)`, `weekday_of(n)`, `quarter`. Undefined
variables raise a render error — typos are caught loudly, not silently
blank.

A tiny example template — `templates-ft/daily.md`:

```markdown
# {{ title }}

## Plan

## Time Blocks

## Reflections
```

Used by `[periodic_notes.daily]` when the day's note is missing, or
explicitly via `ft notes create journal/scratch.md --template daily`.

## Append-with-template, from the CLI

```sh
ft notes append journal.md --template daily-log
ft notes append journal.md --template session --section "Sessions"
ft notes append journal.md --template daily --var mood=good --var energy=high
```

The rendered template lands at one of three places, in priority order:

1. **`--section "Heading"`** — explicit flag. Renders after the body
   of the named section (case-insensitive, any ATX level).
2. **`ft-append-section: Heading`** in the target's YAML frontmatter.
   When set, you can `ft notes append note.md --template X` without
   passing `--section` every time.
3. **End of file** — if neither is configured.

When multiple headings share the same text, the first one (document
order) wins. After writing, `$EDITOR` opens at the line where the
template was inserted, so you land on the new content immediately.

`--obsidian` swaps the editor handoff for an `obsidian://open` URL.
`--no-open` suppresses both.

## Quick capture

Quick-capture presets fire a templated entry — either an append or a
create — from a single keystroke. They live under `[capture_presets]`
in your config; the name in the table is the key you press in the
fuzzy picker after `Q`.

### Append preset

```toml
[capture_presets.session]
action = "append"
template = "session"
note = "Areas/therapy.md"
section = "Sessions"
```

Press `Q`, pick `session`, `Enter`. A new entry from `session.md`
appears under `## Sessions` in `Areas/therapy.md`. Editor opens at
the insertion line.

When `note` is set, the target is hardcoded. When it's absent, the
target comes from context:

- **Graph tab** — the currently selected note.
- **Notes tab** — opens a fuzzy file picker.

When `section` is set, the template lands there. When it's absent,
`ft-append-section` frontmatter is consulted. When that's also absent,
end-of-file.

`note` accepts strftime tokens, so you can always append to today's
daily without configuring a daily periodic-notes block:

```toml
[capture_presets.jot]
action = "append"
template = "quick-log"
note = "journal/%Y/%Y-%m-%d"
```

### Create preset

```toml
[capture_presets.thought]
action = "create"
template = "inbox"
path = "%Y%m%d-%H%M"
folder = "Inbox"
```

Creates `Inbox/20260601-1430.md` from `inbox.md`, opening the editor
at the *last* line of the new file (so you land on content, not the
top of a heading). `path` and `folder` together name the destination;
both accept the same strftime tokens as periodic notes. If `path` is
absent, a filename prompt opens.

The full key reference (every preset field, every behavior, every
recipe) is in
[docs/append-and-capture.md](../append-and-capture.md#configuration).

## A handful of recipes

### Session log under a heading

```toml
[capture_presets.session]
action = "append"
template = "session"
note = "Areas/therapy.md"
section = "Sessions"
```

```markdown
# templates-ft/session.md
### {{ now | date(format="%Y-%m-%d %H:%M") }}

-
```

Result: `Q` → `session` adds a new `### YYYY-MM-DD HH:MM` row under
`## Sessions`, editor lands on the new bullet.

### Quick log to the selected note (Graph tab)

```toml
[capture_presets.log]
action = "append"
template = "quick-log"
```

```markdown
# templates-ft/quick-log.md
- [ ] {{ now | date(format="%H:%M") }} 
```

Result: navigate the graph tree to any note, `Q` → `log`. New line at
end of that note's file.

### Meeting note in a folder

```toml
[capture_presets.meeting]
action = "create"
template = "meeting"
path = "%Y-%m-%d meeting"
folder = "Meetings"
```

```markdown
# templates-ft/meeting.md
# {{ now | date(format="%Y-%m-%d") }} Meeting

**Attendees:**

**Agenda:**

-
```

Result: `Q` → `meeting` creates `Meetings/2026-06-01 meeting.md`,
editor opens at the last line.

## TUI keys for capture and append

| Tab     | Key | Effect                                                                                |
|---------|-----|---------------------------------------------------------------------------------------|
| Graph   | `A` | Append template to the **selected** note.                                              |
| Graph   | `Q` | Open the capture-preset picker. Append presets without `note` use the selected note.  |
| Notes   | `a` | Append template — picker selects the target.                                          |
| Notes   | `Q` | Open the capture-preset picker. Append presets without `note` prompt for a target.    |
| Both    | `C` | Create from template (a multi-step flow: pick template, pick folder, name).            |

The created or appended file opens in the configured editor strategy
(see [vault-and-config.md](vault-and-config.md#editor-strategy)).
Inside tmux, the default is a popup window so the TUI stays drawing
behind it.

## Where this fits

Append-with-template + quick capture together cover the "don't break
flow" path. For one-off, free-form new notes, prefer `ft notes
create`. For periodic notes with templates that should render
automatically when missing, prefer the `[periodic_notes.<period>]`
blocks (see
[vault-and-config.md](vault-and-config.md#periodic-notes)).

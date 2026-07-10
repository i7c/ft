# Append with Template & Quick Capture

Two features that share the same template engine and jointly cover
"add content to notes in one motion":

- **Append with template** renders a template into an *existing* note —
  at end-of-file or after a named section heading. Available via CLI
  (`ft notes append`) and TUI (`a` / `A`).
- **Quick capture** is a config-driven shortcut that fires a preset
  (append or create from a template) in one keypress from the TUI
  (`Q`). No template prompt, and — for pre-configured presets — no
  file prompt either.

---

## Append with template

### CLI: `ft notes append`

```bash
ft notes append <PATH> --template <TEMPLATE>
```

| Flag            | Required | Description                                                                 |
|-----------------|----------|-----------------------------------------------------------------------------|
| `PATH`          | yes      | Target note (vault-relative or absolute). Must already exist.               |
| `--template`    | yes      | Template name (resolved under `[notes].templates_dir`). `.md` auto-appended. |
| `--section`     | no       | Section heading to append under. Case-insensitive, any ATX level. Takes precedence over frontmatter. |
| `--title`       | no       | Override the `title` variable in the template (defaults to the target file's stem). |
| `--var KEY=VAL` | no       | Custom template variable, surfaced as `{{ vars.KEY }}`. Repeatable.         |
| `--no-open`     | no       | Don't open the file in `$EDITOR` after appending.                           |
| `--editor`      | no       | Override `$EDITOR` for this invocation.                                     |
| `--obsidian`    | no       | Open via `obsidian://open` URL after appending.                             |
| `--vault-name`  | no       | Vault name used in `--obsidian` URL.                                        |

The CLI always spawns `$EDITOR` as a one-shot process (resolved via
`VISUAL` → `EDITOR` → `vi`). The TUI's editor handoff is configured
separately — see [docs/config.md §`[editor]`](config.md#editor) for
the tmux-popup / window / split / suspend strategies.

**Examples:**

```bash
# Append the "daily-log" template to the end of journal.md
ft notes append journal.md --template daily-log

# Append under a specific section heading
ft notes append journal.md --template session --section "Sessions"

# Append with custom template variables
ft notes append journal.md --template daily --var mood=good --var energy=high
```

The editor opens at the line where the template was inserted, so you
land directly on new content — not the top of the file.

### Section targeting

Three ways to choose *where* in the file the template lands:

1. **End of file** — the default when nothing else is configured.
2. **Frontmatter key** — set `ft.append.section` in the target note's YAML frontmatter:

   ```yaml
   ---
   ft:
     append:
       section: Sessions
   ---
   # Journal
   ## Sessions
   ...
   ```

   When you run `ft notes append journal.md --template session`,
   the rendered template is inserted after the body of the `## Sessions`
   section. No `--section` flag needed — the note carries its own
   append target.

3. **`--section` flag** — an explicit `--section "Sessions"` takes
   precedence over the frontmatter key. Use this when you want to
   append to a different section than what the note's frontmatter
   specifies.

Matching is case-insensitive and trimmed. Any ATX level works —
`### Sessions` matches just as well as `## Sessions`. When multiple
headings share the same text, the first one (in document order) wins.

### TUI: `a` and `A`

| Tab       | Key   | Behavior                                                                 |
|-----------|-------|--------------------------------------------------------------------------|
| Graph tab | `A`   | Append with template to the **selected note**. Opens the template picker; on Enter, renders and appends, then opens the editor at the insertion line. |
| Notes tab | `a`   | Append with template. Opens the template picker, then the vault file picker to choose the target note. |

Both flows respect the target note's `ft.append.section` frontmatter.

---

## Quick capture

Quick capture presets let you fire off a templated entry with a single
keystroke. No template prompt, and (when the preset is fully specified)
no file prompt either. Designed for thoughts you want to capture without
breaking flow.

### Configuration

Presets live under `[capture_presets.<name>]` in your config:

```toml
[capture_presets.journal]
action = "append"
template = "daily-log"
note = "Journal/daily.md"
section = "Daily Log"

[capture_presets.idea]
action = "create"
template = "inbox"
path = "inbox-%Y%m%d-%H%M"
folder = "Inbox"
```

| Key        | Required | Applies to    | Description                                                                                     |
|------------|----------|---------------|-------------------------------------------------------------------------------------------------|
| `action`   | yes      | both          | `"append"` or `"create"`.                                                                      |
| `template` | yes      | both          | Template name resolved under `[notes].templates_dir`. `.md` auto-appended.                      |
| `note`     | no       | append        | Hardcoded target note (vault-relative path). Supports strftime tokens (`%Y`, `%m`, `%d`, etc.). When absent, the target comes from tab context.     |
| `section`  | no       | append        | Section heading to append under. Case-insensitive, any ATX level. Takes precedence over `ft.append.section` frontmatter. |
| `path`     | no       | create        | Filename pattern with strftime tokens (`%Y`, `%m`, `%d`, `%q`, `%Q`). `.md` auto-appended. When absent, opens a filename prompt. |
| `folder`   | no       | create        | Target folder (vault-relative). Defaults to vault root when absent.                             |

Unknown keys in a preset are rejected at config load time — typos
surface immediately.

### How presets resolve

#### Append presets

1. If `note` is set, append to that file directly. No prompt.
2. If `note` is absent:
   - **Graph tab** → uses the currently selected note.
   - **Notes tab** → opens the vault file picker to choose the target.
3. If `section` is set, append under that heading. Otherwise, read
   `ft.append.section` from the target note's frontmatter. If neither,
   append to end of file.

#### Create presets

1. If `path` is set, expand strftime tokens against today's date
   (`FT_TODAY`-aware), combine with `folder`, and create the note.
   If the file already exists, it is silently overwritten (quick
   capture is optimistic).
2. If `path` is absent, open a filename prompt. The file is created
   under `folder` (or vault root if `folder` is absent).

After creation, the editor opens at the **last line** of the new file —
not line 1, so you land on the templated content.

### Path pattern tokens

Create presets accept the same strftime tokens as
[periodic notes](config.md#token-surface):

| Token | Meaning                | Example (2026-06-01)    |
|-------|------------------------|-------------------------|
| `%Y`  | 4-digit year           | `2026`                  |
| `%m`  | Zero-padded month      | `06`                    |
| `%d`  | Zero-padded day        | `01`                    |
| `%H`  | Zero-padded hour (24h) | `14`                    |
| `%M`  | Zero-padded minute     | `30`                    |
| `%q`  | Quarter digit (1..=4)  | `2`                     |
| `%Q`  | Quarter with `Q` prefix| `Q2`                    |

Non-`%` characters are passed through literally.

### TUI: `Q`

| Tab       | Key | Behavior                                                                                |
|-----------|-----|-----------------------------------------------------------------------------------------|
| Graph tab | `Q` | Opens a fuzzy picker listing all `[capture_presets]` names. On Enter, executes the preset — append presets without `note` use the selected note. |
| Notes tab | `Q` | Same preset picker. Append presets without `note` open the vault file picker for target selection. |

---

## Recipes

### Session log (append to section)

Goal: append a timestamped entry to `## Sessions` in `Areas/therapy.md`
every time you finish a session.

**Config:**

```toml
[capture_presets.session]
action = "append"
template = "session"
note = "Areas/therapy.md"
section = "Sessions"
```

**Template** (`templates-ft/session.md`):

```markdown
### {{ now | date(format="%Y-%m-%d %H:%M") }}

-
```

**Usage:** Press `Q` → `Enter` on `session`. A new `### 2026-06-01
14:30` entry appears under `## Sessions`. Editor opens at that line.

### Daily thought inbox (create with timestamp)

Goal: drop a quick note into an inbox folder with a unique filename.

**Config:**

```toml
[capture_presets.thought]
action = "create"
template = "inbox"
path = "%Y%m%d-%H%M"
folder = "Inbox"
```

**Template** (`templates-ft/inbox.md`):

```markdown
# {{ title }}

---
```

**Usage:** Press `Q` → `thought`. Creates
`Inbox/20260601-1430.md`. Editor opens at line 3 (last line).

### Meeting notes (create in folder)

Goal: create meeting notes in a `Meetings/` folder with a readable
filename, then jump straight in.

**Config:**

```toml
[capture_presets.meeting]
action = "create"
template = "meeting"
path = "%Y-%m-%d meeting"
folder = "Meetings"
```

**Template** (`templates-ft/meeting.md`):

```markdown
# {{ now | date(format="%Y-%m-%d") }} Meeting

**Attendees:**

**Agenda:**

-
```

**Usage:** Press `Q` → `meeting`. Creates
`Meetings/2026-06-01 meeting.md`. Editor opens at line 7 (last line).

### Weekly review append (frontmatter-driven)

Goal: append a review template to `journal/%Y/%G-W%V.md` under the
`## Review` section — but let the note's frontmatter decide the section
name.

**Config:**

```toml
[capture_presets.review]
action = "append"
template = "weekly-review"
# note not set — target comes from tab context
section = "Review"
```

**Target note frontmatter:**

```yaml
---
ft:
  append:
    section: Review
---
```

**Usage:** Graph tab → navigate to the weekly note → `Q` → `review`.

### Graph-tab quick log (no hardcoded note)

Goal: append a quick log entry to whichever note you have selected
in the graph tab.

**Config:**

```toml
[capture_presets.log]
action = "append"
template = "quick-log"
```

**Template** (`templates-ft/quick-log.md`):

```markdown
- [ ] {{ now | date(format="%H:%M") }} 
```

**Usage:** Graph tab → select any note → `Q` → `log`. The template
lands at end-of-file (no `section` and no frontmatter configured).
Editor opens at the new line.

### Daily journal append (dated note path)

Goal: always append to today's daily note without configuring periodic
notes or selecting a target.

**Config:**

```toml
[capture_presets.jot]
action = "append"
template = "quick-log"
note = "journal/%Y/%Y-%m-%d"
```

**Usage:** Press `Q` → `jot` from any tab. The template is appended to
`journal/2026/2026-06-01.md` (today's date), regardless of which note
is selected.

---

## Template variables

Both features use the same MiniJinja template engine and variable
surface as `ft notes create`. Available variables:

| Variable | Type                          | Description                                   |
|----------|-------------------------------|-----------------------------------------------|
| `title`  | string                        | Basename of the target file without `.md`.     |
| `today`  | date                          | Today's date (`FT_TODAY`-aware).              |
| `now`    | datetime                      | Current instant (`FT_TODAY`-aware).           |
| `vars`   | map of strings                | Custom variables passed via `--var`.           |

Available filters: `date(format=)`, `parse_date(format=)`, `add_days(n)`,
`add_weeks(n)`, `add_months(n)`, `weekday_of(n)`, `quarter`.

Unknown variables (including typos) raise render errors — strict
undefined mode prevents silent blanks in generated Markdown.

---

## Editor jump behavior

| Operation                  | Editor opens at                              |
|----------------------------|----------------------------------------------|
| `ft notes create`          | Line 1 (existing behavior, unchanged)        |
| `ft notes append`          | Line where template was inserted             |
| Quick capture (create)     | Last line of the new file                    |
| Quick capture (append)     | Line where template was inserted             |

This means quick capture always lands you on the newly added content
immediately — no scrolling required.

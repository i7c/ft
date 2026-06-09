# Configuration

`ft` reads configuration from two TOML files and merges them. The vault
file wins over the user file for every key.

## File locations

| Layer | Path                                          | Purpose                          |
|-------|-----------------------------------------------|----------------------------------|
| User  | `$XDG_CONFIG_HOME/ft/config.toml` (defaults to `~/.config/ft/config.toml`) | Defaults that span every vault.  |
| Vault | `<vault>/.ft/config.toml`                     | Vault-specific overrides.        |

Both files are optional. Missing files are silently skipped. Unknown
keys are rejected with an error — typos surface immediately rather than
silently default.

## Top-level keys

| Key                     | Type                              | Where         | Notes                                                  |
|-------------------------|-----------------------------------|---------------|--------------------------------------------------------|
| `default_vault`         | string (path)                     | user only     | Fallback vault when none of the discovery paths match. |
| `default_task_location` | string (vault-relative path)      | either        | File used by `ft tasks create` when `--file` is unset and `[periodic_notes.daily]` isn't configured. |
| `ignored_paths`         | array of glob strings             | either        | Folders/files excluded from the vault scan.            |
| `[notes]`               | table                             | either        | Note-creation settings (template folder).              |
| `[periodic_notes.*]`    | table per period                  | either        | Daily/weekly/monthly/quarterly/yearly note layout.     |
| `[editor]`              | table                             | either        | How the TUI hands off to `$EDITOR` (inline / tmux popup / window / split). |
| `[git]`                 | table                             | either        | `ft git sync` settings (pull strategy).                |
| `[presets]`             | table of `name = "query"` entries | either        | Named [query DSL](query-dsl.md) presets.               |
| `[synth]`               | table                             | either        | Synthesis ritual: default folder for new synth notes; path-prefix exclude filter for `ft review`. |

`default_vault` is only honored in the user config; setting it in a
vault config does nothing (the vault has already been chosen by the
time that config is read).

## `default_task_location`

Vault-relative path to the file that receives new tasks when neither
`--file` is passed nor `[periodic_notes.daily]` is configured.

```toml
default_task_location = "Inbox.md"
```

If unset and no daily note is configured, `ft tasks create` errors with
a hint pointing at this key.

## `ignored_paths`

Globs (relative to the vault root) excluded from scanning. Always
combined with the built-in defaults `.obsidian/`, `.git/`,
`attachments/`, and any `.gitignore` rules in the vault.

```toml
ignored_paths = ["archive/", "drafts/**", "*.tmp.md"]
```

## `[notes]`

```toml
[notes]
templates_dir = "templates-ft"   # vault-relative; default: "templates-ft"
```

| Key             | Type   | Default          | Notes                                                              |
|-----------------|--------|------------------|--------------------------------------------------------------------|
| `templates_dir` | string | `"templates-ft"` | Folder holding the ft template set used by `ft notes create` and the TUI `C`/`c` flows. Missing folder is fine — pickers just show an empty list. |

The template engine and its variable surface are documented next to the
templates themselves (see `templates-ft/README.md` in the vault, written
by plan 009).

## `[periodic_notes.*]`

Per-period configuration for periodic notes — daily, weekly, monthly,
quarterly, yearly. Each period is independent; only the periods you
configure are reachable from `ft notes periodic <period>` and the TUI
chord.

```toml
[periodic_notes.daily]
path = "journal/%Y"
format = "%Y-%m-%d"
template = "daily"

[periodic_notes.weekly]
path = "journal/%Y"
format = "%G-W%V"
template = "weeks"

[periodic_notes.monthly]
path = "journal/%Y"
format = "%Y-%m"

[periodic_notes.quarterly]
path = "journal/%Y"
format = "%Y-Q%q"
template = "quarterly"

[periodic_notes.yearly]
path = "journal"
format = "%Y"
```

| Key        | Type             | Required | Notes                                                                    |
|------------|------------------|----------|--------------------------------------------------------------------------|
| `path`     | string (pattern) | yes      | Folder pattern, vault-relative. Empty string means "vault root."         |
| `format`   | string (pattern) | yes      | Filename pattern, **without `.md`**.                                     |
| `template` | string           | no       | Template name resolved under `[notes].templates_dir`; absolute paths (starting with `/`) used as-is. When unset, new notes get a blank `# <title>\n\n` body. |

`path` and `format` both accept the chrono-strftime tokens listed below.
`ft` resolves them against the target date — `today` by default, or the
date supplied via `--date`/`--offset`.

The `daily` period is special: it's also consulted by `ft tasks create`
(and the TUI quickline) when no `--file` is supplied, so most users
will at least want a `[periodic_notes.daily]` block.

### Token surface

Standard chrono [strftime] tokens are supported. The ones you're likely
to use:

| Token  | Meaning                                         | Example output for 2026-05-14 |
|--------|-------------------------------------------------|-------------------------------|
| `%Y`   | 4-digit year                                    | `2026`                        |
| `%y`   | 2-digit year                                    | `26`                          |
| `%m`   | Zero-padded month (01..12)                      | `05`                          |
| `%-m`  | Month with no padding                           | `5`                           |
| `%d`   | Zero-padded day of month (01..31)               | `14`                          |
| `%-d`  | Day of month, no padding                        | `14`                          |
| `%B`   | Full month name                                 | `May`                         |
| `%b`   | Abbreviated month name                          | `May`                         |
| `%A`   | Full weekday name                               | `Thursday`                    |
| `%a`   | Abbreviated weekday name                        | `Thu`                         |
| `%j`   | Day of year (001..366)                          | `134`                         |
| `%G`   | ISO-week-numbering year                         | `2026`                        |
| `%V`   | ISO week number (01..53)                        | `20`                          |
| `%u`   | ISO weekday (1=Mon..7=Sun)                      | `4`                           |
| `%H`   | Zero-padded hour, 24h                           | `00`                          |
| `%M`   | Zero-padded minute                              | `00`                          |
| `%%`   | Literal `%`                                     | `%`                           |

Non-`%` characters in `path` and `format` are passed through literally
— `journal/%Y` resolves to `journal/2026/`, no escaping needed.

#### Quarter tokens (ft extension)

chrono's strftime has no quarter token, so `ft` pre-processes two of
its own before delegating to chrono:

| Token | Meaning                | Example output (2026-05-14) |
|-------|------------------------|-----------------------------|
| `%q`  | Quarter digit (1..=4)  | `2`                         |
| `%Q`  | Quarter prefixed with `Q` | `Q2`                     |

`%%q` and `%%Q` escape the tokens to literal text.

### `path` and `format` together

The final on-disk path is `<vault>/<path>/<format>.md`. Empty `path`
means the note lives at the vault root. Both fields are templated
independently against the same date, so you can split year out of the
folder and keep the rest in the filename:

```toml
[periodic_notes.daily]
path = "journal/%Y"    # → journal/2026
format = "%Y-%m-%d"    # → 2026-05-14
# resolved file: journal/2026/2026-05-14.md
```

### Templates

When `template` is set, `ft` reads that file under
`<vault>/<templates_dir>/<template>.md` (the `.md` is added when
missing) and renders it through the MiniJinja engine. The `title`
variable inside the template is the filename stem — e.g. `2026-05-14`
for the daily example above, `2026-W20` for the weekly. See
`templates-ft/README.md` in your vault for the full template surface
(filters, variables, gotchas).

## `[editor]`

Controls how the TUI launches `$EDITOR` when a tab raises an
"open in editor" request (the Notes-tab `o`/`c`/`C`/`t`/`p<…>` flows,
the section-move new-target sub-flow, the Tasks-tab `e` edit, …). The
CLI (`ft notes open` / `create` / `periodic` / `today`) is unaffected —
it's always a one-shot spawn.

```toml
[editor]
strategy = "tmux-popup"   # default; also: tmux-window, tmux-split, suspend
popup_width = "90%"       # tmux-popup only; tmux geometry syntax
popup_height = "90%"      # tmux-popup only
```

| Key            | Type   | Default        | Notes                                                                                          |
|----------------|--------|----------------|------------------------------------------------------------------------------------------------|
| `strategy`     | string | `"tmux-popup"` | One of `tmux-popup`, `tmux-window`, `tmux-split`, `suspend`. See below.                        |
| `popup_width`  | string | `"90%"`        | Width passed to `tmux display-popup -w`. Accepts tmux syntax (`"90%"`, `"120"`). Popup only.   |
| `popup_height` | string | `"90%"`        | Height passed to `tmux display-popup -h`. Same syntax as width. Popup only.                    |

### Strategies

- **`tmux-popup`** *(default)* — Runs the editor in a centered
  `tmux display-popup -E` overlay. Requires tmux ≥ 3.2 and ft running
  inside tmux (`$TMUX` set). The popup forwards every keystroke to the
  editor (including ESC, so nvim's mode switches work normally) and
  closes when the editor exits. ft keeps drawing behind the popup —
  no alt-screen dance, no terminal-mode handshake, no swallowed
  keystrokes.

- **`tmux-window`** — Opens the editor in a fresh tmux window via
  `tmux new-window`. ft blocks on a `tmux wait-for` handshake so the
  post-edit refresh runs against on-disk state.

- **`tmux-split`** — Same as `tmux-window` but `tmux split-window`
  (horizontal split). ft refreshes via the same `wait-for` handshake.

- **`suspend`** — The pre-plan-011 behavior: ft disables raw mode,
  leaves the alt-screen, runs the editor inline, and restores the
  alt-screen on exit. Used when the user is not inside tmux (any
  `tmux-*` value falls back to `suspend` automatically — see below)
  or explicitly prefers the inline experience.

### `$TMUX` fallback

The three `tmux-*` strategies all require ft to be running inside a
tmux session. When `$TMUX` is unset or empty, ft resolves any `tmux-*`
value to `suspend` at use time — there's no warning, just the inline
behavior. This means the default of `tmux-popup` does the right thing
for users who sometimes run ft inside tmux and sometimes don't: same
config, different terminal contexts.

### Missing `tmux` binary

If the configured strategy is `tmux-*` and ft can't find `tmux` on
`$PATH`, the dispatch falls back to `suspend` for that one open and
surfaces an error toast: `"tmux not found — opening editor inline"`.
Configuration is unchanged; the next open under a tmux strategy will
re-attempt.

## `[git]`

Settings for `ft git sync` (and the TUI `g s` chord). The repo is
discovered by walking up from the vault root looking for a `.git/`
entry — if none exists anywhere up the tree, the feature is
unavailable.

```toml
[git]
pull_strategy = "merge"   # default; also: rebase
```

| Key             | Type   | Default   | Notes                                                       |
|-----------------|--------|-----------|-------------------------------------------------------------|
| `pull_strategy` | string | `"merge"` | `"merge"` → `git pull --no-rebase`; `"rebase"` → `git pull --rebase`. |

### What `ft git sync` does

1. Pre-check the current branch's upstream (`@{u}`). If none, error
   out **before** touching the tree (no orphan local commit).
2. Snapshot the working tree. `git add -A` then `git commit -m "ft
   sync <iso8601-utc>"` if there's anything to stage — modifications,
   deletions, and untracked files all included. `.gitignore` is
   honored (git filters ignored entries from staging automatically).
   Override the auto-generated commit message with `-m / --message`.
3. `git pull --no-rebase` or `git pull --rebase` per
   `pull_strategy`.
4. On conflict (merge or rebase), leave the working tree in its
   conflicted state — markers stay in the files, the merge/rebase
   stays in progress. `ft git sync` exits **2** with the conflicted
   file list on stderr. Resolve manually.
5. On success, `git push`. Authentication uses your existing
   credential helper / SSH agent / GPG signing — ft inherits the
   process environment.

`ft git sync --dry-run` reads `status` + `upstream` and prints the
plan without writing anything.

## `[keymap]`

Override TUI key bindings without recompiling. The vault `[keymap]`
replaces the user `[keymap]` whole — there is no per-entry merge
between layers. If both files set `[keymap]`, the vault file wins entirely.

### Schema

```toml
[keymap]
strict = false       # default: false; true → TUI startup fails on any error

# Per-scope overrides: chord string → command name.
# Valid scopes: global, tab/graph, tab/tasks, tab/notes, tab/timeblocks,
#               tab/journal, modal/create, modal/append, modal/section-move,
#               modal/capture-var, modal/periodic-leader, modal/query-bar,
#               modal/rename, modal/search, modal/preset-picker,
#               modal/capture-picker, modal/related, modal/move
[keymap.global]
"h" = "app.help"        # add a new alias for the help overlay
"x" = "app.quit"        # bind a different chord to an existing command

[keymap."tab/graph"]
"F5" = "graph.refresh"  # override an existing chord

# Remove default chords (one entry per chord to remove):
[[keymap.unbind]]
scope = "global"
chord = "q"             # remove q → app.quit (Ctrl+c still quits)
```

### Chord syntax

Chords are the same strings accepted by `chord_from_str`:

| Example           | Meaning                             |
|-------------------|-------------------------------------|
| `"q"`             | Lowercase `q`, no modifiers         |
| `"Q"` / `"Shift+q"` | Shift + q (equivalent)           |
| `"Ctrl+s"`        | Ctrl + s                            |
| `"Alt+1"`         | Alt + 1                             |
| `"F5"`            | Function key 5                      |
| `"Esc"`           | Escape                              |
| `"Enter"`         | Enter / Return                      |
| `"Backspace"`     | Backspace                           |
| `"Tab"`           | Tab                                 |
| `"BackTab"`       | Shift+Tab (reverse tab)             |
| `"Ctrl+PageDown"` | Ctrl + Page Down                    |

`ft commands list` prints every registered command name and its scope.
`ft commands check-keymap` validates your config and reports errors
without launching the TUI.

### `strict` flag

When `strict = false` (the default), a bad `[keymap]` entry (unknown
command, invalid chord, missing unbind target) is silently ignored and
the binding falls back to the default. When `strict = true`, the TUI
refuses to start and prints every error to stderr. Use `strict = true`
in CI or personal configs where you want typos surfaced immediately.

### Examples

**Rebind quit to `x`, keep `Ctrl+c`:**

```toml
[keymap.global]
"x" = "app.quit"

[[keymap.unbind]]
scope = "global"
chord = "q"
```

**Add a second chord for the help overlay:**

```toml
[keymap.global]
"h" = "app.help"
```

**Override a tab-specific chord:**

```toml
[keymap."tab/tasks"]
"F5" = "tasks.refresh"
```

## `[presets]`

Map of preset names to query strings. Use a preset from the CLI with
`ft tasks list --preset <name>`.

```toml
[presets]
work = "tag is #work and not done"
today = "due on today"
overdue = "due before today and not done"
```

The query syntax is documented in [query-dsl.md](query-dsl.md).

## `[synth]`

Settings for the synthesis ritual (`ft review`, `ft notes journal --link`,
`ft synth`). See [guide/synthesis.md](guide/synthesis.md) for the user
walkthrough.

```toml
[synth]
# Default folder for `ft synth scaffold <bare-name>` (the CLI's
# convenience fallback when no folder is part of the path). Has no
# effect on the TUI flow, which always asks where to put new notes.
# Vault-relative. Trailing slash optional. Default: "Synthesis/".
folder = "Synthesis/"

# Files whose vault-relative path starts with any listed prefix are
# excluded from `ft review`. Conventional use: filter out your
# periodic-notes folder so daily-note repetition doesn't drown out
# real signal. Default: empty.
exclude_prefixes = ["journal/"]
```

Both keys are optional. Synth notes themselves are identified by
`ft-synth: true` in their YAML frontmatter, not by location — they
can live anywhere in the vault regardless of `folder`.

## Discovery and merge

For each invocation, `ft`:

1. Locates the vault (`--vault` flag → `$FT_VAULT` → walk up for
   `.obsidian/` → `default_vault` from the user config).
2. Reads `<user_config_dir>/ft/config.toml` if present.
3. Reads `<vault>/.ft/config.toml` if present.
4. Merges them — vault wins per-key.

Vault discovery uses `default_vault` from the user config; nothing from
the vault config can affect discovery (the vault has been chosen by
then).

## Environment variables

These influence `ft` at runtime but are not config keys:

| Variable               | Purpose                                                                                       |
|------------------------|-----------------------------------------------------------------------------------------------|
| `FT_VAULT`             | Vault path. Higher priority than `default_vault`; lower than `--vault`.                       |
| `FT_TODAY`             | Override "today" for date-aware commands (`YYYY-MM-DD`). Used heavily by tests; safe in scripts that want determinism. |
| `EDITOR` / `VISUAL`    | Editor binary for the open-in-editor flow. `VISUAL` wins when set; falls back to `vi`.        |
| `XDG_CONFIG_HOME`      | Where to look for the user config (defaults to `~/.config`).                                  |
| `FT_OBSIDIAN_DRY_RUN`  | When set to `1`, the `--obsidian` flag prints the `obsidian://` URL instead of opening it.    |
| `FT_PERF_TESTS`        | When set to `1`, the perf-tagged unit tests run; otherwise they're skipped.                   |

## A small worked example

```toml
# ~/.config/ft/config.toml
default_vault = "~/notes"
default_task_location = "Inbox.md"

[notes]
templates_dir = "templates-ft"

[periodic_notes.daily]
path = "journal/%Y"
format = "%Y-%m-%d"
template = "daily"

[periodic_notes.weekly]
path = "journal/%Y"
format = "%G-W%V"

[presets]
today = "due on today and not done"
work = "tag is #work and not done"
```

```toml
# ~/notes/.ft/config.toml
ignored_paths = ["archive/"]

# Override the user-level daily template just for this vault.
[periodic_notes.daily]
path = "journal/%Y"
format = "%Y-%m-%d"
template = "minimal-daily"
```

Result: `ft notes today` writes
`~/notes/journal/2026/2026-05-14.md` (created from the
`minimal-daily` template), and `archive/` is excluded from every
vault scan.

[strftime]: https://docs.rs/chrono/latest/chrono/format/strftime/index.html

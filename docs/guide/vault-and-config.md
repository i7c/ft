# Vault discovery and configuration

`ft` always operates against a single Obsidian vault — the directory
that contains an `.obsidian/` folder. This chapter explains how that
vault is located, the two-layer configuration model, and the small
number of settings you'll probably want to write before doing anything
serious.

The full schema (every key, every default, every token) lives in
[docs/config.md](../config.md). This chapter is the workflow-shaped
walkthrough.

## Vault discovery, in order

When you run any `ft` subcommand, the binary resolves the vault by
trying these in order and stopping at the first hit:

1. **`--vault DIR`** — an explicit flag passed on the command line.
2. **`$FT_VAULT`** — environment variable.
3. **Walk up from CWD** — looks for `.obsidian/` in the current
   directory, then each parent, all the way to `/`.
4. **`default_vault`** in `~/.config/ft/config.toml`.

The first candidate that points at a directory containing `.obsidian/`
wins. If every candidate fails, `ft` prints exactly which paths it
tried, so the failure is debuggable:

```
Error: could not locate an Obsidian vault
  --vault /tmp/notes: no .obsidian/ found
  $FT_VAULT: not set
  CWD walk from /home/you: no ancestor contains .obsidian/
  /home/you/.config/ft/config.toml: default_vault not set
```

`ft vault` shows the resolved path and the merged config without
running anything else. Use it whenever you're unsure which vault a
command will hit.

### Working with multiple vaults

The walk-up rule means you can `cd` into any vault and just run `ft`.
Set `default_vault` in your user config to whichever vault you live in
most of the time, and use `--vault` or `$FT_VAULT` to override.

## The two config layers

`ft` reads two TOML files and merges them. The vault layer wins per
key over the user layer:

| Layer | Path                                  | What it's for                                  |
|-------|---------------------------------------|------------------------------------------------|
| User  | `~/.config/ft/config.toml`            | Defaults across every vault you own.           |
| Vault | `<vault>/.ft/config.toml`             | Per-vault overrides — different daily template, different ignored paths. |

Both files are optional. Missing files are silently skipped. Unknown
keys are rejected with a clear error so typos surface immediately.

`default_vault` is only honored in the user file — by the time the
vault file is read, the vault has already been chosen.

## A starter config

If you do nothing else, create `~/.config/ft/config.toml` with this:

```toml
default_vault = "~/my-vault"
default_task_location = "Inbox.md"

[notes]
templates_dir = "templates-ft"

[periodic_notes.daily]
path = "journal/%Y"
format = "%Y-%m-%d"
template = "daily"

[presets]
today = "due on today and not done"
work = "tag is #work and not done"
```

That's enough to make `ft notes today`, `ft tasks list today`, and
`ft tasks create "…"` all do the right thing.

## Periodic notes

`[periodic_notes.<period>]` blocks tell `ft` where each kind of
periodic note lives and which template to render when it's missing.
The five periods are `daily`, `weekly`, `monthly`, `quarterly`,
`yearly` — only the ones you configure are reachable from
`ft notes periodic` and the TUI's `p` leader.

```toml
[periodic_notes.daily]
path = "journal/%Y"
format = "%Y-%m-%d"
template = "daily"

[periodic_notes.weekly]
path = "journal/%Y"
format = "%G-W%V"
template = "weekly"

[periodic_notes.monthly]
path = "journal/%Y"
format = "%Y-%m"
```

`path` is the folder pattern, `format` is the filename without `.md`.
Both accept the standard `chrono` strftime tokens — `%Y`, `%m`, `%d`,
`%G`/`%V` for ISO weeks, `%B` for full month names, plus two
`ft`-only tokens for quarters: `%q` (digit, `2`) and `%Q` (prefixed,
`Q2`). The full token table is in
[docs/config.md](../config.md#token-surface).

`template` is resolved as `<vault>/<templates_dir>/<template>.md`. If
the file doesn't exist when you open the daily note, `ft` renders the
template through MiniJinja with `title`, `today`, `now`, and your
custom `--var KEY=VAL` bindings exposed. When `template` is unset, new
notes get a blank `# <title>` body.

The **daily** block is load-bearing for more than `ft notes today`:
`ft tasks create` and `ft timeblocks add` both default to "today's
daily note" when no explicit `--file` is passed. If you don't
configure a daily block, set `default_task_location` to a fallback
file (e.g. `Inbox.md`).

## Presets

Tasks-query presets are short names for longer queries:

```toml
[presets]
backlog = "not done and no due date and tag is project"
review  = "completed after 2026-05-01 sort by completed reverse"
```

Use them positionally on `ft tasks list`:

```sh
ft tasks list backlog
ft tasks list review --format markdown
```

User presets shadow the built-ins of the same name. The built-ins
(`today`, `overdue`, `upcoming`, `done-today`) are defined in
[docs/query-dsl.md](../query-dsl.md#built-in-presets).

Graph-query presets are configured separately under `[graph.presets]`,
and follow the same shadowing rule. See [graph.md](graph.md) for the
graph query language.

## Editor strategy

When the TUI hands off to `$EDITOR` (the Notes-tab `o`/`c`/`C`/`t`/`p`
chords, section moves, Tasks-tab `e`, …), the **strategy** decides how
the editor gets the screen. The CLI side is unaffected — it's always
a one-shot spawn.

```toml
[editor]
strategy = "tmux-popup"   # default; also tmux-window, tmux-split, suspend
popup_width = "90%"
popup_height = "90%"
```

- **`tmux-popup`** — the default. Centered floating window via
  `tmux display-popup -E`. Needs tmux ≥ 3.2 and `ft` to be running
  inside tmux (`$TMUX` set). Keystrokes pass through, so nvim's mode
  switches work. The TUI keeps drawing behind the popup.
- **`tmux-window`** / **`tmux-split`** — fresh window or split.
- **`suspend`** — drop the alt-screen, run the editor inline, restore
  on exit. The fallback when `$TMUX` is empty.

Any `tmux-*` strategy silently falls back to `suspend` when `$TMUX` is
unset, so the same config works both inside and outside tmux.

## Ignored paths

`[ignored_paths]` is a list of globs (relative to the vault root) that
the scanner skips, combined with the always-on defaults (`.obsidian/`,
`.git/`, `attachments/`) and your `.gitignore`:

```toml
ignored_paths = ["archive/", "drafts/**", "*.tmp.md"]
```

Useful for excluding archive folders from `ft tasks list` and the
graph build without deleting them.

## Environment variables

These affect runtime behavior without being config keys:

| Variable               | Effect                                                                                         |
|------------------------|------------------------------------------------------------------------------------------------|
| `FT_VAULT`             | Vault path. Higher precedence than `default_vault`, lower than `--vault`.                       |
| `FT_TODAY`             | Override "today" everywhere. Format `YYYY-MM-DD`. Used heavily by tests and reproducible scripts. |
| `EDITOR` / `VISUAL`    | Editor binary. `VISUAL` wins when set; falls back to `vi`.                                      |
| `NO_COLOR`             | Suppress all colored output. Honored in addition to `--no-color`.                               |
| `FT_OBSIDIAN_DRY_RUN`  | When `1`, `--obsidian` prints the URL instead of handing off to the OS.                         |

## Verifying everything

Run `ft vault` after editing either config file. The "Merged config"
block shows what `ft` actually sees, so a `path` typo or a missing
period block is one command away from visible.

```sh
ft vault
```

Once `ft vault` is happy, you're ready for the workflow chapters:
[tasks.md](tasks.md), [notes.md](notes.md), and
[timeblocks.md](timeblocks.md) are the three most likely starting
points.

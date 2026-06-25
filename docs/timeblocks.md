# Timeblocks

`ft timeblocks` manages the day-planner block list inside each daily
note — the same `- HH:MM - HH:MM <desc> @tag` format that
Obsidian's Day Planner plugin and [blockary](https://github.com/cweisser/blockary)
both read.

## Block format

Each entry is a Markdown list item under a configurable heading
(default `## Time Blocks`):

```markdown
## Time Blocks
- 09:00 - 10:00 standup @work
- 10:00 - 10:30 review @work/code
- 12:00 - 13:00 lunch @break
```

Canonical line shape:

```
- HH:MM - HH:MM <desc> [@tag...]
```

A short form is also accepted on input — `- HH:MM <desc>` derives
`end = start + 30m`. The short form is normalized to the full form
on write.

## Configurable heading

The heading the section lives under is set in `.ft/config.toml`:

```toml
[timeblocks]
heading = "Time Blocks"   # optional; default "Time Blocks"
```

Matching is case-insensitive and any ATX level works — `## Time Blocks`
and `### time blocks` both anchor to the configured name.

## Daily-note resolution

Both the CLI and the TUI resolve the daily-note path through the
existing `[periodic_notes.daily]` config block:

```toml
[periodic_notes.daily]
path   = "journal"
format = "%Y-%m-%d"
template = "daily"   # optional
```

When the daily note **file** doesn't yet exist on disk, `ft timeblocks
add` (and the TUI `a` / `c` chords) render the configured template first
via the same `create_or_get_periodic_path` that `ft notes today` uses —
so a brand-new day always starts with your full template, not a bare
`## Time Blocks`-only file.

Pass `--file PATH` to operate on an explicit file; that opts out of the
template-render behavior so user-named paths aren't tampered with.

## Tag grammar

```
tag    ::= '@' level ('/' level)*           # max 3 levels
level  ::= [A-Za-z0-9_-]+
```

- Up to **3 levels**: `@work`, `@work/meeting`, `@work/meeting/1on1`.
- Each level is alphanumerics + `_` / `-`. Whitespace, `@`, and `/`
  are not allowed inside a level.
- Brackets / parens (`@p/[[Project]]/x`) are explicitly out of scope —
  the inline parser silently skips malformed `@…` tokens so legacy
  blockary notes still read.

Strict validation kicks in for the CLI's `--tag X` / `--add-tag X` /
`--remove-tag X` flags and the TUI `t` modal's tokens.

## CLI

### `ft timeblocks list`

```
ft timeblocks list [--date DATE] [--tag TAG]... [--file PATH]
                   [--format table|json|ndjson|markdown] [--allow-empty]
```

- `--date` accepts every form `ft_core::dates::parse` supports
  (`today`, `tomorrow`, `2026-05-10`, `+3d`).
- `--tag` is a prefix filter: `--tag work` matches `@work` and
  `@work/meeting`. Repeatable; multiple compose as OR.
- `--format markdown` emits source lines, round-trippable through
  `ft timeblocks add`.
- Exits 1 on empty result unless `--allow-empty`.

### `ft timeblocks add`

Two equivalent forms (mutually exclusive):

```
ft timeblocks add "<blockstring>" [--date DATE] [--file PATH]
                                  [--force] [--dry-run]
ft timeblocks add --start HH:MM --end HH:MM --desc "..."
                  [--tag TAG]... [--date DATE] [--file PATH]
                  [--force] [--dry-run]
```

- Refuses exact duplicates (same start + end + desc) unless `--force`.
- Creates the `[timeblocks]` heading at file end when missing.
- `--dry-run` prints a unified diff via `similar` and leaves the file
  untouched.
- All writes go through `fs::write_atomic` (temp-file + rename), so the
  file is never half-written even if Obsidian has it open.

### `ft timeblocks edit`

```
ft timeblocks edit <SELECTOR> [--start TIME_OR_DELTA] [--end TIME_OR_DELTA]
                              [--desc "..."] [--add-tag TAG]...
                              [--remove-tag TAG]... [--date DATE]
                              [--file PATH] [--dry-run]
```

- Selector forms: `<N>` (1-indexed line in the section), `HH:MM`
  (exact start match), or anything else (case-insensitive substring
  on description).
- `--start` / `--end` accept absolute (`HH:MM`) or relative (`+5m`,
  `-15m`); relative shifts clamp at 00:00 / 23:59.
- Ambiguous fuzzy matches list up to 5 candidates with line numbers.

### `ft timeblocks delete`

```
ft timeblocks delete <SELECTOR> [--date DATE] [--file PATH]
                                [--yes] [--dry-run]
```

- Same selector grammar as `edit`.
- Prompts via dialoguer when stdin is a TTY; errors with a `--yes`
  hint when run non-interactively.

### `ft timeblocks spent`

```
ft timeblocks spent [PERIOD] [--from DATE --to DATE] [--tag TAG]...
                    [--format text|json] [--allow-empty]
```

`PERIOD` is one of `today` (default), `this-week`, `this-month`,
`this-year`, `last-week`. Mutually exclusive with `--from/--to`.

- Walks every daily note in range; missing files are silently skipped.
- Aggregates per-tag hierarchically via `report::time_per_tag`.
- Text format: comfy-table with Tag / .. / .. / Time / % columns and
  a total row (excluding `@break`).
- JSON shape:
  ```json
  {
    "from": "2026-05-11",
    "to": "2026-05-17",
    "total_minutes": 300,
    "tags": [
      { "tag": "work", "minutes": 240, "children": [...] }
    ]
  }
  ```

## TUI

A dedicated tab (the rightmost of `ft tui`) shows today + tomorrow
side-by-side, with per-day block lists, a live clock, and a per-tag
totals panel for the focused day. Block row height scales with
duration: `ceil(duration_minutes / 60)` rows per block, so a 2-hour
meeting renders as a visually tall row with a dim `│` continuation
marker.

### Keymap

These are the default chords; the canonical command names (e.g.
`timeblocks.add-quickline`, `timeblocks.delete-start`) and any
user overrides live in [docs/keybindings.md](keybindings.md) under
`tab/timeblocks`.

Navigation:

| Key | Action |
| --- | --- |
| `j` / `k` / `↓` / `↑` | move selection within focused pane |
| `g` / `G` | jump to first / last block |
| `h` / `l` / `←` / `→` | flip pane focus (split) or flip day (single) |
| `f` | toggle split (today + tomorrow) vs single-day full-width |
| `H` / `L` | slide the date window back / forward by 1 day |
| `T` | jump back to actual today |
| `r` | re-read both daily notes from disk |

Mutation chords (operate on the focused pane's selected block):

| Key | Action |
| --- | --- |
| `a` | quickline create (bottom-line blockstring) |
| `A` | modal form (Start / End / Desc rows) |
| `e` | inline description edit |
| `t` | tag modal (`+@tag` / `-@tag`, space-separated) |
| `d d` | delete (two-stroke; Esc cancels) |
| `]` / `[` | extend / shrink end-time by 5 min |
| `}` / `{` | push / pull start-time by 5 min |
| `>` / `<` | shift the whole block later / earlier (duration preserved) |
| `c` | create the focused pane's daily note via the configured template |

Tab / Shift+Tab are reserved for the App's global tab-cycle, so we
don't shadow them inside the Timeblocks tab.

## Compatibility with blockary

This format is a strict subset of what
[`blockary`](https://github.com/cweisser/blockary) reads, so the same
daily notes work with both tools:

- `- HH:MM - HH:MM <desc>` list items under the configured heading.
- `@tag` / `@parent/child` tags (we restrict to 3 levels; blockary
  doesn't enforce a max, but most fixtures use ≤ 3).
- Configurable section heading (blockary hard-codes `Time Blocks`;
  ours defaults to the same).

You can keep running `blockary sync` / `blockary spent` against the
same vault.

## Out of scope (today)

- iCalendar pull / multi-vault sync — `ft` is single-vault by design.
- Block completion state (`- [x] HH:MM - HH:MM …`) — plain
  Day-Planner format only.
- Obsidian `#tag` syntax inside blocks — tags use `@…` exclusively.
- Non-overlap enforcement — overlap is allowed (matches blockary).

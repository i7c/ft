# Timeblocks

The Timeblocks feature manages the day-planner block list inside each
daily note — the same `- HH:MM - HH:MM <desc> @tag` format Obsidian's
Day Planner plugin and [blockary](https://github.com/cweisser/blockary)
both read. CLI: `ft timeblocks`. TUI: the rightmost tab.

The format spec, tag grammar, blockary compatibility notes, and every
edge case are in [docs/timeblocks.md](../timeblocks.md). This chapter
shows the everyday flow.

## The block format

Each block is a Markdown list item under a configurable heading
(default `## Time Blocks`):

```markdown
## Time Blocks
- 09:00 - 10:00 standup @work
- 10:00 - 10:30 review @work/code
- 12:00 - 13:00 lunch @break
```

The short form `- HH:MM <desc>` is also accepted on input (end
derived as `start + 30m`) and normalized to the full form on the next
rewrite.

Tags use `@` (not Obsidian's `#`) so they don't collide with note
tags. Hierarchical up to three levels, `[A-Za-z0-9_-]+` per level:
`@work`, `@work/meeting`, `@work/meeting/1on1`.

## Configuration

Time blocks live inside daily notes, so `ft` reuses the
`[periodic_notes.daily]` block from your config — no separate
"timeblocks file" setting. If the daily note file doesn't exist yet,
`ft timeblocks add` first renders the configured daily template so the
new day starts with your full layout, not just a bare `## Time
Blocks` line.

To use a non-default section heading, add:

```toml
[timeblocks]
heading = "Time Blocks"   # default; case-insensitive, any ATX level
```

## Reading a day

```sh
ft timeblocks list                            # today
ft timeblocks list --date tomorrow
ft timeblocks list --date 2026-05-15
ft timeblocks list --tag work                 # OR-of-tag-prefix filter
ft timeblocks list --tag work --format json
```

`--tag work` matches `@work` *and* `@work/meeting` (prefix filter).
Multiple `--tag` flags compose as OR. Output formats match the rest of
the CLI: `table` (default), `json`, `ndjson`, `markdown`. `markdown`
emits the source lines, round-trippable through `ft timeblocks add`.

## Adding a block

Two equivalent forms, mutually exclusive:

```sh
# Positional blockstring (parsed via the same parser used at read time)
ft timeblocks add "09:00 - 10:00 standup @work"

# Flag form (validated more strictly — explicit start/end/tag)
ft timeblocks add --start 14:00 --end 14:30 \
                  --desc "1on1 with Hans" --tag work

# Short form — end derived as start + 30m
ft timeblocks add "14:00 1on1 @work"

# Different day, or different file
ft timeblocks add "09:00 standup @work" --date tomorrow
ft timeblocks add "09:00 standup @work" --file Areas/scratch.md

# Preview only
ft timeblocks add "09:00 standup @work" --dry-run
```

Exact duplicates (same start + end + description) are refused unless
`--force`. Writes go through atomic temp-file + rename, so the daily
note is never half-written even if Obsidian has it open.

## Editing a block

Selectors are uniform across `edit` and `delete`:

- An integer `<N>` — 1-indexed line within the timeblocks section.
- `HH:MM` — exact start-time match.
- Anything else — case-insensitive substring match on the description
  (ambiguous matches list up to five candidates with line numbers).

```sh
# Shift the end out 15 minutes
ft timeblocks edit standup --end +15m

# Move the whole block to 09:30
ft timeblocks edit 2 --start 09:30 --end 10:00

# Add and remove tags
ft timeblocks edit "1on1" --add-tag work/meeting --remove-tag draft

# Edit the description
ft timeblocks edit lunch --desc "lunch (out)"
```

`--start` and `--end` accept absolute times (`HH:MM`) or relative
shifts (`+5m`, `-15m`). Relative shifts clamp at `00:00`/`23:59`.

`--dry-run` shows a unified diff of the change.

## Deleting a block

```sh
ft timeblocks delete standup
ft timeblocks delete 3 --yes      # skip confirmation
ft timeblocks delete "lunch" --dry-run
```

Interactive confirmation on a TTY; `--yes` required on a non-TTY
stdin.

## Time spent reports

`ft timeblocks spent` aggregates time per tag over a date range,
walking every daily note in the period:

```sh
ft timeblocks spent today
ft timeblocks spent this-week
ft timeblocks spent this-month
ft timeblocks spent last-week
ft timeblocks spent this-year --format json
ft timeblocks spent --from 2026-05-01 --to 2026-05-15 --tag work
```

The text format is a comfy-table grouped by tag hierarchy, with
percentages computed against the **non-break** total — so the numbers
stay comparable across reports that include vs. exclude `@break`.

JSON output is structured: a top-level object with `from`, `to`,
`total_minutes`, and a recursive `tags` tree where each node has
`tag`, `minutes`, and `children`.

## The Timeblocks tab in the TUI

Press `4` (or cycle with `Tab`) to land on the Timeblocks tab. By
default it shows a today + tomorrow split with a live clock; press
`f` to swap to a single-day full-width view. The block rows scale
vertically with duration: a 2-hour meeting renders taller than a
30-minute standup with a dim `│` continuation marker.

### Navigation

| Key                  | Action                                                      |
|----------------------|-------------------------------------------------------------|
| `j` / `k` / `↓` / `↑`| move selection within the focused pane                       |
| `g` / `G`            | first / last block                                          |
| `h` / `l` / `←` / `→`| flip pane focus (split) or flip day (single)                |
| `f`                  | toggle split (today + tomorrow) vs single-day full-width    |
| `H` / `L`            | slide the date window back / forward by 1 day               |
| `T`                  | jump back to actual today                                   |
| `r`                  | reload both panes from disk                                 |

### Mutations

| Key       | Action                                                          |
|-----------|-----------------------------------------------------------------|
| `a`       | quickline create (type a blockstring at the bottom)            |
| `A`       | modal form (Start / End / Desc rows)                            |
| `e`       | inline edit of the focused block's description                  |
| `t`       | tag editor modal (`+@tag` / `-@tag`, space-separated)           |
| `d d`     | delete the focused block (two-stroke; `Esc` cancels)            |
| `c`       | create the focused pane's daily note from your template         |

### Time edits (5-minute increments)

| Key       | Action                                                          |
|-----------|-----------------------------------------------------------------|
| `]` / `[` | extend / shrink end-time                                        |
| `}` / `{` | push / pull start-time                                          |
| `>` / `<` | shift the whole block (duration preserved)                      |

`Tab` and `Shift+Tab` stay reserved for the App's tab cycle, so they
don't shadow anything inside the Timeblocks tab.

## Compatibility

The format is a strict subset of what
[blockary](https://github.com/cweisser/blockary) reads, so you can
keep using `blockary sync` / `blockary spent` against the same vault
if you have an existing pipeline. Differences:

- `ft` caps tags at 3 levels and validates `[A-Za-z0-9_-]+` per
  level; blockary is lenient.
- `ft` enforces the same `## Time Blocks` heading by default and
  exposes it as `[timeblocks].heading`.
- Block completion state (`- [x] HH:MM - HH:MM …`) and overlap
  enforcement are out of scope today.

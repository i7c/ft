# Scripting with `ft`

`ft` is built to compose with the rest of your shell. Every CLI
command has a machine-readable output format, every error has a
predictable exit code, and there's a headless dispatch path
(`ft do`) for executing TUI-registered commands without the TUI.

This chapter is the operational manual for that side of `ft`.

## Output formats

`tasks list`, `notes backlinks`, `notes links`, `notes gather`,
`timeblocks list`, `graph query`, and the others all accept
`--format <fmt>`. The set is consistent:

| Format    | Use it for                                                   |
|-----------|--------------------------------------------------------------|
| `table`   | Human reading. Colour when stdout is a TTY.                  |
| `markdown`| Pipeable back into another vault tool (the original task / block / link lines). |
| `json`    | A single JSON array. Easy to feed into `jq -e`.              |
| `ndjson`  | One JSON object per line. Best for streaming / `xargs`.      |

Colour suppresses automatically when stdout is not a TTY, when
`NO_COLOR` is set, or when `--no-color` is passed.

`ft find` is plain-text by default and offers `ndjson`. `ft commands
list` adds `json` to the same set.

## Exit codes

The CLI uses a small, predictable set:

| Code | Meaning                                                                                                |
|------|--------------------------------------------------------------------------------------------------------|
| `0`  | Happy path. Non-empty result, no error.                                                                |
| `1`  | Generic runtime error, **or** "empty result and `--allow-empty` not passed."                           |
| `2`  | Usage error (DSL parse failure, unknown preset, modal-opening command via `ft do`, missing required arg) **or** merge/rebase conflict from `ft git sync`. |
| `3`  | The command is registered in the TUI registry but has no headless `ft do` handler yet.                  |

Empty-result-is-error is the default for the listing commands so a
`set -euo pipefail` script catches "today's preset returned nothing"
loudly. Pass `--allow-empty` when that's what you want:

```sh
# Block the rest of the script if any are overdue
if ft tasks list overdue --allow-empty --format ndjson | jq -e '.'; then
  echo "you have overdue tasks; fix those first"
  exit 1
fi
```

## `--json-errors`

The top-level `--json-errors` flag wraps any error in a single JSON
object on stderr (the operation's body still goes to stdout). The
shape is `{"error": "...", "chain": [...]}` where `chain` is the
anyhow context stack from innermost to outermost.

```sh
ft --json-errors tasks list overdue --format ndjson \
  | jq -r '.description' \
  | head -5
```

Without `--json-errors`, errors land as plain text on stderr.

## Determinism: `FT_TODAY`

Any command that resolves "today" — `--due today`, `today` keyword in
the DSL, the built-in presets, template `today`/`now` variables, the
TUI clock — reads through one seam. Set `FT_TODAY=YYYY-MM-DD` to pin
it:

```sh
FT_TODAY=2026-05-10 ft tasks list today
FT_TODAY=2026-05-10 ft notes today --no-open
FT_TODAY=2026-05-10 ft timeblocks spent today --format json
```

Useful in tests, in scripts that should give the same result
regardless of when they run, and in CI.

## Headless dispatch: `ft do`

Most `ft` operations are reachable from the top-level subcommands
(`ft tasks complete`, `ft notes rename`, `ft timeblocks add`, …). For
the few that are only wired through the TUI's command registry,
`ft do <name>` dispatches them directly:

```sh
# The headless analog of the TUI `x` chord on the Tasks tab
ft do tasks.complete-by-id --arg id=xyz123

# JSON output
ft do tasks.complete-by-id --arg id=xyz123 --format json
```

The set of commands available via `ft do` is the same set you'd see in
the TUI's `?` overlay — `ft commands list` enumerates them:

```sh
ft commands list                           # everything, grouped by scope
ft commands list --opens-modal false       # ft-do-eligible commands only
ft commands list --scope tab/tasks         # tasks-tab commands
ft commands list --format ndjson           # one JSON object per line
```

What `ft do` rejects:

- **Unknown command** — exit `2`. Use `ft commands list` to discover
  the right name.
- **Modal-opening command** — exit `2`. The verb needs the TUI's
  ambient state (cursor position, selection, an active view). Run it
  inside `ft tui` instead.
- **No headless handler yet** — exit `3`. The command is in the
  registry but the dispatch path hasn't been factored out of the TUI
  loop. The set of these shrinks over time as the underlying ops
  become atomic enough to call without TUI state.

The full Command/Keymap model and how to add headless handlers is in
[docs/commands.md](../commands.md).

## Pipeline patterns

A few patterns that show up often:

### Fan out from `ft tasks list`

```sh
# Open each overdue task in $EDITOR one at a time
ft tasks list overdue --format ndjson \
  | jq -r '"\(.source_file):\(.source_line)"' \
  | while read -r loc; do
      $EDITOR "+${loc##*:}" "${loc%:*}"
    done
```

### Drive the day-planner from a script

```sh
# Empty-week template from a script
for day in mon tue wed thu fri; do
  FT_TODAY=$(date -d "next $day" +%F) \
    ft timeblocks add --start 09:00 --end 09:15 --desc "standup" --tag work
done
```

### Bulk move + dry-run gate

```sh
# Show what would move; abort if it touches more than 50 lines
diff=$(ft tasks move --query 'tags includes "legacy" and status in {Open, InProgress}' \
                    --to inbox/triage.md#Triage --dry-run)
lines=$(printf '%s\n' "$diff" | wc -l)
if [ "$lines" -gt 50 ]; then
  echo "too many lines ($lines); review manually" >&2
  exit 1
fi
ft tasks move --query 'tags includes "legacy" and status in {Open, InProgress}' \
              --to inbox/triage.md#Triage --yes
```

### Walk the graph into a script

```sh
# Every note with TODO in the title, into a triage queue
ft graph query 'node where kind = Note and title includes "TODO"' \
              --format ndjson \
  | jq -r '.path' \
  | xargs -I{} ft notes append {} --template triage
```

### Cron-friendly git sync

```sh
*/15 * * * *  FT_VAULT=$HOME/my-vault \
              ft --json-errors git sync >>$HOME/.cache/ft-sync.log 2>&1
```

`--json-errors` keeps cron-flavored logs structured; rotate the log
the usual way.

## Quoting and dates

Almost every date-shaped flag understands the same set of inputs:

- ISO: `2026-05-10`
- Keywords: `today`, `tomorrow`, `yesterday`
- Relative: `+3d`, `-1w`, `+10days`
- Natural language: `next monday`, `in 2 weeks`

The natural-language forms can be ambiguous in scripts (locale,
parser version). Stick to ISO or relative shifts when running
non-interactively and lean on `FT_TODAY` to pin the anchor.

## What stays off the scripting path

- Anything inside `ft tui` that opens a modal. Those depend on
  cursor / selection / picker state and aren't shippable through `ft
  do`.
- Operations that need a TTY for user confirmation. Most of these
  honor `--yes` or `--dry-run` so you can sequence them without a
  prompt; the exceptions error early with a clear "pass `--yes` or
  redirect" message.
- `ft notes update-related` — the modal *is* the feature.

# ft

A command-line interface to your Obsidian vault, focused on the
[Tasks plugin](https://publish.obsidian.md/tasks/) emoji format. Read,
create, complete, and move tasks across thousands of notes without booting
the Electron app.

```
$ ft tasks list overdue --format markdown
- [ ] Pay rent ⏫ 🔁 every month on the 1st 📅 2026-04-01
- [ ] Finish PR review 🔼 📅 2026-05-08

$ ft tasks create "Call dentist" --due tomorrow --priority high
Created task at journal/2026-05-10.md:42
  - [ ] Call dentist ⏫ 📅 2026-05-11

$ ft tasks complete dentist --on today
Completed journal/2026-05-10.md:42
  - [x] Call dentist ⏫ 📅 2026-05-11 ✅ 2026-05-10
```

## Install

```sh
cargo install --path ft
```

This drops a single `ft` binary in `~/.cargo/bin/` (or your configured
target). MSRV is pinned in `rust-toolchain.toml`.

After install, generate shell completions:

```sh
ft completions bash > ~/.local/share/bash-completion/completions/ft
ft completions zsh  > "${fpath[1]}/_ft"
ft completions fish > ~/.config/fish/completions/ft.fish
```

…and man pages:

```sh
ft man --out ~/.local/share/man/man1
```

## Quick start

ft auto-discovers your vault by walking up from the current directory
looking for a `.obsidian/` folder. You can override that with `--vault DIR`,
the `FT_VAULT` env var, or by setting `default_vault` in
`~/.config/ft/config.toml`. Run `ft vault info` to see the resolved path
and the merged configuration.

```sh
# Find every open task across the vault
ft tasks list --status open

# Use a built-in preset
ft tasks list today
ft tasks list overdue
ft tasks list upcoming

# Filter with the query DSL (subset of the plugin's own language)
ft tasks list --query 'priority is high and not done'

# Group / sort
ft tasks list today --group-by priority --sort due

# Add a task to today's daily note
ft tasks create "Send invoice" --due +3d --priority medium --tag work

# Mark something complete (selector: id, file:line, or fuzzy)
ft tasks complete invoice
ft tasks complete journal/2026-05-10.md:7
ft tasks complete xyz123 --on 2026-05-09

# Move tasks (single, or in bulk by query) — preview first with --dry-run
ft tasks move stale-id --to inbox/triage.md
ft tasks move --query 'tag is legacy' --to inbox/triage.md#Triage --dry-run
```

## Interactive TUI

`ft tui` launches a full-screen tabbed terminal UI built on `ratatui`.
Four tabs: **Graph** (DSL-driven link walker), **Tasks** (the daily
triage view from the CLI, live), **Notes** (fuzzy file picker +
section operations), and **Timeblocks** (today/tomorrow day planner).
Global keys: `Tab`/`Shift-Tab` to cycle tabs, `1`–`4` to jump, `?`
opens a per-tab help overlay, `q` or `Ctrl-C` quits.

```sh
ft tui                          # discover vault + open
ft tui --vault path/to/vault
```

Every TUI action is a named `Command`; bindings live in scoped
`KeyMap`s. The `?` overlay, [`docs/keybindings.md`](docs/keybindings.md),
`ft commands list`, and `ft do` all read from the same registry.

```sh
# List every registered command (table / json / ndjson).
ft commands list
ft commands list --scope tab/graph --format ndjson

# Regenerate the markdown keybindings reference (CI checks freshness).
ft commands docs > docs/keybindings.md
ft commands docs --check

# Dispatch an atomic command headlessly (modal-opening commands rejected).
ft do tasks.complete-by-id --arg id=xyz123
```

See [`docs/commands.md`](docs/commands.md) for the full Command/Keymap
model, adding new commands, and the headless-dispatch contract.

## Notes

`ft notes` covers everything that operates on whole notes — open,
create, move sections between them, and the link-graph reads/rewrites.

```sh
# Fuzzy-open the top hit in $EDITOR (or Obsidian via --obsidian)
ft notes open finance
ft notes open meeting#Action items

# Create from a template (or a blank `# <title>` stub)
ft notes create areas/finance.md --template area
ft notes create journal/scratch.md --var topic=oncall

# Jump to today's daily note (creates it from your daily template if missing)
ft notes today
ft notes periodic weekly --offset -1     # last week's note
ft notes periodic monthly --date 2026-01-15

# Move one or more sections between notes (interactive picker by default)
ft notes move-section source.md --to target.md#Archive
```

## Fuzzy find

`ft find` is a scriptable file/heading picker — same fuzzy syntax as
`ft notes open`, but it prints candidates instead of opening anything.

```sh
ft find finance                  # filenames matching "finance"
ft find meeting#Action           # heading matches inside files
ft find '#TODO' --format ndjson  # vault-wide heading hunt
```

## Timeblocks

`ft timeblocks` manages the day-planner block list inside each daily
note — the `- HH:MM - HH:MM <desc> @tag` format Obsidian's Day Planner
plugin and [blockary](https://github.com/cweisser/blockary) both read.

```sh
# Read today's block list
ft timeblocks list

# Add a block (positional blockstring or --start/--end/--desc flag form)
ft timeblocks add "09:00 - 10:00 standup @work"
ft timeblocks add --start 14:00 --end 14:30 --desc "1on1 with Hans" --tag work

# Shift an existing block 15 minutes later via relative end-time shift
ft timeblocks edit standup --end +15m

# Time spent per tag across a date range
ft timeblocks spent this-week --format json
```

The TUI's Timeblocks tab has a today + tomorrow split (or single-day
full-width via `f`), one-key time chords (`]`/`[`/`}`/`{` for ±5-minute
edge shifts, `<`/`>` to shift the whole block), `H`/`L` to slide the
date window, and `T` to jump back to today. See
[docs/timeblocks.md](docs/timeblocks.md) for the block format, tag
grammar, full CLI reference, and TUI keymap.

## Note links

`ft notes backlinks <note>` lists every other note that links *to* the
target; `ft notes links <note>` lists every link going *out* of the
target (including `[[Unresolved]]` ghost targets). `<note>` accepts a
vault-relative path, a bare title, or a fuzzy query (in that order),
matching the ergonomics of `ft notes open`.

```sh
ft notes backlinks finance              # who links to Areas/finance.md?
ft notes backlinks Areas/finance.md     # explicit path also works
ft notes links Journal/2026-05-15.md    # what does today's note link to?
ft notes links hub --format ndjson      # script-friendly output
```

The link graph (`ft_core::graph`) is built from a parallel scan of
every markdown file in the vault, recognising wikilinks (`[[Foo]]`,
`[[Foo|alias]]`, `[[Foo#anchor]]`), markdown links (`[Foo](foo.md)`),
and embeds (`![[Foo]]`, `![alt](image.png)`). Resolution follows
Obsidian's defaults — for collisions, the shortest path wins, with
alphabetical tiebreak. Unresolved targets become "ghost" nodes that
backlinks queries can still find.

The four `--format` values (`table` / `json` / `ndjson` / `markdown`)
are the same as `ft tasks list`. `--allow-empty` is honored — pass it
in scripts that don't want a 1 exit on a no-link query.

`ft notes rename <note> <new-name-or-path>` moves a note and rewrites
every link in the vault to point at the new name. Bare new name keeps
the same directory; a path with `/` is vault-relative. `.md` is
appended automatically. Wikilink display aliases (`[[foo|My Foo]]`)
and heading anchors (`[[foo#H]]`) survive the rewrite verbatim;
markdown links re-render with the URL-encoded path relative to each
linker's directory; embeds keep their `!` prefix.

```sh
ft notes rename foo bar                 # foo.md → bar.md, link rewrites
ft notes rename notes/foo notes/bar     # explicit vault-relative path
ft notes rename foo archive/foo         # move across directories
ft notes rename "[[Phantom]]" Real      # rewrite linkers; no file created
ft notes rename foo bar --dry-run       # print plan, write nothing
```

A freshness guard (`(mtime, len)` per touched file at plan time)
catches the "user edited a file in another tool between plan and
apply" case and aborts before any write. The applier sorts same-file
edits by descending byte offset so multi-link rewrites in one file
are byte-safe; the file rename happens last so a self-linking note
stays correct.

## Graph queries

`ft graph query` runs a small DSL against the in-memory link graph
and prints the walked subtree. The DSL is a *navigation policy* — a
`node` block picks the initial set, and an optional `expand` block
says which edges to follow on each hop — so traversals like "every
orphan note" or "every note's link subgraph" are one-liners.

```sh
# Everything under a folder
ft graph query 'node where path starts_with "Areas/finance/"'

# Notes whose title includes "TODO"
ft graph query 'node where kind = Note and title includes "TODO"'

# Full link subgraph of every note, depth-bounded
ft graph query --depth 2 \
  'node where kind = Note;
   expand where edge.kind in {link, embed};'

# Long query from a file
ft graph query --from-file queries/related.gql --format ndjson
```

Output formats: `tree` (default), `json`, `ndjson`, `edges`,
`markdown`. See [docs/graph-query-dsl.md](docs/graph-query-dsl.md)
for the full grammar, attribute compatibility matrix, and worked
examples.

## Git sync

`ft git sync` commits any working-tree changes in the vault repo,
pulls the configured upstream, and pushes — one shot. The repo is
discovered by walking up from the vault root; the feature is
unavailable if no `.git/` exists anywhere up the tree. The same
operation is available in the TUI via the `g s` chord — it runs on
a background thread so you can keep working while it completes, with
a `⟳ sync` indicator in the status bar.

```sh
ft git sync                     # commit, pull, push
ft git sync -m "msg override"   # override the auto-generated message
ft git sync --dry-run           # print the plan, write nothing
```

Conflicts (merge or rebase) leave markers in the files and exit `2`
with the conflicted-file list on stderr — resolve manually. The
pull strategy (`merge` default, `rebase` opt-in) is configured under
`[git]` in [docs/config.md](docs/config.md).

## Output formats

`ft tasks list --format <fmt>` accepts:

- `table` (default) — terminal-aware, color when stdout is a TTY
- `markdown` — emits the source task lines, pipeable back into another
  vault tool
- `json` — single JSON array of full Task objects
- `ndjson` — one JSON Task per line (script-friendly)

Color is auto-suppressed when `NO_COLOR` is set, when `--no-color` is
passed, or when stdout is not a TTY.

## Scripting

For pipelines, pass `--json-errors` at the top level to get errors as a
JSON object on stderr (`{"error": ..., "chain": [...]}`). Combined with
`--allow-empty` on `tasks list` (so empty results aren't an error), `ft`
fits cleanly into shell loops and `xargs`.

```sh
ft --json-errors tasks list overdue --format ndjson \
  | jq -r '.description' \
  | head -5
```

## Documentation

- [docs/architecture.md](docs/architecture.md) — workspace layout, key
  traits, where to add a new subcommand or task format
- [docs/commands.md](docs/commands.md) — Command/Keymap model, `ft do`,
  `ft commands list`/`docs`, status-bar hint, adding new commands
- [docs/keybindings.md](docs/keybindings.md) — generated reference of
  every registered command, grouped by scope
- [docs/task-format.md](docs/task-format.md) — exactly which Tasks-plugin
  emoji fields are supported, with examples and the deferred list
- [docs/query-dsl.md](docs/query-dsl.md) — supported subset of the
  Tasks-plugin query language with grammar, examples, and an error catalog
- [docs/graph-query-dsl.md](docs/graph-query-dsl.md) — grammar and worked
  examples for `ft graph query` and the TUI Graph tab
- [docs/timeblocks.md](docs/timeblocks.md) — day-planner block format,
  tag grammar, full CLI reference, and TUI keymap
- [docs/config.md](docs/config.md) — full config schema (vault discovery,
  `[daily_notes]`, `[periodic_notes]`, `[git]`, presets)

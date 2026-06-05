# The `ft` user guide

`ft` is a command-line companion to your Obsidian vault. It reads and
writes the same Markdown files Obsidian does, focused on the
[Tasks-plugin](https://publish.obsidian.md/tasks/) emoji format, the
day-planner block format, and the link graph. It does not replace
Obsidian — it sits next to it.

This guide explains the tool in narrative order: what it's for, how to
install and configure it, how to drive it from the CLI and the TUI, and
the philosophy behind the design choices. Reference material (config
schema, query grammars, generated command list) stays in `docs/` and is
linked from each chapter.

## Where to start

| If you want to…                                          | Read                                  |
|----------------------------------------------------------|---------------------------------------|
| Install `ft` and see it find your vault                  | [install.md](install.md)              |
| Set up vault discovery, periodic notes, and presets      | [vault-and-config.md](vault-and-config.md) |
| Triage tasks from the CLI or the TUI                     | [tasks.md](tasks.md)                  |
| Open, create, rename, or link-walk notes                 | [notes.md](notes.md)                  |
| Capture thoughts in one keystroke                        | [capture-and-templates.md](capture-and-templates.md) |
| Plan your day in timeblocks                              | [timeblocks.md](timeblocks.md)        |
| Query and walk the link graph                            | [graph.md](graph.md)                  |
| Live in the TUI                                          | [tui.md](tui.md)                      |
| Keep the vault repo in sync                              | [git-sync.md](git-sync.md)            |
| Wire `ft` into shell pipelines or other tools            | [scripting.md](scripting.md)          |
| Understand the design choices and the relationship to Obsidian | [philosophy.md](philosophy.md)  |

## What `ft` covers

- **Tasks.** List, filter, sort, group, create, complete, and bulk-move
  tasks in the Tasks-plugin emoji format. A small subset of the
  plugin's query language is parsed natively.
- **Notes.** Fuzzy-open, create from templates, jump to today's daily
  note, rename across links, move between folders, append templates,
  and build a paragraph-level journal from `git blame`.
- **Timeblocks.** Read, add, edit, delete, and report on day-planner
  blocks (`- HH:MM - HH:MM <desc> @tag`) in daily notes.
- **Link graph.** Walk wikilinks, markdown links, and embeds; query
  with a small DSL; rewrite all references when a note is renamed or
  moved.
- **Git sync.** Commit, pull, and push the vault repo in one command,
  with the same operation available on a background thread in the TUI.
- **Interactive TUI.** Four tabs (Graph, Tasks, Notes, Timeblocks,
  Journal) tied together by a common command/keymap registry.

## What `ft` does *not* do

- Replace Obsidian. There is no rich-text editor, no preview pane, no
  plugin host, no canvas, no whiteboard.
- Mutate the vault outside the formats it understands. Everything that
  writes goes through atomic temp-file + rename and only touches the
  bytes it owns.
- Maintain its own database. Every command discovers the vault, scans
  it, does its work, and exits.

## Philosophy in one paragraph

`ft` exists for the moments where Obsidian's UI is overkill: a tasks
triage in the terminal during a triage window, a quick capture without
breaking flow, a vault-wide rename, a scripted backlog query, a sync
from a server with no GUI. It treats your vault as the source of
truth — same files, same formats, same conventions — and aims to be
the kind of tool you can run a dozen times an hour without noticing.
The longer version lives in [philosophy.md](philosophy.md).

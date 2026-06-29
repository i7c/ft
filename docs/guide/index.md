# The `ft` user guide

`ft` is a tool to organize, analyze, and manipulate notes. On top of
that core it adds exactly two things — task management and time
management — and nothing else. It is format-compatible with Obsidian
(same wikilinks, same [Tasks-plugin](https://publish.obsidian.md/tasks/)
emoji format, same Day-Planner blocks) so it reads and writes the same
vault, but it is not a replacement for Obsidian: there is no Markdown
editor and no Markdown renderer — bring your own. It can be an
*alternative* if you supply your own editor and renderer, and it is
designed to sit next to Obsidian in the same vault.

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
| Run the review → journal → synth-note ritual             | [synthesis.md](synthesis.md)          |
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
- **Interactive TUI.** Six tabs (Graph, Tasks, Notes, Timeblocks,
  Journal, Review) tied together by a common command/keymap registry.
- **Synthesis ritual.** Review recently-mentioned `[[wikilinks]]`,
  aggregate cross-vault context across a chosen subset, and produce
  "synth notes" whose quoted excerpts are pinned to verifiable git
  commits. See [synthesis.md](synthesis.md).

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

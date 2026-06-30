# ft

`ft` is a tool to **organize, analyze, and manipulate notes**. On top
of that core it adds exactly two things — **task management** and
**time management** — and nothing else. It is format-compatible with
Obsidian (same wikilinks, same Tasks-plugin format, same Day-Planner
blocks) so it reads and writes the same vault, but it is not a
replacement for Obsidian. It runs as a CLI and a TUI, on a laptop, on
a headless server, in cron, or in a shell pipeline.

```
$ ft review --since 7d
(3) [[onboarding]]
(2) [[analytics migration]]
(1) [[activation]]?

$ ft notes journal --link "[[onboarding]]" --link "[[analytics migration]]"
2026-06-12  Daily
──────────────────────────────────────────────────────
matched: onboarding, analytics migration
Talked to Priya — the new onboarding flow doubles as a dogfood path
for the [[analytics migration]] since every signup event now flows
through the new pipeline.

2026-06-09  1:1/marketing
──────────────────────────────────────────────────────
The [[onboarding]] metrics she keeps asking for are really a proxy
for activation, not setup completion.

$ ft synth scaffold Synthesis/onboarding-and-analytics.md \
    --link "[[onboarding]]" --link "[[analytics migration]]"
created Synthesis/onboarding-and-analytics.md with 2 section(s)
```

The scaffolded note is plain text — prose you write, with each
excerpt pinned to the commit it came from:

```markdown
---
ft-synth: true
---

Onboarding and the analytics migration keep colliding.

> [!ft-source] "daily/2026-06-12.md" L3-4 @9c2f1a7 #b1e024
> Talked to Priya — the new onboarding flow doubles as a dogfood path
> for the [[analytics migration]] since every signup event now flows
> through the new pipeline.

… which is really a question about activation, not setup …
```

## What `ft` is

A note-organizing, -analyzing, and -manipulating tool, plus task and
time management. The task and time features are deliberately
**format-compatible with Obsidian** so the same vault round-trips
through both tools:

| Surface        | Compatible with                                              |
|----------------|--------------------------------------------------------------|
| Links          | Obsidian's wikilink format and resolution rules              |
| Tasks          | the [Tasks plugin](https://publish.obsidian.md/tasks/) emoji format |
| Time tracking  | the Day Planner plugin's block format                        |

Where `ft` departs from Obsidian is that **the graph is a tool, not a
visualization.** In Obsidian the graph is mostly something you look at;
in `ft` you actually work on it — the interconnections between notes
(and between paragraphs inside notes) are the substrate the
operations run over. The multi-source journal above is a graph walk
across every paragraph that mentions a concept. Backlinks surface
every reference, including ghosts — links to notes that don't exist
yet. The query DSL walks the graph ad hoc.

## What `ft` is not

- **Not a Markdown editor.** Bring your own — `$EDITOR`, Obsidian,
  whatever. `ft` opens your editor for the writing; it is not a text
  editor.
- **Not a Markdown renderer.** Bring your own for previews, embeds,
  canvas, plugins. `ft` emits and consumes plain Markdown.
- **Not a replacement for Obsidian.** It can be an *alternative* — you
  can migrate to it if you supply your own editor and renderer — but
  not a replacement, because Obsidian does many things `ft` has no
  knowledge of and no interest in duplicating.

## Philosophy: quick add anywhere, connect later

There are many ways to organize notes — folder structures, numbering
schemes, PARA, Zettelkasten — and most assume that the point of
organizing is to *retrieve* notes later, so they impose structure up
front to keep things from sliding into chaos. `ft` is flexible about
which system you layer on top, but it is tuned for one need in
particular: **quick add anywhere, connect later.**

That need comes from a dynamic work style. You have a day you can't
pre-plan. A conversation starts about project A, reveals a
connection to project B, and turns out to touch a longstanding problem
that has no home yet. You can't decide where a note belongs before
the conversation that produces it — and by the time you could, the
thought is gone. `ft` doesn't try to make you file correctly the first
time.

So write anywhere: a daily note, an inbox, a scratch file — it
doesn't matter where. What `ft` gives you is a *process* to gather and
reorganize after the fact. Notes don't need to be right-placed or
right-sized at creation. You connect later, with wikilinks used to
mention concepts (not only to link to notes that exist) and a journal
that regathers every paragraph mentioning a concept — even one whose
note doesn't exist yet. The longer version is in
[docs/guide/philosophy.md](docs/guide/philosophy.md).

## Synthesis: plain text with provenance, not live embeds

Reorganizing notes and synthesizing new ones are two distinct
activities. *Reorganizing* gathers what you wrote anywhere into one
feed. *Synthesizing* compiles a new, focused note on a topic from that
material.

Obsidian and Roam suggest doing the second with **block links and
block embeds** — a dynamic mechanism where editing the source block
updates every note that embeds it. `ft` deliberately does not. The
updatable aspect adds little in practice; people rarely keep editing
a source block once it's been pulled into a composed note. And
critically, live embeds **require Markdown rendering** to be useful,
which makes the resulting note hostile to machines: a composed note
full of embed links has to be resolved before you can read it or hand
it to an AI.

What `ft` keeps from the embed idea is the only part that matters:
**provenance.** Each excerpt is quoted into the synth note as plain
text and pinned to the git commit it came from via an `[!ft-source]`
callout. The note is a self-contained document you can read as-is *and*
pass to a machine as-is — no resolution step, no rendering required,
and a `verify` command that confirms each excerpt still matches its
pinned source. The full ritual is in
[docs/guide/synthesis.md](docs/guide/synthesis.md).

## Install

```sh
cargo install --path ft
```

This drops a single `ft` binary in `~/.cargo/bin/`. MSRV is pinned
in `rust-toolchain.toml`. After installing, generate completions and
man pages — see [install.md](docs/guide/install.md).

## First run

`ft` auto-discovers your vault by walking up from the current
directory looking for `.obsidian/` or `.ft/`. Override with `--vault DIR`,
the `FT_VAULT` env var, or `default_vault` in `~/.config/ft/config.toml`.

```sh
cd ~/my-vault
ft vault              # show the resolved vault + merged config
ft tasks list today
ft tui
```

## User guide

Start here — the user guide walks through install, setup, every
feature, common workflows, and the design philosophy:

**→ [docs/guide/index.md](docs/guide/index.md)**

| Chapter                                                  | Covers                                                     |
|----------------------------------------------------------|------------------------------------------------------------|
| [install.md](docs/guide/install.md)                      | Build from source, completions, man pages, first run.      |
| [vault-and-config.md](docs/guide/vault-and-config.md)    | Vault discovery, the two config layers, periodic notes.    |
| [tasks.md](docs/guide/tasks.md)                          | List / filter / create / complete / move, CLI + TUI.       |
| [notes.md](docs/guide/notes.md)                          | Open, create, periodic, rename, mv, links, journal.        |
| [capture-and-templates.md](docs/guide/capture-and-templates.md) | Append-with-template and quick-capture presets.     |
| [timeblocks.md](docs/guide/timeblocks.md)                | Day-planner blocks and time-spent reports.                  |
| [graph.md](docs/guide/graph.md)                          | The link graph and the graph-query DSL.                    |
| [synthesis.md](docs/guide/synthesis.md)                  | The review → multi-source journal → synth-note ritual.     |
| [tui.md](docs/guide/tui.md)                              | The TUI tour and the command/keymap model.                  |
| [git-sync.md](docs/guide/git-sync.md)                    | One-shot commit + pull + push.                              |
| [scripting.md](docs/guide/scripting.md)                  | Pipelines, exit codes, `--json-errors`, `ft do`.            |
| [philosophy.md](docs/guide/philosophy.md)                | Why the tool is shaped this way.                            |

## Reference docs

Everything below is the underlying reference material — schemas,
grammars, generated tables. The guide chapters link into these for
depth.

- [docs/architecture.md](docs/architecture.md) — workspace layout, key
  traits, where to add a new subcommand or task format
- [docs/commands.md](docs/commands.md) — Command/Keymap model, `ft do`,
  `ft commands list`/`docs`, status-bar hint, adding new commands
- [docs/keybindings.md](docs/keybindings.md) — generated reference of
  every registered command, grouped by scope; TUI bindings are
  user-configurable via `[keymap]` in `config.toml` (see
  [docs/config.md](docs/config.md#keymap))
- [docs/config.md](docs/config.md) — full config schema (vault
  discovery, `[periodic_notes]`, `[editor]`, `[git]`, `[keymap]`, presets)
- [docs/task-format.md](docs/task-format.md) — exactly which
  Tasks-plugin emoji fields are supported, with examples and the
  deferred list
- [docs/graph-query-dsl.md](docs/graph-query-dsl.md) — the unified
  query DSL. Powers `ft graph query` and the TUI Graph tab, and also
  drives `ft tasks list` / the TUI Tasks tab under `Profile::Tasks`
- [docs/migrating-task-queries.md](docs/migrating-task-queries.md) —
  predicate translation table for users coming from the previous
  task DSL
- [docs/timeblocks.md](docs/timeblocks.md) — day-planner block format,
  tag grammar, full CLI reference, and TUI keymap
- [docs/append-and-capture.md](docs/append-and-capture.md) —
  exhaustive reference for append-with-template and quick capture
- [docs/architecture.md#synthesis-ritual-link_review--synth](docs/architecture.md#synthesis-ritual-link_review--synth)
  — the post-connecting ritual (`ft review`, `ft notes journal --link`,
  `ft synth scaffold` / `verify`), the `[!ft-source]` callout grammar
  used in synth notes, and the new `[synth]` config table

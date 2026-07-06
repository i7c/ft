# ft

You're in a conversation about project A. It surfaces a connection to
project B, and turns out to touch a problem that has no home in your
notes yet. There is no time to decide where any of this belongs — and
by the time you could decide, the thought is gone.

Most note systems fight that moment. Folder structures, PARA,
Zettelkasten — they impose structure up front, so that retrieval works
later. The price is paid at the worst possible time: capture. `ft` is
built for the opposite bet: **capture can't wait, and filing can't be
predicted.** Write the thought anywhere — the daily note is fine.
Name what it's about with a `[[wikilink]]` or two. Move on with your
day. Everything else is `ft`'s job, and it happens later, on your
terms.

`ft` is a CLI and TUI over a plain-Markdown vault. It shares its file
formats with Obsidian, so the two tools can work the same vault side
by side — but `ft` runs where a GUI can't: over SSH, in cron, inside a
shell pipeline.

## How it works

**Write anywhere.** Ad-hoc thought goes into today's daily note as a
few lines under a heading. An inbox note or a scratch file work just
as well. No filing decision is made, because none is needed yet.

**Name what it's about.** Drop `[[concept]]` mentions into the
paragraph as you write. The target note doesn't have to exist — `ft`
tracks links to nonexistent notes as *ghosts*, and they count the same.
This is the only organizing act capture asks of you, and it's the cheap
one: not *"where does this go?"* (a global decision — it needs your
whole system in your head) but *"what is this about?"* (a local
decision — you already know, because you're thinking it). If you write
in Neovim, [ft.nvim](https://github.com/i7c/ft.nvim) autocompletes
concept names when you type `[[`; Obsidian does the same out of the
box.

**Resurface on demand.** Because paragraphs are tagged with what
they're about, the unsorted pile stays retrievable. Two directions:

- *Pull* — you need everything on a topic, now, maybe minutes before a
  meeting: `ft notes gather --link "[[topic]]"` gathers every
  paragraph in the vault that mentions it, newest first, dated from
  git history.
- *Sweep* — you want to know what accumulated: `ft notes pulse` ranks the
  concepts most mentioned in a recent window, and `ft notes recent`
  feeds back every paragraph you touched in it.

**Consolidate when a topic earns it.** Some concepts keep coming up.
When one does, `ft notes synth scaffold` compiles the scattered paragraphs
into one focused note, each excerpt pinned to the git commit it came
from. Move sections between notes, rename a ghost into a real note
with every link rewritten — the structure grows out of what you
actually wrote, instead of being guessed in advance.

The point of the four steps: **retrieval never depends on filing.**
You can defer the tidying for a week or a year without penalty,
because the `[[links]]` you dropped at capture keep the pile
queryable the whole time. Consolidation becomes something you do for
the topics that matter, when they matter — not maintenance you owe
the system.

## Ninety seconds of it

```
$ ft notes pulse --since 7d        # sweep: what's been on my mind?
(3) [[onboarding]]
(2) [[analytics migration]]
(1) [[activation]]?

$ ft notes gather --link "[[onboarding]]" --link "[[analytics migration]]"
2026-06-13  2026-06-13
matched: onboarding, analytics migration
──────────────────────
More [[onboarding]] questions in standup — nobody can say what
"done" means for the [[analytics migration]].

2026-06-12  2026-06-12
matched: onboarding, analytics migration
──────────────────────
Talked to Priya — the new [[onboarding]] flow doubles as a dogfood
path for the [[analytics migration]] since every signup event now
flows through the new pipeline.

2026-06-09  marketing
─────────────────────
The [[onboarding]] metrics she keeps asking for are really a proxy
for activation, not setup completion.

$ ft notes synth scaffold Synthesis/onboarding-and-analytics.md \
    --link "[[onboarding]]" --link "[[analytics migration]]"
created Synthesis/onboarding-and-analytics.md with 3 section(s)
```

The `?` in the pulse marks a ghost — a concept mentioned three
times that has no note yet. The `matched:` badge marks co-occurrence:
paragraphs where the two topics collided, which is usually the
interesting part. And the scaffolded note is plain text — prose you
write, with each excerpt pinned to the commit it came from:

```markdown
---
ft-synth: true
ft-synth-targets: ["[[onboarding]]", "[[analytics migration]]"]
---

Onboarding and the analytics migration keep colliding.

> [!ft-source] "daily/2026-06-12.md" L3-5 @ef2a468 #7d302f
> Talked to Priya — the new [[onboarding]] flow doubles as a dogfood
> path for the [[analytics migration]] since every signup event now
> flows through the new pipeline.

… which is really a question about activation, not setup …
```

## What `ft` is (and is not)

A note-organizing, -analyzing, and -manipulating tool, deliberately
**format-compatible with Obsidian** so the same vault round-trips
through both:

| Surface        | Compatible with                                              |
|----------------|--------------------------------------------------------------|
| Links          | Obsidian's wikilink format and resolution rules              |
| Tasks          | the [Tasks plugin](https://publish.obsidian.md/tasks/) emoji format |
| Time tracking  | the Day Planner plugin's block format                        |

Where `ft` departs from Obsidian is that **the graph is a tool, not a
visualization.** In Obsidian the graph is mostly something you look
at; in `ft` you work on it — the interconnections between notes (and
between paragraphs inside notes) are the substrate the operations run
over. The gather feed above is a graph walk across every paragraph that
mentions a concept; backlinks surface every reference including
ghosts; the query DSL walks the graph ad hoc.

What `ft` is **not**:

- **Not a Markdown editor.** Bring your own — `$EDITOR`, Obsidian,
  whatever. `ft` opens your editor for the writing.
- **Not a Markdown renderer.** Bring your own for previews, embeds,
  canvas, plugins. `ft` emits and consumes plain Markdown.
- **Not a replacement for Obsidian.** It can be an *alternative* — you
  can migrate to it if you supply your own editor and renderer — but
  the intended relationship is side by side: the vault is the
  contract, and both tools honour it.

The longer version of the philosophy — including why concept links
beat full-text search for this job, and what keeps concept names from
drifting apart — is in
[docs/guide/philosophy.md](docs/guide/philosophy.md).

## Also on board: tasks and time

On top of the note-flow core, `ft` adds exactly two things — **task
management** (list, filter, create, complete, and move tasks in the
Tasks-plugin emoji format) and **time management** (Day-Planner
timeblocks and time-spent reports) — and nothing else. They live in
the same vault, the same daily notes, and the same TUI, so triaging
tasks and capturing thoughts are one workflow, not two tools.

Because they're adjacent features, their TUI tabs are off by default —
the CLI (`ft tasks`, `ft timeblocks`) always works. Two config lines
bring the tabs back:

```toml
[tui]
tasks_tab = true
timeblocks_tab = true
```

## Synthesis: plain text with provenance, not live embeds

Obsidian and Roam suggest composing new notes from old material with
**block links and block embeds** — live transclusion, where editing
the source updates every note that embeds it. `ft` deliberately does
not: embeds require a renderer to be readable, which makes the
composed note hostile to machines and to plain-text tooling. What
`ft` keeps from the embed idea is the part that matters —
**provenance**. Each excerpt is quoted as plain text and pinned to a
git commit via an `[!ft-source]` callout; `ft notes synth verify` confirms
every excerpt still matches its pinned source. The note reads as-is,
greps as-is, and can be handed to a machine as-is. The full argument
and the callout grammar are in
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

One thing to know up front: the resurfacing commands (`gather`,
`recent`, `pulse`) date paragraphs from `git blame`, so the vault
should be a git repository and committed to regularly — `ft git sync`
does commit + pull + push in one shot. Commit cadence is the temporal
resolution of your history.

## User guide

Start here — the user guide walks through install, setup, every
feature, common workflows, and the design philosophy:

**→ [docs/guide/index.md](docs/guide/index.md)**

| Chapter                                                  | Covers                                                     |
|----------------------------------------------------------|------------------------------------------------------------|
| [install.md](docs/guide/install.md)                      | Build from source, completions, man pages, first run.      |
| [vault-and-config.md](docs/guide/vault-and-config.md)    | Vault discovery, the two config layers, periodic notes.    |
| [tasks.md](docs/guide/tasks.md)                          | List / filter / create / complete / move, CLI + TUI.       |
| [notes.md](docs/guide/notes.md)                          | Open, create, periodic, rename, mv, links, gather, recent. |
| [capture-and-templates.md](docs/guide/capture-and-templates.md) | Append-with-template and quick-capture presets.     |
| [timeblocks.md](docs/guide/timeblocks.md)                | Day-planner blocks and time-spent reports.                  |
| [graph.md](docs/guide/graph.md)                          | The link graph and the graph-query DSL.                    |
| [synthesis.md](docs/guide/synthesis.md)                  | Pulse → multi-source gather → synth notes.                  |
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
- [docs/architecture.md](docs/architecture.md) (synthesis section) —
  internals of the consolidation flow (`ft notes pulse`, `ft notes
  gather --link`, `ft notes recent`, `ft notes synth scaffold` / `verify`),
  the `[!ft-source]` callout grammar used in synth notes, and the
  `[synth]` config table

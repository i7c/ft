# ft

A command-line companion to your Obsidian vault. `ft` reads and writes
the same Markdown files Obsidian does — the
[Tasks plugin](https://publish.obsidian.md/tasks/) emoji format, the
day-planner block format, the link graph — and is built to sit next
to Obsidian, not replace it. It runs on a laptop, on a headless
server, in cron, or in a shell pipeline.

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

$ ft review --since 7d
(3) [[Eigen-decomposition]]
(2) [[Memoization]]
(1) [[Curry-Howard]]?

$ ft synth verify --all
Synthesis/eigen-and-memo.md
  ok             | Synthesis/eigen-and-memo.md:5 → notes/spectral.md L42-44 @abc1234
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

## Install

```sh
cargo install --path ft
```

This drops a single `ft` binary in `~/.cargo/bin/`. MSRV is pinned
in `rust-toolchain.toml`. After installing, generate completions and
man pages — see [install.md](docs/guide/install.md).

## First run

`ft` auto-discovers your vault by walking up from the current
directory looking for `.obsidian/`. Override with `--vault DIR`, the
`FT_VAULT` env var, or `default_vault` in `~/.config/ft/config.toml`.

```sh
cd ~/my-vault
ft vault              # show the resolved vault + merged config
ft tasks list today
ft tui
```

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

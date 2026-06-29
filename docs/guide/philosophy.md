# Philosophy

`ft` exists because Obsidian is excellent at being Obsidian, and bad
at being a CLI. This chapter is the long-form answer to *why the tool
is shaped the way it is* — an alternative that is built to sit next to
Obsidian, not a replacement for it.

## A companion, not a replacement

Obsidian's vault is a folder of Markdown files. The plugin ecosystem
adds conventions on top: the Tasks plugin's emoji format, the Day
Planner plugin's blockstring, periodic notes plugins' file layouts,
the wikilink graph. `ft` treats those conventions as the contract —
not Obsidian's internal data model, not its plugin API, not its rich
text rendering.

This has practical consequences:

- **No Obsidian process required.** Run `ft` on a headless server,
  inside a cron job, from a remote SSH session. The vault works the
  same way it does on your laptop.
- **The two tools share the same files.** Open the vault in Obsidian
  and `ft` at the same time. Changes from either land on disk; the
  other picks them up on the next read.
- **No proprietary state.** `ft` doesn't maintain a separate database,
  index, or sync log. There's a small `git blame` cache so
  `ft notes journal` doesn't re-shell-out, but it's derivative — you
  can delete it and the only cost is the next journal taking longer.
- **Compatible with the Tasks plugin's canonical output.** A task
  rewritten by `ft` is byte-equivalent to what the plugin produces
  for the same fields, so the plugin keeps working over `ft`'s
  writes.

Because `ft` brings no editor and no renderer, it is not a *drop-in*
replacement for Obsidian: there are many things Obsidian does (canvas,
plugins, rich preview, collaboration) that `ft` has no knowledge of.
It can be an *alternative* — you can migrate to it if you supply your
own editor and renderer — but the practical, intended relationship is
side-by-side: the vault is the contract, and both tools honour it.

## Quick add anywhere, connect later

There are many ways to organize notes — folder structures, numbering
schemes, PARA, Zettelkasten — and most assume that the point of
organizing is to *retrieve* notes later, so they impose structure up
front to keep things from sliding into chaos. `ft` is flexible about
which system you layer on top, but it is tuned for one need in
particular: **quick add anywhere, connect later.**

That need comes from a dynamic work style. You have a day you can't
pre-plan. A conversation starts about project A, reveals a connection
to project B, and turns out to touch a longstanding problem that has
no home yet. You can't decide where a note belongs before the
conversation that produces it — and by the time you could, the thought
is gone. The systems that make you choose the right folder first are
fighting that reality.

So `ft` doesn't try to make you file correctly the first time. Write
anywhere — a daily note, an inbox, a scratch file — it doesn't matter
where. What `ft` provides is a *process* to gather and reorganize
after the fact:

- **Wikilinks mention concepts, not only notes that exist.** Write
  about a topic and drop `[[that topic]]` into the paragraph. The link
  target doesn't need to exist; `ft` tracks it as a ghost. Backlinks
  and the journal surface every reference.
- **The multi-source journal regathers context.** `ft notes journal
  --link "[[Foo]]" --link "[[Bar]]"` walks the graph and pulls every
  paragraph mentioning either concept into one reverse-chronological
  feed, with a `matched:` badge on paragraphs that touch both. So a
  note's "right place" is found *after* the fact, not imposed before.
- **Synthesis compiles new focused notes from that feed**, with each
  excerpt pinned to the commit it came from. See
  [synthesis.md](synthesis.md).

Notes don't need to be right-placed or right-sized at creation. You
connect later.

## The CLI / TUI split

Many CLI tools optionally ship a TUI; many TUIs optionally drop you
into a shell. `ft` does both, but they're not bolt-ons:

- **The CLI is the headless surface.** Every command is one-shot,
  scriptable, has a stable exit code, and produces parseable output.
  It's the right tool for cron jobs, shell pipelines, and "I know
  exactly what I want to do" actions.
- **The TUI is the live surface.** It's a tabbed, modal interface
  built on a single command/keymap registry. It's the right tool for
  "I want to see what's happening, then decide."

They share the `ft-core` crate underneath: parsers, planners,
mutation primitives. So an action you take in the TUI is the same on
disk as the equivalent CLI command — there's no "TUI variant" of a
task completion or a section move. This is why the registry is the
source of truth: it's what makes the TUI's bindings legible from the
CLI (`ft commands list`), explorable from the docs
([docs/keybindings.md](../keybindings.md)), and — where the
underlying op is atomic enough — invokable headlessly (`ft do`).

## Atomic writes, planning before applying

Every mutation that touches a file goes through one of two patterns:

1. **Plan then apply.** A pure planner (`task::ops::plan_move`,
   `graph::rename::plan_rename`, …) produces a struct of per-file
   edits without writing anything; a separate `apply_*` step writes
   each file via `ft_core::fs::write_atomic` (same-directory
   tempfile + rename, preserving file mode). The atomic write means
   the file is either fully old or fully new on disk — no half-written
   state, even if `ft` is killed mid-write.
2. **Same-file edits in descending byte order.** When multiple edits
   land in one file, they apply from the end backward so earlier
   rewrites don't invalidate the offsets of later ones.

The "freshness guard" on multi-file rewrites (`ft notes rename`)
records `(mtime, len)` per touched file at plan time and aborts
before any write if the file changed between plan and apply. So if
you're editing a note in Obsidian while `ft notes rename` is computing
a rewrite plan, the rename will fail rather than overwrite your
in-flight changes.

This is the foundation that lets `ft` write into a vault that's open
in another tool without worrying about clobber-corruption.

## One way to spell each thing

Configuration:

- One vault per invocation. No multi-vault commands. `--vault` /
  `$FT_VAULT` / walk-up / `default_vault` — first hit wins.
- Two config layers: user and vault. Vault wins per key. Unknown keys
  rejected so typos surface immediately.
- One "today" seam: `FT_TODAY=YYYY-MM-DD` overrides everywhere.

Commands:

- One command name per action, of the form `<context>.<verb>`. Bound
  to chords in keymaps; introspectable via `ft commands list`;
  reachable headlessly via `ft do` when the verb is selector-driven.
- One source of truth for what the TUI can do — the command registry.
  `?` overlay, the markdown reference, the CLI list command, the
  headless dispatch all read from it.

Output:

- One set of format names (`table`, `markdown`, `json`, `ndjson`)
  across every command that lists things.
- One way to mute colour (`--no-color`, `NO_COLOR`, or "not a TTY").
- One way to make empty results non-fatal (`--allow-empty`).

Errors:

- `thiserror` enums in the library, `anyhow::Context` in the binary.
  Library errors carry typed data (a path, a line number, a count);
  the binary turns them into a vault-relative human-friendly message,
  or — under `--json-errors` — a structured JSON object on stderr.

## What it deliberately doesn't do

- **No "ft database."** `ft` scans the vault on every invocation. The
  scan is parallel and fast. A long-running index would mean a stale
  state any time Obsidian wrote something `ft` didn't watch for, and
  the design rejects that.
- **No backwards-compatibility shims.** When something is removed, it
  goes away — no rename-keeps-the-old-name, no
  `// kept for legacy reasons` comments, no dead `_var` placeholders.
- **No plugin host.** Adding a feature means adding code, not loading
  a plugin. The trait seams (`TaskFormat`, the modal driver) keep the
  cost of that bounded.
- **No silent failures.** A missing daily-notes config errors with a
  hint pointing at the right key. An unknown query token names itself
  and points at the grammar. A duplicate task refuses to insert
  without `--force`.
- **No locking the vault.** Atomic writes and the freshness guard
  cover the concurrency story without needing a lock.

## Defaults that match how people actually work

A few choices that fall out of "use the same vault as Obsidian":

- The Tasks plugin's emoji format is the v1 wire format. Dataview
  format is structured as a plug-in point but not yet shipped — most
  Obsidian users in this corner of the world already use emojis.
- Wikilinks (`[[foo]]`) are first-class. Markdown links work too, but
  the rename / move ops keep them in their original form rather than
  normalising to one shape.
- Daily notes are the default destination for `ft tasks create`. If
  you don't have a daily-notes setup, fall back to
  `default_task_location` — both are easy to configure, hard to
  miss.
- The TUI's editor handoff defaults to tmux popups when inside tmux,
  and silently falls back to inline-suspend outside tmux. Same
  config, different terminal context.

## When to reach for `ft`

- The first dozen times every day you'd otherwise be opening Obsidian
  for one or two operations. Triage, capture, today's note, "did I
  finish that recurring task," "what links to this thing."
- Anywhere the GUI's startup time bites — over SSH, in cron, inside a
  pipeline, in a quick check before lunch.
- Scripted operations across the whole vault: bulk renames, queries
  that filter on multiple fields, reports.

## When to reach for Obsidian

- Anywhere you want a rendered preview, embedded images, plugins
  (Dataview queries, canvas, embeds), or just the visual layout.
- For the writing itself — `ft` opens `$EDITOR` for that; it's not
  trying to be a text editor.
- For collaboration features (Obsidian Sync, Publish) and anything
  outside the file format.

`ft` and Obsidian were never meant to be either/or. The vault is the
contract, and both tools are built to honour it.

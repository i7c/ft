# Philosophy

`ft` is built around one bet about note-taking: **capture can't wait,
and filing can't be predicted.** Everything else in the tool — the
graph, the journal, the synthesis flow, the CLI/TUI split — follows
from taking that bet seriously. This chapter is the long-form answer
to *why the tool is shaped the way it is*.

## The problem

There are many ways to organize notes — folder structures, numbering
schemes, PARA, Zettelkasten — and most assume that the point of
organizing is to *retrieve* notes later, so they impose structure up
front to keep things from sliding into chaos. At the other extreme
sits the unstructured pile: capture everything into one heap and trust
search to find it again.

Both extremes fail, and they fail at different moments. Enforce the
system at capture and you pay at the worst possible time: you have a
day you can't pre-plan, a conversation starts about project A, reveals
a connection to project B, and turns out to touch a longstanding
problem that has no home yet. You can't decide where that note belongs
before the conversation that produces it — and by the time you could,
the thought is gone. Skip the system entirely and you pay later:
retrieval degrades into keyword guessing, and the connections between
thoughts — the actual value — were never recorded anywhere.

`ft` is flexible about which system you layer on top, but it is tuned
for one resolution of that tension: **quick add anywhere, connect
later.** Write anywhere — a daily note, an inbox, a scratch file — it
doesn't matter where. `ft`'s job is to make sure "later" actually
works.

## The one organizing act that survives capture

"Add anywhere" does not mean capture is organization-free. As you
write, you drop `[[concept]]` mentions into the paragraph — and that
is organization, just a different kind. The filing systems ask a
**global** question at capture: *where does this go?* Answering it
means holding your whole taxonomy in your head, under time pressure,
for a thought that may not fit anywhere yet. The wikilink asks a
**local** question: *what is this about?* You already know the
answer — you're thinking it. It costs a couple of seconds and no
context.

Two properties make this cheap act load-bearing:

- **The target doesn't need to exist.** Write `[[activation]]` even
  though there is no `activation.md`. `ft` tracks it as a *ghost*, and
  ghosts participate in everything — backlinks, the journal, the
  review ranking. A concept accumulates weight before anyone decides
  it deserves a page.
- **Completion keeps it frictionless.** If you write in Neovim,
  [ft.nvim](https://github.com/i7c/ft.nvim) pops up note titles from
  your vault when you type `[[` (and follows links with `gf`, and
  renders `![[embeds]]` inline). If you write in Obsidian, its built-in
  completion does the same. Naming a concept should never require
  remembering its exact spelling.

So the honest description of `ft`'s capture contract: the *where*
decision is deferred — possibly forever — and the *what* decision is
kept, because it's the one you can afford in the moment.

## Retrieval without filing

Here is the payoff of that contract. Because every paragraph carries
its concept mentions, the unsorted pile never becomes an unqueryable
pile:

- `ft notes journal --link "[[Foo]]"` regathers every paragraph in the
  vault that mentions the concept, reverse-chronological, dated from
  git history. Add more `--link` flags and co-occurring paragraphs get
  a `matched:` badge — the places where two topics collided.
- `ft notes related` scores which concepts appear together with a
  note, straight from the graph.

This severs the dependency that makes filing systems feel mandatory:
**retrieval no longer requires the notes to have been organized.** A
note's "right place" can be discovered *after* the fact — or never,
if the topic stays minor.

That changes what consolidation *is*. Compiling a focused note from
the scattered material (`ft synth scaffold`, see
[synthesis.md](synthesis.md)) stops being maintenance you owe the
system and becomes compression you apply selectively — to the topics
that keep coming up, when they keep coming up. Moving sections
between notes (`ft notes move-section`) and renaming a ghost into a
real note (`ft notes rename`, which rewrites every link in the vault)
work the same way: structure grows out of the observed weight of what
you wrote, rather than being guessed in advance.

## Two triggers: pull and sweep

The resurfacing tools divide by what triggers them, and it's worth
being precise about this, because they feel different in use:

- **Pull** — topic-shaped, genuinely on demand. You need everything
  about `[[onboarding]]` *now*, perhaps minutes before a meeting:
  `ft notes journal --link` and `ft notes related` answer from a
  standing start.
- **Sweep** — time-shaped. You want to know what accumulated:
  `ft review --since 7d` ranks the concepts most mentioned in the
  window; `ft notes history` feeds back every paragraph edited in it.

A sweep is periodic by nature — a window-shaped query implies you'll
run it again. `ft` doesn't pretend otherwise, and it doesn't abolish
the sit-down-and-connect session. What it changes is the cost of
skipping one: because the links were captured with the thoughts, a
sweep after three weeks costs the same as a sweep after one. Nothing
rots. The session is **deferrable without penalty**, which is exactly
what a calendar-enforced weekly review is not.

## Why links, and not just search?

The obvious objection: full-text search — or an embeddings index, or
an LLM over your vault — retrieves from an unorganized pile too. Why
maintain wikilinks at all?

Three reasons, in increasing order of importance:

- **Aboutness beats string matching.** A wikilink records what a
  paragraph is *about* at the moment of thought — including when the
  concept's name never appears in the text. "The metrics she keeps
  asking for are really a proxy for activation" mentions
  `[[onboarding]]` because you knew it was an onboarding thought when
  you wrote it. No search over the words recovers that; similarity
  search recovers it probabilistically, sometimes.
- **The operations need identity, not similarity.** Co-occurrence
  scoring, the `matched:` badge, review's ranking, rename's vault-wide
  rewrite, synthesis's excerpt gathering — all of these count and
  transform *references to the same thing*. "Probably about the same
  thing" isn't a unit you can count, rank, or rewrite.
- **Determinism, locally, in plain text.** The link is right there in
  the file: greppable, diffable, versioned with the note, resolvable
  by any tool that honours the format — Obsidian included — with no
  index to rebuild, no model to query, and the same answer every time.

Search still exists and is still useful (`ft find`, or plain grep —
it's all Markdown). Links don't replace it; they record the one thing
it can't see.

## Keeping names honest

The recurring cost of a link-based system is not filing — it's
vocabulary. Left alone, `[[onboarding]]`, `[[onboarding-flow]]`, and
`[[new user onboarding]]` silently split one concept across three
names, and every tool that counts references undercounts by three.
`ft`'s defenses, in the order they apply:

1. **Completion at capture** (ft.nvim, Obsidian) makes the existing
   spelling the path of least resistance — most drift never happens.
2. **Aliases when multiple names are legitimate.** Links listed under
   a note's `## Related` heading act as aliases: the journal for that
   note also gathers paragraphs mentioning them. Two names can
   coexist without splitting the concept's history.
   `ft notes update-related` maintains the section interactively,
   suggesting candidates scored by graph co-occurrence.
3. **Merge when one name should win.** `ft notes rename` renames a
   note *or a ghost* and rewrites every reference in the vault — a
   drifted sibling is one command away from being folded in.

## Where notes actually live

A concrete picture of the shape this produces. The capture surface is
mostly the **daily note**: ad-hoc thought lands there as short
sections, a few paragraphs each, named in the moment. The retrieval
unit is the **paragraph** — the journal, history, and review all
operate at paragraph granularity, which is what makes a daily note of
unrelated thoughts usable: each paragraph carries its own concepts and
resurfaces independently. This is finer-grained than note-level
backlinks and search, and it's what synth notes excerpt from.

The memory behind it is **git**. Paragraph dates come from `git
blame`; the review window is a commit range; synth excerpts pin to
commits. Two consequences worth knowing before you start:

- The vault should be a git repository, and committed to regularly.
  `ft git sync` (commit + pull + push in one shot, also available as a
  background operation in the TUI) makes the habit cheap.
- Commit cadence is the temporal resolution of your history. Commit
  daily and "what was I thinking about last week" has daily
  resolution; commit monthly and it doesn't.

There is no separate database behind any of this — see "What it
deliberately doesn't do" below.

## A companion, not a replacement

Obsidian's vault is a folder of Markdown files, and the plugin
ecosystem adds conventions on top: the Tasks plugin's emoji format,
the Day Planner blockstring, periodic-notes file layouts, the wikilink
graph. `ft` treats those conventions as the contract — not Obsidian's
internal data model, not its plugin API, not its rendering.

Practical consequences:

- **No Obsidian process required.** Run `ft` on a headless server,
  inside a cron job, from a remote SSH session.
- **The two tools share the same files.** Open the vault in Obsidian
  and `ft` at the same time; changes from either land on disk and the
  other picks them up on the next read.
- **No proprietary state.** No separate database, index, or sync log.
  There's a small `git blame` cache so `ft notes journal` doesn't
  re-shell-out, but it's derivative — delete it and the only cost is
  the next journal taking longer.
- **Byte-compatible task writes.** A task rewritten by `ft` is
  byte-equivalent to what the Tasks plugin produces for the same
  fields, so the plugin keeps working over `ft`'s writes.

Because `ft` brings no editor and no renderer, it is not a *drop-in*
replacement for Obsidian. It can be an *alternative* — you can migrate
to it if you supply your own editor and renderer — but the practical,
intended relationship is side-by-side: the vault is the contract, and
both tools honour it.

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

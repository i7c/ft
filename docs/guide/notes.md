# Notes

`ft notes` is the umbrella for everything that operates on whole notes:
open them, create them, jump to periodic notes, move and rename them,
walk their links, and gather a paragraph-level feed of how a note
has been referenced over time. Quick capture and append-template live
in their own chapter — see
[capture-and-templates.md](capture-and-templates.md).

## Fuzzy-opening a note

```sh
# Top hit is opened in $EDITOR
ft notes open finance

# Open at a specific heading
ft notes open meeting#Action items

# Hand off to Obsidian instead of $EDITOR
ft notes open finance --obsidian
```

The query syntax matches `ft find`: plain text fuzzy-matches
filenames; `text#heading` also matches headings inside each candidate
file; a bare `#heading` searches headings across the whole vault.

`ft find` is the same matcher but prints candidates instead of opening
anything — useful for scripts and pipes:

```sh
ft find finance                   # filenames
ft find meeting#Action            # path + heading
ft find '#TODO' --format ndjson   # script-friendly
```

By default `ft notes open` picks the top hit; for ambiguous queries
that's not always right. If you want a picker, use the TUI's Notes tab
(`o`) — see [tui.md](tui.md).

## Creating notes

`ft notes create` makes a new file. With nothing else, it writes a
blank `# <title>` body where `<title>` is the filename stem:

```sh
ft notes create areas/finance.md
ft notes create areas/finance               # `.md` is appended automatically
```

With `--template`, `ft` renders a MiniJinja template from your
configured templates folder (`[notes].templates_dir`, default
`templates-ft/`):

```sh
ft notes create areas/finance.md --template area
ft notes create journal/scratch.md --var topic=oncall --var owner=me
```

The template sees `title`, `today`, `now`, and any `vars.KEY` you pass
on the CLI. Available MiniJinja filters and the full variable surface
are documented in
[docs/append-and-capture.md](../append-and-capture.md#template-variables).

After writing, `ft` opens the new note in `$EDITOR` at line 1. Pass
`--no-open` to suppress that, or `--obsidian` to print (and on macOS,
hand off) the `obsidian://open` URL instead.

`--force` overwrites an existing file. Without it, a collision exits
`2` without touching the file — `ft notes create` is conservative by
default.

## Periodic notes

```sh
ft notes today                       # alias for `ft notes periodic daily`
ft notes periodic daily
ft notes periodic weekly --offset -1     # last week's note
ft notes periodic monthly --date 2026-01-15
ft notes periodic quarterly
ft notes periodic yearly
```

Each periodic command resolves a path from your
`[periodic_notes.<period>]` config block, creates the file from the
configured template if missing, and opens it in `$EDITOR`. The created
flag (`Created journal/2026-05-14.md` vs `Opened journal/…`) tells you
whether the file was new.

`--date YYYY-MM-DD` overrides today; `--offset N` shifts by N period
units relative to that base, so `--offset -1` on `weekly` is "last
week" regardless of the current weekday. Out-of-month overflow on
`monthly` clamps to the last day of the destination month.

The TUI's `p` leader (then `d`/`w`/`m`/`q`/`y`) does the same thing
without leaving the alt-screen.

## Renaming a note

`ft notes rename` moves a note **and** rewrites every link in the
vault to point at the new name. Wikilinks (`[[foo]]`,
`[[foo|alias]]`, `[[foo#anchor]]`), markdown links (`[foo](foo.md)`),
and embeds (`![[foo]]`, `![alt](image.png)`) are all updated; display
aliases and anchors survive verbatim.

```sh
# Rename without moving
ft notes rename foo bar

# Same folder, explicit path
ft notes rename notes/foo notes/bar

# Move across directories
ft notes rename foo archive/foo

# Rewrite linkers without creating the file (rename a phantom)
ft notes rename "[[Phantom]]" Real

# Always start with a dry-run on anything non-trivial
ft notes rename foo bar --dry-run
```

The renamer plans every rewrite first, then applies them atomically.
A freshness check records `(mtime, len)` for each affected file at
plan time and aborts if anything changed between plan and apply — so
"someone (or something) just edited a file in another tool" can't
cause partial writes.

## Moving notes between folders

`ft notes mv` moves one or more notes (or whole directories) into a
target directory, updating every reference vault-wide:

```sh
# Single note → target directory
ft notes mv foo.md archive/

# Multiple notes
ft notes mv a.md b.md target/

# Whole directory
ft notes mv projects/old/ archive/

# Mix
ft notes mv alpha.md projects/beta/ target/
```

The target must be an existing directory (`mv` semantics, not
`rename`). All sources are vault-relative paths. After the move, the
old source directory is removed if empty.

For a single-file rename in place, use `ft notes rename`; for a single
file moved to a directory, either works.

## Moving sections between notes

`ft notes move-section` extracts one or more sections (by heading text
or regex) from a source note and drops them into a target — preserving
heading levels by default, or shifting them to a target ATX level via
`--at-level N`:

```sh
ft notes move-section \
    --from areas/finance.md \
    --to archive/finance-old.md \
    --heading "Q1 receipts"

# Multiple headings, by regex, with a fuzzy source resolver
ft notes move-section \
    --from-query finance \
    --to archive/finance-old.md \
    --heading-regex '^Q[1-4] ' \
    --match-policy all \
    --after "Archive"
```

Always run with `--dry-run` or read the diff `ft` prints by default
before confirming. The same flow exists in the TUI (Notes-tab `m`).

## Backlinks and outbound links

```sh
ft notes backlinks finance                 # who links to Areas/finance.md?
ft notes backlinks Areas/finance.md        # explicit path also works
ft notes links Journal/2026-05-15.md       # what does today's note link to?
ft notes backlinks hub --format ndjson     # script-friendly
```

Note resolution is the same as `ft notes open`: exact path → title
lookup (shortest path wins on collision) → fuzzy fallback.

The link graph that powers these queries is built from a parallel
scan of every markdown file. It recognises wikilinks, markdown links,
and embeds, and follows Obsidian's defaults for collision resolution
(shortest path wins; alphabetical tiebreak). Unresolved link targets
become "ghost" nodes that backlinks queries can still find.

Output formats match `ft tasks list`: `table` (default), `json`,
`ndjson`, `markdown`. Empty results exit `1` unless `--allow-empty`.

## Ghosts: concepts that earned a note

A ghost accumulating mentions is the vault telling you a note has
earned its existence. `ft notes ghosts` ranks every ghost by how many
*distinct paragraphs* mention it (the same dedup rule as `ft notes pulse`
— three mentions in one paragraph count once), highest first. Pure
graph: no git history needed.

```sh
ft notes ghosts                        # (N) [[ghost]] rows, ranked
ft notes ghosts --min-mentions 3       # only the heavy hitters
ft notes ghosts --limit 10 --json      # [{target, mentions}, …]
```

Promotion — giving the concept its page — happens where the ghost is:

- **In the TUI graph tab**, ghost rows show their count
  (`G activation (3)`) and the `ghosts` preset lists them ranked.
  On a ghost row: `c` creates the note blank at the ghost's path,
  `Shift+c` creates it from a template, and `Shift+p` **promotes with
  material** — the note is created as a synth note scaffolded with
  every paragraph that mentions the ghost, `ft.synth.targets` set,
  editor open (needs git history for the seeded journal). Because the
  note takes the ghost's exact name, every existing link resolves
  without any rewriting.
- **From the CLI**, the seeded equivalent is
  `ft notes synth scaffold <path>.md --link "[[ghost]]"` — see
  [synthesis.md](synthesis.md).

## Drift: one concept, several spellings

The recurring cost of a link-based vault is vocabulary:
`[[onboarding]]`, `[[onboarding-flow]]`, and `[[new user onboarding]]`
silently split one concept across three names, and everything that
counts references undercounts. `ft notes drift` finds the likely
splits:

```sh
ft notes drift
# [[onboarding]] (31) ↔ [[onboarding-flow]]? (4)
#   merge: ft notes rename "[[onboarding-flow]]" "onboarding"
```

Pairs are gated by name similarity (compound names and typos), then
confirmed by shared co-occurrence neighborhoods — and *penalized* when
the two names appear together in the same paragraph, because you never
write both spellings in one sentence: co-occurring concepts are
related, not drifted. Ranking weighs in the combined mention count, so
the splits that distort your vault most come first. `?` marks ghost
sides; `--limit` and `--json` behave as everywhere else.

The report is read-only — each pair just carries its fix, ready to
paste. If linked attachments pollute the report (`[[fig-v1.png]]` ↔
`[[fig-v2.png]]` is a filename pattern, not concept drift), exclude
them by glob in config — see [docs/config.md](../config.md#drift):

```toml
[drift]
exclude = ["*.png", "*.pdf"]
``` A ghost folds into its sibling with `ft notes rename` (all
links rewritten vault-wide); when both sides are real notes the
suggestion is a `## Related` alias, since two files' *content* can
only be merged by hand.

## The Gather feed

`ft notes gather <note>` is a reverse-chronological feed of
*paragraph-level* mentions of a note across the vault. Dates come from
`git blame`, so the feed only makes sense inside a vault that's a git
repository. The note's own `## Related` section feeds in aliases —
every `[[wikilink]]` inside that section is treated as another name
for the target, so mentions of the alias surface too.

```sh
ft notes gather finance              # human-readable feed
ft notes gather finance --json       # machine-readable
```

A typical use: gather a project note before a status
update, to see every paragraph (across daily notes, area notes,
meeting notes) that touched it. The note itself is excluded from its
own feed.

Entries already woven into a synth note carry a citation badge:
`cited: <note>` when the paragraph is pinned byte-identically in a
`[!ft-source]` callout, `cited*: <note>` when it was edited *after*
being cited (the pin is stale). `--json` carries the same data as a
`cited_in` array of `{note, stale}` objects. Pass `--uncited` to keep
only entries not yet cited — stale entries stay, since they still need
attention — which turns a long feed into "what haven't I dealt with":

```sh
ft notes gather finance --uncited    # only the unsynthesized mentions
```

The TUI's Gather tab is the same feed with a fuzzy picker on top —
press `5` in the TUI and pick a note. It shows the same badges, `u`
toggles the uncited-only filter, and `o` picks a synth note to work
*toward*: its `ft.synth.targets` load as the sources and every entry
badges as `in note` / `missing` relative to that note. See
[tui.md](tui.md) and [synthesis.md](synthesis.md).

## The Recent feed

Where gather is *target-shaped* ("what mentions this note?"),
`ft notes recent` is *time-shaped*: a whole-vault, reverse-chronological
feed of every paragraph edited within a window — "what did I actually
write or change lately, everywhere?" It takes the same `--since` / `--range`
window arguments as gather (defaulting to `7d`) and, like gather,
needs a git-backed vault.

```sh
ft notes recent                      # last 7 days, human-readable
ft notes recent --since 2w --json    # last two weeks, machine-readable
ft notes recent --range v1.0..HEAD   # a commit range
```

Synth notes (`ft.synth.enabled: true`) are excluded by default; pass
`--include-synth` to include them. Periodic/daily notes are included.

Recent carries the same citation badges and `--uncited` filter as
gather (`cited:` / `cited*:` lines, `cited_in` in `--json`), so a
sweep can be incremental: `ft notes recent --since 7d --uncited`
shows only the paragraphs from the window you haven't synthesized yet.

The TUI's Recent tab (press `6`) renders the same feed and adds the
synthesis actions: select one/several/all rows and `s` / `S` them into a
synth note as protected `[!ft-source]` sections, or press `m` to move the
selected row's section into another note (the section-move flow, seeded to
that note). `u` toggles the uncited-only filter. See [tui.md](tui.md).

## Updating the Related section

`ft notes update-related <note>` launches the TUI graph tab on top of
a co-occurrence-scoring modal for the target note. The modal proposes
notes to append to the target's `## Related` section, ranked by how
often they appear near it in other notes; you check the ones you want
and commit.

This is the one CLI command that requires a TTY — there's no headless
form because the modal is the whole point.

## Append a template to an existing note

For appending pre-rendered content into existing notes — log
templates, session entries, meeting boilerplate — see
[capture-and-templates.md](capture-and-templates.md). Both `ft notes
append` and the TUI's quick-capture key (`Q`) are documented there.

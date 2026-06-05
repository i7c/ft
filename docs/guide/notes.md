# Notes

`ft notes` is the umbrella for everything that operates on whole notes:
open them, create them, jump to periodic notes, move and rename them,
walk their links, and build a paragraph-level "journal" of how a note
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

## The Journal

`ft notes journal <note>` is a reverse-chronological feed of
*paragraph-level* mentions of a note across the vault. Dates come from
`git blame`, so the feed only makes sense inside a vault that's a git
repository. The note's own `## Related` section feeds in aliases —
every `[[wikilink]]` inside that section is treated as another name
for the target, so mentions of the alias surface too.

```sh
ft notes journal finance              # human-readable feed
ft notes journal finance --json       # machine-readable
```

A typical use: read the journal for a project note before a status
update, to see every paragraph (across daily notes, area notes,
meeting notes) that touched it. The note itself is excluded from its
own journal.

The TUI's Journal tab is the same feed with a fuzzy picker on top —
press `5` in the TUI and pick a note. See [tui.md](tui.md).

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

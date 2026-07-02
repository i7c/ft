# Synthesis

`ft` has a three-step "post-connecting" ritual designed for quick-capture
note-taking: write freely with `[[wikilinks]]`, file nothing, then once
in a while sit down and connect dots. The ritual:

1. **Review** — see which `[[wikilinks]]` have been on your mind
   recently (`ft review`, or the Review tab).
2. **Aggregate** — pull every paragraph mentioning a chosen subset of
   those links into one journal feed (`ft notes journal --link …`, or
   the Journal tab in multi-target mode).
3. **Synthesize** — turn that feed into a new note (or append to an
   existing one) with verifiable excerpts pinned to the git commits
   they came from.

The whole thing assumes your vault is a git repository — without
history, "what's been on my mind recently" can't be computed.

## Why plain text with provenance, not live embeds

Step 3 — synthesis — compiles a new, focused note from gathered
material. Obsidian and Roam suggest doing that with **block links and
block embeds**: a dynamic mechanism where editing a source block
updates every note that embeds it. `ft` deliberately does not.

The updatable aspect adds little in practice; people rarely keep
editing a source block once it's been pulled into a composed note.
And critically, live embeds **require Markdown rendering** to be
useful, which makes the resulting note hostile to machines: a composed
note full of embed links has to be resolved before you can read it or
hand it to an AI. What `ft` keeps from the embed idea is the only part
that matters — **provenance.** Each excerpt is quoted into the synth
note as plain text and pinned to the git commit it came from via an
`[!ft-source]` callout. The note is a self-contained document you can
read as-is *and* pass to a machine as-is: no resolution step, no
rendering required, and `ft synth verify` confirms each excerpt still
matches its pinned source.

## Step 1: the review window

`ft review` scans every wikilink added between two commits and ranks
them by how many *distinct paragraphs* mention each target.

```sh
ft review --since 7d
# (3) [[Eigen-decomposition]]
# (2) [[Memoization]]
# (1) [[Curry-Howard]]?
```

The `?` suffix marks ghosts — wikilinks whose target note doesn't
exist yet. Those are often the ripest candidates: a concept you keep
referring to but haven't given its own page.

Other window shapes:

```sh
ft review --since 24h         # last day
ft review --since 2w          # last fortnight
ft review --range main~10..HEAD   # explicit commit range
ft review --json              # array of { count, target, is_ghost, source_paths }
```

Paragraph-level dedup: mentioning `[[Foo]]` three times in one
paragraph counts once; in three separate paragraphs, three times.
Wikilinks inside fenced code blocks are skipped. So are wikilinks
quoted inside `[!ft-source]` callouts in a synth note (more on those
below) — recycled material doesn't double-count next ritual.

Add a `[synth].exclude_prefixes` line to your config to filter out
folders that produce noise. The conventional choice is your periodic
notes folder, since daily notes mention the same recurring topics:

```toml
[synth]
exclude_prefixes = ["journal/"]
```

## Step 2: the multi-source journal

Once you know which links you want to revisit, `ft notes journal`
takes a `--link` flag (repeatable) and merges paragraphs across the
vault:

```sh
ft notes journal --link "[[Eigen-decomposition]]" --link "[[Memoization]]"
```

Output is reverse-chronological by `git blame` date. A paragraph that
matched more than one of your selected links shows a
`matched: Eigen-decomposition, Memoization` indicator after the date —
co-occurrence is exactly the signal you're looking for when
synthesizing.

`--json` gives the same data structured for scripts: each entry has
`date`, `source_title`, `source_path`, `section`, and a `matched`
array.

To restrict the feed to paragraphs that were *touched* in the window
(rather than every all-time mention), add `--in-window`:

```sh
ft notes journal --link "[[Foo]]" --since 7d --in-window
```

By default the all-time feed is what you want — synthesis is about
connecting recent thoughts to older ones. The in-window flag is for
when you specifically want "what was written this week."

The TUI Journal tab gains the same `--link`-style multi-target mode
when you hand off from the Review tab (more below).

## Step 3: synthesis

A **synth note** is a regular `.md` file with `ft-synth: true` in its
frontmatter. It contains **protected sections** — quoted source
paragraphs wrapped in an Obsidian-style callout that pins the excerpt
to a specific git commit. Between callouts, you write whatever prose
you want:

```markdown
---
ft-synth: true
---

I keep coming back to this idea that …

> [!ft-source] "notes/spectral.md" L42-44 @abc1234 #7f3a91
> An eigen-decomposition factors a matrix into directions of
> stretching and scaling. Most of linear algebra rides on it.

… which connects to …

> [!ft-source] "daily/2026-05-08.md" L12-13 @def5678 #2a1b9c
> Memoizing the recursive build of the Hessian is what made the
> whole thing tractable.

… and so the connection is that …
```

The header tokens are, in order:

- vault-relative source path
- inclusive line range at the pinned commit (`L42-44`)
- short (7-hex) commit SHA (`@abc1234`)
- short (6-hex) blake3 content-hash prefix (`#7f3a91`)

That last one lets `ft synth verify` confirm the excerpt is still
byte-for-byte what the source file said at the pinned commit, without
needing the git blob. If you edit inside a protected section, verify
will tell you it drifted.

Synth notes live wherever you want them in the vault. They participate
in the link graph normally — backlinks work, the regular `[[wikilinks]]`
you write in the prose between callouts count for next ritual's
review. The link-review just knows to skip the quoted material in
`[!ft-source]` blocks so it doesn't show you yesterday's synthesis
back as today's recent thoughts.

### Creating from the CLI

```sh
# Create a new synth note with every paragraph mentioning [[Foo]] or [[Bar]]
ft synth scaffold Synthesis/eigen-and-memo.md \
    --link "[[Eigen-decomposition]]" \
    --link "[[Memoization]]"
```

The note gets created with the frontmatter marker and every entry
from the multi-source journal as a protected section, newest first.
`$EDITOR` then opens at the bottom of the file so you can write prose
around the excerpts.

If the file already exists, sections are appended:

```sh
ft synth scaffold Synthesis/eigen-and-memo.md --link "[[Curry-Howard]]"
```

For a specific source paragraph rather than a link-driven set, use
`--from <path>:<line>` (repeatable). The `<line>` is the paragraph's
`line_start`:

```sh
ft synth scaffold Notes/connections.md \
    --link "[[Foo]]" \
    --from notes/bar.md:42 \
    --from journal/2026-05-08.md:7
```

Other flags:

- `--since 7d` / `--range X..Y` + `--in-window` — same in-window
  semantics as `ft notes journal`.
- `--no-edit` — write the file but don't launch `$EDITOR`, useful for
  scripting.

### Creating from the TUI

The TUI version is the friendlier flow. Press a digit (or `Tab`) to
reach the **Review** tab — it's the rightmost tab.

```
┌ Review — since 7d (3 links, 2 selected) ──────────────────────────┐
│[*] (3) [[Eigen-decomposition]]                                    │
│[*] (2) [[Memoization]]                                            │
│    (1) [[Curry-Howard]]?                                          │
└───────────────────────────────────────────────────────────────────┘
```

Keymap:

| Key | Action |
|-|-|
| `j` / `k` (or `↓` / `↑`) | move cursor |
| `Space` | toggle multi-select on the current row |
| `[` / `]` | halve / double the window (since-style only) |
| `Enter` | hand off selected (or cursor) links to the Journal tab |
| `R` | reload |

Pressing `Enter` switches to the Journal tab with those targets
queued. The Journal tab then renders the multi-source feed with
`matched: Foo, Bar` badges on co-occurrence entries.

From there, the synth keys take over:

| Key | Action |
|-|-|
| `Space` | toggle multi-select on the current entry |
| `w` | toggle in-window-only filter (when a window came in via handoff) |
| `s` | append to an **existing** note (fuzzy picker) |
| `Shift+s` | create a **new** synth note (folder picker → title prompt) |

If you press `s` and pick a note that doesn't have `ft-synth: true` in
its frontmatter, a small 3-way prompt asks whether to append anyway,
mark and append (insert `ft-synth: true` first), or cancel.

If you press `Shift+s`, the folder picker comes up; `.` is the vault
root. Pick a folder, type a title, hit Enter. The note is created
with the right frontmatter and the scaffolded excerpts, then
`$EDITOR` opens at the bottom so you can compose.

If you have entries selected with `Space`, only those go into the
scaffold. With no selection, the whole displayed feed is sent.

## Editing a protected section

Sometimes the captured excerpt is one line short, or one line too long.
`ft synth reslice` grows or shrinks a section's range **without changing
the commit it's pinned to** — important, because by the time you revisit
a note that commit is usually no longer `HEAD`. The body and content-hash
are recomputed from the source blob at the pinned commit, so the section
keeps verifying `ok`.

```sh
# Add one line of context below the excerpt:
ft synth reslice Synthesis/eigen-and-memo.md --down 1

# Two more lines above, one fewer below:
ft synth reslice Synthesis/eigen-and-memo.md --up 2 --down -1

# Or set the range outright:
ft synth reslice Synthesis/eigen-and-memo.md --lines 40-46
```

`--up`/`--down` adjust the top and bottom edges (negatives shrink);
`--lines A-B` replaces the range. When a note holds more than one section,
pass `--at <line>` with the header line `ft synth verify` prints to pick
which one.

Because the new body always comes from the committed blob, reslicing also
**heals drift**: if you'd hand-edited inside the callout, the canonical
slice overwrites the edit and the command tells you it did so. A
zero-change reslice (`--down 0`) is a quick way to re-pin a drifted
section back to its source.

### From the TUI

In the Notes tab, press `r`:

| Key | Action |
|-|-|
| (picker) | fuzzy-pick the synth note |
| `j` / `k` (`↑`/`↓`) | choose which `[!ft-source]` section |
| `Tab` | switch the active edge (top / bottom) |
| `↑` / `↓` | move the active boundary up / down — the preview re-slices live |
| `Enter` | commit the reslice |
| `Esc` | back a step |

The boundary editor previews the source lines straight from the pinned
commit, so you see exactly what the resliced excerpt will contain before
you commit.

## Verifying

`ft synth verify` checks every protected section in a synth note (or
across the whole vault with `--all`) against its pinned git blob:

```sh
ft synth verify Synthesis/eigen-and-memo.md
# Synthesis/eigen-and-memo.md
#   ok             | Synthesis/eigen-and-memo.md:5 → notes/spectral.md L42-44 @abc1234
#   ok             | Synthesis/eigen-and-memo.md:13 → daily/2026-05-08.md L12-13 @def5678
```

Possible per-section statuses:

- **ok** — body and content hash both match the source at the pinned
  commit.
- **drifted** — body differs from the git blob (someone hand-edited
  inside the callout) or the recomputed hash doesn't match the
  header.
- **source-missing** — pinned commit unreachable in local history, or
  the file at that commit doesn't have the expected line range.
- **malformed** — header didn't parse cleanly (token missing, regex
  rejected).

```sh
ft synth verify --all          # sweep every ft-synth: true note in the vault
ft synth verify --all --json   # script-friendly
```

Exit code is 0 when every section is `ok`, 1 otherwise — wire it into
a pre-commit hook if you want to guard against accidental edits.

## Repairing

History rewrites (an interactive rebase, a squash-merge, an aggressive
`git gc`) can strand a pin: the commit SHA in the header no longer
resolves, and every affected section reports `source-missing` forever
even though the quoted text is perfectly fine. `ft synth repair` closes
that gap — the callout **body** is treated as the source of truth, and
the provenance is rebuilt around it:

```sh
ft synth repair Synthesis/eigen-and-memo.md
# Synthesis/eigen-and-memo.md
#   repinned       | Synthesis/eigen-and-memo.md:5 → notes/spectral.md L42-44 @abc1234 ⇒ L42-44 @9f21c3a #7f3a91
#   ok             | Synthesis/eigen-and-memo.md:13 → daily/2026-05-08.md L12-13 @def5678
#   repaired 1 section(s)
```

Per-section outcomes:

- **ok** — already verifies; untouched.
- **rehashed** — the body still matches the pinned blob and only the
  content hash was wrong (hand-mangled header). Hash recomputed, pin
  kept.
- **repinned** — the body was located in the source file at HEAD
  (exact line match first, then trailing-whitespace-insensitive) and
  the section now pins to HEAD's SHA at the matched range. If the
  paragraph appears more than once, the location nearest the old range
  wins and repair says so.
- **unrecoverable** — the body doesn't appear in the source at HEAD
  (or the file is gone). The section is left untouched; `ft synth
  reslice` (restores the canonical text from a still-valid pin) or
  re-scaffolding are the manual escape hatches.

```sh
ft synth repair --all            # sweep the whole vault
ft synth repair --all --dry-run  # show what would change, write nothing
ft synth repair --all --json     # script-friendly
```

Exit code is 0 when nothing is left broken, 1 when any section is
unrecoverable — so `ft synth repair --all && ft synth verify --all` is
the full recover-and-confirm loop after a history rewrite.

## Config

A single `[synth]` table in `config.toml`:

```toml
[synth]
# Convenience default for bare-filename targets in `ft synth scaffold`
# (the CLI only — the TUI flow always asks where to put new notes).
# Vault-relative. Trailing slash optional.
folder = "Synthesis/"

# Files whose vault-relative path starts with any of these prefixes
# are excluded from `ft review`. Useful for filtering out periodic
# notes that mention the same recurring topics every day.
exclude_prefixes = ["journal/"]
```

Both are optional. See [docs/config.md](../config.md) for the full
schema.

## A worked session

Mine looks roughly like this. After a couple of weeks of capture-only
note-taking:

```sh
# 1. What's been on my mind?
ft tui                  # open the TUI
# Tab to the Review tab. Window defaults to 7 days.
# Browse the list, Space on 2–3 links that catch my eye.
# Enter.

# 2. In the Journal tab now, multi-target mode.
# Skim entries. Space on the ones I actually want to pull together.
# Shift+s to create a new synth note.
# Pick a folder, type a title.
# Editor opens at the bottom of the new file.

# 3. Write prose between the quoted excerpts. Save. Exit editor.

# 4. Later, sanity-check.
ft synth verify --all
```

That's the loop. About thirty minutes, and the synth notes accumulate
into a second layer of structure over the quick-capture base.

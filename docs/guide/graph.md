# Graph

Every link, embed, and directory relationship in your vault is a node
or edge in the link graph that `ft` builds at scan time. The graph
powers several user-facing features:

- `ft notes backlinks` / `ft notes links` — who links to a note, who
  the note links to.
- `ft notes rename` and `ft notes mv` — vault-wide reference
  rewriting.
- `ft notes journal` — paragraph-level "mentions over time."
- `ft graph query` — a small DSL for ad-hoc graph walks.
- The TUI Graph tab — an interactive tree view driven by the same DSL.

This chapter focuses on the query language and the Graph tab. The
formal grammar, attribute table, and error catalog are in
[docs/graph-query-dsl.md](../graph-query-dsl.md).

## What the graph contains

Nodes:

- **Note** — a markdown file in the vault. Path, title (first heading
  or filename stem), kind.
- **Directory** — a folder. Path. Connected to its contents via
  `directory-contains` edges.
- **Ghost** — an unresolved wikilink target. Lets backlink queries
  surface broken references.
- **Task** — an individual task line (referenced from rename and
  related flows).
- **Paragraph** — a paragraph in a note (used by the Journal
  feature).

Edges: `link` (wikilink or markdown link), `embed` (`![[X]]` /
`![alt](x)`), `directory-contains`, and paragraph-level link edges
used by the Journal.

Resolution follows Obsidian's defaults: when two notes share a title,
the **shortest path** wins; alphabetical tiebreak.

## Querying from the CLI

`ft graph query` takes a DSL expression and walks the resulting
subgraph:

```sh
# A single node — every note in the vault
ft graph query 'node where kind = Note'

# Everything in a folder
ft graph query 'node where path starts_with "Areas/finance/"'

# Notes whose title contains TODO
ft graph query 'node where kind = Note and title includes "TODO"'

# Orphans (no incoming edges)
ft graph query 'node where indegree = 0'

# Long queries from a file
ft graph query --from-file queries/related.gql --format ndjson

# Pre-named query (built-in or from config)
ft graph query --preset notes-no-tasks
```

The DSL has two clauses:

- **`node`** — picks the initial set (the roots of the walk).
- **`expand`** — for each parent, which edges to follow on the next
  hop. Optional.

Without an `expand` clause, the walk stops at the initial set — you
get a flat list of nodes. With `expand`, you get a tree.

```sh
# Every note's outbound link subgraph, depth-bounded
ft graph query --depth 2 \
  'node where kind = Note;
   expand where edge.kind in {link, embed};'

# Full directory tree, starting from the vault root
ft graph query \
  'node where indegree = 0;
   expand where from.kind = Directory
            and edge.kind = directory-contains
            and to.kind in {Note, Directory};'
```

Cycle handling: the default is `--cycle-policy stop`, which emits a
cycle marker and halts that branch. `allow` lets the walk re-visit
ancestors but requires `--depth N` so it terminates.

### Output formats

- `tree` (default) — indented walk, terminal-friendly.
- `json` — full subtree as nested JSON.
- `ndjson` — one node per line.
- `edges` — every traversed edge, one per line.
- `markdown` — bulleted list, copy-pastable into a note.

### Errors

DSL errors print a precise message naming the offending token plus a
byte offset. The error catalog (every variant, every cause) is in
[docs/graph-query-dsl.md](../graph-query-dsl.md#errors).

## Presets

Just like the task DSL, the graph DSL supports named presets. They
live under `[graph.presets]` in your config (separate map from
`[presets]` so the two languages don't collide):

```toml
[graph.presets]
orphans      = "node where kind = Note and indegree = 0"
notes-recent = "node where kind = Note and path starts_with \"Journal/\""
```

Then:

```sh
ft graph query --preset orphans
```

User presets shadow built-ins of the same name; unknown preset names
exit `2`.

## The Graph tab in the TUI

The Graph tab is the same DSL plus a tree view, a side bar of views,
and a query bar. It's the most feature-rich tab and is worth
spending time in.

### Navigation

| Key                  | Action                                         |
|----------------------|------------------------------------------------|
| `j` / `k` / `↓` / `↑`| move the cursor                                |
| `g` / `G`            | first / last row                               |
| `Ctrl+d` / `Ctrl+u`  | half-page down / up                            |
| `Enter` / `l`        | expand (or collapse if already expanded)       |
| `h`                  | collapse, or jump to parent if already collapsed |
| `/`                  | open the query bar to edit the active view     |
| `f`                  | open the in-tree fuzzy search picker           |
| `z`                  | re-root the active view on the selected node    |
| `Space`              | toggle multi-selection on the focused row       |
| `Esc`                | clear the multi-selection                       |

### Views

You can have multiple named views open at once — different queries,
each with their own cursor and expansion state.

| Key             | Action                                       |
|-----------------|----------------------------------------------|
| `Ctrl+n`        | add a new view (blank or pick a preset)      |
| `Ctrl+p`        | load a preset into the active view           |
| `Ctrl+w`        | close the active view                        |
| `Ctrl+PgUp/Dn`  | switch to previous / next view               |
| `1`–`9`         | jump to a view by index (within the tab)     |

### Mutations and actions on the selected note

| Key       | Action                                                          |
|-----------|-----------------------------------------------------------------|
| `o`       | open the selected note in `$EDITOR`                              |
| `Ctrl+o`  | open the selected note in Obsidian                               |
| `c`       | create a new blank note in the selected folder                   |
| `C`       | create a new note from a template                                |
| `A`       | append a template to the selected note                           |
| `Q`       | quick capture (run a preset)                                     |
| `m`       | enter the move-section flow with the selected note as source     |
| `r`       | rename the selected note (or move the multi-selection)           |
| `R`       | open the Related-section updater modal                           |
| `J`       | open the Journal tab for the selected note                       |
| `Ctrl+r`  | refresh the graph from disk                                      |

### Periodic notes

| Key       | Action                                                          |
|-----------|-----------------------------------------------------------------|
| `t`       | open today's daily note                                          |
| `p`       | periodic-note leader (then `d` / `w` / `m` / `q` / `y`)          |

### The query bar

`/` opens an inline editor for the active view's DSL. The prompt is
coloured yellow while the bar is active; commit with `Enter`, cancel
with `Esc`. Parse errors render in-place so you can fix the query
without leaving the bar.

`z` is the cheap version: re-root the current view on whatever
node is under the cursor — useful for drilling into a subtree
without writing a new query.

## When to use the graph vs. backlinks

`ft notes backlinks` and `ft notes links` are the answer for "who
links to / from this one specific note" — direct, one shot.

`ft graph query` and the Graph tab are the answer when you want to
walk, filter, or visualise a *region* of the graph — every note in a
folder and their outbound links, every orphan note vault-wide, the
directory tree, the link subgraph of a project hub.

Both read from the same scan, so results stay consistent.

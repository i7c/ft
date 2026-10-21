## Context

**Hierarchy today.** `Vault::scan` parses each file once (in parallel)
into tasks plus `ParsedFile { links, paragraphs, headings, frontmatter }`
(`scan.rs`). Each file's tasks get `Task::parent: Option<usize>`, the
line of the nearest less-indented task above (`resolve_hierarchy`), so
hierarchy is strictly within one file.

**One choke point.** Every consumer derives children from that pointer
through two private helpers in `task/hierarchy.rs`, `parent_idx` and
`children_map`:
- `expand_forest` (CLI `--tree`);
- `expand_forest_visible` (TUI tree);
- `child_index_map`;
- `Graph::insert_subtask_edges` (`graph/mod.rs:1039`), which looks up
  `(source_file, parent_line)` directly.

**Links.** `RawLink { form, target_text, anchor, display, line,
byte_range }` comes out of `graph::parser::extract_links`. Resolution
(`graph/resolve.rs`) is path form, then title form with a shortest-path
tiebreak, but it needs a `Graph` for `note_by_path` / `note_by_title`.

**Block ids and frontmatter.** `Task::block_link` already parses a
trailing `^id` and round-trips it. The frontmatter readers follow the
nested `ft.<area>.<key>` pattern (`ft_tasks_section_in`).

**Phase 1.** It added `MoveTarget::UnderTask`, `plan_indent` /
`plan_outdent`, and a TUI cut set (`d`). Its moves don't know about
links at all.

## Goals / Non-Goals

**Goals:**
- Express a task's parent in another file with Obsidian-native syntax
  (`[[note#^id]]`, or `[[note]]` for anchor notes), so links stay
  clickable in Obsidian.
- A single effective hierarchy (physical first, then link) used by every
  consumer, including graph edges and the query language.
- Fully offline. No I/O added to the scan, and roughly zero
  performance cost.
- First-class operations: link, unlink, create-linked, check, from the
  CLI, the TUI and editor clients.
- Moves never silently break links.
- A source-agnostic hook (`ft.tasks.anchor`) that Phase 3 issue notes
  plug into.

**Non-Goals:**
- Writing anchor notes or fetching remote issues (Phase 3).
- Syncing parent/child structure back to Jira (Phase 4 `promote`).
- Rewriting links when a parent *line* is edited by hand in Obsidian
  (the block id travels with the line, so that's fine) or when a parent
  is deleted. Deleted parents surface as `dangling` in `ft tasks check`.
- Supporting a parent named through Dataview fields or a new emoji
  field. This was rejected in favour of wikilinks, per the user's
  choice.
- Incremental `Graph::refresh_note` correctness for cross-file parents.
  The TUI always rebuilds the shared snapshot fully, and
  `refresh_note`'s callers don't read `Subtask` edges.

## Decisions

### D1. Resolve in a scan post-pass, store on `Task`

```rust
pub struct ParentLink {
    pub raw: String,                  // e.g. "[[proj#^vo1]]"
    pub range: std::ops::Range<usize>, // byte range within the task line
    pub extra: Vec<String>,           // later candidates, diagnostics only
    pub state: ParentLinkState,
}
pub enum ParentLinkState { Resolved(TaskKey), Dangling, Ignored, Cycle }
// Task gains: pub parent_link: Option<ParentLink>   (serde-skipped if None)
```

`scan_vault` already returns every file's links, frontmatter and tasks.
After the parallel parse, `task::links::resolve_parent_links(&mut Scan)`
runs serially:

1. **Build indexes.**
   - A `NoteLookup` (path → rel, title → [rel]) from `scan.files`.
   - `block_index: (rel, id) → Option<task idx>`, where `None` means the
     id is on a non-task line. Non-task block ids come from a cheap
     per-file list of `^id` line suffixes that the parse pass already
     collects; `ParsedFile` gains `block_ids: Vec<(String, usize)>`.
   - `anchor_index: rel → task idx`, taken from `ft_tasks_anchor_in` and
     matching the `🆔`.
2. **Find each task's candidates.** For every task, walk the file's
   links whose `line == source_line` (links are in document order, so a
   per-file cursor gives O(links)). Classify each link per the spec,
   record the first candidate and any extras, and set the state:
   `Ignored` if the task has a physical parent, otherwise `Resolved` or
   `Dangling`.
3. **Detect cycles.** Over effective parents, use iterative colouring.
   For each cycle found, mark the member with the greatest `TaskKey` as
   `Cycle`.

Storing the result on `Task` (rather than in a side table) means every
consumer holding a `&[Task]` (CLI, TUI snapshot, graph build) sees the
same hierarchy with no new plumbing. `Task` derives `Default`, so test
construction is unaffected.

Considered: resolving inside `Graph::build` and reading it back from
the graph. Rejected, because the CLI paths (`ft tasks list --tree`) and
the TUI hierarchy work from `scan.tasks` without querying the graph,
and the graph is downstream of the scan.

### D2. Factor link resolution behind a `NoteLookup` trait

`graph::resolve::{resolve_wiki, resolve_md}` currently take `&Graph`.
They get generic over a `trait NoteLookup { fn by_path(&Path) -> …;
fn by_title(&str) -> &[…] }`. `Graph` implements it (no behaviour
change), and so does a lightweight `ScanNoteIndex` built from
`scan.files`. This gives one set of resolution rules at every level,
which the `graph-model` spec already requires.

### D3. Effective parent via `hierarchy::parent_key`

`parent_idx` and `children_map` switch to `parent_key(task)`, and
`insert_subtask_edges` uses the same function. Within `children_map`,
physical children keep source order and linked children follow, sorted
by `TaskKey`, so tree output is deterministic.

The two `pub(crate)` helpers are the only places that interpret
`parent`. A grep shows `Task::parent` is read directly in only a few
other places (TUI `reveal`, `plan_indent` / `plan_outdent`), and those
deliberately want the *physical* parent.

### D4. Writing links: `task::links::{plan_link, plan_unlink}`

These are plan/apply like the moves. Description edits go through the
format: parse the line, edit `description`, `serialize_line`, keep the
leading whitespace. This puts the link before the emoji fields and
preserves canonical order.

Removing a link deletes `raw` plus one adjacent space, using the `range`
recorded by the scan, which is validated by the expected-task guard.

Link text rules (title if unique, path otherwise, `[[#^id]]` within the
same file, anchor-note form for anchor tasks) live in one function,
`link_text(from_rel, to_rel, anchor, &NoteLookup)`. `plan_retarget`
reuses it.

Block ids use a hash (std `DefaultHasher`) of `(path, line, description,
SystemTime::now nanos, counter)`, rendered as base 36 and truncated to 6
characters, re-hashed until unique among the file's `^id`s. This needs
no new crate. Tests inject a fixed time and counter through a
`#[cfg(test)]` seam.

### D5. Strip on physical nest; retarget on cross-file move

**Strip on nest.** `plan_move` already has each source's `expected:
Option<Task>`. For `UnderTask` targets, when `expected.parent_link` is
`Some`, the head line is rewritten without `raw` (located by `range` in
the original line, which the guard has just validated) before it is
re-indented. This lives in `plan_move` itself, so the CLI, the TUI
paste, indent and outdent all get it for free.

**Retarget on move.** `plan_retarget(plan, scan, ctx)` is a second,
optional pure step, because it needs vault-wide link data that
`plan_move` deliberately doesn't take:
1. Collect `(source rel, id)` for every `^id` on any line in the moved
   blocks.
2. Find the files with links resolving to those `(rel, id)` pairs, using
   `scan.files[*].links` and `NoteLookup`.
3. For each such file, take the plan's `new` content if the file is
   already edited, otherwise read it. Re-extract links from that
   content, rewrite the matching ones with `link_text(…)` pointing at
   the target rel, and merge the result into the plan's edits.

Links inside the moved blocks themselves are handled by the same pass,
because their file is the target file, whose `new` content is
re-extracted.

The callers are CLI `move` and the TUI paste and `M` move. Indent and
outdent never change file, so they skip it.

### D6. CLI surface

- `ft tasks link <SEL>... --under <SEL>`, `ft tasks unlink <SEL>`
  (both with `--dry-run` / `--format`).
- `ft tasks create --link-parent <SEL>` (conflicts with `--parent`).
- `ft tasks check [--format json]` (exit 1 on issues).

Selector resolution reuses `resolve_unique` from Phase 1. JSON shapes
follow Phase 1's move output, so ft.nvim parses one family of objects.

### D7. TUI

| Key | Command | Behaviour |
|-----|---------|-----------|
| `L` | `tasks.link` | Link the cut set under the cursor task. Lines don't move. Clear the set and expand the parent. |
| `U` | `tasks.unlink` | Remove the cursor task's parent link. |
| `S` | `tasks.new-linked-subtask` | Same quickline as `c`, with a pending `link_parent` (mirrors `subtask_parent` for `s`). The commit path calls `create_task` and then `plan_link` on the new line in the same handler. |

Linked children render with `↳ ` (the `DisplayRow` gains `linked:
bool`, set when the row's effective parent comes from the link). Every
other tree behaviour (expand, collapse, dedupe, reveal) comes from D3
unchanged.

### D8. DSL attributes and presets

- **Attributes:** `parent` (Str, optional), `parent_status` (Enum,
  optional), `parent_link` (Enum, optional) and `children` (Int) are
  added to `Attr`.
- **Where the values come from:** `TaskData` gains the matching fields,
  filled in at `Graph::build` from the effective hierarchy (the parent
  task's description and status, the link state, the child count).
- **Rules:** operator validation follows the existing `ValueType`
  rules, and there is no new boolean type (`children > 0` covers "has
  children").
- **Presets:** `orphaned` and `broken-links` join the task preset table.

## Risks / Trade-offs

- **The scan grows a serial post-pass.** It is linear, uses in-memory
  data, and is guarded by a perf test gated on `FT_PERF_TESTS=1`.
- **Link ambiguity: a block id duplicated in a note.** Obsidian takes
  the first one, and so does ft. `check` doesn't flag it, because that
  is Obsidian's concern.
- **Title collisions change link resolution later.** A new `proj.md`
  can shadow the parent's title. This is inherent to title-form links,
  and `ft tasks check` shows the resulting `dangling` or mis-parenting.
  Writing the path form when titles are ambiguous at link time reduces
  it.
- **Hand edits in Obsidian can create `Ignored` links.** `check` and
  the `broken-links` preset surface them, and nothing breaks.
- **The retarget pass touches files the user didn't name.** It is
  always visible in `--dry-run` diffs. In the TUI, the success toast
  says so (`moved 1 task(s) under X · retargeted 2 link(s)`).
- **`Task::parent` keeps its physical meaning while the effective
  parent is separate.** That's the right split, but it's easy to
  misuse, so `parent_key` is documented as the default and `parent` as
  physical-only.
- **This change modifies `task-reparent`, which isn't archived yet.**
  Archive Phase 1 before archiving this change so the delta applies
  cleanly.

## Roadmap notes

- Phase 3 issue notes SHALL use `ft.tasks.anchor: <KEY>` in their `ft:`
  frontmatter block, replacing the flat keys sketched in the Phase 1
  roadmap. Other sync metadata follows the same nesting
  (`ft.source.*`). The anchor task line carries `🆔 <KEY>`.
- Phase 4 `promote` turns a linked child into a remote sub-issue: it
  writes a new anchor note and moves the line there. Retargeting from
  D5 keeps the grandchildren's links valid.

## Open Questions

None blocking.

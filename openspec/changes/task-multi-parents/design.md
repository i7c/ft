## Context

**Phase 2 as built.**
- The scan post-pass (`task::links::resolve_parent_links`) classifies
  wikilinks on each task line into one `Task::parent_link:
  Option<ParentLink { raw, extra, state }>`. `state` is
  `Resolved | Dangling | Ignored | Cycle`.
- The lookups behind it are a `ParentIndex` (file/task indexes, a
  per-file block-id map, anchor notes, frontmatter parents).
- `hierarchy::parent_key` returns the physical parent, otherwise the
  resolved link. `children_map` / `expand_forest*` / `child_index_map` /
  `Graph::insert_subtask_edges` all derive from it.
- Both forest expanders dedupe with a global `seen: HashSet<usize>`, so
  a task appears exactly once.

**Writes.**
- `plan_link` adds a block id to the parent (`new_block_id`, unique per
  file) and writes link text chosen by `link_text`.
- `plan_unlink` removes `raw`.
- `plan_move` strips `raw` when nesting physically.
- `plan_retarget` rewrites `[[file#^id]]` references after cross-file
  moves. Promote uses it too.

**Identifiers already present.**
- `EmojiFormat` parses Tasks-plugin `🆔 <id>` into `Task::id` and
  `⛔ a,b` into `Task::depends_on`.
- Jira/GitHub anchor tasks carry `🆔 <KEY>`.
- Selectors already resolve a bare `🆔` value (`ft tasks done <id>`).

**Sources.**
- Sync writes `ft.tasks.parent: "[[EPIC-7]]"` and rewrites the anchor
  line. It keeps local fields (tags, ⏳, 🛫, ➕, recurrence, block id,
  unknown trailing fields).
- Promote takes `parent_key(task)` as the remote parent.

## Goals / Non-Goals

**Goals:**
- Any number of logical parents per task, in addition to the physical
  parent.
- A compact, move-proof reference syntax shared by local tasks and
  remote tickets.
- Every hierarchy consumer (CLI tree, TUI tree, graph, DSL, check,
  promote) sees the same multi-parent hierarchy, still from a single
  choke point.
- Scan cost stays linear, with no I/O.

**Non-Goals:**
- Migrating existing wikilink parents (user decision: drop).
- Obsidian click-through from child to parent. ft, the TUI and ft.nvim
  are the UI.
- Pushing extra parents to Jira (no issue links). Jira keeps exactly the
  one parent promote chose.
- Weighting or progress roll-ups across goals.
- Changing `⛔` (`depends_on`). It keeps its Tasks-plugin meaning and
  has nothing to do with parents.

## Decisions

### D1. Syntax: `⤴ id,id` as a task field

`EmojiFormat` gains `PARENTS = "⤴"` (U+2934), added to
`ALL_FIELD_EMOJIS`.
- **Parsing:** the parser accepts `⤴` with or without a trailing U+FE0F.
  The value is a comma list split like `⛔`, with surrounding whitespace
  trimmed and empty items dropped, stored in `Task::parent_ids:
  Vec<String>` in written order.
- **Serializing:** `serialize_line` writes the bare `⤴` (no VS16), so it
  is one terminal column wide and needs no wide-char handling. It goes
  **directly after the description**, before the priority. The Obsidian
  Tasks plugin strips known fields from the end of the line and stops at
  the first unknown token. Putting `⤴` first keeps due, priority and the
  rest parseable by Tasks, while Tasks reads `⤴ …` as description text.
  That costs nothing, so we keep it even though Obsidian compatibility
  isn't required.
- Duplicate ids in one field are kept on parse (round-trip fidelity),
  deduplicated during resolution, and reported by `check` as
  `redundant`.

Considered:
- Several wikilinks: too long, and not move-proof.
- Dataview `[parent:: …]`: the Tasks plugin treats it as description
  text, and it was rejected in Phase 2.
- Reversing the direction onto `⛔` on the parent: capturing from the
  journal would mean editing the parent line, and anchor lines are
  sync-managed.

### D2. Ids: vault-wide `🆔`, generated on demand

- **Index.** The post-pass builds `ids: HashMap<&str, IdHit>` from every
  task's `Task::id`, where `IdHit` is `One(task idx)` or `Many`. Anchor
  tasks are just tasks with `🆔 KEY`, so local and remote parents share
  one namespace. `ft.tasks.anchor` is still read by sync and promote to
  find the anchor task, but parent resolution no longer needs it.
- **Generation.** `task::links::new_task_id(seed, &ids) -> String`
  reuses the existing hash seed (path, line, description, time,
  counter). It renders 6 lowercase base-36 characters and re-derives
  until the id is absent from the vault-wide index **and** from ids
  already generated in the same plan. The `#[cfg(test)]` time/counter
  seam carries over. There is no counter state in the vault, so two
  machines syncing over git can't collide by construction.
- **When ids are assigned.** Only when a task becomes a parent: `link`,
  `create --link-parent`, the TUI `L`/`S`. A parent that already has a
  `🆔` keeps it. That includes user-chosen ids and Jira keys.

`new_block_id` and `prepare_parent`'s block-id path are deleted.
`plan_retarget` keeps working on existing `^id`s; it never generated
them.

### D3. Model: `parent_ids` (parsed) + `parent_refs` (resolved)

```rust
pub struct Task {
    // …
    pub parent_ids: Vec<String>,        // the ⤴ field as written; format-owned
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parent_refs: Vec<ParentRef>,    // scan post-pass; one per distinct id
}
pub struct ParentRef { pub id: String, pub origin: RefOrigin, pub state: RefState }
pub enum RefOrigin { Line, Frontmatter }   // frontmatter = anchor note's ft.tasks.parent
pub enum RefState { Resolved(TaskKey), Dangling, Ambiguous, Cycle }
```

- `ParentLink`, `ParentLinkState`, `ParentLinkKind` and the `Ignored` /
  `extra` concepts are deleted.
- `TaskData` (the graph node) carries `parent_refs: RefKinds`, a
  one-byte set of the states present, which keeps `NodeKind` within the
  size lint the same way `ParentLinkKind` did. The task's `🆔` lives in
  a graph side table (`Graph::task_id`, like `task_remote`). Parent
  descriptions, statuses and counts come from the incoming `Subtask`
  edges at evaluation time, so the DSL needs no per-parent copies.

### D4. Resolution post-pass

`resolve_parent_refs(&mut Scan)` runs serially after the parallel parse,
as today:

1. **Indexes.** Build `task_at` (file idx, line → task idx), the `ids`
   index (D2), and frontmatter parents (`anchor task idx → id`, read
   with `ft_tasks_parent_in`, which now returns the scalar as an id).
2. **Refs.**
   - For each task, take the distinct ids in `parent_ids`. For anchor
     tasks, append the frontmatter id (origin `Frontmatter`) when it
     isn't already listed.
   - Map each id through `ids`: `One(i)` → `Resolved(key(i))` (or
     `Cycle` if `i` is the task itself); `Many` → `Ambiguous`; absent →
     `Dangling`.
   - A ref that resolves to the task's own physical parent stays
     `Resolved` and is flagged `redundant` for `check` only (a derived
     check, not a state).
3. **Cycles.**
   - Iterative three-colour DFS over effective parents: the physical
     parent plus every `Resolved` ref.
   - When a back edge is found, mark one logical ref on the cycle as
     `Cycle`: the ref of the member with the greatest `TaskKey`. If that
     member has several refs on the cycle, take the ref named by the
     back edge.
   - Physical edges alone can't cycle, so a logical edge always exists.
   - Repeat until no back edge remains. Each repeat removes one edge,
     and in practice there are few.

The cost stays O(tasks + refs).

### D5. Effective parents and the DAG choke point

`hierarchy::parent_keys(task) -> impl Iterator<Item = TaskKey>` yields
the physical parent first, then the `Resolved` refs in order, with
duplicates removed. `parent_key` is deleted. Its callers are reviewed:
`promote` gets explicit selection (D9), the TUI reveal uses the first
key, and `plan_indent` / `plan_outdent` keep using physical
`Task::parent`.

`children_map` pushes each task under **every** parent from
`parent_keys`. Ordering per parent: physical children in source order
first, then linked children by `(path, line)`.

**Forest expansion.** `expand_forest` and `expand_forest_visible` keep
their signatures. `ForestRow` / `VisibleRow` gain `via:
Option<usize>`, the parent this occurrence hangs under (`None` for
roots).
- **Display set.** As today: the matched tasks plus all their
  descendants, now over the DAG.
- **Roots.** A matched task is a root iff **none of its parents is in
  the display set**. This replaces the "already seen" test. In
  matched-first order, a matched task under a matched parent shows only
  under that parent (or parents).
- **Emission.** Pre-order recursion with a per-path ancestor set instead
  of the global `seen`. A child already on the current path is skipped,
  which cuts cycles that resolution left behind, such as cycles made
  purely of physical plus `Cycle`-marked edges. Otherwise children are
  emitted wherever their parent is emitted, so subtrees repeat.
- **Blow-up guard.** A per-call row cap (`MAX_FOREST_ROWS = 50_000`).
  When it is reached, emission stops, and `ft tasks list --tree` prints
  a warning to stderr. This is only a guard against pathological DAGs.

The TUI expansion state stays keyed by `TaskKey`: expanding a task
expands it in every occurrence.

### D6. Output shapes

- **`table` and `markdown` trees:** repeat the subtree under every
  shown parent. `table` adds `⤴N` after the description when a task has
  N > 1 effective parents.
- **`json`:** nested `subtasks` repeat. Every task object (tree or flat)
  carries `"id": <🆔|null>` and, when it has references,
  `"parents": [{"id", "path", "line", "state", "origin"}]` (omitted when
  empty, like the other optional collections). Unresolved refs have
  `null` path and line.
- **`ndjson` tree:** emits pre-order rows *with* repeats, each carrying
  `"depth"` and `"via": {"path","line"}|null`, so streaming consumers can
  rebuild the DAG.

### D7. DSL

- **`parent`** (Str, optional) and **`parent_status`** (Enum, optional)
  evaluate over the list of effective parents with **any** semantics.
  - A comparison is true if it is true for at least one parent.
  - `is null` is true iff there are no effective parents.
  - Negated forms (`!=`, `not in`) keep the evaluator's existing
    per-value semantics, so they are also existential.
  - Universal questions use counts. This is documented with the
    `orphaned` preset.
- **`parents`** (Int): the number of distinct effective parents.
- **`children`** (Int): the number of distinct effective children.
- **`id`** (Str, optional): the task's `🆔`.
- **`parent_ref`** (Enum, optional, any): `resolved | dangling |
  ambiguous | cycle` over all refs. It replaces `parent_link`; the old
  name is removed (no alias).
- **Presets.**
  - `orphaned`: an open task (`status in {Open, InProgress}`) that has
    parents and whose parents are all done or cancelled. With only
    existential `parent_status`, "all closed" needs a dedicated count
    attribute, `open_parents` (Int: effective parents whose status is
    Open or InProgress). Then `orphaned` = `status in {Open, InProgress}
    and parents > 0 and open_parents = 0`.
  - `broken-links`: `parent_ref in {dangling, ambiguous, cycle}`.

`TaskData` gains the backing values at `Graph::build`.

### D8. Writes

- **`plan_link(children, parent, ctx, format)`.**
  - It ensures the parent has a `🆔` (D2), then appends the parent's id
    to each child's `parent_ids` via parse → edit → `serialize_line`, as
    today.
  - Already present → no edit; the child is reported as `unchanged`.
  - `WouldCycle` stays.
  - `PhysicallyNested` is deleted (nesting no longer excludes links).
  - Linking a child under its own physical parent →
    `LinkError::AlreadyPhysicalParent`, because the ref would be
    redundant.
- **`plan_unlink(task, which, format)`**, with `which: Unlink::Only |
  Unlink::Parent(TaskKey) | Unlink::All`:
  - `Only` with ≠1 line refs → `LinkError::NeedsChoice { refs }`, or
    `NoParentLink` when there are none.
  - `Parent(k)` with no line ref resolving to `k` → `NotLinkedTo`. A ref
    whose origin is `Frontmatter` → `ParentFromFrontmatter`, as today.
  - Removing the last id removes the whole `⤴` field. The parent's `🆔`
    stays.
- **`plan_move` strip-on-nest.** When nesting under task P, remove P's
  `🆔` from the moved head line's `parent_ids` (if listed). Other ids
  stay. `link_text` and `plan_retarget` are unchanged; they now only
  serve ordinary block references and promote.
- **`check`** kinds: `dangling`, `ambiguous`, `cycle`, `redundant`, plus
  `duplicate-id` (one entry per extra task carrying an already-used
  `🆔`, so users can find the copy-pasted line). `ignored` and `extra`
  are removed.

### D9. Promote with several parents

`promote::preconditions` computes the *eligible* parents: effective
parents that are anchor tasks of issue notes belonging to a configured
source with `push` enabled.
- **Zero** → the existing message.
- **One** → use it.
- **More than one** → `PromoteError::ChooseParent { candidates }`. The
  CLI renders it as `several ticket parents (ABC-1, ABC-2) — pass
  --parent KEY` and exits 2. `ft tasks promote --parent KEY` picks one,
  which must be among the candidates.

**Restructuring.** The new issue note gets `ft.tasks.parent: KEY`. The
anchor line keeps the promoted line's `⤴` ids **minus** `KEY` (the
relation is now owned by the frontmatter). If the promoted task was
physically nested under the chosen anchor, nothing changes beyond
today's flow. If it was physically nested under a different, local
parent, that parent is added to the new anchor line's `⤴` before the
line moves, so the relation survives (this needs a `🆔` on that parent;
D2 assigns one). If the promoted line itself had a `🆔`, the anchor's
`🆔` becomes the issue key. Every other task's `⤴` reference to the old
id is rewritten to the key in the same plan, so children linked by
reference follow the task. The scan's id index provides them, so no
extra read pass is needed.

### D10. Sync

- **Frontmatter.** `write_frontmatter` writes `parent: <KEY>`. The
  deterministic planner rewrites existing notes the first time it runs,
  giving a one-off diff per issue note with a parent. Until that sync
  runs, an old `"[[EPIC-7]]"` value is just an unknown id (`dangling`).
  This follows the no-migration decision; there is no bracket-stripping
  compatibility code.
- **Anchor line.** The anchor-line rewrite preserves `parent_ids`, like
  the other local fields. They are never pushed.

### D11. TUI

- **Rows.** `DisplayRow` gains `via: Option<usize>` (from D5) and
  `parent_count: usize`.
  - `linked` becomes "`via` is a logical, not physical, parent of this
    task".
  - The description omits the `⤴ …` field text. The task's own `🆔` is
    already rendered as today.
  - Rows with `parent_count > 1` show a dim `⤴N` badge.
- **Cursor and anchors.** The pending cursor anchor and the selection
  become `(TaskKey, Option<TaskKey /*via*/>)`. After a refresh, the
  cursor restores the exact occurrence, falling back to the first
  occurrence of the task.
- **`L`.** Additive link of the cut set under the cursor task; the toast
  counts unchanged children (`linked 2 task(s) under X (1 already
  linked)`).
- **`U`.**
  - If the cursor row's `via` is a logical parent → unlink from it.
  - If `via` is the physical parent, or the row is a root, and the task
    has exactly one line ref → remove it.
  - With several → open `ActiveModal::TaskParentPick` (a fuzzy picker
    over the task's line refs with parent descriptions plus an
    `all parents` entry; `Enter` removes the choice, `Esc` cancels).
  - No line refs → `task has no parent link`.
- **`I`** (promote). On `ChooseParent`, open the same `ParentPick` modal
  in "choose" mode and re-run promote with the picked key.
- **`S`.** Unchanged; it creates a task linked under the cursor task.

The keymap is unchanged. The command descriptions for `tasks.link` /
`tasks.unlink` change, so `docs/keybindings.md` is regenerated.

### D12. ft.nvim

The plugin stays a thin client over the CLI contract.
- `:FtTaskLink` is additive.
- `:FtTaskUnlink` calls `ft tasks unlink … --format json`. On the
  `NeedsChoice` error (a JSON error with a `refs` payload), it shows
  `vim.ui.select` and re-runs with `--from`.
- Goto-parent uses the task's JSON `parents`, with a picker when there
  are several.
- Virtual text after a `⤴` field shows the parent descriptions, using
  `ft tasks list --format json` data the plugin already caches,
  invalidated on its existing events.
- The `check` quickfix list gains the new kinds.
- `ARCHITECTURE.md` documents the new flags and JSON fields.

## Risks / Trade-offs

- **Breaking for existing wikilink parents.** They silently become plain
  links. The user accepted this; the guide calls it out, and `ft tasks
  list --query 'parents = 0'` helps find tasks that lost a parent.
- **Ids are not human-readable.** `⤴ k3x9qa` means nothing in a plain
  editor. This is mitigated by the TUI, ft.nvim virtual text and `ft
  tasks list --query 'id = k3x9qa'`. Users may still choose readable
  `🆔` values by hand.
- **Copy-pasted lines duplicate `🆔`.** References turn `ambiguous`
  instead of silently picking one, and `check` lists `duplicate-id`
  entries.
- **Repeated subtrees inflate output.** That is intended, and bounded by
  the row cap. Counts in `--format table` summaries count occurrences,
  not distinct tasks; the JSON `id`/`path`/`line` fields let scripts
  dedupe.
- **Test ripple.** Deleting `parent_link` touches every test that
  builds wikilink parents (`links.rs`, `sources/*`, the TUI
  `tasks_link.rs`, graph and DSL tests). This is budgeted as its own
  task group, with fixtures rewritten mechanically to `🆔`/`⤴`.
- **Existentially-quantified negation in the DSL** (`parent_status !=
  Done` is true if *any* parent isn't done) can surprise users. It is
  documented, and `open_parents` covers the universal case.

## Migration Plan

None for user data (decision). Sync rewrites issue-note frontmatter on
its next run. Deployment is a normal release. Rollback means reverting
the release; `⤴` fields left in the vault then read as description text
to the old version.

## Open Questions

None blocking.

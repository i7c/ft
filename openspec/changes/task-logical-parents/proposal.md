## Why

In ft today, a task's parent can only be the task above it in the same
file, one indentation level up (`resolve_hierarchy`,
`task/hierarchy.rs:240`). Phase 1 (`task-reparent`) made physical
nesting easy, but the most common real workflow still can't be
expressed. You capture "reproduce on staging" in today's daily note and
want it to count as a subtask of a project task, or (in Phase 3) of a
Jira issue, without moving the line out of the journal.

The user also wants a local subtask whose parent is a remote issue.
That is only possible if parents can live in other files.

This is Phase 2 of the task revamp (roadmap in
`openspec/changes/task-reparent/design.md`). It adds cross-file
*logical* parents expressed with plain Obsidian wikilinks, so the vault
stays readable and navigable in Obsidian, and everything works offline.

## What Changes

- **Parent links.** A task line names its parent with a wikilink in its
  description. Only two kinds of link count:
  - a block reference to a task, `[[note#^id]]` (or `[[#^id]]` within the
    same file);
  - a link to an *anchor note*, `[[note]]`, whose frontmatter has
    `ft.tasks.anchor: <id>` and which contains a task with `🆔 <id>`.
    This is the source-agnostic hook that Phase 3's Jira and GitHub
    issue notes will use.

  The first qualifying link in the description wins. Ordinary note links
  never count.
- **One resolved hierarchy.** A post-pass at the end of `Vault::scan`
  resolves parent links into a new `Task::parent_link` field, which
  records the raw link and a state: `Resolved(key)`, `Dangling`,
  `Ignored` (the task is physically nested, and physical nesting wins)
  or `Cycle`.
  - `hierarchy::parent_key` returns the effective parent: the physical
    parent first, otherwise the resolved link.
  - `expand_forest`, `expand_forest_visible`, `child_index_map`, the TUI
    tree, `ft tasks list --tree` and graph `Subtask` edges all switch to
    it, so cross-file children show up everywhere with no per-consumer
    work.
- **Query language.** New task attributes:
  - `parent`: the parent's description, optional.
  - `parent_status`: an enum, optional.
  - `parent_link`: an enum of `resolved | dangling | ignored | cycle`,
    optional.
  - `children`: an integer, the number of direct children.

  New built-in presets: `orphaned` (open tasks whose parent is done or
  cancelled) and `broken-links`.
- **Block ids on demand.** Linking to a parent that has no `^id` gives
  it a fresh 6-character id, unique within its file, in the same plan.
- **CLI.**
  - `ft tasks link <SEL>… --under <SEL>` links one or more tasks to a
    parent (they stay where they are). `ft tasks unlink <SEL>` removes a
    task's parent link. Both support `--dry-run` and `--format json`.
  - `ft tasks create --link-parent <SEL>` creates a task at the usual
    target (the daily note by default) already linked to the parent.
  - `ft tasks check [--format json]` reports dangling links, cycles,
    ignored links and extra parent links, and exits 1 when it finds any.
- **Moves keep links valid (fixes a Phase 1 gap).**
  - Moving a task that carries a block id to another file (with `--to`,
    `--under`, a TUI paste or the `M` picker) now also rewrites every
    `[[…#^id]]` link that pointed at the old file.
  - Physically nesting a task (paste `p`/`P`, `--under`, indent) strips
    its now-redundant parent link from the moved line.
- **TUI Tasks tab.**
  - `L` links the cut set (marked with `d`) under the cursor task.
  - `U` unlinks the selected task.
  - `S` opens a quickline that creates a new task in the default target,
    linked under the selected task.
  - Linked children render with a `↳` glyph so you can tell them apart
    from physically nested ones.
- **[ft.nvim]** Link and unlink commands, plus `ft tasks check --format
  json` fed into the quickfix list.

## Capabilities

### New Capabilities

- `task-logical-parents`: Parent-link syntax and resolution rules
  (block references, anchor notes via `ft.tasks.anchor`), the
  `ParentLink` states, effective-parent hierarchy across files, graph
  `Subtask` edges across files, and the new DSL attributes and presets.
- `task-link-cli`: `ft tasks link`, `unlink`, `check` and
  `create --link-parent`; block-id generation; link-text formatting.
- `tui-tasks-link`: `L` link, `U` unlink, `S` linked-subtask quickline,
  and `↳` rendering on the Tasks tab.

### Modified Capabilities

- `task-reparent`: task-relative moves strip the moved task's parent
  link. Cross-file moves retarget incoming block-reference links.
- `tui-keymaps`: Tasks-tab bindings `L`, `U`, `S`.

## Impact

- **ft-core:**
  - `task/mod.rs`: a new `parent_link: Option<ParentLink>` on `Task`.
    Defaults to `None`, so `..Default::default()` test construction is
    unaffected.
  - A new module, `task/links.rs`: resolution post-pass, `plan_link`,
    `plan_unlink`, `plan_retarget`, `check`.
  - `task/hierarchy.rs` (`parent_key`), `scan.rs` (calls the post-pass),
    `frontmatter.rs` (`ft_tasks_anchor_in`).
  - `graph/mod.rs`: `TaskData` fields and cross-file `Subtask` edges.
  - `graph/resolve.rs`: link resolution is factored behind a small
    lookup trait so the scan can resolve links without building a
    `Graph`.
  - `graph/query`: new attributes. `query/preset.rs`: new presets.
- **ft binary:** `cmd/tasks.rs` (link/unlink/check/create flag; move
  wires up retargeting); the Tasks tab (`search.rs`: keys, glyph,
  link/unlink/create flows; `modals.rs`: the `M` move also retargets).
- **No new dependencies.** Block ids come from a hash of path, line,
  description and time, with a collision check against the file.
- **Performance:** the post-pass is O(tasks + task-line links) over data
  the scan already holds. No extra file reads, and no change to the
  ~0.14 s scan baseline beyond noise. This is checked with a perf test
  gated on `FT_PERF_TESTS=1`.

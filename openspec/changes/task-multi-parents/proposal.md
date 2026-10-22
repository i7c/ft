## Why

Phase 2 (`task-logical-parents`) gave a task exactly one parent: its
physical (indentation) parent, or else the first wikilink parent on its
line. Real planning doesn't fit a strict tree. One task often contributes
to several goals at once, for example two local project tasks and a Jira
ticket. Today the user has to pick one of them.

The wikilink syntax also doesn't scale to several parents.
`[[projects/vendor#^vo1]] [[goals-2026#^g7x2]] [[ABC-123]]` makes the
line unreadable. Block ids are only unique per file, which is why every
cross-file move needs a link-retargeting pass. The vault already has a
vault-wide identifier: Tasks-plugin `🆔`, which Jira anchor tasks already
carry (`🆔 ABC-123`). Short id references on a dedicated field are
shorter, survive moves without any rewrite, and treat local and remote
parents the same way.

## What Changes

- **BREAKING: parent syntax.** A task names its logical parents with a
  new emoji field, `⤴ id1,id2,…`, listing the `🆔` values of its
  parents. ft writes the field first, right after the description, so
  the Tasks plugin's trailing-field parsing still works. Wikilinks never
  make a parent any more. Existing `[[note#^id]]` and `[[anchor-note]]`
  parents become ordinary links, with no migration (the user's choice).
- **Multiple parents.** A task's effective parents are the union of its
  physical parent (first) and every `⤴` reference, in line order, with
  duplicates removed. Indentation no longer overrides links, so the
  `ignored` state goes away.
- **Vault-wide ids.** `🆔` values are looked up across the whole vault.
  ft adds a `🆔` to a parent only on demand, when something is linked
  under it. The id is 6 random lowercase base-36 characters, checked for
  collisions against the vault-wide index. A `🆔` that appears on more
  than one task makes references to it `ambiguous`.
- **Resolution states.** `Task::parent_link: Option<ParentLink>` becomes
  `Task::parent_refs: Vec<ParentRef>`, with states
  `Resolved(key) | Dangling | Ambiguous | Cycle`.
- **Trees become DAG views.** A task appears under *every* parent in the
  output. It is a root only if it matches the query and none of its
  parents is in the output. Subtrees repeat. Cycles are cut per path.
  Graph `Subtask` edges go one per parent/child pair.
- **Query DSL.**
  - `parent` and `parent_status` match if any parent matches.
  - New attributes: `parents` (count), `id` (the task's `🆔`), and
    `parent_ref`, an any-match enum over the reference states that
    replaces `parent_link`.
  - Presets: `orphaned` becomes "open, and every parent is closed";
    `broken-links` becomes "any reference is unresolved".
- **CLI.**
  - `ft tasks link … --under SEL` *adds* a parent; running it twice
    changes nothing.
  - `ft tasks unlink SEL [--from SEL | --all]` removes references; with
    several references and neither flag it exits 2 and lists them.
  - `ft tasks create --link-parent SEL` can be repeated.
  - `ft tasks check` reports `dangling`, `ambiguous`, `cycle` and
    `redundant` (a reference that names the physical parent again).
  - Physically nesting a task under X strips only the reference to X.
- **TUI Tasks tab.**
  - `L` adds a parent.
  - `U` removes the parent the cursor row is displayed under; on a row
    with no such context and several references, it opens a picker
    modal.
  - Rows with more than one parent show a `⤴N` badge.
  - The cursor anchor becomes the pair (task, context parent).
- **Sources.**
  - Sync writes `ft.tasks.parent: EPIC-7` (a plain id, not a wikilink).
  - `⤴` on an anchor line is a local field: sync preserves it and never
    pushes it.
  - `ft tasks promote` needs exactly one parent that is a ticket of a
    source with `push` enabled. If several qualify, the CLI needs
    `--parent KEY` and the TUI shows a picker. The chosen reference
    moves into the new note's frontmatter; the other references stay on
    the promoted line.
- **Kept:** `plan_retarget` remains as general link hygiene, so ordinary
  `[[note#^id]]` references survive cross-file moves. Promote also uses
  it. Parents themselves no longer depend on it.
- **[ft.nvim]**
  - Link and unlink with multiple parents; unlink picks a reference via
    `vim.ui.select`.
  - Virtual text that shows the parent description for each `⤴` id.
  - Goto-parent, with a picker when there are several parents.
  - `check` results, including `ambiguous`, go to the quickfix list.

## Capabilities

### New Capabilities

None. Every behaviour here changes an existing capability.

### Modified Capabilities

- `task-logical-parents`:
  - parent syntax (`⤴` ids instead of wikilinks);
  - vault-wide `🆔` index;
  - `ParentRef` states;
  - multi-parent effective hierarchy;
  - DAG tree semantics;
  - graph edges;
  - DSL attributes and presets;
  - id-valued anchor-note frontmatter parent.
- `task-link-cli`:
  - additive `link`;
  - `unlink --from/--all`;
  - repeatable `--link-parent`;
  - `check` kinds;
  - id generation replaces block-id generation;
  - anchor-note link preference removed.
- `tui-tasks-link`: additive `L`, context-aware `U` with a picker modal,
  `⤴N` badge, linked-child glyph, and the cursor anchor per occurrence.
- `task-reparent`: physical nesting strips only the reference to the new
  parent.
- `task-source-sync`: plain-id `ft.tasks.parent`; `⤴` on anchor lines is
  local and preserved.
- `task-source-promote`: preconditions with multiple parents,
  `--parent KEY`, and which references stay after the restructuring.
- `tui-source-push`: a picker for promote when several ticket parents
  qualify.

## Impact

- **ft-core:**
  - `task/mod.rs`: `parent_ids: Vec<String>` (the parsed field) and
    `parent_refs: Vec<ParentRef>` (resolved), both defaulting to empty.
    This replaces `parent_link`.
  - `task/emoji.rs`: the `⤴` field; parse and serialize; proptest.
  - `task/links.rs`: rewrite the resolution post-pass around an id
    index; `plan_link`, `plan_unlink` and `check` work on id lists;
    delete wikilink candidate classification; replace `new_block_id`
    with vault-unique task id generation.
  - `task/hierarchy.rs`: `parent_keys`; DAG-aware `children_map`,
    `expand_forest` and `expand_forest_visible`.
  - `graph/mod.rs` (`TaskData` fields, Subtask edges);
    `graph/query/{parser,eval,display}.rs` (attributes);
    `query/preset.rs`.
  - `frontmatter.rs`: `ft_tasks_parent_in` returns an id.
  - `sources/sync.rs`: writes the frontmatter as an id and preserves
    `⤴` on the anchor line.
  - `sources/promote.rs`: choosing the remote parent.
- **ft binary:**
  - `cmd/tasks.rs`: link/unlink/check/create flags, promote `--parent`,
    `--tree` rendering and JSON `parents`.
  - The TUI Tasks tab: `search.rs` (rows per occurrence, keys, badge);
    `modals.rs` (parent picker for `U` and `I`).
- **Tests:** the change from `parent_link` to `parent_refs` ripples
  through `links.rs`, graph, TUI and sources tests. Existing wikilink
  fixtures are rewritten to `⤴`/`🆔`. TUI snapshots change for the
  badge.
- **Docs:** `docs/guide/tasks.md` (Linked parents, Remote sources,
  Promote), `docs/task-format.md` (the `⤴` field),
  `docs/query-dsl.md` (attributes), `docs/keybindings.md` (regenerated
  if descriptions change).
- **No new dependencies.** Scan cost stays linear: one more hash index,
  no I/O.
- **[ft.nvim]:** `lua/ft/tasks.lua` (link/unlink/goto-parent, virtual
  text) and `ARCHITECTURE.md` (protocol contract: the new flags and
  JSON shapes).

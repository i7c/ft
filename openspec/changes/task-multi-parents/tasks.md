## 1. Format: `⤴` field

- [x] 1.1 `EmojiFormat`: `PARENTS = "⤴"` in `ALL_FIELD_EMOJIS`; parse with optional U+FE0F; comma split like `⛔` (trim, drop empties, keep duplicates) into `Task::parent_ids`.
- [x] 1.2 `serialize_line` writes `⤴ a,b` directly after the description (before priority); no field when empty.
- [x] 1.3 Tests: parse/serialize scenarios, FE0F accepted and normalised on rewrite, field order; extend the proptest strategy with `parent_ids` and assert `serialize(parse(line)) == line`.
- [x] 1.4 `docs/task-format.md`: document the field and its placement.

## 2. Model + resolution post-pass

- [x] 2.1 Replace `ParentLink`/`ParentLinkState`/`ParentLinkKind` with `ParentRef { id, origin, state }`, `RefOrigin`, `RefState`, `RefStateKind`; `Task::parent_refs: Vec<ParentRef>` (serde skip when empty).
- [x] 2.2 `ParentIndex`: drop block/anchor-link classification; add the vault-wide `🆔` index (`One`/`Many`) and frontmatter parents read as plain ids (`ft_tasks_parent_in` returns the scalar).
- [x] 2.3 `resolve_parent_refs(&mut Scan)`: distinct ids per task in order, then the frontmatter ref; `Resolved`/`Dangling`/`Ambiguous`; self-reference → `Cycle`.
- [x] 2.4 Cycle detection over physical + resolved edges; mark the greatest-`TaskKey` member's logical ref on the cycle; repeat until acyclic.
- [x] 2.5 Delete wikilink candidate code, `ParsedFile::block_ids` consumers that only served parents (keep what `plan_retarget` needs), `ParentFromFrontmatter` stays.
- [x] 2.6 Tests: local + remote ids, wikilink not a parent, duplicate id → ambiguous, dangling, deleted parent, 2- and 3-cycles, self-ref, frontmatter + line union and dedupe, redundant (same as physical parent).
- [x] 2.7 Update the perf test to multi-parent references; keep the < 10% budget.

## 3. DAG hierarchy

- [x] 3.1 `hierarchy::parent_keys` (physical first, resolved refs, dedupe); delete `parent_key`; review callers (TUI reveal → first key; promote → D9; indent/outdent keep `Task::parent`).
- [x] 3.2 `children_map` lists a child under every parent; ordering physical-then-linked by `(path, line)`.
- [x] 3.3 `expand_forest` / `expand_forest_visible`: display set over DAG, root rule "no parent in display set", per-path ancestor guard, `via` on rows, 50k row cap (+ flag for the CLI warning).
- [x] 3.4 `Graph::insert_subtask_edges`: one edge per effective parent; none for unresolved; no duplicate for redundant refs.
- [x] 3.5 Tests: task under two parents, only one parent in view, physical + logical, unmatched child pulled in, cycle remnant cut per path, row cap, graph edges.

## 4. DSL + presets

- [x] 4.1 `TaskData`: per-parent descriptions/statuses, ref state kinds, `parents`, `open_parents`, `children`, `id`; filled at `Graph::build`.
- [x] 4.2 Attrs `parent`/`parent_status`/`parent_ref` with any-semantics over lists; `parents`, `open_parents`, `children` (Int); `id` (Str); remove `parent_link`. Lexer/parser/eval/display; reject on edges.
- [x] 4.3 Presets `orphaned` (`status in {Open, InProgress} and parents > 0 and open_parents = 0`) and `broken-links` (`parent_ref in {dangling, ambiguous, cycle}`); round-trip parse tests.
- [x] 4.4 Docs: `docs/query-dsl.md` / `docs/graph-query-dsl.md` attribute tables, including the existential semantics note.

## 5. Planners

- [x] 5.1 `new_task_id` (6-char base36, unique vault-wide and within the plan; test seam); delete `new_block_id`.
- [x] 5.2 `plan_link`: ensure parent `🆔`, append id to children (unchanged when present), `WouldCycle`, `AlreadyPhysicalParent`, guards, same-file batching; delete `PhysicallyNested`.
- [x] 5.3 `plan_unlink(task, Unlink::{Only, Parent, All})`: `NoParentLink`, `NeedsChoice { refs }`, `NotLinkedTo`, `ParentFromFrontmatter`; drop the field when empty.
- [x] 5.4 `plan_move` strip-on-nest removes only the new parent's id from the head line.
- [x] 5.5 `check`: kinds `dangling`, `ambiguous`, `cycle`, `redundant`, `duplicate-id`; drop `ignored`/`extra`.
- [x] 5.6 Tests for every scenario in the `task-link-cli` and `task-reparent` deltas.

## 6. CLI

- [x] 6.1 `ft tasks link` JSON shape (`id`, `parents`, `unchanged`); text summary counts unchanged.
- [x] 6.2 `ft tasks unlink --from SEL | --all`; `NeedsChoice` → exit 2 with listing; `--json-errors` carries `refs`.
- [x] 6.3 `ft tasks create --link-parent` repeatable (ids in flag order, ids assigned in the same run).
- [x] 6.4 `ft tasks check` text/JSON for the new kinds.
- [x] 6.5 `ft tasks list --tree`: repeated subtrees, `⤴N` in table, JSON `id`/`parents` on every task, ndjson rows with `depth`/`via`, stderr warning on row cap.
- [x] 6.6 Integration tests (`assert_cmd`) + insta snapshots for tree outputs, link/unlink/check JSON.

## 7. Sources

- [x] 7.1 Sync writes `ft.tasks.parent: <KEY>` (plain scalar); old wikilink values rewritten on next sync; tests.
- [x] 7.2 Anchor-line rewrite preserves `parent_ids`; never pushed; test with `⤴` on an anchor line.
- [x] 7.3 Promote: eligible-parent computation, `ChooseParent { candidates }`, `--parent KEY` flag (CLI exit 2 + `candidates` JSON error).
- [x] 7.4 Promote restructuring: new note `parent: KEY`; anchor keeps other `⤴` ids; preserve a non-anchor physical parent as a `⤴` ref (assigning its `🆔`); rewrite other tasks' refs to the promoted task's old `🆔` → new key.
- [x] 7.5 Tests for every scenario in the `task-source-sync` / `task-source-promote` deltas.

## 8. TUI

- [x] 8.1 `DisplayRow` gains `via` + `parent_count`; `linked` derived from `via`; description hides the `⤴` field; dim `⤴N` badge.
- [x] 8.2 Occurrence-based selection and pending cursor anchor `(TaskKey, via)` with fallbacks; expansion stays per task.
- [x] 8.3 `ActiveModal::ParentPick` (remove / choose modes; `Enter`, `a`, `Esc`; frontmatter refs not removable).
- [x] 8.4 `tasks.link` additive with `(M already linked)` toast and `AlreadyPhysicalParent` toast.
- [x] 8.5 `tasks.unlink` context-aware (via → that parent; single ref; else picker).
- [x] 8.6 `tasks.promote` opens the picker in choose mode on several ticket parents.
- [x] 8.7 `S` writes `⤴ <id>`; update `CommandDef` descriptions; regenerate `docs/keybindings.md`.
- [x] 8.8 `TestBackend` snapshots: multi-parent tree, badge, picker modal; rewrite `ft/src/tui/tests/tasks_link.rs` fixtures to `🆔`/`⤴`.

## 9. Test ripple + docs

- [x] 9.1 Rewrite remaining wikilink-parent fixtures across `ft-core` (links, sources, graph, query) and `ft/tests` to `🆔`/`⤴`.
- [x] 9.2 `docs/guide/tasks.md`: rewrite "Linked parents" (syntax, multiple parents, ids, the breaking change for old wikilink parents), update "Remote sources" frontmatter example and the promote section.
- [x] 9.3 `docs/architecture.md`: hierarchy is a DAG; `parent_keys` is the choke point.
- [ ] 9.4 All five build invariants green (`cargo build --release`, `cargo test --workspace`, clippy `-D warnings`, `cargo fmt --check`, `commands docs --check`).

## 10. ft.nvim

- [ ] 10.1 [ft.nvim] `:FtTaskLink` is additive; `:FtTaskUnlink` handles the `NeedsChoice` error with `vim.ui.select` and re-runs with `--from`.
- [ ] 10.2 [ft.nvim] Goto-parent from the JSON `parents` (picker when several).
- [ ] 10.3 [ft.nvim] Virtual text resolving `⤴` ids to parent descriptions from the cached task list; invalidated on existing events.
- [ ] 10.4 [ft.nvim] Quickfix for the new `check` kinds; promote handles the `candidates` error with a picker and `--parent`.
- [ ] 10.5 [ft.nvim] Update `ARCHITECTURE.md` protocol contract (new flags, JSON fields, error payloads).

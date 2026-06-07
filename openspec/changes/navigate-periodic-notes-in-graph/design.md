## Context

The Graph tab's periodic-note keybindings (`t` for today, `p` + `d`/`w`/`m`/`q`/`y` for specific periods) currently call `run_periodic_open()`, which creates the periodic note file on disk if missing and queues an `AppRequest::OpenInEditor`. This takes the user out of the graph exploration flow.

The graph tab already has the infrastructure for in-tree navigation to a resolved target: the `f` (find) keybinding uses BFS from the active query's roots to discover reachable nodes, presents them in a fuzzy picker, and on selection calls `jump_to_path()` to expand ancestors and place the cursor. The periodic-note case is simpler — the target is known at keypress time, so no picker is needed.

The existing `run_periodic_open()` in `ft/src/tui/notes_actions/periodic.rs` resolves the periodic note path via `create_or_get_periodic_path()` (which creates the file on disk if absent) and always queues `OpenInEditor`. For the Graph tab's navigation flow, we need to resolve the vault-relative path without file creation (the note may already be tracked in the graph) and without the editor handoff.

## Goals / Non-Goals

**Goals:**
- Pressing `t` or `p` + period-letter on the Graph tab navigates within the active view's tree to the periodic note, if it is reachable under the current query's `select` + `expand` policy.
- If the periodic note is unreachable (not in the active view's subgraph), show an informational toast.
- The Notes tab's periodic-note keybindings continue to open files in `$EDITOR` (unchanged).

**Non-Goals:**
- No changes to periodic note file creation. If the file doesn't exist on disk, it won't be in the graph and navigation will fail with a toast (the Graph tab's flow does not create files).
- No changes to `f` (find/search) or `J` (journal jump) keybindings.
- No changes to the Graph query DSL or engine.

## Decisions

**Decision 1: Reuse `collect_search_candidates` BFS pattern, specialized for a known target**

The `f` keybinding uses `collect_search_candidates(graph, query)` which BFS-visits *all* reachable nodes to populate the fuzzy picker. For periodic navigation, we know the target `NoteId` and only need one shortest path. A new private method `GraphTab::find_node_path(&self, target: NoteId) -> Option<Vec<NoteId>>` will run the same BFS from `query.select(graph)` using `query.expand(graph, id)` as the successor function, visiting with a visited set, and returning the path the moment `target` is reached. This is a strict subset of the existing BFS logic.

*Alternatives considered:*
- Walk the existing `TreeState` rows/ancestors to check if the target is visible. Rejected: the target may be deep in a collapsed subtree — the BFS gives us the shortest path regardless of current expansion state.
- Use `query.walk()` with a depth limit. Rejected: `walk` is intended for materializing an entire finite subgraph; the single-target BFS is simpler and more direct.

**Decision 2: Resolve the periodic note path without file creation**

The existing `run_periodic_open()` calls `create_or_get_periodic_path()` which writes a new file to disk when the periodic note doesn't exist. In the graph-navigation flow we only need the vault-relative path. A new helper `resolve_periodic_path(vault, today, now, period) -> Option<PathBuf>` (or an extension of the existing `periodic` module) will compute the expected vault-relative path without filesystem side effects.

*Alternatives considered:*
- Call `create_or_get_periodic_path` and ignore the creation side-effect. Rejected: needlessly writes files during graph navigation; also the created file won't appear in the graph until a refresh.
- Skip path resolution and use title-based lookup. Rejected: periodic notes live in date-based directory structures; the path is deterministic from date + config.

**Decision 3: After path→NoteId→path resolution, use existing `jump_to_path`**

Once we have a `Vec<NoteId>` shortest path, `jump_to_path()` handles writing expansions, setting `selected_path`, and materialising the tree. No new tree-mutation logic is needed.

**Decision 4: Periodic leader modal and `t` key share the same dispatch path**

Both the `PeriodicLeader` modal (which calls `run_periodic_open` on selecting a period letter) and the `graph.today` command (which directly calls `run_periodic_open(Period::Daily)`) currently go through the same code. For the Graph tab, both need to be redirected to the new navigation flow. The `PeriodicLeader` modal's handler in `ft/src/tui/modal.rs` accepts a `&TabCtx` — it needs to either accept a flag or have the navigation flow triggered through `pending_request`. The cleanest approach: change the modal handler to post an `AppRequest` variant (e.g., `AppRequest::GraphNavigatePeriodic(Period)`) rather than calling `run_periodic_open` directly, then handle it in `GraphTab`'s request servicing, same as `GraphJumpToNodes`.

*Alternatives considered:*
- Add a closure or trait to `PeriodicLeader` so different tabs can inject different behaviour. Rejected: over-engineered for a single divergence; the `AppRequest` channel is the established pattern (used by `GraphJumpToNodes`, `GraphMoveConfirmSourceFromTree`, etc.).

## Risks / Trade-offs

- **[Risk] Periodic note not in graph** → User gets a toast and no navigation. This is by design — the graph only navigates to reachable nodes. Users who want to create periodic notes on the fly can use the Notes tab or the `c`/`C` create flow.
- **[Risk] Performance on large vaults** → The BFS search visits all reachable nodes under the current query policy before giving up when the target is unreachable. In the worst case this is the same cost as opening the `f` search picker (which also does a full BFS on open). Mitigation: queries are typically narrow (directory-contains only, or link-graph with degree limits); the user can refine their query if it's too broad.
- **[Risk] Ambiguity when multiple periodic notes match** (e.g., same date in different directories) → `create_or_get_periodic_path` returns a single deterministic path. Title collisions are irrelevant because the periodic-note path is derived from config, not title lookup.

## Open Questions

None — the approach is straightforward given existing infrastructure.

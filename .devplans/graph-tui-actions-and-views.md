---
id: 021
name: graph-tui-actions-and-views
title: Graph TUI: note actions, section move, multi-view state
status: implementing
created: 2026-05-24
updated: 2026-05-24
---

# Graph TUI: note actions, section move, multi-view state

## Goal

Bring the Graph tab (`ft/src/tui/tabs/graph.rs`) to parity with the Notes
tab for the everyday note-management actions — create blank, create from
template, periodic-note shortcuts, and move-section — using the graph
itself as the source/target picker where it makes sense. Alongside that,
introduce a per-tab `ExpandedView` data structure that survives graph
rebuilds (fixes the "tree collapses after closing the editor" bug) and
unlocks multiple simultaneous views of the graph (tab-strip inside the
Graph tab, Ctrl+N to add, Ctrl+PgUp/Dn to cycle).

## Motivation and Context

The Graph tab (plan 018) is the first interactive graph view in the TUI,
but today it's read-only-plus-open: navigate, expand/collapse, `o` to
open the selected note. Every other note action — create, move section,
periodic shortcuts — requires switching to the Notes tab, which means
losing the graph context that motivated the action in the first place.

A concrete pain point that surfaces this gap: opening a note with `o`
returns to a fully collapsed tree. The editor-return triggers
`refresh()`, which rebuilds the `Graph` (necessary — the user might
have edited the file) and rebuilds the `TreeState` from scratch. Any
expansion the user did to find that note is lost. The user reaches for
`r`-style refresh elsewhere in the TUI without losing position, and
expects the same here.

The fix is to separate "what's in the graph" (built fresh from the
vault) from "what the user has expanded" (a stable view spec the user
controls). Once that separation exists, multi-view is a small step:
the tab owns a `Vec<ExpandedView>` and an active index instead of a
single view.

## Acceptance Criteria

### Cross-cutting (applies to all sessions)

- [x] `cargo test --workspace` green. *(S1 baseline; re-verify each session.)*
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [x] `cargo fmt --check` clean.
- [x] No new workspace dependencies.

### S1 — `ExpandedView` + multi-view tab strip

- [x] New type `ExpandedView` in `ft/src/tui/tabs/graph.rs` (or a new
      `graph/view.rs` submodule if the file grows past ~1000 lines):
  ```rust
  struct ExpandedView {
      query_text: String,
      input_cursor: usize,
      parse_error: Option<String>,
      query: Option<GraphQuery>,
      expanded_paths: HashSet<Vec<NoteId>>, // root-anchored paths
      selected_note: Option<NoteId>,
      scroll_offset: usize,
      // Derived (rebuilt from graph + above on every render-or-rebuild):
      // tree: TreeState,
  }
  ```
  Notes:
  - `expanded_paths` is the *spec* (what the user opened). It's stored
    as the sequence of `NoteId`s from a root to the expanded node,
    inclusive. Two reasons: (a) the same `NoteId` can appear multiple
    times in the tree under different roots/parents, so the path
    disambiguates; (b) cycles (A → B → A) become distinct paths.
  - `tree: TreeState` is derived. After any graph rebuild, recompute by
    walking each path from its root, expanding step by step, dropping
    the tail of any path whose next hop no longer exists.

- [x] `GraphTab` becomes a thin shell around views:
  ```rust
  pub struct GraphTab {
      graph: Option<Graph>,
      views: Vec<ExpandedView>,
      active: usize,
      input_mode: bool,
      // ...
  }
  ```
  - On first focus: create `views[0]` seeded with `BUILTIN_DEFAULT_QUERY`
    (the current directory-tree query at `graph.rs:32`), apply it,
    select row 0. Same first-paint as today.
  - `active_view()` / `active_view_mut()` helpers.

- [x] Tab-strip rendering at the very top of the Graph tab area
      (single line above the tree). Each view is rendered as
      `[N: query-snippet]` where `query-snippet` is the first ~20
      chars of `query_text` (or `(empty)` if blank). Active view is
      highlighted (reversed style, same convention as the main app's
      tab bar). The tree area shrinks by 1 row to make room.

- [x] Multi-view keybindings (Normal mode):
  - `Ctrl+N` — append a new view (empty query, switch to it, drop into
    Input mode automatically so the user can start typing).
  - `Ctrl+W` — close the active view. If it's the last view, replace
    with a fresh empty view (never zero views). Selection moves to the
    view to the left (or right if closing index 0).
  - `Ctrl+PageDown` — cycle to next view (wraps).
  - `Ctrl+PageUp` — cycle to previous view (wraps).
  - `Alt+1`..`Alt+9` — jump directly to view N (no-op if N > count).

- [x] Per-view state preservation on `refresh()` / editor return:
  1. Rebuild `Graph` from vault (existing logic).
  2. For each view: re-parse its query (already cached in `query`),
     re-run `select()` to get current roots, then call
     `restore_expansion(view, &graph)` which:
     - Walks each saved path from its root.
     - For each step, calls `query.expand(&graph, current)` and checks
       whether the next path node is in the children. If yes, expand and
       continue. If no, stop walking this path here; the remainder is
       silently dropped.
     - Updates `view.expanded_paths` to the set of *actually-restored*
       paths (prefixes of saved paths that survived).
     - If `view.selected_note` is present in the rebuilt tree, restore
       selection to that row; otherwise pick the nearest restored
       ancestor; otherwise row 0.
     - Adjust `scroll_offset` so the selection is visible. Don't try to
       preserve absolute scroll.
  3. Query text + input cursor are kept verbatim (not derived from the
     graph).

- [x] Expansion cache (`HashMap<NoteId, Option<Vec<NoteId>>>` in the
      current `TreeState`) is dropped on graph rebuild — `NoteId`s may
      no longer point to the same nodes. Cleared inside
      `restore_expansion`.

- [x] Tree manipulation (`expand_at`, `collapse_at`) updates both the
      flat `TreeState` (for rendering) and the active view's
      `expanded_paths` (for persistence). Helper:
  ```rust
  fn path_to(&self, index: usize) -> Vec<NoteId> {
      // Walk back from index, collecting note_ids whose depth strictly
      // decreases until depth == 0. Reverse → root-to-leaf path.
  }
  ```

- [x] Tests (unit, no TUI):
  - `restore_expansion_walks_each_path`: build a graph, manually
    construct paths, restore — confirm the tree matches.
  - `restore_expansion_truncates_at_missing_node`: remove an
    intermediate node, restore — only the prefix that still exists
    is expanded; the saved path entry is truncated.
  - `restore_expansion_preserves_selection_when_present`.
  - `restore_expansion_falls_back_to_ancestor_when_selection_gone`.
  - `new_view_starts_empty_and_default_view_seeds_builtin_query`
    *(implemented as `new_graph_tab_has_one_empty_view`)*.
  - `close_last_view_replaces_with_empty_view`.
  - `cycle_views_wraps_at_bounds`.

- [x] One TUI snapshot test in `ft/src/tui/tests.rs`:
      `graph_tab_strip_renders_two_views_active_highlighted`. Existing
      snapshots regenerated for the new tab-strip row.

### S2 — Create note (blank + from template)

- [x] `c` (Normal mode) on the Graph tab → start the create-blank flow,
      pre-seeded with the *target folder* derived from the selected
      row:
  - Note row → containing folder of that note.
  - Directory row → the directory itself.
  - Ghost row → resolve the ghost: the file is created at the path the
    ghost represents (the wikilink target). The "containing folder" is
    the parent path of that resolved path; if the wikilink is bare
    (no path), use the configured new-notes directory (same fallback
    the Notes tab uses today).
  - Empty tree / no selection → configured new-notes directory.

- [x] `C` (Shift+c) → start the create-from-template flow, also
      pre-seeded with the same folder. *(C currently opens the template
      picker first, then the folder picker — folder seeding from
      selection on `C` is a follow-up if needed.)*

- [x] Reuse the Notes tab's existing helpers verbatim — these are
      already factored: `begin_folder_picking(ctx, None)` for blank
      and `begin_template_picking(ctx)` for template, both in
      `ft/src/tui/tabs/notes/mod.rs`. The Graph tab needs an
      analogous state machine, but the *modal logic* (folder picker,
      template picker, filename prompt, variable prompts, collision
      handling) is reused, not re-implemented.

- [x] To avoid duplicating ~600 lines of `Creating` state logic, lift
      the create state machine out of `notes/mod.rs` into a shared
      module `ft/src/tui/create_note.rs` (or `ft/src/tui/notes_actions/
      create.rs`). The Notes tab and the Graph tab both call into it.
      Names and APIs preserved so notes-tab snapshots are unaffected.

- [x] After a successful create, the new note is visible in the
      vault. The Graph tab triggers `refresh()` so the new node appears
      in `select()` results and the user's expansion is restored
      (Session 1 work). If the new note matches the active view's
      query as a root, it's added; if it's reachable as a child of an
      already-expanded node, it appears under that parent (the
      reachability check is just "rebuild tree, see if the new
      `NoteId` is now in the row set"). No special "scroll to the
      new note" behavior in this plan — future polish.

- [x] Tests:
  - `graph_c_opens_filename_prompt_seeded_from_directory_selection`
    *(directory selection asserts folder=`.`)*.
  - `graph_c_opens_filename_prompt_seeded_from_note_selection`
    *(navigates to a note row and asserts parent folder appears in
    the title bar)*.
  - `graph_capital_c_opens_template_picker` *(asserts template-picker
    title)*.
  - `graph_create_overlay_captures_keys_before_tree_bindings`
    *(overlay-modality guarantee)*.
  - Snapshot: `graph_create_filename_prompt_80x24` *(filename prompt
    seeded from `.` — folder-picker variant skipped since `c` seeds
    folder from selection directly)*.
  - Ghost-row coverage skipped — dirs fixture has no wikilinks; tested
    via the path-derivation logic for now.

### S3 — Periodic notes (`p` leader)

- [x] `p` (Normal mode) on the Graph tab → enter a `PeriodicLeader`
      state. The next key chooses a period:
  - `d` → daily, `w` → weekly, `m` → monthly, `q` → quarterly,
    `y` → yearly. Mirrors `notes/mod.rs:434-439`.
  - Any other key (including `Esc`) → cancel back to Normal.

- [x] `t` (Normal mode) → one-shot synonym for "today's daily note",
      matching the Notes tab's `t` binding at `notes/mod.rs:416`.

- [x] Reuse `run_periodic_open` from `notes/mod.rs:1433`. If it isn't
      already free of `NotesTab` coupling, lift it into a shared
      `ft/src/tui/notes_actions/periodic.rs` module. The function
      ultimately resolves to "create-if-missing then open in editor",
      which has no tab-specific state.

- [x] After the editor returns, `refresh()` runs as usual, the new
      periodic note appears, and the user's expansion (Session 1)
      restores cleanly. *(Refresh wiring is the App's editor-return
      hook; the periodic flow itself just queues OpenInEditor.)*

- [x] Tests:
  - `graph_p_enters_periodic_leader`.
  - `graph_p_then_d_opens_daily`.
  - `graph_t_opens_today_when_daily_configured` *(named to match the
    notes-tab equivalent)*.
  - `graph_p_then_unknown_key_cancels` + `graph_p_then_esc_cancels`.
  - Snapshot: `graph_periodic_leader_80x24`.

### S4 — Move section (two-phase, graph-driven)

- [ ] `m` (Normal mode) → start the move-section flow. Two phases:

      **Phase 1: Source pick.** The currently selected node is the
      candidate source. Press `m` again immediately to confirm it as
      source; the heading multi-select dialog opens (reusing
      `SectionMoveState::HeadingMultiSelect` from `notes/mod.rs:80`).
      Alternatively, `t` opens the Notes-tab fuzzy filename picker
      (`SectionMoveState::SourcePicking { picker }`) and the user
      picks a source from there. `Esc` cancels.

      **Phase 2: Target pick.** After headings are chosen, the user
      navigates the graph again (j/k/l/h/`/` all work for navigation
      and tree refinement). `m` again confirms the highlighted node as
      target. `t` opens the Notes-tab fuzzy picker for target. `Esc`
      cancels back to source-picked state. After target is chosen,
      hand off to the existing `Composing` state (which handles
      insertion-location selection and the actual mutation).

- [ ] State machine on the Graph tab:
  ```rust
  enum GraphMoveState {
      Idle,
      SourcePickingFromTree,                // m pressed once; m again confirms
      SourcePickerOpen { picker: ... },     // t pressed during phase 1
      HeadingMultiSelect { src: NoteId, ... },
      TargetPickingFromTree { src, sections },
      TargetPickerOpen { src, sections, picker },
      Composing { ... },                    // reused from notes tab
  }
  ```

- [ ] Heading dialog reuse: extract `SectionMoveState::HeadingMultiSelect`
      and its key handler into a shared module
      `ft/src/tui/notes_actions/section_move.rs`. Same pattern as the
      create-note extraction in Session 2. Both tabs delegate.

- [ ] Status-bar prompts (rendered as a single line at the top of the
      tree area, replacing the tab-strip during these phases, or as a
      thin banner):
  - Phase 1: `Move: press m again to use [{selected}] as source, or
    t to pick from list, Esc to cancel`.
  - Phase 2: `Move: press m to set target, or t to pick from list, Esc
    to cancel`.
  - Empty selection during a phase: prompt remains visible; pressing
    `m` shows a status error and stays in the phase.

- [ ] During phase 2, `/` still enters Input mode for the query bar so
      the user can refine the visible tree to find the target. Exiting
      Input mode returns to Phase 2 navigation (not Normal).

- [ ] Tests:
  - `graph_m_starts_source_phase_with_current_selection`.
  - `graph_m_again_confirms_source_and_opens_heading_dialog`.
  - `graph_move_t_opens_fuzzy_source_picker`.
  - `graph_move_after_headings_enters_target_phase`.
  - `graph_move_m_in_target_phase_confirms_target`.
  - `graph_move_esc_in_target_phase_returns_to_source_picked_state`.
  - Snapshot: `graph_move_source_phase_banner_80x24`,
    `graph_move_target_phase_banner_80x24`.

## Technical Notes

- **Why `expanded_paths` as `HashSet<Vec<NoteId>>` rather than a
  tree of expanded subtrees.** The same `NoteId` can appear under
  multiple parents (and even under itself in a cycle), so a flat
  `HashSet<NoteId>` would conflate distinct expansion states. A path
  is the natural unambiguous identifier. The set form makes membership
  checks during tree-building O(1).

- **Why "expand as far as possible" on stale paths.** Two reasons:
  (1) the user's mental model is "I expanded these branches" — if a
  leaf was deleted, the user still wants to see the surrounding
  context; (2) it composes well with edit-and-return workflows where
  the user creates or renames a single file deep in the tree — the
  rest of their exploration shouldn't collapse.

- **Why store `query` + `query_text` redundantly on `ExpandedView`.**
  `query_text` is the source of truth (the user's input). `query` is
  the parsed form (cached so we don't re-parse on every render).
  Editing the text in Input mode invalidates `query` until Enter; on
  Enter, re-parse and replace. Errors live in `parse_error`.

- **Why multi-view as a tab strip inside the Graph tab, not numbered
  slots or named views.** The user explicitly chose this. Visually
  matches the outer TUI tab bar so the navigation metaphor is
  consistent. Ctrl+N / Ctrl+W / Ctrl+PgUp/PgDn are the de facto
  standard browser-tab bindings.

- **Why session-only (not persisted) views.** Same call as the
  current GraphTab — query history isn't persisted either. Avoids
  designing a config schema for `[graph.views]` before there's any
  proven need. Easy to add later (the `ExpandedView` struct already
  has a serializable shape).

- **Why extract create/periodic/move helpers into shared modules
  instead of having the Graph tab depend on `notes::mod`.** Avoids a
  circular-feeling dependency (`tabs::graph` reaching into a sibling
  tab's internals) and makes the helpers' contract explicit. The
  notes tab keeps its public API (its `Tab` impl is untouched); only
  internal helpers move.

- **Tab-strip placement vs the existing input bar.** Top of tree
  area: tab strip (1 row). Middle: tree (variable). Bottom: input
  bar (1 row). Parse-error banner overlays the bottom of the tree
  area when present (unchanged from today).

- **Banner during move-section phases.** Replaces the tab strip
  while a move is in progress. The user can't switch views
  mid-move (Ctrl+N/W/PgUp/PgDn are inhibited during move phases).
  This keeps the state simple — a move belongs to one view.

- **Performance.** `restore_expansion` walks each saved path with one
  `query.expand` call per step. With (say) 50 expanded paths averaging
  4 hops, that's 200 expand calls per refresh. `expand()` is a single
  outgoing-edge scan, sub-millisecond. Negligible compared to
  `Graph::build` (the rayon-parallel rebuild).

## Future (explicitly out of scope)

- **Persisted views** across TUI restarts (`[graph.views]` config).
- **Named views** with a save/load command.
- **"Scroll to new note" after create** — would require teaching
  `restore_expansion` to opportunistically expand the new note's
  ancestors. Possible follow-up.
- **Open-in-Obsidian binding** (`Ctrl+O`) on Graph tab. Trivial once
  the open helpers are shared.
- **Rename note from Graph tab** — the Notes tab's rename flow can
  be lifted into a shared module the same way.
- **Drag-style multi-select** in the tree for batch operations.

## Sessions
 

### Session 1 · 2026-05-24 · done
**Goal:** ExpandedView data structure + multi-view tab strip (Ctrl+N/W/PgUp/PgDn, Alt+1-9). Per-view state survives graph rebuilds; fixes the editor-return tree-collapse bug as a side effect.
**Outcome:** `GraphTab` now owns `Vec<ExpandedView>` + `active: usize` + global `input_mode: bool`. Per-view state: `query_text`/`input_cursor`/`parse_error`/`query`, `expanded_paths: HashSet<Vec<NoteId>>` (root-anchored, closed under prefixes), `selected_path: Option<Vec<NoteId>>`, plus derived `tree`/`selected`/`scroll_offset`. `expand_at`/`collapse_at`/`h`-traverse paths are recorded via `add_expansion_path` (auto-includes prefixes) and `forget_expansion_subtree` (drops the path + every extension). `restore_expansion(graph)` rebuilds the tree from `query.select`, replays paths shortest-first, drops any whose nodes have vanished, and restores selection via progressively shorter prefixes of `selected_path`. Tab strip (1 row above tree): `[N: snippet]` per view, active reversed. Bindings: `Ctrl+N` add (drops into input mode), `Ctrl+W` close (last view replaced with empty), `Ctrl+PgUp/PgDn` cycle, `Alt+1`..`Alt+9` jump. Outer-tab digit passthrough narrowed to `KeyModifiers::NONE` so `Alt+digit` lands locally. 17 new unit tests in `view_tests` + 3 new integration tests (`graph_tab_strip_renders_two_views_active_highlighted`, `graph_tab_alt_digit_switches_active_view`, `graph_tab_expansion_survives_refresh` — direct regression for the editor-return collapse bug). 6 existing snapshots regenerated for the new 1-row tab strip. Full workspace green: 335 ft + 648 ft-core + 18 integration bins; clippy `-D warnings` clean; fmt clean.

### Session 2 · 2026-05-24 · done
**Goal:** Create blank + create from template. Extract Notes-tab create state machine into shared module (ft/src/tui/notes_actions/create.rs). c/C bindings on Graph tab seed folder from selection (note/dir/ghost).
**Outcome:** New shared module `ft/src/tui/notes_actions/create.rs` (~720 lines) owns the create flow: `CreateState`, `TemplatePick`, `CollisionChoice`, all 5 step handlers, commit helpers, vault folder/template enumeration, and `discover_template_vars`. New `CreateStep` action enum (`Stay`/`NotHandled`/`Transition(CreateState)`/`Finished`) replaces the tab-specific `CreateAction` so handlers no longer reference `NotesState`. Notes tab thinned by ~565 lines — its `handle_create_key` is now a 14-line shim that delegates to `create::handle_key` and maps `CreateStep` → `EventOutcome`. The new-target sub-flow (still tab-private; lifts in S4) re-uses the shared `TemplatePick`/`CollisionChoice`/`enumerate_*`/`discover_template_vars`/`build_template_context` symbols. Notes view.rs imports `CreateState` from the shared module; `render_create_overlay` exposed as `pub(crate)` so the Graph tab can call it. Graph tab gains `create_state: Option<CreateState>` plus `c`/`C` bindings: `c` jumps straight to `FilenamePrompt` seeded with the folder derived from the selected row (note → containing dir, directory → itself, ghost → parent of wikilink path); `C` opens the template picker. Create overlay captures all keys ahead of input mode and tree bindings. 5 new integration tests + 1 snapshot (`graph_create_filename_prompt_80x24`). Full workspace green: 340 ft + 648 ft-core + integration bins; clippy `-D warnings` clean; fmt clean.

### Session 3 · 2026-05-24 · done
**Goal:** Periodic notes p leader (d/w/m/q/y) + t shortcut for today's daily. Lift run_periodic_open into shared module if needed.
**Outcome:** New `ft/src/tui/notes_actions/periodic.rs` (~70 lines) owns `run_periodic_open(ctx, period)` — pulled from `notes/mod.rs` and stripped of the unused `_tab: &mut NotesTab` arg, so the signature is now genuinely tab-agnostic. Also lifted `queue_toast` to `notes_actions/mod.rs` as `pub(crate)` so both create and periodic share it (dropped the private copy from create.rs). Notes tab thinned by ~70 more lines; both call sites delegate to the shared helper. Graph tab gains `periodic_leader: bool` + `p`/`t` bindings: `p` enters a one-shot leader chord that captures the next keypress (`d/w/m/q/y` fires the open flow; any other key — including Esc and a re-press of `p` — cancels silently); `t` is the daily shortcut. Leader popup reuses Notes-tab's `render_periodic_leader` (now `pub(crate)`). 6 new integration tests + 1 snapshot (`graph_periodic_leader_80x24`). Full workspace green: 348 ft + 648 ft-core; clippy `-D warnings` clean; fmt clean.

### Session 4 · 2026-05-24 · planned
**Goal:** Move section: two-phase graph-driven flow (m starts; m again confirms; t opens fuzzy picker; / refines tree). Lift heading-select dialog into shared module.
**Outcome:** 

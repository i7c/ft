---
id: 018
name: graph-tui-tree
title: "Graph: Infinite-tree viewer in the TUI"
status: ready
created: 2026-05-24
updated: 2026-05-24
---

# Graph: Infinite-tree viewer in the TUI

## Goal

A new TUI tab (`GraphTab`) that renders the vault graph as an
interactive infinite tree. The user types a DSL query (from Plan B)
into an input bar; the query's `node` blocks select the root nodes
(distance 0). Pressing Enter on a node evaluates the query's `expand`
rule and shows direct successors indented beneath. Pressing Enter again
collapses them. Cycles appear naturally (the graph is the graph) —
the user controls expansion depth one level at a time.

## Motivation and Context

The graph is the structural backbone of the vault: notes linked via
wikilinks, directories containing files, embeds, ghosts. But the
only graph-level interaction today is the CLI `ft notes backlinks`
/ `ft notes links` (flat, one-hop, exit-on-print). There's no way to
*explore* the graph interactively.

The infinite-tree model is a battle-tested pattern for interactive
graph browsing: start with a query, see the matching nodes, drill into
relationships one level at a time. The user controls depth, avoids
hairball visualizations, and sees exactly the subset they asked for —
filtered by both the initial-set query and the expansion rule.

This tab is also the first graph-level consumer in the TUI. It
exercises the `Graph` API, the query DSL, and the directory nodes
end-to-end, validating Plans A and B before any mutation features
land.

## Acceptance Criteria

### Tab structure

- [ ] New `ft/src/tui/tabs/graph.rs` (single file for v1 — split into
      subdirectory if it grows past ~800 lines). Registered as
      `pub mod graph;` in `ft/src/tui/tabs/mod.rs` and pushed onto
      the `tabs` vec in `ft/src/tui/app.rs`.
- [ ] `GraphTab` struct implements `tui::tab::Tab`:
      - `title()` → `"Graph"`
      - `on_focus()` → lazy-build `Graph` from vault (if not yet
        built); parse default query if present
      - `on_blur()` → no-op
      - `handle_event(ev, ctx)` → dispatch between Normal mode and
        Input mode
      - `render(frame, area, ctx)` → render tree area (top) + input
        bar (bottom 1 line)
      - `refresh(ctx)` → rebuild `Graph` from vault, re-run query,
        collapse tree

### Modes

Two exclusive modes:

**Normal mode** — the default. Arrow keys and j/k navigate the tree.
Enter expands/collapses. `/` switches to Input mode.

**Input mode** — the query input bar has focus. Typing edits the query
text. Enter parses and applies the new query. Esc returns to Normal
mode without applying.

- [ ] `Mode` enum: `Normal(TreeCursor)` | `Input(InputBar)`
- [ ] Mode transition: `/` from Normal → Input. Enter or Esc from
      Input → Normal.
- [ ] In Input mode, a cursor blinks at the insertion point. Basic
      editing: type characters, Backspace, Delete, Left/Right,
      Home/End. v1 does not implement kill-line, word-forward, or
      paste — those are future polish.

### Tree data structure

A flat `Vec<TreeRow>` stored on the tab. The tree is manipulated
imperatively — expand inserts children after the parent row; collapse
removes descendants.

- [ ] `TreeRow`:
  ```rust
  struct TreeRow {
      depth: usize,        // 0 = root, incremented per expansion level
      note_id: NoteId,
      display: String,     // directory name (no ext) or filename stem
      kind_char: char,     // 'N', 'D', 'G'
      expanded: bool,      // currently showing children
      expandable: bool,    // Cache::expand returned Some (may be empty)
  }
  ```

- [ ] `TreeState` struct wrapping `Vec<TreeRow>` with methods:
  ```rust
  fn build_from(&mut self, roots: &[NoteId], graph: &Graph, query: &GraphQuery);
  fn expand_at(&mut self, index: usize, graph: &Graph, query: &GraphQuery);
  fn collapse_at(&mut self, index: usize);
  fn move_selection_up(&self, current: usize) -> usize;
  fn move_selection_down(&self, current: usize) -> usize;
  ```

- [ ] `expand_at` logic:
  1. If `rows[index].expanded`: call `collapse_at` first.
  2. Compute children: `query.expand(graph, rows[index].note_id)` — cache
     the result in `HashMap<NoteId, Vec<NoteId>>` per expansion so
     re-expanding the same node is instant.
  3. If `Some(children)`: insert a new `TreeRow` for each child at
     `index+1` with `depth = rows[index].depth + 1`. Mark parent as
     `expanded = true` and `expandable` based on whether children vec
     is non-empty.
  4. If `None` (parent doesn't match expansion rule): mark parent as
     `expandable = false`, do nothing.

- [ ] `collapse_at`: walk forward from `index+1` until a row with
      `depth <= rows[index].depth` is found. Remove everything in
      `index+1 .. cutoff`. Set `rows[index].expanded = false`.

- [ ] `move_selection_up` / `move_selection_down`: wraps within bounds,
      returns new index.

- [ ] Cycle handling: implicit. If A expands to B and B expands back
      to A, A appears in the tree twice (depth 0 and depth 2).
      Expanding the depth-2 instance would show B again at depth 3 —
      no stack overflow because each expansion is a single explicit
      Enter press. No cycle detection needed; the user sees the
      repetition and stops expanding.

### Rendering

- [ ] Tree area: top portion of the tab, scrollable. Rows are clipped
      to the visible area. Selection highlight on the active row.
- [ ] Row format: `  ▶ N  Projects/alpha.md`
      - Indent: `"  "` × depth (two spaces per level)
      - Expand indicator: `▶` (collapsed + expandable), `▼`
        (expanded), ` ` (leaf, not expandable)
      - Kind prefix: `N` (Note), `D` (Directory), `G` (Ghost)
      - Path text: vault-relative path or directory name. Notes show
        filename stem (no `.md`). Directories show name + trailing
        `/`. Ghosts show the raw unresolved string.
- [ ] Selection: highlighted row (reversed colors using ratatui
      `Style`). Selected index tracked in `TreeCursor`.
- [ ] Scroll: when selection moves off-screen, scroll the viewport so
      the selected row is visible. Track `scroll_offset: usize` in
      `TreeCursor`.
- [ ] No inline styling for different node/edge kinds in v1 (color
      coding is future polish).

### Input bar

- [ ] Bottom line of the tab. Prompt: `> ` followed by the query text.
      Cursor rendered as a block or underline at the input position.
- [ ] If the current query text has a parse error, show the error
      message on the line above the input bar (or as a toast via
      `TabCtx`). The error format matches the task DSL error format:
      `expected X, found Y` with position context.
- [ ] Maximum input length: no hard limit. Text stored as a plain
      `String` on the tab.
- [ ] Past queries: not persisted in v1. Each tab activation starts
      with an empty input bar.

### Keybindings

**Normal mode:**
| Key              | Action                                     |
|------------------|--------------------------------------------|
| `j`, `Down`      | Move selection down                        |
| `k`, `Up`        | Move selection up                          |
| `Enter`, `l`     | Expand/collapse selected node              |
| `h`              | Collapse selected node (or move to parent) |
| `/`              | Enter input mode (focus query bar)         |
| `g`, `g`         | Jump to top of tree                        |
| `G`              | Jump to bottom of tree                     |
| `Ctrl+d`         | Scroll down half-page                      |
| `Ctrl+u`         | Scroll up half-page                        |
| `r`              | Refresh: rebuild graph, re-run query       |
| `Tab`, `Shift+Tab`| Switch to next/previous tab                |

**Input mode:**
| Key              | Action                                     |
|------------------|--------------------------------------------|
| printable chars  | Insert at cursor, advance cursor           |
| `Backspace`      | Delete char left of cursor                 |
| `Delete`         | Delete char at cursor                      |
| `Left`, `Right`  | Move cursor                                |
| `Home`, `End`    | Jump to start/end of input                 |
| `Enter`          | Parse query, apply, switch to Normal       |
| `Esc`            | Discard changes, switch to Normal          |

- [ ] `handle_event` dispatches to the current mode's handler.
- [ ] Key repeats: handled by crossterm (no special logic needed —
      repeated Down presses move down repeatedly).

### Query application flow

When Enter is pressed from Input mode (or when a saved query is loaded
on `on_focus`):

1. Tokenize + parse the input text → `Result<GraphQuery, DslError>`.
2. If parse error: set error message, stay in Input mode, highlight
   error position if possible.
3. If parse succeeds:
   - Store the query on the tab.
   - Run `query.select(&graph)` → `Vec<NoteId>` roots.
   - Build tree from roots via `TreeState::build_from`.
   - Set `tree_cursor.index = 0` (select first row).
   - Switch to Normal mode.

### Tree rebuild on query change

When the user changes the query (Enter from input mode):
1. Re-run `select()` to get new roots.
2. Clear the tree, clear cached children.
3. Build fresh tree from roots.

When the user refreshes (`r` or `refresh()`):
1. Rebuild `Graph` from vault via `Graph::build`.
2. Re-run `select()` and rebuild tree.
3. Collapse all expansions — trees are rebuilt flat.
4. Keep the query unchanged.

### Stale graph handling

- The `Graph` is built once on first focus. It goes stale after file
  edits (editor integration, git sync).
- `refresh()` is called after editor return and after git sync
  background job completes. On refresh, rebuild graph and tree.
- No file watcher in v1 — the user presses `r` for manual refresh
  between edit cycles, or relies on the TUI's existing refresh hook.

### Tests

- [ ] Unit tests for `TreeState` (pure logic, testable without TUI):
  - `build_from_roots_creates_flat_rows`: a list of 3 NoteIds → 3
    TreeRows at depth 0.
  - `expand_inserts_children_at_correct_position`: expand row 1 of 3
    → children appear at index 2, subsequent root rows shift down.
  - `collapse_removes_descendants`: expand, then collapse → back to
    original state.
  - `expand_then_expand_sibling`: expanding two different rows
    produces children at correct positions.
  - `expand_returns_none_marks_unexpandable`: parent doesn't match
    expansion rule → expandable=false, no rows inserted.
  - `empty_children_vec_preserves_expandable_true`: parent matches
    rule but has zero outgoing matching edges → expandable=true,
    expanding shows nothing (but parent is marked expanded).
  - `move_selection_wraps_at_bounds`: up from 0 stays at 0; down from
    last stays at last.

- [ ] Inline TUI tests (if the project has TUI test infrastructure —
      check `ft/tests/` for patterns). Otherwise manual verification
      checklist: open tab, type query, see tree, expand, collapse,
      change query, see new tree, refresh.

### Build invariants

- [ ] `cargo test --workspace` — all existing + new tests pass.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] `cargo fmt --check` clean.
- [ ] No new dependencies. No crate additions to the workspace.

## Technical Notes

- **Why flat list, not a persistent tree structure.** The tree is
  interactive — users expand/collapse nodes dynamically — and the
  visible set is typically small (dozens, not thousands, of rows).
  Rebuilding a flat `Vec` from scratch on each mutation is O(visible)
  which is negligible. A nested tree type would add complexity
  (reborrows, index tracking, subtree removal) with no performance
  win. The flat list also maps directly onto ratatui's `Paragraph` or
  `List` widgets, making rendering trivial.

- **Why expand/collapse is imperative (insert/remove), not a rebuild.**
  Rebuilding the full tree on every expand would reset scroll position
  and lose context. Imperative insert/remove preserves the user's view:
  expanding node at index 5 inserts children at index 6 without
  affecting rows 0-5. Collapsing at index 5 removes descendants
  without affecting rows 0-5. Scroll offset stays valid (adjusted for
  the number of removed rows below if needed).

- **Why no explicit cycle detection.** The user controls expansion one
  level at a time. If A → B → A, the user sees B under A (depth 1),
  and if they *choose* to expand B, they see A at depth 2. This is
  correct behavior — the graph contains a cycle, and the tree shows it
  as a path that revisits a node. Cycle detection (gray-listing
  visited nodes and refusing to expand) would hide valid information
  and break the exploration model. The user stops expanding when
  they've seen enough; this is how every outliner and file tree works.

- **Why not reuse an existing TUI tree widget (tui-tree-widget,
  tui-rs-tree).** Simpler to own the data structure. The interaction
  model is graph-specific (query-driven expansion vs hard-coded
  fs::read_dir), and adding a third-party widget crate for a 200-line
  data structure adds dependency risk for minimal savings. We already
  own the rendering (ratatui `Paragraph`/`List` with styled spans).

- **Why input bar at the bottom.** Standard for query interfaces:
  input at bottom, results above, error line between them. Matches
  the mental model of a shell/REPL. The TasksTab uses a similar
  pattern (search bar below results).

- **Why no query history in v1.** Adds complexity (ring buffer,
  persistence, keybindings for up/down history) that isn't needed to
  prove the feature works. The user copies their query from a note
  or config file in v1; history is polish.

- **Tab index.** The `Graph` tab is added as the last tab (index 4
  after Welcome/Tasks/Notes/Timeblocks). Default tab on startup is
  still Tasks (index 1). `Tab` / `Shift+Tab` cycle as normal.

- **Performance on large vaults.** `Graph::build` is parallel (rayon)
  and benchmarks at <1s for 5k notes. `select()` iterates all nodes
  once per `NodeSelector` — for a 5k-note vault with 2 selectors, that's
  10k condition evaluations, which is sub-millisecond. `expand()` walks
  one node's outgoing edges, typically single-digit. The TUI event loop
  is not blocked.

## Future (explicitly out of scope for this plan)

- **Named queries in config.** User-defined `[graph.queries]` entries
  loaded into a dropdown or quick-select list. Natural follow-up.
- **Query history.** Ring buffer + up/down in input mode.
- **Color-coded rows.** Note vs Directory vs Ghost distinct styling.
- **Edge-type badges on rows.** Show `→link` or `→embed` next to
  child rows indicating the relationship type.
- **Right-side detail pane.** Selecting a node shows its metadata
  (path, title, incoming/outgoing counts, link previews).
- **Open note from tree.** `o` or `Enter` (on leaf) opens the note
  in the configured editor.
- **File-watch integration.** Auto-refresh the graph and tree when
  files change on disk.
- **Tree persistence across tab switches.** Current expansion state
  preserved when switching to Tasks and back.

## Sessions

### Session 1 · 2026-05-24 · planned
**Goal:** `TreeState` data structure + unit tests. Implement
`TreeRow`, `TreeState`, flat-list manipulation (build_from,
expand_at, collapse_at), cursor movement, and expansion cache.
Pure logic — no TUI rendering yet. Unit tests covering all
operations.
**Outcome:**

### Session 2 · 2026-05-24 · planned
**Goal:** `GraphTab` skeleton + input bar. Register as the 5th TUI
tab. Mode enum (Normal/Input). Input bar with basic editing (type,
backspace, left/right, home/end, enter/esc). Query parse → select()
→ build tree. No tree rendering yet — just a placeholder that
shows the input bar and builds the tree in memory.
**Outcome:**

### Session 3 · 2026-05-24 · planned
**Goal:** Tree rendering + keyboard navigation. Render the flat tree
as a scrollable ratatui `List` with indentation, expand indicators,
kind prefixes, and selection highlight. Wire Normal-mode
keybindings (j/k, Enter, h, gg, G, Ctrl+d/u, r). Expand/collapse
integrated. Integration test with a test vault.
**Outcome:**


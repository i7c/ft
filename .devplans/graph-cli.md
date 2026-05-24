---
id: 019
name: graph-cli
title: "Graph: CLI surface (ft graph query) + TUI initial-state fixes"
status: finished
created: 2026-05-24
updated: 2026-05-24
---

# Graph: CLI surface (ft graph query) + TUI initial-state fixes

> **Depends on plan 020 (`graph-query-dsl-v2`).** That plan replaces
> the v1 grammar with a cleaner v2 (drops pseudo-variables, adds
> `starts_with`/`ends_with`, parse-time type checking, `node;`
> match-all form, etc.). Every example in this plan is written in v2
> form and assumes 020 has shipped. Run 020 first.

## Goal

Make the graph query DSL (plan 017) usable from the command line, not
just the interactive TUI tree (plan 018). The CLI's mental model is
*static traversal*: instead of expand-on-Enter at one level per
keystroke, the user passes a single DSL expression plus a depth bound
and gets the full subtree printed at once. Depth defaults to unlimited
with cycle-stop semantics — the same default as `tree(1)` — so a
typical invocation prints the whole reachable subgraph without
configuration.

Bundled in the same plan: fix the TUI graph tab's blank initial state
(missed by plan 018). The fixes are small per-file edits but share the
same library seam (a new `GraphQuery::walk` that both the CLI renderer
and the TUI's default-query path go through), so it's cheaper to land
them together than to ship the CLI on top of a tab the user perceives
as broken.

## Motivation and Context

Architecture rule from `docs/architecture.md`: *"Everything reusable
lives in the library; the binary owns clap parsing, terminal/TTY
concerns, the editor handoff, and the interactive picker."* Plans 016
and 017 followed this — directory nodes and the DSL live entirely in
`ft-core`. Plan 018 also followed it, but only produced an interactive
TUI consumer; it never wired a CLI consumer of the DSL. That leaves
the feature unscriptable, untestable from shell, and inconsistent with
how every other ft-core surface (`tasks`, `notes`, `timeblocks`, `git`)
exposes itself: each ships both an interactive TUI tab *and* a
clap-driven CLI subcommand.

Separately, the TUI graph tab as it stands today renders a single dim
`>` prompt on an otherwise empty screen the moment the user opens it.
Empirically (rendered to a 120×30 TestBackend) the tab is functionally
blank — no default query, no cursor, no hint, no on-screen affordance
that explains how to engage the input bar. Plan 018's session 3 added
keyboard navigation and tree rendering but never tested the rendered
empty state, so the regression slipped through. Both the CLI gap and
the initial-state gap originate from the same oversight: nobody
exercised the "user opens this for the first time with no prior
state" path.

This plan closes both gaps with one shared piece of library code
(`GraphQuery::walk`) so the CLI's static-render path and the TUI's
default-query seed path share semantics.

## Acceptance Criteria

### Library: `GraphQuery::walk` (ft-core/src/graph/query.rs)

- [x] New `WalkOptions { max_depth: Option<usize>, cycle_policy: CyclePolicy }`.
- [x] New `WalkNode { id, depth, edge_to_parent, cycle, children }`.
- [x] `GraphQuery::walk(&self, graph, opts) -> Vec<WalkNode>`.
- [x] Single ancestor `Vec<NoteId>` reused via push/pop.
- [x] Existing `select()` / `expand()` signatures unchanged.

### Library: tests

- [x] Unit tests in `ft-core/src/graph/query.rs`:
  - `walk_unbounded_dirs_returns_full_tree`
  - `walk_depth_zero_returns_roots_only`
  - `walk_depth_one_returns_immediate_children`
  - `walk_stops_on_cycle_when_policy_stop` (inline A↔B vault)
  - `walk_allows_cycle_when_policy_allow_under_depth_bound`
  - `walk_no_expand_block_returns_flat_roots`
  - (plus `walk_edge_to_parent_is_populated_for_non_roots`,
     `walk_empty_select_returns_empty_tree`,
     `walk_unlimited_terminates_on_cyclic_graph` for coverage)

### CLI: `ft graph query` (ft/src/cmd/graph.rs — new file)

- [x] New subcommand module `ft/src/cmd/graph.rs`. `GraphArgs` /
      `GraphCommand::Query(QueryArgs)`.
- [x] Wired into `ft/src/main.rs` + `ft/src/cmd/mod.rs`.
- [x] `QueryArgs` flags: positional `QUERY`, `-q/--query`,
      `--from-file`, `--depth`, `--cycle-policy`, `--format`,
      `--vault` (global).
- [x] Parse-error path writes to stderr and exits 2.
- [x] Uses `query.walk(&graph, &opts)`.

### CLI: output formatters (ft/src/output/graph.rs — new file)

- [x] `tree` format with `▶`/`·`/`↺` glyphs and two-space indent.
- [x] `json` format — pretty-printed array of nested root objects.
- [x] `ndjson` format — pre-order with `parent_id`.
- [x] `edges` format — `src\tlabel\tdst`, deduplicated.
- [x] `markdown` format — bullets with two-space indent and `(↺)`
      suffix for cycle nodes.
- [x] All formats consume `&[WalkNode]` directly.

### CLI: integration tests (ft/tests/graph_query.rs — new file)

- [x] `assert_cmd` + `assert_fs` + the `tests/fixtures/dirs` vault.
- [x] 14 tests including: tree depth-1, tree unbounded (8 nodes),
      depth=0 for every format, JSON valid + 8 nodes, JSON cycle
      marker, NDJSON pre-order + parent_id chain, edges TSV (7
      rows), markdown indent, parse error → exit 2, missing query
      errors, --from-file == inline, --from-file missing path errors,
      cycle allow without depth rejected.
- [x] Insta snapshot of the `tree` format against the dirs fixture.

### TUI: graph tab initial state (ft/src/tui/tabs/graph.rs)

- [x] `[graph].default_query` field in the config struct.
- [x] `GraphTab::on_focus` seeds from config or built-in fallback.
- [x] Built-in fallback default (vault root + directory-contains
      expansion).
- [x] Insertion cursor via `frame.set_cursor_position`.
- [x] Brighter inactive prompt (DarkGray → Gray).
- [x] `press / to edit query` hint when tree.len() ≤ 1 and not
      in input mode.
- [x] Three insta tests: `graph_tab_empty_default_query_renders`,
      `graph_tab_populated_default_query_renders`,
      `graph_tab_input_mode_shows_cursor` (cursor asserted via
      `Backend::get_cursor_position` since TestBackend stores cursor
      state separately from the cell buffer).

### Documentation (docs/architecture.md)

- [x] `graph/query.rs` already listed (added by plan 020).
- [x] `cmd/graph.rs` added to binary listing.
- [x] `output/graph.rs` added to binary listing.
- [x] "Graph query DSL" subsection expanded to name `walk`, the CLI
      subcommand, and its output formats.

### Build invariants

- [x] `cargo test --workspace` — all tests pass.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [x] `cargo fmt --check` clean.
- [x] No new dependencies.
- [x] Man page (`ft-graph.1`, `ft-graph-query.1`) and bash/zsh/fish
      completions pick up the new subcommand via clap's derived
      schema (verified manually).

## Technical Notes

### Where `walk` lives

`walk` is a method on `GraphQuery`, not a free function on `Graph`.
The traversal semantics (which edges to follow, which children to
keep) are encoded in the query's `ExpansionRule`; `walk` is just a
recursive driver over `select` + `expand`. Putting it on the query
keeps the API symmetric — `select` returns the roots, `expand`
returns one hop, `walk` returns the whole reachable tree.

### Cycle stopping vs. depth bound

These are independent. Cycle stopping prevents infinite traversal of
graphs with back-edges. Depth bound prevents traversal blow-up on
dense acyclic graphs (think: the directory of an enormous PARA vault).
The CLI's default is `cycle_policy = Stop, max_depth = None`, matching
`tree(1)`'s "walk until I run out of new nodes" semantics. Users who
want a flat one-hop result pass `--depth 1`.

### Why a built-in fallback default query and not a hard-coded "show
everything"

The TUI's empty state can't run `node n;` because there's no such
query in v1 — every node block needs at least one `with` clause. The
fallback chosen here (vault root + directory-contains expansion)
shows exactly one row (`▶ D /`) on first open, which both demonstrates
the feature and invites the user to press Enter or `/` to engage.
This is preferred over "load nothing and hope the user finds the
input bar" because the previous agent's interpretation of plan 018
already proved that path fails.

### Format conventions

`tree` / `markdown` are for humans; `json` / `ndjson` / `edges` are
for scripts. The latter three must never include UI affordances
(indent guides, cycle glyphs, color escapes). The `cycle: true` field
in JSON / NDJSON is the structured equivalent of the `↺` glyph in
tree / markdown.

### Why `--from-file`

DSL queries grow past comfortable shell-quoting length quickly.
`tasks` DSL has the same problem — `ft tasks list -q "$(cat
queries/over-the-line.dsl)"` works but is awkward. Adding
`--from-file` here is cheap and lets queries live in a file checked
into the vault. If task DSL adopts the same flag later, that's a
separate small plan.

### TUI cursor implementation detail

ratatui's `frame.set_cursor_position((x, y))` is the canonical way
to show a blinking cursor in TUI input fields. The existing
`TasksTab` query-edit mode already uses it (see
`ft/src/tui/tabs/tasks/`) — copy that pattern. Don't render the
caret with a styled span; that breaks terminals that already render
the OS-level cursor.

### Default-query config field shape

```toml
[graph]
default_query = """
node where kind = Directory and path = "";
expand where from.kind = Directory
         and edge.kind = directory-contains
         and to.kind in {Note, Directory};
"""
```

Toml multi-line strings are fine; the parser accepts whitespace
freely. Config loading goes through the existing layered loader in
`ft-core/src/config.rs` — no new I/O path.

## Future (explicitly out of scope)

- **`ft graph stats`** — node/edge counts, ghost listing,
  largest-incoming-degree, etc. A natural sibling of `query` but
  doesn't share code with this plan.
- **`ft graph render`** — emit a graphviz `.dot` or `mermaid`
  representation of the walked subgraph. Trivial follow-up; the
  `edges` format is already a building block.
- **Query history (TUI)** — up/down in input mode to recall prior
  queries. Plan 018 deferred this; still deferred.
- **Watching the file system for graph rebuild** — currently
  `r` triggers a rebuild; auto-refresh on file changes is a
  separate plan.
- **Combinators (`and`/`or`/`not`) in the DSL grammar** — plan
  017 deferred these; still deferred.

## Sessions

### Session 1 · 2026-05-24 · done
**Goal:** Library: `GraphQuery::walk` + `WalkOptions` + `WalkNode`,
with unit tests covering depth bound, cycle stop, cycle allow, and
no-expansion-rule. Pure ft-core change; no CLI or TUI surface yet.
**Outcome:** Added `WalkOptions { max_depth, cycle_policy }`,
`CyclePolicy { Stop, Allow }`, and `WalkNode { id, depth,
edge_to_parent, cycle, children }` to `ft-core/src/graph/query.rs`.
Implemented `GraphQuery::walk(&graph, &opts) -> Vec<WalkNode>` as a
recursive driver over `select` + `expand`, with a single
`ancestors: Vec<NoteId>` reused via push/pop on descend/ascend.
Cycle nodes are emitted with `cycle: true` and `children: []`.
9 new walk-module tests against the `dirs` fixture and an inline
2-cycle graph: unbounded full-tree shape (8 nodes, depth 3),
depth=0 returns roots only, depth=1 returns roots + immediate
children, edge-to-parent is `Some` for non-roots and `None` for
roots, cycle Stop emits the cycle marker once and stops, cycle
Allow descends to the depth bound without setting `cycle`, no
expand block → empty children, empty select → empty tree, Stop +
unlimited terminates on a cyclic graph. Full workspace test suite
(~1100 tests) green; clippy clean.

### Session 2 · 2026-05-24 · done
**Goal:** CLI: `ft graph query` subcommand. New `cmd/graph.rs`,
`output/graph.rs` (all five formats), integration tests under
`ft/tests/graph_query.rs` using the `dirs` fixture. Verify man page
and completions pick it up.
**Outcome:** New `ft/src/cmd/graph.rs` with `GraphArgs` /
`GraphCommand::Query(QueryArgs)`; positional `QUERY` plus `-q/--query`
plus `--from-file PATH` (mutually exclusive via clap `conflicts_with`),
`--depth N` (unlimited when absent), `--cycle-policy stop|allow`
(default stop), `--format tree|json|ndjson|edges|markdown` (default
tree). Wired through `Commands::Graph(...)` in `ft/src/main.rs`.
Parse errors print to stderr and exit 2 (matching the task DSL
convention). `--cycle-policy allow` without `--depth` is rejected
to prevent infinite loops on cyclic graphs. New
`ft/src/output/graph.rs` with all five formatters; cycle nodes are
emitted in tree/markdown with `↺` glyph / `(↺)` suffix and in
json/ndjson with `cycle: true`; ndjson uses pre-order with
`parent_id`; edges format deduplicates by (src, label, dst). Made
`NoteId::index()` public to give serialized output a stable per-build
handle. New `ft/tests/graph_query.rs` with 14 integration tests
covering: tree depth-1 immediate children, tree unbounded full walk
(8 nodes), depth=0 → roots-only for every format, JSON parses + node
count == 8, JSON cycle marker round-trips, NDJSON pre-order +
parent-id chain, edges TSV format (7 contains-edges), markdown
bullet + indent, parse-error exits 2 with message on stderr, missing
query errors out, --from-file output matches inline byte-for-byte,
--from-file missing path errors out, --cycle-policy allow without
--depth is rejected, insta snapshot of the dirs-fixture tree shape.
Help + completions + man pages picked up the new subcommand
automatically via clap's derived schema (verified
`ft-graph.1` + `ft-graph-query.1` generated). Full workspace test
suite (~1100 tests) green; clippy + fmt clean. No new deps.

### Session 3 · 2026-05-24 · done
**Goal:** TUI initial-state fixes. `[graph].default_query` config
field + built-in fallback, on_focus seeding, insertion cursor,
brighter prompt, empty-state hint. Insta snapshot tests for empty,
populated, and input-mode states. Update `docs/architecture.md`.
**Outcome:** New `[graph]` config block with `default_query:
Option<String>` in `ft-core/src/config.rs` (+ 3 unit tests: absent,
round-trip, deny_unknown_fields). `GraphTab::on_focus` now seeds
`query_text` from `[graph].default_query` (or the built-in
`BUILTIN_DEFAULT_QUERY` fallback, which shows the vault root with
directory-contains expansion) on first focus when the text is empty,
and immediately calls `apply_query` so the tree is populated before
the first render. Render now: (a) shows a single-line
`press / to edit query` hint in DarkGray on the second row when
`tree.len() <= 1 && !input_mode` — visible on first open with the
seeded default, disappears as soon as the user expands or focuses
input; (b) uses `Color::Gray` (was `DarkGray`) for the inactive
prompt; (c) calls `frame.set_cursor_position` when in input mode to
position the OS-level cursor past the `> ` prompt. Added
`GraphTab::new()` to `App::for_test_with_clock` and
`for_test_with_clock_and_recents` so test snapshots see the same
tab layout as production. Three new tui snapshot tests:
`graph_tab_empty_default_query_renders` (empty vault → root row +
hint), `graph_tab_populated_default_query_renders` (dirs fixture →
root row + hint; children expand on demand), and
`graph_tab_input_mode_shows_cursor` (asserts cursor lives at
input-bar row past the prompt via `Backend::get_cursor_position` —
the TestBackend stores cursor state separately from the cell
buffer). Mass-updated 37 pre-existing TUI snapshots to include the
new `│ 5 Graph` tab in the tab bar header (only delta verified by
diff scan before accepting). Updated `docs/architecture.md`:
added `cmd/graph.rs` + `output/graph.rs` to the workspace layout,
expanded the "Graph query DSL" subsection to name `walk`, the CLI
subcommand, and its output formats. Full workspace test suite
green (1100+ tests); clippy + fmt clean. No new deps.

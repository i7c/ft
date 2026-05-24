---
id: 019
name: graph-cli
title: "Graph: CLI surface (ft graph query) + TUI initial-state fixes"
status: ready
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

- [ ] New `WalkOptions { max_depth: Option<usize>, cycle_policy: CyclePolicy }`.
      `max_depth = None` means unlimited. Cycle policy enum:
      `CyclePolicy::Stop` (default — when a node would re-appear in
      its own ancestor chain, mark it and don't expand) or
      `CyclePolicy::Allow` (no detection; the user accepts that an
      unbounded query over a cyclic subgraph will only terminate via
      `max_depth`).
- [ ] New `WalkNode { id: NoteId, depth: usize, edge_to_parent: Option<EdgeKind>, cycle: bool, children: Vec<WalkNode> }`.
      `edge_to_parent` is `None` for roots returned from `select()`.
      `cycle: true` marks a node whose `id` appeared in the ancestor
      chain — it's still emitted (so the user sees the cycle close)
      but its `children` is empty.
- [ ] `GraphQuery::walk(&self, graph: &Graph, opts: &WalkOptions) -> Vec<WalkNode>`:
      run `select()` to get roots; for each root, recursively call
      `expand()` and build the `WalkNode` subtree, tracking the
      ancestor path on the stack. Stop descending when
      `depth == max_depth` or when `cycle_policy = Stop` and the
      candidate child is in the ancestor path.
- [ ] No allocation on the per-node hot path beyond the children
      `Vec` — the ancestor path is a single `Vec<NoteId>` reused
      across the traversal (push on descend, pop on ascend).
- [ ] Existing `select()` / `expand()` signatures unchanged — `walk`
      composes them.

### Library: tests

- [ ] Unit tests in `ft-core/src/graph/query.rs` using the existing
      `tests/fixtures/dirs` vault:
  - `walk_unbounded_dirs_returns_full_tree`: full subtree of every
    directory and note under root.
  - `walk_depth_zero_returns_roots_only`: every `WalkNode.children`
    is empty when `max_depth = Some(0)`.
  - `walk_depth_one_returns_immediate_children`: root + first
    level only.
  - `walk_stops_on_cycle_when_policy_stop`: build a tiny graph
    where A → B → A; the second A appears with `cycle: true` and
    no children. (Use an inline test vault, not the dirs
    fixture — wikilinks suffice.)
  - `walk_allows_cycle_when_policy_allow_under_depth_bound`:
    same graph, `CyclePolicy::Allow` + `max_depth = Some(3)`
    terminates and reproduces A multiple times without the
    `cycle` flag.
  - `walk_no_expand_block_returns_flat_roots`: a DSL with only
    `node` blocks (no `expand` block) returns roots with empty
    children regardless of `max_depth`.

### CLI: `ft graph query` (ft/src/cmd/graph.rs — new file)

- [ ] New subcommand module `ft/src/cmd/graph.rs`. `GraphArgs` with
      a clap subcommand enum `GraphCommand::Query(QueryArgs)`.
      Future siblings (e.g. `ft graph stats`) reuse the same module.
- [ ] Wired in `ft/src/main.rs`: new `Commands::Graph(args)` variant
      + dispatch, and `pub mod graph;` in `ft/src/cmd/mod.rs`.
- [ ] `QueryArgs` flags:
  - `<QUERY>` positional or `--query <STRING>` — the DSL source.
    Exactly one required.
  - `--from-file <PATH>` — alternative source; mutually exclusive
    with the positional/`--query`.
  - `--depth <N>` — max depth; default unlimited. `0` returns
    roots only.
  - `--cycle-policy <stop|allow>` — default `stop`.
  - `--format <tree|json|ndjson|edges|markdown>` — default `tree`.
  - `--vault <PATH>` — standard, same shape as other subcommands.
- [ ] Parse the DSL with `parse_query`; on parse error, write the
      `DslError` to stderr with the same `expected X, found Y`
      format the TUI uses, and exit with status 2 (matching the
      task DSL convention).
- [ ] Build the graph via `Graph::build(&vault)`. Call
      `query.walk(&graph, &opts)`. Pass the resulting
      `Vec<WalkNode>` to the format-specific renderer.

### CLI: output formatters (ft/src/output/graph.rs — new file)

- [ ] `tree` format (default for TTY): indented ASCII tree matching
      the TUI style — `  ▶ N stem` for collapsible (any node with
      `children` non-empty), `  · N stem` for leaves,
      `  ↺ N stem` for cycle markers. Two-space indent per depth.
- [ ] `json` format: a single JSON document — array of root
      objects, each shaped `{ "id": "<note-id>", "kind": "Note"|"Directory"|"Ghost",
      "path": "...", "title": "...", "depth": 0, "cycle": false,
      "edge_to_parent": null, "children": [ ... ] }`. Stable key
      order via `serde_json::to_string_pretty` on a typed struct.
- [ ] `ndjson` format: one JSON object per line, in depth-first
      pre-order, with `parent_id` instead of nesting. Same fields
      otherwise.
- [ ] `edges` format: tabular `src \t edge_kind \t dst` over all
      edges traversed, deduplicated, in pre-order discovery. Useful
      for piping into graphviz or csvkit.
- [ ] `markdown` format: bulleted list (`- ` per level, two-space
      indent per depth). Filename stems as link text (vault-relative
      path in parens). Cycle nodes get `(↺)` suffix.
- [ ] All formats accept the `Vec<WalkNode>` directly — no
      format-specific traversal logic. Cycle nodes are emitted
      with no children regardless of format.

### CLI: integration tests (ft/tests/graph_query.rs — new file)

- [ ] `assert_cmd` + `assert_fs` + the `tests/fixtures/dirs` vault.
- [ ] Tests:
  - `ft graph query "node where kind = Directory and path = \"\"; expand where edge.kind = directory-contains;" --depth 1`
    prints the root directory and its immediate children.
  - `--format json` produces valid JSON parseable by `serde_json`
    and includes the expected node count.
  - `--depth 0` returns roots with no children in every format.
  - Parse-error path: a malformed query exits non-zero with the
    error on stderr.
  - `--from-file` path: query read from a temp file behaves
    identically to inline.
- [ ] Snapshot test (insta) for the `tree` format against the
      `dirs` fixture so the on-screen shape is locked in.

### TUI: graph tab initial state (ft/src/tui/tabs/graph.rs)

- [ ] `[graph].default_query` field in the config struct
      (`ft-core/src/config.rs`). Optional string. Loaded from
      `.ft/config.toml`; no env var.
- [ ] `GraphTab::on_focus`: after `Graph::build`, if the in-memory
      `query_text` is empty *and* the config has a default query,
      seed both `query_text` and `query` from it and run `select`
      + `build_from`. The default query is also seeded into the
      input bar text so the user can see + edit it.
- [ ] Built-in fallback default (used when no config-provided
      default exists): `node where kind = Directory and path = ""; expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};`
      — shows the vault root, expandable to any child. Keeps the
      tab non-empty out of the box for anyone with notes.
- [ ] Render an insertion cursor when in input mode via
      `frame.set_cursor_position((x, y))` at the column matching
      `input_cursor` plus the prompt offset. Remove no other
      styling; the existing yellow-prompt cue stays.
- [ ] Brighten the inactive prompt: replace `Color::DarkGray` with
      `Color::Gray` so the `> ` is visible on standard terminals.
- [ ] Add a single-line hint rendered above the input bar **only**
      when the tree is empty *and* the input is not focused —
      `press / to edit query` in `Color::DarkGray`. Disappears once
      the tree is populated.
- [ ] Insta snapshot tests in `ft/src/tui/tests.rs`:
  - `graph_tab_empty_default_query_renders`: switch to graph tab
    on a vault with no notes — shows root directory only + hint.
  - `graph_tab_populated_default_query_renders`: dirs fixture —
    shows root + a child node.
  - `graph_tab_input_mode_shows_cursor`: after `/`, prompt is
    yellow and cursor is positioned past the prompt.

### Documentation (docs/architecture.md)

- [ ] Add `graph/query.rs` (plan 017) to the workspace layout
      listing under `ft-core/src/graph/`. It's missing today.
- [ ] Add `cmd/graph.rs` to the binary's `cmd/` listing.
- [ ] Add `output/graph.rs` to the binary's `output/` listing.
- [ ] Add a short "Graph query DSL" subsection under "Key traits
      and seams" — three sentences pointing at `query::parse`,
      `select/expand/walk`, and the CLI vs. TUI consumers.

### Build invariants

- [ ] `cargo test --workspace` — all existing + new tests pass.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] `cargo fmt --check` clean.
- [ ] No new dependencies. No crate additions to the workspace.
- [ ] Man page + completions regenerated (`ft completions` and
      `ft man` pick up the new subcommand automatically because
      both walk clap's derived schema — no manual updates needed,
      but verify by diffing the generated output).

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

### Session 1 · planned
**Goal:** Library: `GraphQuery::walk` + `WalkOptions` + `WalkNode`,
with unit tests covering depth bound, cycle stop, cycle allow, and
no-expansion-rule. Pure ft-core change; no CLI or TUI surface yet.

### Session 2 · planned
**Goal:** CLI: `ft graph query` subcommand. New `cmd/graph.rs`,
`output/graph.rs` (all five formats), integration tests under
`ft/tests/graph_query.rs` using the `dirs` fixture. Verify man page
and completions pick it up.

### Session 3 · planned
**Goal:** TUI initial-state fixes. `[graph].default_query` config
field + built-in fallback, on_focus seeding, insertion cursor,
brighter prompt, empty-state hint. Insta snapshot tests for empty,
populated, and input-mode states. Update `docs/architecture.md`.

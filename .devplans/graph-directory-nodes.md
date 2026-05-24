---
id: 016
name: graph-directory-nodes
title: Graph: Directory nodes and contains edges
status: ready
created: 2026-05-24
updated: 2026-05-24
---

# Graph: Directory nodes and contains edges

## Goal

Add `NodeKind::Directory` and `EdgeKind::Contains` to the graph model so
the file-tree structure of the vault is queryable via the same graph
traversal primitives as wikilinks. Every vault directory becomes a graph
node; `Directory --contains--> child` edges connect directories to their
immediate children (subdirectories and notes).

This is the first additive use of the heterogeneous-from-day-one
`NodeKind` / `EdgeKind` enums. It ships purely as a library change —
no CLI or TUI — and serves as the prerequisite for the graph query DSL
(Plan B) and the infinite-tree TUI viewer (Plan C).

## Motivation and Context

The graph query language the user is designing needs to answer questions
like "show me top-level notes," "expand this directory to see its
contents," and "walk the file tree." Without directory nodes, every
file-tree query would need bespoke path-string parsing outside the
graph. With them, the same `incoming` / `outgoing` / node-attribute
matching handles wikilinks, embeds, and the file tree uniformly.

This is also the first new `NodeKind` / `EdgeKind` variant since the
graph shipped in plan 013. Getting it right establishes the pattern for
future additive kinds (`Tag`, `Task`, `FrontmatterValue`, …) and
proves the type design isn't just decorative — code that matches on
`NodeKind` / `EdgeKind` genuinely needs no semantic change after the
addition, only new match arms where exhaustiveness is required.

## Acceptance Criteria

### Types

- [ ] `NodeKind::Directory(DirData)` variant added to the enum.
      `DirData { path: PathBuf, name: String }` — path is
      vault-relative directory (e.g. `Areas/finance`, no trailing
      slash); `name` is the last component.
- [ ] `EdgeKind::Contains` variant added (unit variant — no extra
      payload per edge, the fact of containment is the whole edge).
- [ ] `DirData::root()` constructor returns the vault-root directory
      with `path: PathBuf::new()` and `name: String::new()` — the empty
      string denotes the vault root, so the user can start the file
      tree from the top.
- [ ] No new dependencies. No external crate additions to the
      workspace.

### Build

- [ ] During `Graph::build`, after inserting all `Note` nodes (existing
      step), walk every note's vault-relative path, collect all unique
      directory paths (every prefix component chain), and create
      `Directory` nodes for each. Insert a root `Directory` node
      (path `""`) so the vault root is always present.
- [ ] For each `Directory` node, insert `Contains` edges to its
      **immediate** children — those whose path is exactly one
      component deeper. A child can be a `Note` or another
      `Directory`.
- [ ] Directory → subdirectory edges and directory → note edges both
      use the same `EdgeKind::Contains` variant. Callers distinguish
      via `Graph::node(child_id)`.

### Indexes

- [ ] `path_index` renamed to a more general name (TBD: `node_by_path`
      still seems right; or keep `path_index` internal and rename
      the public lookup `note_by_path` → `node_by_path`). Directory
      nodes are inserted into this index keyed by directory path.
- [ ] Directory nodes are **not** inserted into `title_index` —
      directories aren't wikilink targets, so title-based lookup
      doesn't apply.
- [ ] A new convenience method `Graph::is_directory(id) -> bool`
      (or the caller uses `graph.node(id)` + pattern match directly).

### API compatibility (audit existing callers)

- [ ] Every existing pattern match on `NodeKind` (parser, resolve,
      rename, TUI, CLI) continues to compile — either it already
      handles the non-`Note`/`Ghost` arm with a fallback, or a new
      arm is added. The goal is zero breakage outside `graph/`.
- [ ] `rename` module: `plan_rename` only rewrites link edges
      (`Link` / `Embed`); `Contains` edges are invisible to rename
      (a note moving directories is a separate future feature).
      `rename` match arms on `EdgeKind` should not panic on
      `Contains` — they should skip it.
- [ ] `refresh_note`: v1 does **not** rebuild directory structure on
      `refresh_note` — adding/removing a file may leave stale
      Directory nodes until the next full `Graph::build`. Documented
      as a known v2 gap (tracking would need to diff the parent
      directory tree, which is complex for a rare operation).

### Tests

- [ ] Dedicated fixture vault `tests/fixtures/dirs/`:
  ```
  tests/fixtures/dirs/
  ├── root.md
  ├── Areas/
  │   ├── finance.md
  │   └── operations/
  │       └── shifts.md
  └── Projects/
      └── alpha.md
  ```
- [ ] Unit tests in `graph/tests.rs` or a new `graph/dir_tests.rs`:
  - `build_includes_directory_nodes`: verify node counts (root + 3
    leaf dirs + root) and that `DirData` paths/names are correct.
  - `graph_includes_contains_edges`: verify edge counts and that
    `incoming(areas_dir_id)` returns `finance.md` and
    `Areas/operations/` as children.
  - `root_contains_top_level_items`: verify root dir's outgoing
    edges connect to `root.md`, `Areas/`, `Projects/`.
  - `note_incoming_includes_containing_directory`: a note's incoming
    edges include the `Contains` edge from its parent directory.
  - `path_index_includes_directories`: `note_by_path("Areas")`
    returns `Some(id)` where `graph.node(id)` is Directory.
  - `no_title_index_for_dirs`: title lookup for "Areas" returns only
    note nodes, never directories.
  - `two_notes_same_stem_different_dirs`: notes `Areas/report.md`
    and `Projects/report.md` share a title; `note_by_title("report")`
    returns both, and their containing directories are distinct.
  - `rename_does_not_panic_on_contains_edge`: verifying plan_rename
    gracefully skips Contains edges.

### Build invariants

- [ ] `cargo test --workspace` — all existing + new tests pass.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] `cargo fmt --check` clean.
- [ ] No new dependencies. Purely additive code in `ft-core/src/graph/`.

## Technical Notes

- **Why a root directory node.** Without a root, the file tree has no
  single entry point — callers would need to discover "top-level" via
  a negative edge query ("notes without an incoming Contains edge").
  That works but it's one more pattern every consumer must replicate.
  A root Directory node is cheap (one extra node) and makes
  traversal uniform: start at root, drill down. The root's `PathBuf`
  is empty and its `name` is empty — callers can display it as `/`
  or "(vault)" at the TUI layer.

- **Why no refresh_note for dirs in v1.** When `refresh_note` adds or
  removes a file, the directory tree might need updating (new note in
  a new subdirectory needs new Directory nodes; deleted last note in a
  leaf directory might need a removed Directory node). Tracking this
  requires walking the directory tree and diffing against current
  state — not hard, but not needed for the initial TUI tree (which
  builds the graph once on startup, same as the current TUI builds the
  vault once). v2 can add a `refresh_dirs` or a full `Graph::rebuild`
  tied to the TUI's file-watch or post-git-sync event.

- **Why Contains is a unit variant (no data payload).** A link edge
  carries `LinkEdge` because rename needs `byte_range` + `raw_text`
  + `form` to rewrite the source. A Contains edge is logical, not
  textual — there's no source byte to rewrite. If we later add
  "move note to another directory," the operation is
  `plan_move_to_dir` which rewrites edges, not in-file link text.

- **Path normalization for directory paths.** Directory paths follow
  the same `normalize_path` convention as note paths — components
  joined with `/`. A directory "path" is the directory prefix of a
  note path, e.g. `Areas/finance`. No trailing slash. The root
  directory's path is `PathBuf::new()` (empty). This avoids creating
  two separate normalization rules.

- **Interaction with ghost nodes.** Directories are never ghost
  targets; the `intern_ghost` path only fires for unresolved link
  resolution, and `Contains` edges are created directly during build.
  No change to ghost logic needed.

## Sessions

### Session 1 · 2026-05-24 · planned
**Goal:** Add `NodeKind::Directory` + `EdgeKind::Contains` types,
`DirData::root()` constructor, build directory nodes and contains
edges in `Graph::build`, new fixture vault, unit tests, audit
existing match arms.
**Outcome:**


## Context

ft has two independent data subsystems:

1. **Task system** — `Vault::scan()` parses every `.md` file, extracts `Task` structs via `EmojiFormat.parse_line()`, and returns a flat `Scan { tasks, errors }`. Tasks carry `source_file` + `source_line` for origin tracking.

2. **Graph system** — `Graph::build(&vault)` independently reads every `.md` file, extracts `RawLink` vectors via `parser::extract_links()`, and builds a petgraph `DiGraph<NodeKind, EdgeKind>` with `Note`, `Ghost`, and `Directory` nodes connected by `Link`, `Embed`, and `Contains` edges.

These two systems never intersect. The `NodeKind` enum already has a comment (line 17) anticipating `Task` as a future variant. This change connects them by making tasks first-class graph nodes reachable via a new `HasTask` edge from their containing note.

## Goals / Non-Goals

**Goals:**
- Tasks appear as `NodeKind::Task(TaskData)` nodes in the graph
- Each note containing tasks has `EdgeKind::HasTask` edges to those task nodes
- Directory nodes transitively contain task nodes (via `Contains` edge to note, then `HasTask` edge to task)
- `kind = "Task"` is a valid graph DSL filter
- Task attributes (`status`, `priority`, `due`, `scheduled`, `tags`, `description`) are queryable via the graph DSL
- `expand` can reveal task nodes alongside notes and directories
- TUI graph tab renders task nodes with a distinct visual
- Built-in presets optionally include tasks in expand targets
- Preview-only task nodes (not created yet – that's the Ghost analog) are not in scope

**Non-Goals:**
- `depends_on` edges between tasks — stays as a `Task` field only
- Mutation of tasks through the graph subsystem — use the existing task ops
- Task nodes as `GhostData`-style "not yet created" placeholders
- Changes to the task query DSL or task list output formats
- Changes to `Vault::scan()` return type or the task parsing pipeline

## Decisions

### D1: `Graph::build()` accepts `&Scan` rather than parsing tasks inline

**Options considered:**
- A) `Graph::build(&vault, &Scan)` — accept scan results as input
- B) `Graph::build(&vault)` parses tasks internally during file read, reusing the same parallel iteration
- C) Separate `Graph::add_tasks(&mut self, &Scan)` method called after initial build

**Decision: A — `Graph::build(&vault, &Scan)`**

Rationale: `Vault::scan()` already parses every file for tasks. Re-parsing (option B) wastes I/O and CPU. Option C splits the build into two steps and complicates the builder API. Option A keeps build atomic, uses existing parsed data, and the signature change is a clean compile-time signal to all callers. Callers that already have `Scan` from `vault.scan()` simply pass it through.

### D2: `TaskData` carries denormalized task fields, not a full `Task`

`TaskData` holds: `description`, `status` (as `String` for consistent `node_string_attr` evaluation), `priority` (as `Option<String>`), `due`, `scheduled`, `tags` (as `Vec<String>`), `source_file`, `source_line`.

This is a subset of `Task` fields — only the ones the graph DSL needs for filtering. The full `Task` can be looked up by `source_file + source_line` if needed, but the graph layer doesn't own that lookup. `TaskData` is constructed from `&Task` during `Graph::build()`.

### D3: Task node identity uses `source_file` + `source_line` as the dedup key

Two tasks in the same file on different lines are distinct nodes. The `task_index: HashMap<(PathBuf, usize), NoteId>` maps `(source_file, source_line)` → `NoteId` for O(1) lookup. This mirrors `path_index` for notes and `title_index`/`ghost_index` for other node types.

### D4: `EdgeKind::HasTask` — a new edge kind, not reusing `Contains`

`Contains` edges model directory→note/directory containment. Tasks are contained *by notes* (the note has the task text), not by directories directly. A new `HasTask` edge kind:
- Makes the relationship explicit (note→task, not directory→task)
- Keeps `Contains` semantics clean (structural parent/child in vault tree)
- Allows DSL queries like `expand where edge.kind = "has-task"` vs `expand where edge.kind = "directory-contains"`
- Edge payload carries no data (like `Contains`) — the `TaskData` on the node carries everything needed

### D5: Task DSL attributes reuse string evaluation

Task attributes that are strings (`status`, `priority`, `due`, `scheduled`, `description`) use the existing `Attr::Custom(name)` → `node_string_attr()` path. `tags` uses `Attr::Tags` → `node_tags_attr()` path (new, returns string for `in`/`includes` comparisons). This avoids a parallel attribute system — tasks slot into the existing evaluation framework with new attribute names recognized for `Task` nodes.

New `Attr` variants aren't needed. Instead, `node_string_attr` gains a `match` arm for `NodeKind::Task` that maps attribute names (`"status"`, `"priority"`, `"due"`, `"scheduled"`, `"description"`) to `TaskData` fields. Unknown attributes on task nodes produce `None` (same as unknown attributes on other node types).

### D6: TUI `kind_char` for tasks is `T`

Follows the established pattern: `N` = Note, `D` = Directory, `G` = Ghost, `T` = Task. Display text shows the task description (truncated to fit column width, same as note titles).

### D7: Built-in presets unchanged, new preset `tasks-in-tree`

Rather than modifying existing presets (which could surprise users who don't want tasks cluttering their tree), add a new built-in preset `tasks-in-tree` that includes `Task` in `expand.to.kind`. The existing `tree` preset remains task-free. Users who want tasks in their default view can shadow `tree` in their config.

## Risks / Trade-offs

- **Graph size increase** — Vaults with many tasks will have significantly more nodes and edges. For pathological vaults (10k+ tasks), this could slow graph operations. → Mitigation: task nodes are only created from `Scan` data passed to `build()`; if `Scan` is empty or not provided, no task nodes exist. The `Graph::build()` signature change makes this explicit.

- **`Source_file` path must match note `NoteData.path`** — Both use vault-relative paths, but `Task.source_file` comes from `parse_file()` while `NoteData.path` comes from `markdown_files()`. These must use the same normalization. → Mitigation: both go through `normalize_path()`. Add an assertions test that verifies matching paths in the build step.

- **`Graph::build()` signature change is a breaking API change** — Callers in `ft/` and tests must be updated. → Mitigation: this is an internal API, not a stable public API. Compile errors guide all updates.

- **Task description in TUI may be long** — Task descriptions can be lengthy (unlike note titles which are filenames). → Mitigation: truncate to column width, matching existing note title display behavior. Full description visible on selection/expand.
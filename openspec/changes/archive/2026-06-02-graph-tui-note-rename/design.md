## Context

The `ft notes rename` CLI command (plan 013) uses `plan_rename` + `apply_rename_plan` from `ft-core/src/graph/rename.rs` to rename a single note and rewrite all vault-wide references. The Graph tab (plan 018, extended by plan 021) renders the note-link graph as an interactive infinite tree. Today the user can open notes, create notes, and move sections from the Graph tab, but cannot rename or relocate notes — they must switch to the CLI, type the command, then manually refresh the TUI.

The graph is the natural surface for note reorganization: the user sees a note's position in the directory tree, discovers misplacements, and wants to fix them without losing context. Two flows emerge:

- **Flow A** (move): the user spots one or more notes in the wrong directory, Space-toggles them, then picks a target directory from the same graph.
- **Flow B** (rename in place): the user wants to change a name without moving — a note with a bad title, or a directory with a loose naming convention.

## Goals / Non-Goals

**Goals:**

- Space-toggle multi-selection on graph tree rows with visual markers and `Esc`-clear.
- `r` key dispatches: single focused node → rename-in-place modal (Flow B); with multi-selection active → target-directory phase (Flow A).
- Flow A: two-phase UX — select sources via Space, then pick target directory from the graph tree (or fuzzy picker via `t`). On confirm, all selected notes move to the target directory; all references update atomically in one combined plan.
- Flow B: modal `EditBuffer` pre-filled with the current name. For notes, the file is renamed in its directory. For directories, every contained file is recursively renamed to the new path prefix, all external references update, and old empty directories are cleaned up.
- Library support: `plan_multi_rename` builds a combined `RenamePlan` from a set of `(NoteId, new_path)` pairs from a single graph snapshot, correctly handling cross-references.
- `Ctrl+r` replaces the old `r` as the graph-tab refresh keybinding.
- All library invariants hold: plan/apply split, `write_atomic` per file, descending-byte-order edits, freshness snapshots.

**Non-Goals:**

- Rename/move from the Notes tab (the Notes tab gets no new bindings in this change).
- CLI `ft notes move` command (the CLI rename command already covers "move by full path").
- Drag-and-drop reorder in the tree.
- Batch rename templates or wildcard renames.
- Undo capability beyond the existing atomic-write-per-file guarantee.
- Persisting multi-selection across graph rebuilds (selections clear on refresh — same as the current behavior for single selection across rebuilds when the note vanishes).

## Decisions

### 1. `RenamePlan` struct change: `renames: Vec<FileRename>` replaces `rename: Option<FileRename>`

**Why**: A directory rename produces multiple file renames. A Flow A multi-note move also produces multiple renames. Rather than splitting the type into `RenamePlan` (single) and `MultiRenamePlan` (multiple), a single unified type with a `Vec` is simpler. Zero renames covers the ghost-only case; one rename covers the existing single-note case.

**Alternatives considered**: A separate `MultiRenamePlan` type. Rejected because it would require either duplicating `apply_*` logic or a trait, and the struct change is trivial — only 3 call sites in `rename.rs` and `notes.rs` read `plan.rename`.

**Breaking change**: Yes — direct `RenamePlan` field access (`plan.rename`) becomes `plan.renames[0]` or similar. All callers are within the workspace; the breakage is localized to `apply_rename_plan`, `run_rename`, and `print_rename_plan_summary`.

### 2. Multi-rename planning: compose single-note plans from one graph snapshot

**Why**: `plan_multi_rename` iterates each `(NoteId, new_path)` pair, runs the same per-note logic that `plan_rename` does (incoming edges → edits, file rename), and merges results. Building from one snapshot guarantees cross-references between moved files are handled correctly — the linker file's old path is used for the edit, then the linker file is renamed last (edit-then-rename ordering).

**Merge rules**:
- **Edits**: Group by linker path. Validate non-overlap in ascending byte order. Since all edits come from the same parser (same byte_range precision), overlap is a planner bug and should `Err`.
- **Renames**: Collect all `FileRename` entries. De-duplicate by `from` path (shouldn't happen — each source note has a unique path, but directories contain non-overlapping files).
- **Snapshots**: Unique by path. A file that links to multiple renamed notes appears once.

**Alternatives considered**: `plan_rename` taking `&[(NoteId, PathBuf)]` directly. Rejected because it would change the single-note API signature; the wrapper pattern keeps `plan_rename` simple and tests unchanged.

### 3. Directory rename: BFS walk of `Contains` edges

**Why**: The graph already has `DirData` nodes with `Contains` edges to immediate children (notes and subdirectories). A BFS from the directory node collects all reachable `NodeKind::Note` paths. For each note at `old_dir/sub/file.md`, the new path is `new_dir/sub/file.md` (replace prefix). This `Vec<(NoteId, PathBuf)>` is then fed to `plan_multi_rename`.

**Alternatives considered**: Walking the filesystem directly (`std::fs::read_dir`). Rejected because (a) the graph is the single source of truth for which notes exist in the vault, (b) the graph is already built and in memory, and (c) the `Contains` edges are reliable — they're built from the parsed file set during `Graph::build`.

### 4. Multi-select state lives on `ExpandedView`

**Why**: `ExpandedView` already owns per-view tree state (`expanded_paths`, `selected`, `tree`). Multi-selection is a per-view concept — the user selects notes in the active view's tree. Adding `multi_selected: HashSet<NoteId>` to `ExpandedView` keeps it co-located with the selection state it augments.

**Clearing on graph rebuild**: `multi_selected` is cleared on `refresh()` / graph rebuild — the `NoteId`s are stale. Same behavior as `selected_path` being restored via path lookup (but we don't attempt to restore multi-selection — it's transient by design, like a clipboard that's disposable).

### 5. Flow A and Flow B share the `r` key, differentiated by multi-selection

**Why**: Two flows, one conceptual operation ("rename"). The Space toggle is the natural differentiator: no Space selections → rename the focused thing in place; Space selections → move those things to a directory. This avoids a leader-key chord or separate bindings.

**Key routing** (Normal mode, `r` pressed):
1. If `multi_selected` is non-empty → enter Flow A target phase.
2. Else if focused row is a Note → enter Flow B rename-note modal.
3. Else if focused row is a Directory → enter Flow B rename-directory modal.
4. Else (Ghost, Task, empty tree) → toast "nothing to rename" / no-op.

### 6. Flow A target phase uses the graph tree with `t` fallback

**Why**: The user is already looking at the graph tree to find the target directory. Navigating to a `D` row and pressing `Enter`/`m` is zero-context-switch. The `t` key opens a fuzzy directory picker (same `FolderListPicker` used by the create flow) as an escape hatch when the target directory isn't visible in the current query's tree.

**Target confirmation**: Only Directory nodes are accepted. Confirming on a Note row toasts "select a directory as target" and stays in phase. Ghost/Task rows are silently ignored (same as non-matching).

### 7. Flow B modal: `EditBuffer` overlay, not a `SectionMoveState`-style variant

**Why**: The rename modal is a single-step interaction (type name, Enter/Esc) — not a multi-step state machine. Modeling it as `rename_state: Option<GraphRenameState>` with its own `handle_rename_key` dispatcher follows the same pattern as `create_state: Option<CreateState>`. It captures the keyboard ahead of tree navigation and input mode (same precedence as the create overlay).

**EditBuffer reuse**: `ft/src/tui/widgets/edit_buffer.rs` already supports insert, backspace, delete, left/right, home/end, delete_word_backward. The rename modal uses a subset (all of these) and adds Enter (commit) and Esc (discard). Ctrl+W word-delete is included for consistency with the section-move compose rename buffer.

### 8. Directory rename creates a single combined plan, not N sequential plans

**Why**: Files within the renamed directory may link to each other. Sequential `plan_rename` + `apply_rename_plan` would fail because after moving file A, the graph state is stale for file B's `incoming` walk. A combined plan built from the pre-move graph snapshot avoids this entirely — all edits are computed against old paths, all renames happen after edits.

### 9. Empty directory cleanup

**Why**: After renaming all files from `old_dir/` to `new_dir/`, the old directory tree is empty. `apply_rename_plan` (step 4) removes empty directories bottom-up after all file renames complete. This is best-effort — if a directory contains non-note files (`.gitkeep`, images), it's left in place. The cleanup uses `std::fs::remove_dir` which fails on non-empty dirs, which is the correct behavior.

## Risks / Trade-offs

- **`RenamePlan` struct breakage**: Changes the public field from `Option<FileRename>` to `Vec<FileRename>`. Mitigation: all callers are in-workspace (2 files); the rename tests in `ft-core` and `ft` are updated in the same commit.

- **Multi-select cleared on refresh**: If the user builds a multi-selection, then presses `Ctrl+r` to refresh, the selection is lost. This is intentional (selections reference stale `NoteId`s) but may surprise users. Mitigation: clear is silent; no toast needed — the visual markers disappear, which is feedback enough.

- **Large directory renames**: Renaming a directory with 1000+ files generates 1000+ `FileRename` entries and potentially thousands of edits. The planner is CPU-bound on the incoming-edge walk but the graph is already built; the merge step is O(total_edits). For vaults with 5k notes this is still sub-second. No special optimization needed for v1.

- **Cross-reference edit ordering**: When file A and B are both renamed and A links to B, the edit to A's body happens against A's old path, then A is renamed. If A's edit range shifts due to another edit in A (e.g., A links to both B and C, both renamed), the descending-byte-order application prevents range corruption. This is already tested in the single-note multi-link case.

- **Directory Contains edges are direct children only**: The BFS correctly reconstructs the full tree by recursively walking edges. This is already how the graph tree rendering works (`expand_at` follows outgoing edges).

## Open Questions

<!-- None — design is complete. -->

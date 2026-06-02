## Why

The `ft notes rename` CLI can rename notes and update all vault-wide references, but there is no way to do this from the Graph tab — where users naturally discover the notes they want to reorganize. Users who browse the graph, spot notes in wrong directories, and want to either rename them in place or move them to a different folder must leave the TUI, build a CLI command, and then manually refresh. This closes that gap.

## What Changes

- **Flow A — Move notes to a directory**: Space-toggle to multi-select notes in the graph tree, then `r` enters a target-directory selection phase (navigate the graph to a directory node, or `t` for fuzzy picker fallback). On confirm, every selected note is renamed to `target_dir/note_filename.md` and all vault-wide references are updated atomically in one combined plan.
- **Flow B — Rename a note or directory in place**: `r` on a focused node (with no Space selections) opens a modal `EditBuffer` pre-filled with the current name. For notes, the file is renamed with the new stem in the same directory. For directories, every file recursively contained under the directory is renamed to the new path prefix, all external references are updated, and old empty directories are removed.
- **Multi-select on graph tree**: `Space` toggles selection on tree rows. Selected notes receive a visual marker. `Esc` clears all selections. Multi-selection is the differentiator: `r` with selections active enters Flow A; `r` with nothing selected enters Flow B.
- **`r` key repurposed**: The existing `r` (refresh) binding in the Graph tab moves to `Ctrl+r` to free `r` for rename.
- **Library**: `ft-core` gains a multi-rename planner capable of atomically planning any set of `(old_path, new_path)` moves from a single graph snapshot, correctly handling cross-references between moved files.

## Capabilities

### New Capabilities

- `graph-multi-select`: Space-toggle selection on graph tree nodes with visual markers and Esc-clear. Shared state on `ExpandedView`.
- `graph-rename-note`: Rename a single note in place from the Graph tab. Modal input pre-filled with current stem; plan + apply via the existing single-note `plan_rename`.
- `graph-rename-directory`: Rename a directory in place from the Graph tab. Recursively finds all contained files via graph `Contains` edges, computes new paths, builds a combined RenamePlan updating all references, applies atomically.
- `graph-move-notes`: Move one or more selected notes to a target directory from the Graph tab. Two-phase flow: Space-select sources, then pick target directory from the graph tree or fuzzy picker. Combined plan + apply.
- `multi-rename-planner`: Library primitive that accepts a set of `(NoteId, new_vault_relative_path)` pairs, builds all edits from a single graph snapshot, and produces a combined `RenamePlan` with multiple `FileRename` entries. Handles cross-references between moved files correctly.

### Modified Capabilities

<!-- No existing specs to modify -->

## Impact

- **`ft-core/src/graph/rename.rs`**: New `plan_multi_rename` function; `RenamePlan` gains `renames: Vec<FileRename>` (from `rename: Option<FileRename>` — **BREAKING** for direct `RenamePlan` consumers but CLI callers are trivially updated). `plan_rename` becomes a convenience wrapper that returns a `RenamePlan` with one entry in `renames`. `apply_rename_plan` handles multiple file renames and cleans up empty directories.
- **`ft/src/cmd/notes.rs`**: `run_rename` updated for the `RenamePlan` struct change (`plan.rename` → `plan.renames`).
- **`ft/src/tui/tabs/graph.rs`**: New `multi_selected: HashSet<NoteId>` on `ExpandedView`. `Space` toggle, `Esc` clear, `r` dispatch (single → Flow B, multi → Flow A). `Ctrl+r` replaces old `r`. New `GraphMoveOuter` variants for Flow A target phase. New `GraphRenameState` for Flow B modal. New `handle_rename_key` / `handle_move_target_key` handlers.
- **`ft/src/tui/tabs/graph.rs` — render**: Visual markers for multi-selected rows. Banner during Flow A target phase. Modal `EditBuffer` during Flow B. Tab-strip hidden during rename/move phases (same pattern as section-move banner).
- **`ft-core/src/graph/mod.rs`**: No changes needed — `DirData`, `Contains` edges, `outgoing()` already exist.
- **Tests**: Library unit tests for `plan_multi_rename` (3+ notes with cross-references, empty set, same-path no-op, conflict detection). TUI integration tests for all new keybindings and state transitions. Snapshot tests for rename modal and move-target banner.

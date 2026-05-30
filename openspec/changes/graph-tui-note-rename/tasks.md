## 1. Library: RenamePlan struct and multi-rename planner

- [ ] 1.1 Change `RenamePlan::rename: Option<FileRename>` to `renames: Vec<FileRename>`. Update `touched_files()` to iterate `renames`.
- [ ] 1.2 Update `apply_rename_plan`: iterate all `renames` (create parent dirs, `std::fs::rename`, then bottom-up remove empty old directories).
- [ ] 1.3 Implement `plan_multi_rename(graph, vault_root, moves: &[(NoteId, PathBuf)]) -> Result<RenamePlan>`: iterate each pair, run per-note logic (incoming edges → edits, file rename), merge edits (group by path, sort descending, validate non-overlap), merge renames, merge snapshots (unique by path). Skip same-path moves silently.
- [ ] 1.4 Update `plan_rename` to delegate to `plan_multi_rename` with a single-element slice, keeping the same public signature.
- [ ] 1.5 Update CLI `run_rename` and `print_rename_plan_summary` in `ft/src/cmd/notes.rs` for the `rename`→`renames` field change.
- [ ] 1.6 Update all existing `plan_rename` unit tests in `ft-core/src/graph/rename.rs` for the struct change (field access, check invariants).
- [ ] 1.7 Add unit tests for `plan_multi_rename`: single note (matches plan_rename output), two notes with no cross-refs, two notes with cross-ref (A links to B, both renamed), ghost in moves, empty moves, same-path skip, target-exists error, non-overlap validation in merged edits.
- [ ] 1.8 Add unit tests for multi-rename apply: plan with 3 renames, directory cleanup (empty old dir removed, non-empty preserved), freshness check across multiple snapshots.

## 2. TUI: Graph tab keybinding and multi-select infrastructure

- [ ] 2.1 Move `r` refresh to `Ctrl+r`: change `(KeyCode::Char('r'), _)` to `(KeyCode::Char('r'), modifiers) if modifiers.contains(KeyModifiers::CONTROL)` in `handle_event` Normal mode dispatch. Leave `r` (no modifier) unhandled for now.
- [ ] 2.2 Add `multi_selected: HashSet<NoteId>` field to `ExpandedView` (with `#[derive(Default)]` so it starts empty). Add `fn clear_multi_selection(&mut self)` helper.
- [ ] 2.3 Wire `Space` keybinding in Normal mode: if focused row is a Note, toggle its `NoteId` in `multi_selected`; otherwise no-op. Consume key event.
- [ ] 2.4 Wire `Esc` keybinding: if `multi_selected` is non-empty, clear it and consume; otherwise return `NotHandled` (fall through to app-level).
- [ ] 2.5 Clear `multi_selected` on graph refresh (in `restore_all_views` or the refresh handler).
- [ ] 2.6 Render multi-selection markers in the tree: between expand indicator and kind prefix, show `●` (yellow) if selected, space if not. Update `render_row` / `make_row` in the graph view rendering code.
- [ ] 2.7 Update existing snapshot tests that show tree rows (`graph_tab_populated_default_query_80x24`, `graph_tab_strip_two_views_80x24`, etc.) — re-accept to baseline the new column (space-only, no markers since no selections in those tests).

## 3. TUI: Flow B — rename note in place

- [ ] 3.1 Add `GraphRenameState` struct (note_id, is_directory: bool, buffer: EditBuffer, source_rel: PathBuf) and `rename_state: Option<GraphRenameState>` field on `GraphTab`.
- [ ] 3.2 Wire `r` in Normal mode when `multi_selected` is empty and focused row is a Note: construct `GraphRenameState` with buffer pre-filled from `NoteData.title`, is_directory = false.
- [ ] 3.3 Wire `r` in Normal mode when `multi_selected` is empty and focused row is a Ghost: toast "cannot rename a ghost — create the note first", do not open modal.
- [ ] 3.4 Implement `handle_rename_key(&mut self, k, ctx) -> EventOutcome`: dispatch to EditBuffer for printable/Backspace/Delete/Left/Right/Home/End/Ctrl+W; Enter validates and commits; Esc discards and closes. Other keys consumed.
- [ ] 3.5 Enter commit logic for notes: validate buffer is non-empty, no '/' in name, compute `new_path = source_rel.parent() / new_stem.md`, call `plan_rename`, check for target-exists error, `apply_rename_plan`, refresh graph, toast "renamed <old> → <new>", `rename_state = None`. On validation error: toast, keep modal open.
- [ ] 3.6 Render rename modal: overlay with title "Rename note" (or "Rename directory"), EditBuffer line, footer with "Enter: commit · Esc: discard". Reuse `EditBuffer` rendering pattern from `edit_buffer.rs`.
- [ ] 3.7 Invoke `handle_rename_key` ahead of all other key dispatch in `handle_event` (same precedence as create_state).

## 4. TUI: Flow B — rename directory in place

- [ ] 4.1 Wire `r` in Normal mode when `multi_selected` is empty and focused row is a Directory (non-root): construct `GraphRenameState` with buffer pre-filled from `DirData.name`, is_directory = true.
- [ ] 4.2 Wire `r` on root Directory: toast "cannot rename vault root", do not open modal.
- [ ] 4.3 Extend Enter commit logic for directories: walk `Contains` edges via BFS to collect all `NoteId`s under old_dir; compute new paths by replacing old_dir prefix with new_dir prefix; call `plan_multi_rename`; apply; toast "renamed directory <old> → <new> (N files)". Validate target directory doesn't already exist.
- [ ] 4.4 BFS helper: `fn collect_directory_notes(graph: &Graph, dir_id: NoteId) -> Vec<(NoteId, PathBuf)>` — returns all notes reachable via Contains edges with their current vault-relative paths. Iterate `graph.outgoing(dir_id)`, filter `EdgeKind::Contains`, recurse into Directory children, collect Note children.

## 5. TUI: Flow A — move selected notes to target directory

- [ ] 5.1 Add `MoveTargetFromTree { selected: HashSet<NoteId> }` and `MoveTargetPicker { picker: FuzzyPicker<...>, selected: HashSet<NoteId> }` variants to `GraphMoveOuter` enum.
- [ ] 5.2 Wire `r` in Normal mode when `multi_selected` is non-empty: set `move_outer = Some(GraphMoveOuter::MoveTargetFromTree { selected: mem::take(&mut active_view_mut().multi_selected) })`.
- [ ] 5.3 Implement `handle_move_target_key` for `MoveTargetFromTree`: `Enter`/`m` confirms target, validates focused row is Directory, computes new paths (`target_dir / note_filename.md`) for each selected note, calls `plan_multi_rename` (skip notes already in target), applies, refreshes, toasts ("moved N note(s) to <dir>"), clears move_outer. `t` opens folder picker. `Esc` cancels (clears selection, move_outer=None).
- [ ] 5.4 Same-directory detection: skip notes where `note_path.parent() == target_dir`. If all skipped, toast "all N note(s) are already in <dir>", clear state.
- [ ] 5.5 Directory picker for Flow A: open a `FuzzyPicker` over vault directories. Use existing `FileDialogPicker` pattern from create flow or a directory-focused source.
- [ ] 5.6 Render move-target banner: single line above tree showing "Move N note(s): navigate to target directory, Enter/m to confirm, t for picker, Esc to cancel". Hide tab strip while move is active (same pattern as section-move banner).
- [ ] 5.7 Merge `handle_move_target_key` into the existing `handle_move_key` dispatcher — the new variants return `NotHandled` for j/k/↑/↓/gg/G (tree navigation passes through while banner stays visible).

## 6. Integration tests

- [ ] 6.1 Multi-select tests in `ft/src/tui/tests.rs`: Space toggles selection on Note row, Space is no-op on Directory row, Space toggles off, Esc clears all selections, Esc passes through when empty, selections clear on Ctrl+r refresh, two selections render markers. (7 tests)
- [ ] 6.2 Flow B note rename tests: r on Note opens modal with stem pre-filled, typing in modal works, Enter renames file on disk and updates references (assert vault file contents), Enter with empty name shows error toast and stays open, Enter with '/' in name shows error, Enter with existing target shows error, Esc closes modal without changes, r on Ghost toasts. (8 tests)
- [ ] 6.3 Flow B directory rename tests: r on Directory opens modal with dir name pre-filled, r on root toasts, Enter renames directory and updates all references (assert files moved and external references updated), cross-references within renamed dir updated correctly, target-exists error, empty name error. (6 tests)
- [ ] 6.4 Flow A move tests: r with selections enters move phase (banner text check), Enter on Directory executes move (assert files moved + references updated), Enter on Note toasts and stays, Enter on empty tree toasts, t opens directory picker, Esc in picker returns to phase, Esc cancels flow and clears selections, all notes already in target toasts, one already there + one moved. (9 tests)
- [ ] 6.5 Snapshot tests: `graph_rename_note_modal_80x24` (rename modal with pre-filled buffer), `graph_move_target_banner_80x24` (banner over tree with two notes selected), `graph_multi_select_markers_80x24` (two rows showing ● markers). Re-accept any existing snapshots changed by new column or Ctrl+r rebind.

## 7. Cleanup and invariants

- [ ] 7.1 `cargo build --release` passes.
- [ ] 7.2 `cargo test --workspace` — all existing + new tests pass.
- [ ] 7.3 `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] 7.4 `cargo fmt --check` clean.

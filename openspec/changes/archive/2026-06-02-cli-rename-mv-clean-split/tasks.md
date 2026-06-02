## 1. Hoist collect_directory_notes to ft-core

- [x] 1.1 Move `collect_directory_notes` from `ft/src/tui/tabs/graph.rs` to `ft-core/src/graph/rename.rs` as `pub fn`. Keep the same signature.
- [x] 1.2 Update `ft/src/tui/tabs/graph.rs` to import `collect_directory_notes` from `ft_core::graph::rename` and remove the local copy.
- [x] 1.3 Verify build + existing tests pass after the hoist.

## 2. Restrict ft notes rename to bare names

- [x] 2.1 In `parse_new_path` (or `run_rename`): reject `<NEW>` containing `/`. Error: "use `ft notes mv` to change directories. To rename in place, pass a bare filename without /."
- [x] 2.2 Simplify `parse_new_path`: remove the `has_slash` branch. The function appends `.md` if missing and joins with `source_dir` (or uses it as-is for ghosts). Rename to `parse_new_name` to reflect its narrower scope.
- [x] 2.3 Update `resolve_rename_source` error messages to not mention path-based resolution (or keep it — it still resolves by path, just doesn't move).
- [x] 2.4 Update existing CLI rename tests in `ft/tests/notes_rename.rs`:
  - Tests that pass bare names (`rename foo bar`) → unchanged.
  - Tests that pass paths in `<NEW>` (`rename foo archive/foo.md`) → update to expect error, or convert to `ft notes mv` calls.
  - Test `rename_full_path_moves_file_across_directories` → convert to `mv` test or assert error.
- [x] 2.5 Add a test that `rename foo bar/baz` exits 2 with message about using `mv`.

## 3. Add ft notes mv subcommand

- [x] 3.1 Add `MoveArgs` struct: `sources: Vec<String>` (at least 1, required), `target: String` (required), `dry_run: bool`.
- [x] 3.2 Add `Move(MoveArgs)` variant to `NotesCommand` enum and dispatch in `run`.
- [x] 3.3 Implement `run_mv(args, vault_flag)`:
  - Discover vault, build graph.
  - Resolve each source to a list of `(NoteId, PathBuf)` pairs — notes map directly; directories are expanded via `collect_directory_notes`.
  - Source resolution helper: `resolve_mv_source(graph, vault_root, raw: &str) -> Result<Vec<(NoteId, PathBuf)>>`. For notes: find by path (`.md` auto-append). For directories: find `DirData` node by path, call `collect_directory_notes`. Error if neither found.
  - Resolve target: assert `target` is an existing directory on disk. Error if file or missing.
  - Call `plan_multi_rename` with all resolved moves.
  - If `--dry-run`, print summary and return.
  - Call `apply_rename_plan`.
  - Print summary: "moved N note(s) to <target>", link counts.
- [x] 3.4 Source resolution: path lookup with `.md` auto-append. Use `graph.note_by_path(path)`, fallback `graph.note_by_path(path.with_extension("md"))`. For directories: use `graph.node_by_path(path)` and match `NodeKind::Directory`.
- [x] 3.5 Target resolution: `let target_abs = vault_root.join(target); assert!(target_abs.is_dir())`. Error if not.

## 4. Integration tests for ft notes mv

- [x] 4.1 `ft/tests/notes_mv.rs`: create new test file.
- [x] 4.2 Single note move: `mv foo.md archive/` — assert file moved, references updated.
- [x] 4.3 Multiple notes move: `mv a.md b.md target/` — assert both moved.
- [x] 4.4 Directory move: `mv projects/old/ archive/` — assert all files moved, old dir cleaned up, references updated.
- [x] 4.5 Mixed notes + directory move.
- [x] 4.6 Source not found → exit 2.
- [x] 4.7 Target not a directory → exit 2.
- [x] 4.8 Target doesn't exist → exit 2.
- [x] 4.9 `mv` with fewer than 2 args → exit 2.
- [x] 4.10 `--dry-run` prints plan, modifies nothing.
- [x] 4.11 Source without `.md` extension auto-appended.
- [x] 4.12 Move with cross-references between moved files (md-link relative URL preserved).

## 5. Cleanup and invariants

- [x] 5.1 `cargo build --release` passes.
- [x] 5.2 `cargo test --workspace` — all tests pass.
- [x] 5.3 `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [x] 5.4 `cargo fmt --check` clean.

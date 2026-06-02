## Why

`ft notes rename` currently conflates two operations: renaming a note's title (identity change) and moving a note to a different directory (location change). Its `<NEW>` argument does double duty — a bare name means "same directory, new stem" while a path with `/` means "move to this path." This is the `mv` command's ambiguity problem in reverse. The TUI already separates these cleanly into Flow B (rename in place) and Flow A (move to directory). The CLI should match that model. Additionally, the TUI can now move entire directories and multiple notes at once via `plan_multi_rename` — the CLI should expose the same capability.

## What Changes

- **BREAKING**: `ft notes rename <NOTE> <NEW>` now rejects `<NEW>` values containing `/`. Use `ft notes mv` to change directories. `<NEW>` is always a bare filename stem (`.md` appended automatically). The command renames a single note in its current directory and updates all vault-wide references. Ghost rename via `[[Phantom]]` syntax is unchanged.
- **New command**: `ft notes mv <SOURCES>... <TARGET>` — moves one or more sources (notes or directories) to a target directory, updating all vault-wide references. Sources are vault-relative paths (`.md` suffix optional for notes). Directories are expanded recursively via graph `Contains` edges. Uses `plan_multi_rename` for atomic combined planning. `--dry-run` prints the plan.
- **Hoist `collect_directory_notes`** from `ft/src/tui/tabs/graph.rs` into `ft-core/src/graph/rename.rs` so both CLI and TUI share the BFS directory-expansion logic.

## Capabilities

### New Capabilities

- `cli-mv`: New `ft notes mv <SOURCES>... <TARGET>` subcommand. Accepts vault-relative paths for sources (notes and directories), resolves directories to contained notes via BFS, calls `plan_multi_rename` + `apply_rename_plan`, prints summary. Supports `--dry-run`.

### Modified Capabilities

- `cli-rename`: `ft notes rename` rejects `<NEW>` values containing `/`. Only bare filename stems are accepted. Exit code 2 with a message pointing to `ft notes mv` for directory changes.

## Impact

- **`ft/src/cmd/notes.rs`**: `RenameArgs.new` validation (reject `/` in name). New `MoveArgs` struct, new `run_mv` function. New `resolve_source_path` helper for path-based source resolution. Update `NotesCommand` enum and dispatch.
- **`ft-core/src/graph/rename.rs`**: `collect_directory_notes` hoisted from TUI, made `pub`.
- **`ft/src/tui/tabs/graph.rs`**: Import `collect_directory_notes` from `ft_core::graph::rename`; delete local copy.
- **`ft/tests/notes_rename.rs`**: Tests that pass paths with `/` to `rename` must be updated to use `mv` or expect errors.
- **New integration tests**: `ft/tests/notes_mv.rs` for the `mv` subcommand.

## Why

The TUI graph view can browse, search, rename, and move notes — but it cannot delete anything, cannot materialize ghost notes with a single keystroke, and cannot create subdirectories. Users who triage their vault from the graph must switch to a file manager or terminal for these basic CRUD operations, breaking flow.

## What Changes

- **Delete (note and directory nodes)**: New `graph.delete` command opens a yes/no confirmation modal on the selected Note or Directory row. On confirm, the note file or directory tree is removed from disk and the graph refreshes. A simple CLI command (`ft graph delete <path>`) exposes the same deletion logic for scripting.
- **Instant ghost creation**: On a Ghost row, `c` (`graph.create-blank`) creates the missing note file immediately at the ghost's resolved vault-relative path with default `# <title>` content, then opens it in the editor — no multi-step flow. `C` (`graph.create-from-template`) prompts for a template, then commits to the ghost's path (skipping folder picker and filename prompt). On non-ghost rows, `c`/`C` continue to open the existing create flow with the folder pre-seeded.
- **Create subfolder on directory nodes**: `n` (`graph.create-subdir`) on a Directory row prompts for a subfolder name, then creates `<dir>/<name>/` on disk and refreshes. On non-directory rows, it shows an error toast.

## Capabilities

### New Capabilities

- `graph-delete`: Delete note files and directory trees from the TUI graph view (with confirmation modal) and via CLI.
- `graph-create-subdir`: Prompt for a subfolder name and create it under the selected directory node in the TUI graph view.

### Modified Capabilities

- `tui-commands`: New `graph.delete` and `graph.create-subdir` commands added to the graph tab's command registry.
- `tui-keymaps`: New bindings for delete (`d`/`D`) and create-subdir (`n`) on the graph tab's keymap.

## Impact

- **ft-core**: New `fs::delete_file` and `fs::delete_directory` functions (using `std::fs::remove_file` and `std::fs::remove_dir_all`). New `graph::delete` module with `plan_delete` + `apply_delete` (plan/apply split matching the project's mutation pattern).
- **ft CLI**: New `ft graph delete <path>` subcommand.
- **ft TUI**: New `ActiveModal::ConfirmDelete` variant wrapping a yes/no confirmation modal. New `ActiveModal::CreateSubdir` variant wrapping a single-line prompt for subfolder name. `graph.rs` gains `dispatch_command` arms for `graph.delete`, `graph.create-subdir`, and modified `graph.create-blank`/`graph.create-from-template` on ghost nodes.
- **Breaking changes**: None.

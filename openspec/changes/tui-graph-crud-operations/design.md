## Context

The TUI graph view (`ft/src/tui/tabs/graph.rs`) already supports creating notes via `c`/`C`, renaming via `r`, and moving via `m`/`r`+multi-select. All mutation flows follow the project's plan/apply split: a planner produces a `*Plan` struct, an `apply_*` function writes via `write_atomic`. The modal system (`ActiveModal` enum) owns every keyboard-capturing overlay through a single App-level slot.

Three operations are missing: deletion, instant ghost materialization, and subdirectory creation. The design must respect the existing patterns (plan/apply split for mutations, modal-first dispatch, command/keymap layer, ghost path resolution) while keeping the implementation minimal.

## Goals / Non-Goals

**Goals:**
- Delete notes and directory trees from the TUI with a confirmation modal
- Expose deletion as a simple CLI command (`ft graph delete <path>`)
- Materialize ghost notes instantly on `c` (empty) and `C` (template) without the multi-step create flow
- Create subdirectories from directory nodes with a single-line prompt
- All mutations follow the plan/apply split and use `write_atomic` for file writes

**Non-Goals:**
- Trash/recycle-bin semantics — deletion is permanent (users have git/backups for undo)
- Multi-node deletion in a single operation
- Deletion of task or paragraph nodes
- Undo support beyond what git provides
- Complex subdirectory creation (nested paths, template seeding)
- Ghost creation and subdirectory creation from the CLI — TUI only

## Decisions

### Decision 1: Delete is file-level, not graph-level

Delete removes actual files/directories from disk, then refreshes the graph. We do NOT attempt to delete "only from the graph" or maintain a soft-delete layer. The graph is always derived from disk state, so the only actionable deletion is on-disk.

**Rationale:** The graph has no concept of "deleted but still in graph" — it reflects the vault filesystem. A graph-only delete would be erased on the next scan. Per-project conventions (`write_atomic` for writes, `std::fs::remove_*` for deletes) are the natural fit.

**Alternatives considered:**
- A `deleted` marker in a `.ft` metadata file — too complex for the stated goal, breaks the "graph mirrors disk" invariant.
- Moving to a `.trash/` directory — out of scope; users have git for undo.

### Decision 2: Plan/apply split for delete

A `plan_delete` function produces a `DeletePlan` struct listing the files to remove. `apply_delete` executes the removals. The plan step validates: node exists, path is under vault root, no path traversal. The apply step removes files and empty parent directories (bottom-up, like `graph-rename-directory`).

**Rationale:** Consistent with every other mutation in the codebase (`plan_rename`, `plan_multi_rename`). Makes testing trivial — test the plan separately from the apply.

**Alternatives considered:**
- Direct `std::fs::remove_*` in the TUI handler — no plan validation layer, untestable, breaks project conventions.

### Decision 3: Confirmation modal reuses existing modal infrastructure

A new `ActiveModal::ConfirmDelete` variant wraps a minimal yes/no state (`selected: ConfirmChoice { Yes, No }`). `Enter` confirms on the highlighted choice, `y`/`n` jump directly, `Esc`/`q` cancels (maps to No). The modal renders a centered box with the question and two buttons.

**Rationale:** This is the project's first yes/no modal. Following the existing modal trait pattern (handle_event → render → commands → keymap) keeps it consistent. We scope the confirmation state inline rather than building a general-purpose confirm dialog framework — YAGNI. If a second confirmation need arises, the pattern can be extracted.

**Alternatives considered:**
- A generic `ConfirmModal { message, on_yes, on_no }` with closures — closures aren't `Debug` or `Clone`, which breaks `ActiveModal` requirements. Could use an enum dispatch, but one use-case doesn't justify the abstraction.
- Two-step: press `d`, see a "press d again to confirm" prompt — confusing UX, easier to mis-trigger.

### Decision 4: Ghost creation shortcuts the create flow

On a Ghost row, `c` immediately calls `commit_create` at the ghost's resolved path with default `# <title>` content (no folder picker, no filename prompt). `C` opens the template picker with `folder_seed = Some(ghost_parent_dir)`, and on template selection immediately calls `commit_create` at the ghost's resolved path (no filename prompt).

The ghost's target path is computed from `g.raw` (e.g., `projects/foo` → `projects/foo.md`) resolved relative to the vault root. The parent directory is created if it doesn't exist (`std::fs::create_dir_all` is already in `commit_create`).

**Rationale:** The ghost already encodes the desired filename and directory — there's nothing to prompt for. The current `c`/`C` on ghosts goes through the full create flow with the folder pre-seeded from `create_folder_from_selection`, which is redundant (the user already sees the ghost row they want to create). The instant path reduces keystrokes from ~3 to 1.

**Alternatives considered:**
- Keep current multi-step flow for ghosts — functional but slow, the user's explicit request.
- Always auto-create with `# <title>` for `c` but prompt for filename with `C` — inconsistent; the ghost already names the file.

### Decision 5: Subdirectory creation uses a simple inline prompt modal

A new `ActiveModal::CreateSubdir` variant wraps a `PathBuf` (parent directory) and an `EditBuffer` (subfolder name). `Enter` creates `<parent>/<name>/` via `std::fs::create_dir_all`, refreshes the graph, and closes. `Esc` cancels. Validation: non-empty name, no path separators, target doesn't already exist.

**Rationale:** The simplest possible UX — type a name, press Enter. No picker, no multi-step flow. Same validation pattern as the rename modal (`graph-rename-note`).

**Alternatives considered:**
- Reuse the create flow's `FilenamePrompt` — wrong semantics (creating a directory vs a file with `.md` extension).
- Fuzzy picker over common subfolder names — premature; users know what they want to name.

### Decision 6: CLI delete command is minimal

`ft graph delete <path>` takes a vault-relative path, validates it exists in the vault, runs `plan_delete` + `apply_delete`, and prints a confirmation message. No `--force` flag (the TUI already has confirmation). No glob support. No dry-run.

**Rationale:** The user's requirement says "this can be rather simple today." The TUI is the primary interface; the CLI is a fallback for scripting. We can add flags later.

### Decision 7: Key bindings

- `d` → `graph.delete` (confirm-delete modal on Note/Directory rows; toast on Ghost/Task)
- `n` → `graph.create-subdir` (subdir prompt on Directory rows; toast on others)
- `c` remains bound to `graph.create-blank` — dispatch logic changes behavior based on row kind
- `C` remains bound to `graph.create-from-template` — dispatch logic changes behavior based on row kind

**Rationale:** `d` is the standard vim-like "delete" mnemonic. `n` is free and maps to "new" (subdirectory). Keeping `c`/`C` avoids churn — the binding stays the same, only the behavior under ghosts changes.

## Risks / Trade-offs

- **Permanent deletion**: No undo. Mitigation: confirmation modal with clear messaging ("Delete note `projects/alpha.md`?" or "Delete directory `archive/` and all its contents?"). The `?` overlay documents the binding so accidental presses are unlikely.
- **Ghost creation collision**: If the ghost's target file already exists (unlikely — ghosts represent missing files), `commit_create` will trigger the collision prompt. This is correct behavior; no special handling needed.
- **Subdirectory creation name collision**: If the target directory already exists, show an error toast and keep the modal open. Same pattern as rename collision handling.
- **Directory delete with non-md files**: `remove_dir_all` will delete everything including images, `.gitkeep`, etc. The confirmation message should make this explicit. Users with mixed-content directories should be warned.

## Context

The `ft notes rename` CLI command was built before the TUI and before `plan_multi_rename`. It served double duty: rename in place (bare `<NEW>`) and move to directory (path-containing `<NEW>`). Now that the TUI has clean Flow A / Flow B separation and `plan_multi_rename` exists in the library, the CLI should mirror that model and gain the missing capabilities (multi-source, directory sources).

## Goals / Non-Goals

**Goals:**
- `ft notes rename` restricted to same-directory renames only. `<NEW>` is always a bare stem.
- New `ft notes mv <SOURCES>... <TARGET>` command: path-based sources, directory target.
- Directory sources expanded via graph BFS to contained notes.
- Combined `plan_multi_rename` for all sources (notes + expanded directories) in one atomic plan.
- `--dry-run` for both commands.
- `collect_directory_notes` hoisted from TUI into `ft-core` as a shared primitive.

**Non-Goals:**
- CLI `mv` does not support title/fuzzy source resolution — sources are path-only.
- No `-i` (interactive) or `-f` (force) flags in v1.
- No glob/wildcard source expansion.
- No `mv` for ghosts — ghosts have no file to move; use `rename` for ghost identity changes.

## Decisions

### 1. Clean split: `rename` rejects `/` in `<NEW>`

**Why**: Eliminates the ambiguity where `rename` sometimes means "change title" and sometimes means "move." Users who want to move use `mv`. This matches the TUI's Flow A/Flow B split and the mental model of `mv` on the command line.

**Breaking change**: Existing scripts or muscle memory that use `ft notes rename foo archive/foo.md` will break. The error message directs to `ft notes mv`. Breaking is acceptable because the change is small, the error is clear, and the replacement command is a direct mapping.

### 2. `mv` sources are path-only (no title/fuzzy/ghost syntax)

**Why**: `mv` is a file-reorganization command. Paths are unambiguous and compose naturally with tab-completion and shell globbing. Title resolution (which note is "My Note"?) would add ambiguity for a command that accepts multiple sources. Users can use `ft notes rename` for title-based identity changes.

**Path resolution**: Sources are vault-relative paths. `.md` extension is optional for notes (appended automatically when the bare path doesn't exist but `<path>.md` does). Directories must match a `DirData` node in the graph (which means at least one `.md` file exists somewhere under that path, since directories are derived from note paths). If a source resolves to both a note and a directory, the note takes precedence.

### 3. `mv` target is always a directory

**Why**: Matches `mv` semantics when the target is an existing directory: every source is moved INTO the target, keeping its filename. This is the only mode that works with multiple sources. Single-source-to-single-file rename is covered by `ft notes rename`.

**Target must exist**: The target directory must exist on disk (and be visible as a `DirData` node in the graph). If it doesn't exist, the command errors with "target directory not found: <path>". This prevents `mv a.md b.md` from being ambiguous (is `b.md` a directory or a target filename?).

### 4. `collect_directory_notes` hoisted to `ft-core`

**Why**: The CLI needs the same BFS directory-expansion logic the TUI uses. Currently it lives in `ft/src/tui/tabs/graph.rs` as a private function. Moving it to `ft_core::graph::rename` (alongside `plan_multi_rename`) makes it a shared library primitive, testable independently, and imports cleanly from both CLI and TUI.

**Signature**: `pub fn collect_directory_notes(graph: &Graph, dir_id: NoteId, old_dir: &Path, new_dir: &Path) -> Vec<(NoteId, PathBuf)>` — same as the current TUI implementation.

### 5. Dry-run output format

Both `rename` and `mv` support `--dry-run`. The summary prints:
- Each source → target path mapping
- Total link edits and affected files
- No files are modified

For `mv` with directory sources, the expansion is shown: "expanding directory N/ → N files" followed by the per-file plan.

## Risks / Trade-offs

- **Breaking change**: `rename` with path-containing `<NEW>` breaks. Mitigation: clear error message pointing to `mv`. The migration is mechanical: `ft notes rename foo archive/foo.md` → `ft notes mv foo.md archive/`.

- **Directory resolution**: A directory must have at least one `.md` file somewhere under it to appear in the graph. Empty directories (no notes) are invisible and error. This is consistent with the TUI behavior.

- **Target directory existence**: If the target directory doesn't exist, the command errors. The user must create it first (e.g., `mkdir` or `ft notes create target/.keep`). Future: could add `--create-target` flag to auto-create.

## Open Questions

<!-- None -->

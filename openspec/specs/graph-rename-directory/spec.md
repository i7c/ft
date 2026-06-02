# graph-rename-directory Specification

## Purpose
TBD - created by archiving change graph-tui-note-rename. Update Purpose after archive.
## Requirements
### Requirement: r on a Directory row with no multi-selection opens rename modal

The system SHALL open a rename-in-place modal when `r` is pressed in Normal mode with an empty `multi_selected` and the focused row is a Directory (not the vault root). The modal SHALL pre-fill an `EditBuffer` with the directory's name (last path component).

#### Scenario: r on a subdirectory opens modal

- **WHEN** the focused row is Directory `projects/old-name` and `multi_selected` is empty
- **THEN** pressing `r` opens a modal with title "Rename directory" and an `EditBuffer` containing "old-name"

#### Scenario: r on vault root does nothing

- **WHEN** the focused row is the root Directory (empty path) and `multi_selected` is empty
- **THEN** pressing `r` shows a toast "cannot rename vault root" and no modal opens

### Requirement: Directory rename recursively finds all contained files

The system SHALL walk `Contains` edges from the directory node via BFS to collect all reachable `NodeKind::Note` entries. For each note at path `<old_dir>/<rest>.md`, the new path SHALL be `<new_dir>/<rest>.md` where `<new_dir>` is the same parent as the old directory plus the new directory name. This collection SHALL be fed to `plan_multi_rename`.

#### Scenario: Directory with two files and a subdirectory

- **WHEN** renaming directory `projects/a` to `projects/b` and the directory contains `projects/a/notes.md`, `projects/a/readme.md`, and subdirectory `projects/a/sub` containing `projects/a/sub/deep.md`
- **THEN** the collected note paths are `["projects/a/notes.md", "projects/a/readme.md", "projects/a/sub/deep.md"]` and the new paths are `["projects/b/notes.md", "projects/b/readme.md", "projects/b/sub/deep.md"]`

#### Scenario: Empty directory rename succeeds

- **WHEN** renaming an empty directory `empty-dir/` to `renamed-dir/`
- **THEN** `plan_multi_rename` receives an empty moves list; the plan has zero `renames` and zero `edits`; applying succeeds (the old directory is removed, the new directory is created if any parent dir differs)

### Requirement: Directory rename updates all external references to contained files

The system SHALL update every vault-wide reference (wikilink, markdown link, embed) that points to any note under the renamed directory. References that use path-form wikilinks (`[[old-dir/file]]`) SHALL be updated to the new path; bare wikilinks (`[[file]]`) SHALL remain unchanged since the title (stem) does not change.

#### Scenario: External note links to a file in the renamed directory

- **WHEN** renaming directory `docs/` to `reference/` and file `external.md` contains `[[docs/guide]]`
- **THEN** after the rename, `external.md` contains `[[reference/guide]]`

#### Scenario: Bare wikilink to a file in the renamed directory stays unchanged

- **WHEN** renaming directory `docs/` to `reference/` and file `external.md` contains `[[guide]]` (the title of `docs/guide.md`)
- **THEN** after the rename, `external.md` still contains `[[guide]]` (the title didn't change)

### Requirement: Directory rename handles cross-references within the renamed directory

The system SHALL correctly update references between files that are both being renamed. Edits SHALL be computed against old file paths, applied to files at their old paths, and then files SHALL be renamed to new paths (edit-then-rename).

#### Scenario: Two files in the renamed directory link to each other

- **WHEN** renaming directory `old/` to `new/` and `old/a.md` contains `[[old/b]]` and `old/b.md` contains `[[old/a]]`
- **THEN** after the rename, `new/a.md` contains `[[new/b]]` and `new/b.md` contains `[[new/a]]`

### Requirement: Directory rename cleans up empty old directories

After all file renames complete, the system SHALL attempt to remove empty directories under the old path, bottom-up (deepest first). Directories that contain non-note files (images, `.gitkeep`, etc.) SHALL be left in place.

#### Scenario: Old directory tree is empty after rename

- **WHEN** renaming directory `old/` (containing only `old/file.md`) to `new/`
- **THEN** after the rename, path `old/` no longer exists on disk and `new/file.md` exists

#### Scenario: Old directory contains a non-note file

- **WHEN** renaming directory `old/` (containing `old/file.md` and `old/logo.png`) to `new/`
- **THEN** after the rename, `new/file.md` and `new/logo.png` exist; `old/logo.png` still exists (the directory is not removed because it's non-empty)

### Requirement: Directory rename validates preconditions

The system SHALL validate before executing the rename: the old directory must exist as a `DirData` node in the graph; the new name must be non-empty and not contain path separators; the new directory path must not already exist on disk.

#### Scenario: Target directory already exists

- **WHEN** renaming directory `projects/a` to `projects/b` and `projects/b/` already exists on disk
- **THEN** the rename SHALL error with "target directory already exists: projects/b"

#### Scenario: New name is empty

- **WHEN** the rename modal buffer is empty after trimming
- **THEN** pressing `Enter` shows an error toast "name cannot be empty" and leaves the modal open


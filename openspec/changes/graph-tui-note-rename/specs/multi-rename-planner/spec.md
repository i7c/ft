## ADDED Requirements

### Requirement: plan_multi_rename accepts a set of note-path pairs

The system SHALL provide `pub fn plan_multi_rename(graph: &Graph, vault_root: &Path, moves: &[(NoteId, PathBuf)]) -> Result<RenamePlan>` that builds a combined `RenamePlan` from a single graph snapshot. Each `(NoteId, PathBuf)` pair specifies a source note (real or ghost) and its new vault-relative path.

#### Scenario: Single note rename via plan_multi_rename

- **WHEN** called with one `(note_id, new_path)` pair for a real note
- **THEN** the result is identical to calling `plan_rename` for that note (same edits, same renames, same snapshots)

#### Scenario: Two notes with no cross-references

- **WHEN** called with pairs `(note_a, "bar/a.md")` and `(note_b, "bar/b.md")` where no linker references both
- **THEN** the plan contains two `FileRename` entries and edits from both notes' incoming edges, no overlap

#### Scenario: Two notes with cross-reference

- **WHEN** called with pairs `(note_a, "bar/a.md")` and `(note_b, "bar/b.md")` where note_a links to note_b via `[[b]]`
- **THEN** the plan contains edits in note_a's old file updating the reference; note_a is then renamed to `bar/a.md`; note_b is renamed to `bar/b.md`

#### Scenario: Ghost rename via plan_multi_rename

- **WHEN** called with one `(ghost_id, new_path)` pair
- **THEN** the plan has zero `renames` and edits for all incoming links to that ghost

### Requirement: plan_multi_rename refuses to clobber existing files

The system SHALL return `Err` if any new path already exists on disk (at vault_root) and is not the same file as the corresponding source path. No-op moves (old path == new path) SHALL be silently skipped.

#### Scenario: Target path exists

- **WHEN** called with `(note_a, "existing.md")` where `existing.md` is a different file already on disk
- **THEN** returns `Err` with message containing "target already exists"

#### Scenario: Same-path move is skipped

- **WHEN** called with `(note_a, "same/path/as/before.md")` where this equals the note's current path
- **THEN** that pair contributes zero edits and zero renames to the plan

### Requirement: plan_multi_rename validates non-overlap of merged edits

The system SHALL group edits by linker path and validate that within each file, no two edit byte-ranges overlap (ascending: each `end ≤ next start`). Overlapping edits from different source notes in the same linker file indicate a planner bug and SHALL return `Err`.

#### Scenario: Non-overlapping edits from two source notes in same linker

- **WHEN** a linker file contains `[[note_a]] ... [[note_b]]` and both notes are being renamed
- **THEN** the edits for note_a and note_b occupy disjoint byte ranges; the plan succeeds

### Requirement: plan_multi_rename returns error for unsupported node kinds

The system SHALL return `Err` with a descriptive message when any `NoteId` in `moves` refers to a `NodeKind::Directory` or `NodeKind::Task`. Only `Note` and `Ghost` nodes are supported.

#### Scenario: Directory in moves errors

- **WHEN** called with a `NoteId` that refers to a Directory node
- **THEN** returns `Err` with message containing "renaming directory nodes is not yet supported"

### Requirement: RenamePlan struct uses Vec instead of Option

`RenamePlan::renames` SHALL be a `Vec<FileRename>` (replacing the previous `rename: Option<FileRename>` field). The `touched_files` method SHALL iterate over all renames. `apply_rename_plan` SHALL process all renames after all edits: create parent directories for each rename target, rename files, then attempt to remove empty directories under each rename source bottom-up.

#### Scenario: Apply plan with three renames

- **WHEN** a plan has three `FileRename` entries
- **THEN** `apply_rename_plan` applies all edits, then renames all three files, then removes empty old directories

#### Scenario: touched_files counts source files from all renames

- **WHEN** a plan has two renames (from `a.md` and `b.md`) and edits in `linker.md`
- **THEN** `touched_files()` returns 3 (`a.md`, `b.md`, `linker.md`)

### Requirement: apply_rename_plan cleans up empty directories

After all file renames complete, the system SHALL attempt to remove each unique parent directory of each rename source path, deepest-first. Removal SHALL be best-effort — `std::fs::remove_dir` fails silently on non-empty directories.

#### Scenario: Single file moved out of a directory

- **WHEN** a plan renames `subdir/file.md` to `root.md`
- **THEN** after the rename, if `subdir/` is empty, it is removed

#### Scenario: Non-empty directory preserved

- **WHEN** a plan renames `subdir/a.md` to `other/a.md` but `subdir/b.md` is not part of the rename
- **THEN** after the rename, `subdir/` is preserved (contains `b.md`)

# graph-delete

## Purpose

Enable deletion of note files and directory trees from the TUI graph view (with confirmation modal) and from the CLI.

## Requirements

### Requirement: Delete a note file from the TUI graph view

The system SHALL delete the note file on disk when the user triggers `graph.delete` on a Note row and confirms. The graph SHALL be refreshed to reflect the deletion.

#### Scenario: Delete a note with confirmation

- **WHEN** the focused row is a Note and the user presses `d`
- **THEN** a confirmation modal appears showing the vault-relative path (e.g., "Delete note `projects/alpha.md`?")
- **AND** the user can choose Yes (`y`/`Enter` on Yes) or No (`n`/`Esc`/`Enter` on No)
- **AND** on Yes, the file `projects/alpha.md` is removed from disk
- **AND** the graph refreshes and a success toast shows "deleted projects/alpha.md"
- **AND** on No/Esc, the modal closes and no files are changed

#### Scenario: Delete a root-level note

- **WHEN** the focused row is Note `readme.md` and the user presses `d` and confirms
- **THEN** the file `readme.md` is removed from disk, the graph refreshes, and a success toast shows

#### Scenario: Delete a note with leading/trailing whitespace in path

- **WHEN** the focused note path has unusual characters
- **THEN** the confirmation dialog shows the exact path and deletion proceeds if confirmed

### Requirement: Delete a directory tree from the TUI graph view

The system SHALL recursively delete a directory and all its contents when the user triggers `graph.delete` on a Directory row and confirms.

#### Scenario: Delete a directory with confirmation

- **WHEN** the focused row is a Directory (e.g., `archive/`) and the user presses `d`
- **THEN** a confirmation modal appears showing "Delete directory `archive/` and all its contents?"
- **AND** on Yes, the directory `archive/` and all files/subdirectories within it are removed from disk
- **AND** the graph refreshes and a success toast shows "deleted archive/"
- **AND** on No/Esc, the modal closes and no files are changed

#### Scenario: Cannot delete vault root

- **WHEN** the focused row is the vault root directory
- **THEN** pressing `d` shows an error toast "cannot delete vault root" and no modal opens

#### Scenario: Delete an empty directory

- **WHEN** the focused row is an empty Directory and the user presses `d` and confirms
- **THEN** the empty directory is removed from disk, the graph refreshes, and a success toast shows

### Requirement: Delete is a no-op on non-deletable node types

The system SHALL show an error toast when `graph.delete` is invoked on Ghost, Task, or Paragraph rows.

#### Scenario: Delete on Ghost toasts

- **WHEN** the focused row is a Ghost and the user presses `d`
- **THEN** an error toast shows "cannot delete a ghost — it does not exist on disk" and no modal opens

#### Scenario: Delete on Task toasts

- **WHEN** the focused row is a Task and the user presses `d`
- **THEN** an error toast shows "cannot delete a task node — delete the task in its source file" and no modal opens

### Requirement: CLI delete command removes files from disk

The system SHALL provide `ft graph delete <path>` which accepts a vault-relative path, validates it exists, removes it, and prints a confirmation message. Directories are removed recursively.

#### Scenario: CLI delete a note

- **WHEN** the user runs `ft graph delete projects/alpha.md`
- **THEN** the file `projects/alpha.md` is removed from disk and a confirmation message is printed to stdout

#### Scenario: CLI delete a directory

- **WHEN** the user runs `ft graph delete archive/`
- **THEN** the directory `archive/` and all its contents are removed recursively and a confirmation message is printed

#### Scenario: CLI delete non-existent path errors

- **WHEN** the user runs `ft graph delete nonexistent/file.md`
- **THEN** the command exits with a non-zero code and an error message

#### Scenario: CLI requires path argument

- **WHEN** the user runs `ft graph delete` with no path argument
- **THEN** the command exits with a non-zero code and shows usage information

### Requirement: Confirmation modal uses the existing modal infrastructure

The confirmation modal SHALL be implemented as `ActiveModal::ConfirmDelete`, implementing the `Modal` trait. The App-level `active_modal` slot SHALL own it during confirmation.

#### Scenario: ConfirmDelete appears in the active modal slot

- **WHEN** the confirmation modal is open
- **THEN** `App::active_modal()` returns `Some(ActiveModal::ConfirmDelete(_))`

#### Scenario: ConfirmDelete renders a centered yes/no dialog

- **WHEN** the confirmation modal is active
- **THEN** it renders a bordered box showing the confirmation message and two labeled buttons (e.g., `[Yes]` and `[No]`) with the focused choice highlighted

#### Scenario: ConfirmDelete navigation keys

- **WHEN** the confirmation modal is active
- **THEN** `Left`/`h`/`Tab` moves focus to the previous choice, `Right`/`l` cycles forward, `y` jumps to Yes, `n`/`Esc`/`q` selects No
- **AND** `Enter` confirms the currently focused choice

# graph-create-subdir

## Purpose

Enable creation of subdirectories from directory nodes in the TUI graph view via a single-line prompt modal.

## Requirements

### Requirement: Create subdirectory on a Directory row

The system SHALL open a prompt modal when `graph.create-subdir` is invoked on a Directory row. The user SHALL enter a subdirectory name, and on confirm the system SHALL create the directory on disk and refresh the graph.

#### Scenario: Create a subdirectory in an existing directory

- **WHEN** the focused row is Directory `projects/` and the user presses `n`
- **THEN** a prompt modal opens with the title "Create subdirectory in `projects/`"
- **AND** the user types `new-project` and presses `Enter`
- **THEN** the directory `projects/new-project/` is created on disk
- **AND** the graph refreshes and a success toast shows "created projects/new-project/"
- **AND** the newly created directory is visible in the tree after refresh

#### Scenario: Create subdirectory at vault root

- **WHEN** the focused row is the vault root (empty path) and the user presses `n`
- **THEN** a prompt modal opens with title "Create subdirectory in vault root"
- **AND** on entering `docs` and pressing `Enter`, `docs/` is created in the vault root

#### Scenario: Subdirectory with special characters

- **WHEN** the prompt is active and the user enters a name with path separators (e.g., `a/b`)
- **THEN** pressing `Enter` shows an error toast "name cannot contain path separators" and the modal remains open

#### Scenario: Subdirectory already exists

- **WHEN** the prompt is active and the user enters a name that already exists on disk (e.g., entering `existing` when `projects/existing/` exists)
- **THEN** pressing `Enter` shows an error toast "directory already exists: projects/existing/" and the modal remains open

#### Scenario: Empty name

- **WHEN** the prompt is active and the user presses `Enter` with an empty buffer
- **THEN** an error toast shows "name cannot be empty" and the modal remains open

### Requirement: Create subdirectory is a no-op on non-Directory rows

The system SHALL show an error toast when `graph.create-subdir` is invoked on any row that is not a Directory.

#### Scenario: Create subdirectory on a Note row toasts

- **WHEN** the focused row is a Note and the user presses `n`
- **THEN** an error toast shows "select a directory first" and no modal opens

#### Scenario: Create subdirectory on a Ghost row toasts

- **WHEN** the focused row is a Ghost and the user presses `n`
- **THEN** an error toast shows "select a directory first" and no modal opens

### Requirement: Subdirectory prompt uses the existing modal infrastructure

The subdirectory prompt SHALL be implemented as `ActiveModal::CreateSubdir`, implementing the `Modal` trait. It SHALL own an `EditBuffer` for the subdirectory name and a `PathBuf` for the parent directory.

#### Scenario: CreateSubdir modal renders a prompt with input field

- **WHEN** the subdirectory modal is active for parent `projects/`
- **THEN** it renders a bordered box with the title "Create subdirectory in projects/", an input field showing the current buffer content with cursor, and any error text below

#### Scenario: CreateSubdir modal editing keys

- **WHEN** the subdirectory modal is active
- **THEN** printable characters insert into the buffer, `Backspace`/`Delete`/`Left`/`Right`/`Home`/`End`/`Ctrl+W` operate on the buffer, `Esc` closes the modal without creating, `Enter` attempts to commit

#### Scenario: Esc cancels subdirectory creation

- **WHEN** the prompt is active and the user presses `Esc`
- **THEN** the modal closes without creating any directory

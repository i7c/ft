## ADDED Requirements

### Requirement: r with multi-selection enters move-target phase

The system SHALL enter the move-target directory phase when `r` is pressed in Normal mode with a non-empty `multi_selected`. A banner SHALL appear above the tree: "Move N note(s): navigate to target directory, Enter/m to confirm, t for picker, Esc to cancel". The tree navigation keys (`j`/`k`/`↑`/`↓`) SHALL remain active so the user can browse to a directory node.

#### Scenario: r with two notes selected enters move phase

- **WHEN** `multi_selected` contains two `NoteId`s and user presses `r`
- **THEN** a banner shows "Move 2 note(s): navigate to target directory, Enter/m to confirm, t for picker, Esc to cancel" and tree navigation keys work normally

#### Scenario: r with one note selected enters move phase

- **WHEN** `multi_selected` contains one `NoteId` and user presses `r`
- **THEN** a banner shows "Move 1 note(s): ..." and tree navigation keys work normally

### Requirement: Confirming on a Directory node executes the move

The system SHALL compute new paths for all selected notes as `<target_dir>/<note_filename>.md`, build a combined `RenamePlan` via `plan_multi_rename`, apply it, clear `multi_selected`, refresh the graph, and show a success toast.

#### Scenario: Move two notes to a directory

- **WHEN** the user has selected `projects/alpha.md` and `areas/beta.md`, navigates to Directory `archive/`, and presses `Enter`
- **THEN** `projects/alpha.md` is renamed to `archive/alpha.md`, `areas/beta.md` is renamed to `archive/beta.md`, all references are updated, graph refreshes, multi-selection clears, and toast shows "moved 2 note(s) to archive/"

#### Scenario: Move note to a subdirectory

- **WHEN** the user has selected `readme.md`, navigates to Directory `docs/`, and presses `m`
- **THEN** `readme.md` is renamed to `docs/readme.md`, references are updated, and toast shows "moved 1 note(s) to docs/"

### Requirement: Confirming on a non-Directory node toasts and stays in phase

The system SHALL reject confirmation on any non-Directory row (Note, Ghost, Task) with an error toast "select a directory as target" and remain in the move-target phase so the user can navigate to a Directory.

#### Scenario: Confirming on a Note row toasts

- **WHEN** the move-target phase is active and the focused row is a Note
- **THEN** pressing `Enter` shows toast "select a directory as target" and the banner/phase remain active

### Requirement: Confirming on empty tree toasts and stays in phase

The system SHALL reject confirmation when no row is focused (empty tree) with an error toast and remain in the move-target phase.

#### Scenario: Confirming with no row focused

- **WHEN** the move-target phase is active and the tree has no rows (empty query)
- **THEN** pressing `Enter` or `m` shows toast "select a directory as target" and the banner/phase remain active

### Requirement: t during move-target phase opens fuzzy directory picker

The system SHALL open a fuzzy directory picker when `t` is pressed during the move-target phase. Selecting a directory from the picker SHALL execute the move (same as confirming on a tree Directory node). `Esc` in the picker SHALL return to the move-target phase with the banner.

#### Scenario: t opens directory picker then select target

- **WHEN** the move-target phase is active and user presses `t`
- **THEN** a fuzzy directory picker opens; selecting `archive/` executes the move and shows success toast

#### Scenario: Esc in directory picker returns to move-target phase

- **WHEN** the directory picker is open during move-target phase
- **THEN** pressing `Esc` closes the picker and the move-target banner reappears

### Requirement: Esc during move-target phase cancels

The system SHALL cancel the move flow when `Esc` is pressed during the move-target phase. `multi_selected` SHALL be cleared, the banner SHALL disappear, and the tab SHALL return to Normal mode.

#### Scenario: Esc cancels move flow

- **WHEN** the move-target phase is active with two notes selected
- **THEN** pressing `Esc` clears `multi_selected`, removes the banner, and returns to Normal mode

### Requirement: Move to same directory is a no-op

The system SHALL detect when all selected notes are already in the target directory and SHALL toast "all N note(s) are already in <dir>" without making any changes. If only some notes are already in the target, those SHALL be silently skipped and only the others SHALL be moved.

#### Scenario: All notes already in target directory

- **WHEN** the user selects `archive/a.md` and `archive/b.md`, and picks target directory `archive/`
- **THEN** a toast shows "all 2 note(s) are already in archive/" and no files are modified

#### Scenario: One note already in target, one not

- **WHEN** the user selects `archive/a.md` and `projects/b.md`, and picks target directory `archive/`
- **THEN** `projects/b.md` is moved to `archive/b.md`, `archive/a.md` is skipped, and toast shows "moved 1 note(s) to archive/ (1 already there)"

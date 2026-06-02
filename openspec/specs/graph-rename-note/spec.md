# graph-rename-note Specification

## Purpose
TBD - created by archiving change graph-tui-note-rename. Update Purpose after archive.
## Requirements
### Requirement: r on a Note row with no multi-selection opens rename modal

The system SHALL open a rename-in-place modal when `r` is pressed in Normal mode with an empty `multi_selected` and the focused row is a Note. The modal SHALL pre-fill an `EditBuffer` with the note's filename stem (without `.md` extension).

#### Scenario: r on a Note opens modal with stem pre-filled

- **WHEN** the focused row is Note `projects/alpha.md` and `multi_selected` is empty
- **THEN** pressing `r` opens a modal with title "Rename note" and an `EditBuffer` containing "alpha"

#### Scenario: r on root-level Note opens modal

- **WHEN** the focused row is Note `readme.md` and `multi_selected` is empty
- **THEN** pressing `r` opens a modal with `EditBuffer` containing "readme"

### Requirement: Enter in rename modal commits the rename

The system SHALL validate the new name (non-empty, no path separators, target doesn't already exist), build a `RenamePlan` via `plan_rename` (same directory, new stem), apply it via `apply_rename_plan`, and close the modal. On success, the graph SHALL be refreshed and a success toast displayed.

#### Scenario: Successful rename

- **WHEN** the rename modal is open for `foo.md` with buffer "bar" and no file `bar.md` exists in the same directory
- **THEN** pressing `Enter` renames `foo.md` to `bar.md`, updates all references, refreshes the graph, and shows toast "renamed foo.md → bar.md"

#### Scenario: Rename to existing name errors

- **WHEN** the rename modal is open for `foo.md` with buffer "bar" and `bar.md` already exists
- **THEN** pressing `Enter` shows an error toast "target already exists: bar.md" and leaves the modal open

#### Scenario: Empty name errors

- **WHEN** the rename modal is open with an empty buffer (after trimming)
- **THEN** pressing `Enter` shows an error toast "name cannot be empty" and leaves the modal open

#### Scenario: Name with path separator errors

- **WHEN** the rename modal is open with buffer "subdir/bar"
- **THEN** pressing `Enter` shows an error toast "name cannot contain / — use move (Space-select + r) to change directories" and leaves the modal open

### Requirement: Rename modal supports EditBuffer editing keys

While the rename modal is open, the system SHALL capture printable characters, `Backspace`, `Delete`, `Left`, `Right`, `Home`, `End`, and `Ctrl+W` and route them to the `EditBuffer`. All other tree-navigation and action keys (including `r`, `Space`, `j`, `k`) SHALL be consumed and not affect tree state. Global keys (tab switch, quit) SHALL still work.

#### Scenario: Typing in the modal

- **WHEN** the rename modal is open with buffer "alpha"
- **THEN** pressing `x` inserts 'x' at cursor; pressing `Backspace` deletes left of cursor; pressing `Ctrl+W` deletes the previous word

#### Scenario: j and k are captured by modal

- **WHEN** the rename modal is open
- **THEN** pressing `j` or `k` inserts the character into the buffer, does NOT move tree selection

### Requirement: Esc in rename modal discards and closes

The system SHALL close the rename modal without making any changes when `Esc` is pressed. The graph state (tree, expansions, selection) SHALL be unchanged.

#### Scenario: Esc discards rename

- **WHEN** the rename modal is open for `foo.md` with buffer "bar"
- **THEN** pressing `Esc` closes the modal, the file `foo.md` is unchanged, and the tree returns to Normal mode with the same selection

### Requirement: r on a Ghost or Task row with no multi-selection does nothing

The system SHALL not open the rename modal when the focused row is a Ghost or Task and `multi_selected` is empty. A toast SHALL inform the user.

#### Scenario: r on Ghost toasts

- **WHEN** the focused row is a Ghost and `multi_selected` is empty
- **THEN** pressing `r` shows a toast "cannot rename a ghost — create the note first" and no modal opens


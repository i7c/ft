## ADDED Requirements

### Requirement: Space toggles multi-selection on graph tree rows

The system SHALL allow the user to toggle multi-selection on graph tree nodes by pressing `Space` in Normal mode. Multi-selection is independent of the single-focus cursor — the cursor moves with `j`/`k`, and `Space` adds or removes the focused node from the selection set without moving the cursor.

#### Scenario: Space toggles a Note row into selection

- **WHEN** the focused row is a Note and no rows are currently multi-selected
- **THEN** pressing `Space` adds that note's `NoteId` to `multi_selected` and the row shows a visual selection marker

#### Scenario: Space toggles a Note row out of selection

- **WHEN** the focused row is a Note that is already multi-selected
- **THEN** pressing `Space` removes that note's `NoteId` from `multi_selected` and the selection marker disappears

#### Scenario: Space on a Directory row is a no-op

- **WHEN** the focused row is a Directory
- **THEN** pressing `Space` does not modify `multi_selected` and shows no visual change

#### Scenario: Space on a Ghost row is a no-op

- **WHEN** the focused row is a Ghost
- **THEN** pressing `Space` does not modify `multi_selected` and shows no visual change

### Requirement: Multi-selected rows have a visual marker

The system SHALL render multi-selected rows with a `●` marker between the expand indicator and the kind prefix. Non-selected rows SHALL have a space in that position to keep column alignment stable. The marker is rendered in a distinct color (yellow or accent) to differentiate it from the expand indicator.

#### Scenario: Two notes selected, one not

- **WHEN** the tree has three Note rows at indices 0, 1, 2 and rows 0 and 2 are multi-selected
- **THEN** rows 0 and 2 show `  ▶ ● N  ...` and row 1 shows `  ▶   N  ...`

#### Scenario: Selection marker survives expand/collapse

- **WHEN** a multi-selected Note row is collapsed (children hidden)
- **THEN** the selection marker on that row persists; the state in `multi_selected` is unchanged

### Requirement: Esc clears all multi-selections

The system SHALL clear all multi-selections when `Esc` is pressed and `multi_selected` is non-empty. If `multi_selected` is already empty, `Esc` SHALL pass through to the app-level `Esc` handler (which currently does nothing in Normal mode).

#### Scenario: Esc with two selections clears both

- **WHEN** `multi_selected` contains two `NoteId`s
- **THEN** pressing `Esc` empties `multi_selected` and all selection markers disappear

#### Scenario: Esc with empty selection passes through

- **WHEN** `multi_selected` is empty
- **THEN** pressing `Esc` returns `EventOutcome::NotHandled`

### Requirement: Multi-selection clears on graph refresh

The system SHALL clear `multi_selected` when the graph is rebuilt (manual `Ctrl+r` refresh or editor-return refresh), because `NoteId`s from a previous graph build are stale.

#### Scenario: Refresh clears selections

- **WHEN** `multi_selected` contains two `NoteId`s and the user presses `Ctrl+r`
- **THEN** after the refresh, `multi_selected` is empty

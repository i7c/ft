# history-tui-tab — delta

## ADDED Requirements

### Requirement: Citation badge on history rows
History tab rows SHALL render the same citation badges as the Journal
tab (`cited` / `cited*` / nothing), read from the shared snapshot's
citation index via `TabCtx::snapshot`.

#### Scenario: Badge visible in the sweep
- **WHEN** the History tab shows a window containing a paragraph
  pinned in a synth note
- **THEN** that row carries the `cited` marker

### Requirement: history.toggle-uncited command
A `history.toggle-uncited` command (bound to `u` by default) SHALL
toggle uncited-only filtering with the same semantics as the Journal
tab, declared in the tab's command/keymap statics.

#### Scenario: Incremental triage in the TUI
- **WHEN** the user presses `u` on the History tab
- **THEN** rows already pinned byte-identically in synth notes
  disappear, leaving the entries still needing attention

# journal-tui-tab — delta

## ADDED Requirements

### Requirement: Citation badge on journal rows
Journal tab rows SHALL render the citation state from the shared
snapshot's citation index: a `cited` marker (with note stem) for
exact citations, a visually distinct `cited*` marker for stale ones,
nothing for uncited. Badge data SHALL come from `TabCtx::snapshot` —
the tab performs no scanning of its own.

#### Scenario: Rows re-badge on snapshot refresh
- **WHEN** the user appends an entry to a synth note and the graph
  refresh completes
- **THEN** that entry's row shows the `cited` marker without reloading
  the tab

### Requirement: journal.toggle-uncited command
A `journal.toggle-uncited` command (bound to `u` by default) SHALL
toggle the feed between all entries and uncited-only (stale counts as
uncited, matching the CLI). The command SHALL be declared in the
tab's command/keymap statics so the `?` overlay, `docs/keybindings.md`,
and `ft commands list` pick it up.

#### Scenario: Toggle filters the visible feed
- **WHEN** the user presses `u` on a feed with cited and uncited
  entries
- **THEN** only non-`Cited` entries remain visible, and pressing `u`
  again restores the full feed

### Requirement: Note-context badges
When a target synth note is in play — the append-to-existing (`s`)
picker flow, or a journal opened from a synth note's
`ft-synth-targets` — row badges SHALL re-scope to that note:
`in note` when the entry is already pinned there (per the
filter_missing rule), `missing` otherwise, with a status-line
indicator naming the target note. Leaving the flow SHALL restore
global badges.

#### Scenario: Working a journal toward a note
- **WHEN** the user enters the `s` flow and picks synth note N
- **THEN** every feed row shows `in note` or `missing` relative to N,
  matching exactly what plan-time dedup would drop or keep
